//! State that must survive a restart: the DSO's commands (a dimming active
//! before a power cut is active after it) and the running totals (a CH
//! curtailment budget must not reset because the box rebooted).
//!
//! Written with write-then-rename, so a power cut never leaves a half file.

use std::path::Path;

use control::{DsoCommands, Totals};
use serde_json::{Value, json};
use tracing::warn;

pub struct Saved {
    pub commands: DsoCommands,
    pub totals: Option<Totals>,
}

pub fn load(path: &Path) -> Saved {
    let v = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let Some(v) = v else {
        return Saved { commands: DsoCommands::default(), totals: None };
    };
    let commands = DsoCommands {
        // "dim_14a" is how version 1 of the gateway called it.
        dim: v["dim"].as_bool().or_else(|| v["dim_14a"].as_bool()).unwrap_or(false),
        feed_in_limit_pct: v["feed_in_limit_pct"].as_f64().unwrap_or(100.0).clamp(0.0, 100.0),
        emergency: v["emergency"].as_bool().unwrap_or(false),
        limit_kw: None,
    };
    let t = &v["totals"];
    let totals = t.is_object().then(|| Totals {
        day: t["day"].as_i64().unwrap_or(0),
        year: t["year"].as_i64().unwrap_or(0) as i32,
        dimmed_s_today: t["dimmed_s_today"].as_f64().unwrap_or(0.0),
        produced_kwh_year: t["produced_kwh_year"].as_f64().unwrap_or(0.0),
        curtailed_kwh_year: t["curtailed_kwh_year"].as_f64().unwrap_or(0.0),
    });
    Saved { commands, totals }
}

pub fn save(path: &Path, c: &DsoCommands, t: &Totals) {
    let text = json!({
        "dim": c.dim,
        "feed_in_limit_pct": c.feed_in_limit_pct,
        "emergency": c.emergency,
        "totals": {
            "day": t.day,
            "year": t.year,
            "dimmed_s_today": t.dimmed_s_today,
            "produced_kwh_year": t.produced_kwh_year,
            "curtailed_kwh_year": t.curtailed_kwh_year,
        }
    })
    .to_string();
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("could not persist state: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_reads_version_1_files() {
        let dir = std::env::temp_dir().join(format!("gw-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("state.json");
        let c = DsoCommands { dim: true, feed_in_limit_pct: 30.0, emergency: true, limit_kw: None };
        let t = Totals { day: 3, year: 2026, dimmed_s_today: 60.0, produced_kwh_year: 12.5, curtailed_kwh_year: 0.5 };
        save(&p, &c, &t);
        let s = load(&p);
        assert_eq!(s.commands, c);
        assert_eq!(s.totals, Some(t));

        std::fs::write(&p, r#"{"dim_14a":true,"feed_in_limit_pct":60.0}"#).unwrap();
        let s = load(&p);
        assert!(s.commands.dim && !s.commands.emergency);
        assert_eq!(s.commands.feed_in_limit_pct, 60.0);
        assert_eq!(s.totals, None);

        std::fs::write(&p, "not json").unwrap();
        assert_eq!(load(&p).commands, DsoCommands::default());
    }
}
