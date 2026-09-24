//! One consistent picture of the site after each control cycle. The IEC 104
//! station reports from it and the dashboard API serves it as JSON.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub time_ms: i64,
    pub dso: DsoView,
    pub mode: &'static str,
    pub grid_kw: Option<f64>,
    pub pv_kw: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub pv_surplus_kw: Option<f64>,
    pub steuve_kw: f64,
    pub steuve_grid_kw: Option<f64>,
    pub steuve_budget_kw: Option<f64>,
    pub pmin_kw: f64,
    pub allowed_export_kw: f64,
    pub pv_limit_pct: f64,
    pub inverters: Vec<InverterView>,
    pub chargers: Vec<ChargerView>,
    pub heat_pumps: Vec<HeatPumpView>,
    pub fallbacks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DsoView {
    pub dim_14a: bool,
    pub feed_in_limit_pct: f64,
    pub connections: usize,
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
