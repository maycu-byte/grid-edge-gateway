//! What the gateway learns and meters for the planner: the site's usual
//! uncontrolled load, and the highest quarter-hour of import in the billing
//! period (what a demand charge is billed on).

use crate::localtime::{cet_offset_s, civil_from_unix, local_seconds_of_day};

const QUARTER_S: f64 = 900.0;
const SLOTS: usize = 96;

/// Index of the quarter-hour containing `unix_s` (quarters of UTC and of
/// Central European time coincide).
fn quarter_of(unix_s: f64) -> i64 {
    (unix_s / QUARTER_S).floor() as i64
}

/// The site's usual uncontrolled load for each quarter-hour of the local
/// day, separately for working days and weekends, learned online. Each
/// finished quarter-hour moves its slot towards the measured mean by
/// `alpha` (an exponential average over days); the misses give the
/// forecast's standard deviation.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseLoadProfile {
    alpha: f64,
    mean: Vec<Option<f64>>,
    miss_sq: Option<f64>,
    last_quarter_kw: Option<f64>,
    /// Quarter being measured: (index, slot, kWh, seconds covered).
    current: Option<(i64, usize, f64, f64)>,
}

fn slot(unix_s: f64) -> usize {
    let weekend = civil_from_unix(unix_s + cet_offset_s(unix_s)).weekday >= 5;
    usize::from(weekend) * SLOTS + ((local_seconds_of_day(unix_s) / QUARTER_S) as usize).min(SLOTS - 1)
}

impl BaseLoadProfile {
    pub fn new(alpha: f64) -> Self {
        BaseLoadProfile { alpha, mean: vec![None; 2 * SLOTS], miss_sq: None, last_quarter_kw: None, current: None }
    }

    /// Continues from a saved profile (192 slots: working days, then weekends).
    pub fn restore(alpha: f64, mean: Vec<Option<f64>>, miss_sq: Option<f64>) -> Self {
        let mut p = BaseLoadProfile::new(alpha);
        if mean.len() == 2 * SLOTS {
            p.mean = mean;
        }
        p.miss_sq = miss_sq.filter(|v| v.is_finite() && *v >= 0.0);
        p
    }

    pub fn means(&self) -> &[Option<f64>] {
        &self.mean
    }

    pub fn miss_sq(&self) -> Option<f64> {
        self.miss_sq
    }

    /// Adds `kw` measured over the `dt_s` seconds ending at `unix_s`.
    pub fn observe(&mut self, unix_s: f64, dt_s: f64, kw: f64) {
        if !kw.is_finite() || dt_s <= 0.0 {
            return;
        }
        let q = quarter_of(unix_s - dt_s / 2.0);
        match &mut self.current {
            Some((i, _, kwh, secs)) if *i == q => {
                *kwh += kw * dt_s / 3600.0;
                *secs += dt_s;
            }
            _ => {
                self.close();
                self.current = Some((q, slot(q as f64 * QUARTER_S), kw * dt_s / 3600.0, dt_s));
            }
        }
    }

    fn close(&mut self) {
        let Some((_, s, kwh, secs)) = self.current.take() else { return };
        if secs < QUARTER_S / 2.0 {
            return; // too little of the quarter seen to learn from
        }
        let x = kwh / (secs / 3600.0);
        self.last_quarter_kw = Some(x);
        let m = &mut self.mean[s];
        match *m {
            Some(old) => {
                let miss = x - old;
                self.miss_sq = Some(match self.miss_sq {
                    Some(v) => v + self.alpha * (miss * miss - v),
                    None => miss * miss,
                });
                *m = Some(old + self.alpha * miss);
            }
            None => *m = Some(x),
        }
    }

    /// Expected base load in the quarter-hour containing `unix_s`: its
    /// learned slot, else the same time on the other kind of day, else the
    /// last quarter measured.
    pub fn expected(&self, unix_s: f64) -> Option<f64> {
        let s = slot(unix_s);
        let other = (s + SLOTS) % (2 * SLOTS);
        self.mean[s].or(self.mean[other]).or(self.last_quarter_kw)
    }

    /// Standard deviation of the forecast's misses, kW, once there are any.
    pub fn sigma_kw(&self) -> Option<f64> {
        self.miss_sq.map(f64::sqrt)
    }
}

/// Meters grid import by quarter-hour, as a billing meter does, and keeps
/// the highest quarter of the billing period.
#[derive(Debug, Clone, PartialEq)]
pub struct QuarterPeak {
    /// Billing period the peak belongs to (any key: a year, a month).
    pub period: i64,
    pub peak_kw: f64,
    /// Quarter being metered: (index, kWh imported).
    current: Option<(i64, f64)>,
}

