//! A plan in force, and what it asks of the real-time layer right now.

use control::Guidance;
use planner::Plan;

/// A plan and the time axis it lives on: step `k` covers
/// `[start_s + k·step_s, start_s + (k+1)·step_s)`. Times are seconds on
/// whatever axis the caller uses (Unix time in the gateway, simulated time
/// in the closed loop).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRecord {
    pub made_at_s: f64,
    pub start_s: f64,
    pub step_s: f64,
    pub plan: Plan,
    /// For each charger, the index of its car in the plan.
    pub ev_of_charger: Vec<Option<usize>>,
}

impl PlanRecord {
    /// The step that covers `t_s`, if the plan does.
    pub fn step_at(&self, t_s: f64) -> Option<usize> {
        let x = (t_s - self.start_s) / self.step_s;
        (x >= 0.0 && x < self.plan.grid_kw.len() as f64).then(|| x.floor() as usize)
    }

    /// What the plan asks of the real-time layer at `t_s`: `None` when the
    /// plan is older than `max_age_s` or does not cover `t_s`, and the site
    /// runs on rules alone. `heat_pumps` is the site's number of heat pumps;
    /// the first one is the one the plan schedules.
    pub fn guidance_at(&self, t_s: f64, max_age_s: f64, heat_pumps: usize) -> Option<Guidance> {
        if !(0.0..max_age_s).contains(&(t_s - self.made_at_s)) {
            return None;
        }
        let k = self.step_at(t_s)?;
        let p = &self.plan;
        let mut heat_pump_kw = vec![None; heat_pumps];
        if let Some(first) = heat_pump_kw.first_mut() {
            *first = p.heat_pump_kw.get(k).copied();
        }
        Some(Guidance {
            grid_kw: Some(p.grid_kw[k]),
            charger_kw: self.ev_of_charger.iter().map(|e| e.and_then(|i| p.ev_kw.get(i).map(|v| v[k]))).collect(),
            heat_pump_kw,
            dim_expected: p.dim_budget_kw[k].is_some(),
        })
    }
}
