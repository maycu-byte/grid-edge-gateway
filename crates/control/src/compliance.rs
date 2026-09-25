//! Evidence that each consumption reduction was carried out.
//!
//! BK6-22-300 Anlage 1, 7.2–7.4: the site operator must be able to show the
//! DSO, case by case, that a reduction was implemented, and keep that for at
//! least two years. The recorder watches every control cycle; when a dimming
//! ends it closes a [`DimmingReport`]: the floor, the controllable devices'
//! grid draw second by second, and the verdict. Reports are chained by
//! SHA-256 — each one names the digest of the one before — so a report that
//! was edited or removed later breaks the chain.
//!
//! No I/O here: the gateway writes the CSV to disk, the browser demo offers
//! it as a download.

use sha2::{Digest, Sha256};

/// Draw above the floor that counts as measurement noise, kW.
pub const TOLERANCE_KW: f64 = 0.2;
/// The chain starts from this digest.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// Longest event kept at full resolution (samples of one second); later
/// samples are dropped and the report says so.
const MAX_SAMPLES: usize = 6 * 3600;

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub t_s: f64,
    /// Controllable devices' draw from the grid (IOA 1004); `None` when the
    /// meter could not see it.
    pub steuve_grid_kw: Option<f64>,
    /// What the controller granted them in that cycle.
    pub budget_kw: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within floor + tolerance for the whole event after the grace time.
    Followed,
    /// Above floor + tolerance for some seconds.
    Exceeded,
    /// Never above, but part of the event could not be measured.
    Unverified,
}

impl Verdict {
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Followed => "followed",
            Verdict::Exceeded => "exceeded",
            Verdict::Unverified => "unverified",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DimmingReport {
    /// Running number, 1 for the first report of this gateway.
    pub seq: u32,
    pub site: String,
    /// Unix time of the controller clock's zero, ms: timestamps in the CSV
    /// are `epoch_ms + t_s × 1000`.
    pub epoch_ms: i64,
    pub start_s: f64,
    pub end_s: f64,
    pub floor_kw: f64,
    pub emergency: bool,
    /// Seconds after the start in which the devices may still settle.
    pub grace_s: f64,
    pub samples: Vec<Sample>,
    /// Samples beyond the kept maximum that were not stored.
    pub dropped: usize,
    /// Highest draw after the grace time, kW.
    pub max_after_grace_kw: Option<f64>,
    /// Time above floor + tolerance after the grace time, s.
    pub exceeded_s: f64,
    /// Time the draw could not be measured after the grace time, s.
    pub unverified_s: f64,
    pub verdict: Verdict,
    /// Digest of the report before this one ([`GENESIS`] for the first).
    pub previous_sha256: String,
    /// Digest of this report's CSV, every line but the last.
    pub sha256: String,
}

struct Open {
    start_s: f64,
    floor_kw: f64,
    emergency: bool,
    samples: Vec<Sample>,
    dropped: usize,
    last_kept_s: f64,
}

/// Opens a report when a dimming starts, closes it when the dimming ends.
pub struct Recorder {
    site: String,
    grace_s: f64,
    epoch_ms: i64,
    open: Option<Open>,
    seq: u32,
    last_sha256: String,
    finished: Vec<DimmingReport>,
}

impl Recorder {
    /// `site`: a name for the report header (site id, market location).
    /// `epoch_ms`: Unix time of the controller clock's zero, for timestamps.
    pub fn new(site: &str, grace_s: f64, epoch_ms: i64) -> Self {
        Recorder {
            site: site.into(),
            grace_s,
            epoch_ms,
            open: None,
            seq: 0,
            last_sha256: GENESIS.into(),
            finished: Vec::new(),
        }
    }

    /// Continues a chain after a restart: the next report gets `seq + 1`
    /// and names `last_sha256` as its predecessor.
    pub fn resume(&mut self, seq: u32, last_sha256: &str) {
        self.seq = seq;
        self.last_sha256 = last_sha256.into();
    }

    /// Unix time of the controller clock's zero, ms.
    pub fn set_epoch(&mut self, epoch_ms: i64) {
        self.epoch_ms = epoch_ms;
    }

    /// Number of reports so far and the last digest, for persisting.
    pub fn chain(&self) -> (u32, &str) {
        (self.seq, &self.last_sha256)
    }

    /// Feeds one control cycle. `dimmed`: a reduction is in force (the
    /// controller's mode, after refusals).
    pub fn observe(
        &mut self,
        t_s: f64,
        dimmed: bool,
        emergency: bool,
        floor_kw: f64,
        steuve_grid_kw: Option<f64>,
        budget_kw: Option<f64>,
    ) {
        match (&mut self.open, dimmed) {
            (None, true) => {
                self.open = Some(Open {
                    start_s: t_s,
                    floor_kw,
                    emergency,
                    samples: vec![Sample { t_s, steuve_grid_kw, budget_kw }],
                    dropped: 0,
                    last_kept_s: t_s,
                });
            }
            (Some(o), true) => {
                o.emergency |= emergency;
                // One sample per second is enough and bounds the memory.
                if t_s - o.last_kept_s >= 1.0 - 1e-9 {
                    if o.samples.len() < MAX_SAMPLES {
                        o.samples.push(Sample { t_s, steuve_grid_kw, budget_kw });
                    } else {
                        o.dropped += 1;
                    }
                    o.last_kept_s = t_s;
                }
            }
            (Some(_), false) => {
                let o = self.open.take().expect("checked above");
                let report = self.close(o, t_s);
                self.last_sha256 = report.sha256.clone();
                self.finished.push(report);
            }
            (None, false) => {}
        }
    }