impl QuarterPeak {
    pub fn new(period: i64, peak_kw: f64) -> Self {
        QuarterPeak { period, peak_kw: peak_kw.max(0.0), current: None }
    }

    /// Adds the import measured over the `dt_s` seconds ending at `unix_s`;
    /// `period` is the billing period at that time. A new period starts
    /// from zero once the quarter that ended the old one is counted.
    pub fn observe(&mut self, unix_s: f64, dt_s: f64, grid_kw: f64, period: i64) {
        if !grid_kw.is_finite() || dt_s <= 0.0 {
            return;
        }
        let q = quarter_of(unix_s - dt_s / 2.0);
        let kwh = grid_kw.max(0.0) * dt_s / 3600.0;
        match &mut self.current {
            Some((i, e)) if *i == q => *e += kwh,
            _ => {
                if let Some((_, e)) = self.current.take() {
                    self.peak_kw = self.peak_kw.max(e / (QUARTER_S / 3600.0));
                }
                if period != self.period {
                    self.period = period;
                    self.peak_kw = 0.0;
                }
                self.current = Some((q, kwh));
            }
        }
    }

    /// Mean import of the quarter so far, as if it ended now, kW.
    pub fn running_kw(&self) -> Option<f64> {
        self.current.map(|(_, e)| e / (QUARTER_S / 3600.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 20 January 2025, 00:00 UTC (01:00 in Germany).
    const MONDAY: f64 = 1_737_331_200.0;

    #[test]
    fn profile_learns_each_quarter_hour_and_its_misses() {
        let mut p = BaseLoadProfile::new(0.5);
        assert_eq!(p.expected(MONDAY), None);
        // one day: 10 kW at night, 30 kW from 07:00 to 18:00 local
        for day in 0..3 {
            for s in (0..86_400).step_by(10) {
                let t = MONDAY + (day * 86_400 + s) as f64;
                let h = local_seconds_of_day(t) / 3600.0;
                let kw = if (7.0..18.0).contains(&h) { 30.0 } else { 10.0 } + if day == 2 { 2.0 } else { 0.0 };
                p.observe(t, 10.0, kw);
            }
        }
        let noon = MONDAY + 3.0 * 86_400.0 + 11.0 * 3600.0; // 12:00 local, Thursday
        assert!((p.expected(noon).unwrap() - 31.0).abs() < 0.01, "{:?}", p.expected(noon));
        let night = MONDAY + 3.0 * 86_400.0 + 2.0 * 3600.0;
        assert!((p.expected(night).unwrap() - 11.0).abs() < 0.01);
        // day 3 missed every quarter by 2 kW
        assert!((p.sigma_kw().unwrap() - 2.0).abs() < 0.3, "{:?}", p.sigma_kw());
        // Saturday has not been seen: the working-day slot stands in
        let saturday_noon = noon + 2.0 * 86_400.0;
        assert_eq!(p.expected(saturday_noon), p.expected(noon));
        // 23:30 UTC on a Friday is already Saturday in Germany
        assert_eq!(slot(MONDAY + 4.0 * 86_400.0 + 23.5 * 3600.0), SLOTS + 2);
        assert_eq!(slot(MONDAY + 4.0 * 86_400.0 + 22.5 * 3600.0), 94, "Friday 23:30 local");
    }

    #[test]
    fn profile_ignores_quarters_it_barely_saw() {
        let mut p = BaseLoadProfile::new(0.5);
        p.observe(MONDAY + 890.0, 10.0, 50.0);
        p.observe(MONDAY + 1800.0 + 5.0, 10.0, 10.0);
        assert_eq!(p.expected(MONDAY), None);
    }

    #[test]
    fn peak_is_the_highest_quarter_mean_and_resets_with_the_period() {
        let mut m = QuarterPeak::new(2025, 0.0);
        // 5 minutes at 90 kW inside a quarter otherwise at 30 kW: 50 kW mean
        for s in (0..900).step_by(5) {
            let kw = if s < 300 { 90.0 } else { 30.0 };
            m.observe(MONDAY + s as f64 + 5.0, 5.0, kw, 2025);
        }
        m.observe(MONDAY + 905.0, 5.0, 10.0, 2025);
        assert!((m.peak_kw - 50.0).abs() < 1e-6, "{}", m.peak_kw);
        // export does not count
        for s in (905..1800).step_by(5) {
            m.observe(MONDAY + s as f64 + 5.0, 5.0, -80.0, 2025);
        }
        m.observe(MONDAY + 1805.0, 5.0, 0.0, 2025);
        assert!((m.peak_kw - 50.0).abs() < 1e-6);
        // a new year starts from zero
        m.observe(MONDAY + 2705.0, 5.0, 20.0, 2026);
        assert_eq!(m.peak_kw, 0.0);
        assert_eq!(m.period, 2026);
    }
}
