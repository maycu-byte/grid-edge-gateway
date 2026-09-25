//! One consistent picture of the site after each control cycle. The IEC 104
//! station reports from it and the dashboard API serves it as JSON.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub time_ms: i64,
    /// "DE", "AT" or "CH".
    pub jurisdiction: &'static str,
    pub dso: DsoView,
    pub mode: &'static str,
    /// While releasing: seconds of random wait left before power returns.
    pub release_wait_s: Option<f64>,
    pub grid_kw: Option<f64>,
    pub pv_kw: Option<f64>,
    pub pv_available_kw: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub pv_surplus_kw: Option<f64>,
    pub steuve_kw: f64,
    pub steuve_grid_kw: Option<f64>,
    pub steuve_budget_kw: Option<f64>,
    /// Pmin,14a in DE, the contracted minimum elsewhere.
    pub floor_kw: f64,
    /// Feed-in limit after country rules (static caps, budgets), %.
    pub feed_in_limit_in_force_pct: f64,
    pub allowed_export_kw: f64,
    pub pv_limit_pct: f64,
    /// Limit sent to each inverter, % (differs from `pv_limit_pct` while
    /// one of them ignores its limit).
    pub inverter_limit_pct: Vec<f64>,
    pub inverters: Vec<InverterView>,
    pub chargers: Vec<ChargerView>,
    pub heat_pumps: Vec<HeatPumpView>,
    pub batteries: Vec<BatteryView>,
    pub totals: TotalsView,
    /// DSO commands not (fully) applied, and why.
    pub refusals: Vec<&'static str>,
    pub fallbacks: Vec<String>,
    /// Compliance reports written since the start, and the last verdict.
    pub reports_written: u32,
    pub last_report_verdict: Option<&'static str>,
    /// Duration of the last control cycle; a cycle longer than the period is
    /// logged as an overrun.
    pub cycle_ms: f64,
    /// The planning layer, when it is on.
    pub planner: Option<crate::ems::PlannerView>,
    /// The FNN control box's relay and EEBUS LPC, when configured.
    pub inputs: crate::inputs::InputState,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DsoView {
    pub dim: bool,
    pub feed_in_limit_pct: f64,
    pub emergency: bool,
    pub connections: usize,
    /// Consumption limit sent with the dimming (EEBUS LPC), kW.
    pub limit_kw: Option<f64>,
    /// Which sources demand the reduction: "iec104", "relay", "eebus".
    pub sources: Vec<&'static str>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InverterView {
    pub online: bool,
    pub kw: Option<f64>,
    pub rated_kw: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChargerView {
    pub online: bool,
    pub status: &'static str,
    pub current_a: Option<f64>,
    pub setpoint_a: f64,
    pub kw: Option<f64>,
    pub session_kwh: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HeatPumpView {
    pub online: bool,
    pub kw: Option<f64>,
    pub demand_kw: Option<f64>,
    pub limit_kw: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BatteryView {
    pub online: bool,
    pub status: &'static str,
    pub kw: Option<f64>,
    pub soc_pct: Option<f64>,
    pub setpoint_kw: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TotalsView {
    pub dimmed_min_today: f64,
    pub produced_kwh_year: f64,
    pub curtailed_kwh_year: f64,
    pub curtailment_budget_used_pct: Option<f64>,
}

/// A telecontrol frame for the dashboard's protocol log.
#[derive(Debug, Clone, Serialize)]
pub struct FrameLog {
    pub time_ms: i64,
    pub peer: String,
    pub dir: &'static str,
    pub hex: String,
    pub text: String,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