    /// A dimming is being recorded right now.
    pub fn recording(&self) -> bool {
        self.open.is_some()
    }

    /// Reports finished and not yet taken, oldest first.
    pub fn reports(&self) -> &[DimmingReport] {
        &self.finished
    }

    /// Hands the finished reports over (the gateway writes them to disk and
    /// keeps memory flat).
    pub fn take_reports(&mut self) -> Vec<DimmingReport> {
        std::mem::take(&mut self.finished)
    }

    fn close(&mut self, o: Open, end_s: f64) -> DimmingReport {
        self.seq += 1;
        let settle_until = o.start_s + self.grace_s;
        let mut exceeded_s = 0.0;
        let mut unverified_s = 0.0;
        let mut max_after: Option<f64> = None;
        // Each sample stands for the time until the next one (or the end);
        // only the part after the grace time counts.
        for (k, s) in o.samples.iter().enumerate() {
            let next = o.samples.get(k + 1).map_or(end_s, |n| n.t_s);
            let span = next - s.t_s.max(settle_until);
            if span <= 0.0 {
                continue;
            }
            match s.steuve_grid_kw {
                Some(v) => {
                    max_after = Some(max_after.map_or(v, |m| m.max(v)));
                    if v > o.floor_kw + TOLERANCE_KW {
                        exceeded_s += span;
                    }
                }
                None => unverified_s += span,
            }
        }
        let verdict = if exceeded_s > 0.0 {
            Verdict::Exceeded
        } else if unverified_s > 0.0 || o.dropped > 0 {
            Verdict::Unverified
        } else {
            Verdict::Followed
        };
        let mut r = DimmingReport {
            seq: self.seq,
            site: self.site.clone(),
            epoch_ms: self.epoch_ms,
            start_s: o.start_s,
            end_s,
            floor_kw: o.floor_kw,
            emergency: o.emergency,
            grace_s: self.grace_s,
            samples: o.samples,
            dropped: o.dropped,
            max_after_grace_kw: max_after,
            exceeded_s,
            unverified_s,
            verdict,
            previous_sha256: self.last_sha256.clone(),
            sha256: String::new(),
        };
        r.sha256 = sha256_hex(r.body().as_bytes());
        r
    }
}

impl DimmingReport {
    pub fn duration_s(&self) -> f64 {
        self.end_s - self.start_s
    }

    /// Time of a controller-clock instant, ISO 8601 UTC.
    pub fn time(&self, t_s: f64) -> String {
        iso8601_utc(self.epoch_ms + (t_s * 1000.0).round() as i64)
    }

    /// A file name that sorts by time: `dimming-0003-2025-01-20T16-30-00Z.csv`.
    pub fn file_name(&self) -> String {
        format!("dimming-{:04}-{}.csv", self.seq, self.time(self.start_s).replace(':', "-"))
    }

    /// The report as CSV: `#` header lines, one row per second, and a last
    /// line with the SHA-256 of everything above it.
    pub fn to_csv(&self) -> String {
        let mut s = self.body();
        s.push_str(&format!("# sha256: {}\n", self.sha256));
        s
    }

    fn body(&self) -> String {
        let mut s = String::new();
        let mut line = |l: String| {
            s.push_str(&l);
            s.push('\n');
        };
        line("# Consumption reduction report (BK6-22-300 Anlage 1, 7.2)".into());
        line(format!("# site: {}", self.site));
        line(format!("# report: {}", self.seq));
        line(format!("# start: {}", self.time(self.start_s)));
        line(format!("# end: {}", self.time(self.end_s)));
        line(format!("# duration_s: {:.0}", self.duration_s()));
        line(format!("# floor_kw: {:.2}", self.floor_kw));
        line(format!("# tolerance_kw: {TOLERANCE_KW:.2}"));
        line(format!("# grace_s: {:.0}", self.grace_s));
        line(format!("# emergency: {}", self.emergency));
        line(format!(
            "# max_after_grace_kw: {}",
            self.max_after_grace_kw.map_or("unknown".into(), |v| format!("{v:.2}"))
        ));
        line(format!("# exceeded_s: {:.0}", self.exceeded_s));
        line(format!("# unverified_s: {:.0}", self.unverified_s));
        line(format!("# samples_dropped: {}", self.dropped));
        line(format!("# verdict: {}", self.verdict.name()));
        line(format!("# previous_sha256: {}", self.previous_sha256));
        line("time,elapsed_s,steuve_grid_kw,floor_kw,granted_kw,within_floor".into());
        for x in &self.samples {
            let within = match x.steuve_grid_kw {
                Some(v) => (v <= self.floor_kw + TOLERANCE_KW).to_string(),
                None => "unknown".into(),
            };
            line(format!(
                "{},{:.0},{},{:.2},{},{}",
                self.time(x.t_s),
                x.t_s - self.start_s,
                x.steuve_grid_kw.map_or(String::new(), |v| format!("{v:.2}")),
                self.floor_kw,
                x.budget_kw.map_or(String::new(), |v| format!("{v:.2}")),
                within,
            ));
        }
        s
    }

