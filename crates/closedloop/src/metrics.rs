//! What a run is judged by, measured on the simulation's physical truth
//! (not on what the controller believes).

use devices::climate::Tariff;
use devices::sim::{Building, SiteSim};
use serde::Serialize;

use crate::runner::DEGRADATION_EUR_PER_KWH;

/// Tolerance for the dimming check (measurement noise, whole amps).
const DIM_TOLERANCE_KW: f64 = 0.5;
/// Time the devices get to follow a new limit before the check starts: a
/// setpoint goes out on the next control cycle and chargers and heat pumps
/// take a few seconds to settle on it.
const DIM_REACTION_S: f64 = 60.0;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Metrics {
    pub hours: f64,
    /// Energy bought minus energy sold, at day-ahead-indexed prices, €.
    pub energy_cost_eur: f64,
    pub import_kwh: f64,
    pub export_kwh: f64,
    pub peak_import_kw: f64,
    /// Highest quarter-hour mean import, kW: what a demand charge bills.
    pub peak_quarter_kw: f64,
    pub pv_available_kwh: f64,
    pub pv_produced_kwh: f64,
    pub battery_throughput_kwh: f64,
    /// Kelvin-hours below the comfort floor.
    pub discomfort_kh: f64,
    pub departures: u32,
    pub ev_requested_kwh: f64,
    pub ev_unmet_kwh: f64,
    pub cars_short: u32,
    /// Energy the controllable devices drew above the dimming limit, kWh (should be 0).
    pub dim_excess_kwh: f64,
    /// How long they did, and by how much at most (kW above the floor).
    pub dim_excess_s: f64,
    pub dim_excess_max_kw: f64,
    pub dimmed_hours: f64,
    #[serde(skip)]
    dimmed_for_s: f64,
    pub plans: u32,
    pub plan_failures: u32,
    pub solve_ms_mean: f64,
    pub solve_ms_max: f64,
    solve_ms_sum: f64,
    /// Quarter-hour being metered: (index, kWh imported).
    #[serde(skip)]
    quarter: Option<(i64, f64)>,
}

impl Metrics {
    pub fn accumulate(&mut self, sim: &SiteSim, dt_s: f64, dimmed: bool, floor_kw: f64) {
        let h = dt_s / 3600.0;
        let tariff = Tariff::default();
        let da = sim.climate.day_ahead_eur_mwh(sim.t_s);
        let grid = sim.grid_kw();
        self.hours += h;
        self.energy_cost_eur +=
            h * (tariff.import_eur_kwh(da) * grid.max(0.0) - tariff.export_eur_kwh(da) * (-grid).max(0.0));
        self.import_kwh += h * grid.max(0.0);
        self.export_kwh += h * (-grid).max(0.0);
        self.peak_import_kw = self.peak_import_kw.max(grid);
        let q = ((sim.t_s - dt_s / 2.0) / 900.0).floor() as i64;
        match &mut self.quarter {
            Some((i, kwh)) if *i == q => *kwh += h * grid.max(0.0),
            _ => {
                self.close_quarter();
                self.quarter = Some((q, h * grid.max(0.0)));
            }
        }
        self.pv_available_kwh += h * sim.pv_available_kw();
        self.pv_produced_kwh += h * sim.pv_kw();
        let battery = sim.batteries_kw();
        self.battery_throughput_kwh += h * battery.abs();
        self.discomfort_kh += h * (Building::comfort_min_c(sim.t_s) - sim.building.indoor_c).max(0.0);
        self.dimmed_for_s = if dimmed { self.dimmed_for_s + dt_s } else { 0.0 };
        if dimmed {
            self.dimmed_hours += h;
        }
        if dimmed && self.dimmed_for_s > DIM_REACTION_S {
            let steuve = sim.chargers_kw() + sim.heat_pumps_kw() + battery.max(0.0);
            let surplus = (sim.pv_kw() - sim.base_kw).max(0.0);
            let from_grid = (steuve - surplus - (-battery).max(0.0)).max(0.0);
            let excess = (from_grid - floor_kw - DIM_TOLERANCE_KW).max(0.0);
            if excess > 0.0 {
                self.dim_excess_kwh += h * excess;
                self.dim_excess_s += dt_s;
                self.dim_excess_max_kw = self.dim_excess_max_kw.max(from_grid - floor_kw);
            }
        }
    }

    fn close_quarter(&mut self) {
        if let Some((_, kwh)) = self.quarter.take() {
            self.peak_quarter_kw = self.peak_quarter_kw.max(kwh / 0.25);
        }
    }

    /// The demand charge the run carries on its peak, as a representative
    /// stretch of the billing period.
    pub fn demand_eur(&self) -> f64 {
        self.peak_quarter_kw * Tariff::default().demand_eur_per_kw(self.hours)
    }

    /// Energy, battery ageing and demand charge.
    pub fn total_eur(&self) -> f64 {
        self.energy_cost_eur + self.degradation_eur() + self.demand_eur()
    }

    pub fn record_plan(&mut self, solve_ms: f64) {
        self.plans += 1;
        self.solve_ms_sum += solve_ms;
        self.solve_ms_max = self.solve_ms_max.max(solve_ms);
        self.solve_ms_mean = self.solve_ms_sum / self.plans as f64;
    }

    pub fn finish(&mut self, sim: &SiteSim, start_s: f64) {
        self.close_quarter();
        for d in sim.departures.iter().filter(|d| d.at_s >= start_s) {
            self.departures += 1;
            self.ev_requested_kwh += d.needs_kwh;
            self.ev_unmet_kwh += d.unmet_kwh();
            if d.unmet_kwh() > 0.5 {
                self.cars_short += 1;
            }
        }
    }

    pub fn degradation_eur(&self) -> f64 {
        self.battery_throughput_kwh * DEGRADATION_EUR_PER_KWH
    }

    pub fn curtailed_kwh(&self) -> f64 {
        (self.pv_available_kwh - self.pv_produced_kwh).max(0.0)
    }
}
