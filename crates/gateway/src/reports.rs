//! Compliance reports on disk: one CSV per dimming, written with
//! write-then-rename, kept (nothing here deletes them; BK6-22-300 7.3 asks
//! for at least two years). The SHA-256 chain continues across restarts
//! from the newest file.

use std::path::{Path, PathBuf};

use control::DimmingReport;
use control::compliance::GENESIS;
use serde::Serialize;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct ReportEntry {
    pub file: String,
    pub bytes: u64,
}

/// Where the chain stands: the number and digest of the newest report.
pub fn resume(dir: &Path) -> (u32, String) {
    let newest = list(dir).into_iter().next_back();
    let Some(entry) = newest else { return (0, GENESIS.into()) };
    let seq = entry.file.get(8..12).and_then(|s| s.parse().ok()).unwrap_or(0);
    let text = std::fs::read_to_string(dir.join(&entry.file)).unwrap_or_default();
    let digest = text.lines().last().and_then(|l| l.strip_prefix("# sha256: ")).map(str::to_string);
    match digest {
        Some(d) if d.len() == 64 => (seq, d),
        _ => {
            warn!("report {} has no digest line; the chain restarts", entry.file);
            (seq, GENESIS.into())
        }
    }
}

pub fn write(dir: &Path, r: &DimmingReport) {
    let path = dir.join(r.file_name());
    let tmp = path.with_extension("csv.tmp");
    let result = std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::write(&tmp, r.to_csv()))
        .and_then(|_| std::fs::rename(&tmp, &path));
    match result {
        Ok(()) => {
            info!("dimming report {}: {} for {:.0} s, {}", r.seq, r.verdict.name(), r.duration_s(), path.display())
        }
        Err(e) => warn!("could not write {}: {e}", path.display()),
    }
}

/// Report files, oldest first.
pub fn list(dir: &Path) -> Vec<ReportEntry> {
    let mut out: Vec<ReportEntry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().into_string().ok()?;
            valid_name(&file).then(|| ReportEntry { bytes: e.metadata().map_or(0, |m| m.len()), file })
        })
        .collect();
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// The CSV of one report, if `name` is a report file name (never a path).
pub fn read(dir: &Path, name: &str) -> Option<String> {
    valid_name(name).then(|| std::fs::read_to_string(dir.join(name)).ok()).flatten()
}

/// `dimming-NNNN-<timestamp>.csv` with only safe characters.
fn valid_name(name: &str) -> bool {
    name.starts_with("dimming-")
        && name.ends_with(".csv")
        && name.len() < 80
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && !name.contains("..")
}

pub fn default_dir(config_path: &Path) -> PathBuf {
    config_path.with_file_name("reports")
}

#[cfg(test)]
mod tests {
    use super::*;
    use control::Recorder;

    fn report_pair() -> Vec<DimmingReport> {
        let mut rec = Recorder::new("depot", 60.0, 1_737_327_600_000);
        for (a, b) in [(0.0, 100.0), (1000.0, 1100.0)] {
            for k in 0..(b - a) as usize {
                rec.observe(a + k as f64, true, false, 18.2, Some(12.0), Some(18.0));
            }
            rec.observe(b, false, false, 18.2, Some(40.0), None);
        }
        rec.take_reports()
    }

    #[test]
    fn written_reports_resume_the_chain_and_refuse_foreign_names() {
        let dir = std::env::temp_dir().join(format!("gw-reports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(resume(&dir), (0, GENESIS.to_string()));
        let reps = report_pair();
        for r in &reps {
            write(&dir, r);
        }
        let files = list(&dir);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].file, "dimming-0001-2025-01-19T23-00-00Z.csv");
        assert_eq!(resume(&dir), (2, reps[1].sha256.clone()));
        let csv = read(&dir, &files[1].file).unwrap();
        assert!(DimmingReport::verify_csv(&csv));
        assert!(read(&dir, "../dso-state.json").is_none());
        assert!(read(&dir, "dimming-..-x.csv").is_none());
        assert!(read(&dir, "dimming-0001\\..\\x.csv").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