    /// Recomputes the digest of a CSV written by [`DimmingReport::to_csv`]
    /// and compares it with the digest on its last line.
    pub fn verify_csv(csv: &str) -> bool {
        let Some(pos) = csv.trim_end_matches('\n').rfind('\n') else { return false };
        let (body, last) = csv.split_at(pos + 1);
        let Some(claimed) = last.trim().strip_prefix("# sha256: ") else { return false };
        sha256_hex(body.as_bytes()) == claimed
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// UTC timestamp, ISO 8601 with seconds, from Unix milliseconds.
pub fn iso8601_utc(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", sod / 3600, sod % 3600 / 60, sod % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(values: &[(f64, Option<f64>)], floor: f64) -> DimmingReport {
        let mut r = Recorder::new("test", 60.0, 0);
        r.observe(0.0, false, false, floor, Some(40.0), None);
        for &(t, v) in values {
            r.observe(t, true, false, floor, v, Some(floor));
        }
        let end = values.last().unwrap().0 + 1.0;
        r.observe(end, false, false, floor, Some(40.0), None);
        r.reports()[0].clone()
    }

    #[test]
    fn a_reduction_within_the_floor_is_followed() {
        let v: Vec<_> = (0..600).map(|k| (100.0 + k as f64, Some(if k < 30 { 60.0 } else { 18.0 }))).collect();
        let r = run(&v, 18.2);
        assert_eq!(r.verdict, Verdict::Followed, "the first 60 s may settle");
        assert_eq!(r.exceeded_s, 0.0);
        assert_eq!(r.samples.len(), 600);
        assert!((r.duration_s() - 600.0).abs() < 1e-9);
    }

    #[test]
    fn draw_above_the_floor_after_the_grace_time_is_counted() {
        let v: Vec<_> = (0..600).map(|k| (k as f64, Some(if (200..230).contains(&k) { 25.0 } else { 18.0 }))).collect();
        let r = run(&v, 18.2);
        assert_eq!(r.verdict, Verdict::Exceeded);
        assert!((r.exceeded_s - 30.0).abs() < 1e-9, "{}", r.exceeded_s);
        assert_eq!(r.max_after_grace_kw, Some(25.0));
    }

    #[test]
    fn a_blind_meter_makes_the_event_unverified() {
        let v: Vec<_> = (0..300).map(|k| (k as f64, if (100..150).contains(&k) { None } else { Some(10.0) })).collect();
        let r = run(&v, 18.2);
        assert_eq!(r.verdict, Verdict::Unverified);
        assert!((r.unverified_s - 50.0).abs() < 1e-9);
    }

    #[test]
    fn reports_are_chained_and_tamper_evident() {
        let mut rec = Recorder::new("depot", 60.0, 1_737_327_600_000);
        for event in 0..2 {
            let t0 = event as f64 * 10_000.0;
            for k in 0..120 {
                rec.observe(t0 + k as f64, true, false, 18.2, Some(15.0), Some(18.0));
            }
            rec.observe(t0 + 120.0, false, false, 18.2, Some(40.0), None);
        }
        let reps = rec.reports();
        assert_eq!(reps.len(), 2);
        assert_eq!(reps[0].previous_sha256, GENESIS);
        assert_eq!(reps[1].previous_sha256, reps[0].sha256);
        assert_eq!((reps[0].seq, reps[1].seq), (1, 2));
        assert_eq!(rec.chain(), (2, reps[1].sha256.as_str()));
        assert_eq!(reps[0].file_name(), "dimming-0001-2025-01-19T23-00-00Z.csv");

        let csv = reps[1].to_csv();
        assert!(csv.contains("# start: 2025-01-20T01:46:40Z"), "{csv}");
        assert!(csv.contains("\n2025-01-20T01:46:41Z,1,15.00,18.20,18.00,true\n"), "{csv}");
        assert!(DimmingReport::verify_csv(&csv));
        let forged = csv.replacen(",15.00,", ",12.00,", 1);
        assert_ne!(forged, csv);
        assert!(!DimmingReport::verify_csv(&forged), "an edited value breaks the digest");
    }

    #[test]
    fn samples_are_thinned_to_one_per_second() {
        let mut rec = Recorder::new("depot", 0.0, 0);
        for k in 0..100 {
            rec.observe(k as f64 * 0.25, true, false, 18.2, Some(10.0), None);
        }
        rec.observe(25.0, false, false, 18.2, None, None);
        assert_eq!(rec.reports()[0].samples.len(), 25);
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(1_737_327_600_000), "2025-01-19T23:00:00Z");
        assert_eq!(iso8601_utc(1_709_164_800_000), "2024-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_767_225_599_000), "2025-12-31T23:59:59Z");
    }
}
