//! Running totals the rules depend on: how long consumption was dimmed
//! today (contract day limits, DE's 2-hour cap on preventive control) and how
//! much PV energy was produced and curtailed this year (the CH 3% budget).
//!
//! The gateway persists [`Totals`] so a restart does not reset a budget.

/// Wall-clock context for one control cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// Monotonic seconds; only differences matter.
    pub t_s: f64,
    /// Local calendar day (any running day number).
    pub day: i64,
    /// Calendar year, for annual budgets.
    pub year: i32,
}

impl Clock {
    /// A clock for tests and the simulation: day 0 of `year`, `t_s` seconds in.
    pub fn at(t_s: f64, year: i32) -> Self {
        Clock { t_s, day: (t_s / 86_400.0).floor() as i64, year }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Totals {
    pub day: i64,
    pub year: i32,
    pub dimmed_s_today: f64,
    pub produced_kwh_year: f64,
    pub curtailed_kwh_year: f64,
}

/// Longest gap integrated at once: after a pause (a stalled loop, a laptop
/// asleep) nothing is extrapolated over the gap.
const MAX_STEP_S: f64 = 10.0;

#[derive(Debug, Clone, Default)]
pub struct Accounting {
    totals: Totals,
    last_t: Option<f64>,
}

impl Accounting {
    pub fn restore(totals: Totals) -> Self {
        Accounting { totals, last_t: None }
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Starts a new day or year when the clock has moved on. Call before
    /// reading [`totals`](Self::totals) in a cycle.
    pub fn roll(&mut self, clock: &Clock) {
        let t = &mut self.totals;
        if t.year != clock.year {
            t.year = clock.year;
            t.produced_kwh_year = 0.0;
            t.curtailed_kwh_year = 0.0;
        }
        if t.day != clock.day {
            t.day = clock.day;
            t.dimmed_s_today = 0.0;
        }
    }

    pub fn advance(&mut self, clock: &Clock, dimmed: bool, pv_kw: Option<f64>, curtailed_kw: f64) {
        self.roll(clock);
        let t = &mut self.totals;
        let dt = self.last_t.map_or(0.0, |last| (clock.t_s - last).clamp(0.0, MAX_STEP_S));
        self.last_t = Some(clock.t_s);
        if dimmed {
            t.dimmed_s_today += dt;
        }
        t.produced_kwh_year += pv_kw.unwrap_or(0.0).max(0.0) * dt / 3600.0;
        t.curtailed_kwh_year += curtailed_kw.max(0.0) * dt / 3600.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_and_resets_by_day_and_year() {
        let mut a = Accounting::default();
        for s in 0..=3600 {
            a.advance(&Clock { t_s: s as f64, day: 5, year: 2026 }, true, Some(10.0), 1.0);
        }
        let t = a.totals();
        assert!((t.dimmed_s_today - 3600.0).abs() < 1e-6);
        assert!((t.produced_kwh_year - 10.0).abs() < 1e-6);
        assert!((t.curtailed_kwh_year - 1.0).abs() < 1e-6);

        a.advance(&Clock { t_s: 3601.0, day: 6, year: 2026 }, false, Some(10.0), 0.0);
        assert_eq!(a.totals().dimmed_s_today, 0.0);
        assert!(a.totals().produced_kwh_year > 10.0);
        a.advance(&Clock { t_s: 3602.0, day: 7, year: 2027 }, false, Some(0.0), 0.0);
        assert_eq!(a.totals().produced_kwh_year, 0.0);
    }

    #[test]
    fn does_not_extrapolate_over_a_pause() {
        let mut a = Accounting::default();
        a.advance(&Clock { t_s: 0.0, day: 0, year: 2026 }, true, Some(100.0), 0.0);
        a.advance(&Clock { t_s: 7200.0, day: 0, year: 2026 }, true, Some(100.0), 0.0);
        assert_eq!(a.totals().dimmed_s_today, MAX_STEP_S);
    }
}
