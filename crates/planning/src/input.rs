//! The planner's input for the site as the real-time layer sees it now.

use control::rules::three_phase_kw;
use control::{ConsumptionRule, Readings, SiteConfig};
use planner::{BatteryModel, DemandCharge, EvRequest, Forecast, HeatPumpModel, PlanInput, Uncertainty, Weights};

/// Everything time-dependent the planner needs for one horizon, one entry
/// per step. The building series are only used when the heat pump is planned.
#[derive(Debug, Clone, PartialEq)]
pub struct Horizon {
    pub dt_h: f64,
    pub forecast: Forecast,
    pub outdoor_c: Vec<f64>,
    pub cop: Vec<f64>,
    pub gains_kw: Vec<f64>,
    /// Comfort band at the end of each step, °C.
    pub t_min_c: Vec<f64>,
    pub t_max_c: Vec<f64>,
}

impl Horizon {
    pub fn steps(&self) -> usize {
        self.forecast.pv_kw.len()
    }
}

/// The building as the planner models it: one thermal mass behind one
/// heat-loss coefficient (first-order RC).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildingModel {
    pub ua_kw_per_k: f64,
    pub cap_kwh_per_k: f64,
}

/// What the planner needs to know about the site beyond the real-time
/// layer's configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteModel {
    /// One-way efficiency of the battery (charge and discharge alike).
    pub battery_efficiency: f64,
    /// Cycle ageing as a throughput cost, € per kWh in or out.
    pub degradation_eur_per_kwh: f64,
    /// State of charge the cells like, as fractions of capacity.
    pub soc_band: (f64, f64),
    /// `None`: the heat pump is not planned; its own thermostat decides and
    /// its draw belongs in the base-load forecast.
    pub building: Option<BuildingModel>,
    /// Plans aim to finish charging this long before a car leaves: a margin
    /// against the small losses of tracking a plan with whole amps.
    pub departure_buffer_h: f64,
    /// Steps it takes to make up a battery deviation by buying from the grid.
    pub recovery_steps: usize,
    pub uncertainty: Uncertainty,
    pub weights: Weights,
}

impl Default for SiteModel {
    fn default() -> Self {
        SiteModel {
            battery_efficiency: 0.95,
            degradation_eur_per_kwh: 0.03,
            soc_band: (0.2, 0.8),
            building: None,
            departure_buffer_h: 0.25,
            recovery_steps: 8,
            uncertainty: Uncertainty::Deterministic,
            weights: Weights::default(),
        }
    }
}

/// The planner's input for the site as it is now.
///
/// * `floor_kw`: what the controllable devices keep while dimmed (Pmin,14a
///   or the contract), as the real-time layer computed it.
/// * `dim`: steps in which a dimming is expected.
///
/// The site's batteries are planned as one; the first heat pump is planned
/// when the model has a building. Returns the input and, for each charger,
/// the index of its car in the plan.
pub fn build_input(
    cfg: &SiteConfig,
    floor_kw: f64,
    model: &SiteModel,
    h: &Horizon,
    r: &Readings,
    dim: Vec<bool>,
    demand_charge: Option<DemandCharge>,
) -> (PlanInput, Vec<Option<usize>>) {
    let n = h.steps();
    let contract = matches!(cfg.policy.consumption, ConsumptionRule::Contract { .. });

    let online: Vec<_> = cfg.batteries.iter().zip(&r.batteries).filter(|(_, b)| b.online).collect();
    let battery = (!online.is_empty()).then(|| {
        let sum = |f: &dyn Fn(&control::BatterySpec, &control::BatteryReading) -> f64| -> f64 {
            online.iter().map(|(s, b)| f(s, b)).sum()
        };
        let cap = sum(&|s, _| s.capacity_kwh);
        BatteryModel {
            energy_kwh: sum(&|s, b| b.soc_pct / 100.0 * s.capacity_kwh),
            capacity_kwh: cap,
            min_kwh: sum(&|s, _| s.min_soc_pct / 100.0 * s.capacity_kwh),
            max_kwh: sum(&|s, _| s.max_soc_pct / 100.0 * s.capacity_kwh),
            band_lo_kwh: model.soc_band.0 * cap,
            band_hi_kwh: model.soc_band.1 * cap,
            charge_kw: sum(&|s, _| s.max_charge_kw),
            discharge_kw: sum(&|s, _| s.max_discharge_kw),
            eta_charge: model.battery_efficiency,
            eta_discharge: model.battery_efficiency,
            degradation_eur_per_kwh: model.degradation_eur_per_kwh,
            dimmable: true,
        }
    });

    let mut evs = Vec::new();
    let mut ev_of_charger = vec![None; cfg.chargers.len()];
    for (i, (spec, c)) in cfg.chargers.iter().zip(&r.chargers).enumerate() {
        let remaining = c.remaining_kwh.unwrap_or(0.0);
        if !(c.online && c.car_waiting && remaining > 0.05) {
            continue;
        }
        let max_a = c.car_max_current_a.unwrap_or(spec.max_current_a).min(spec.max_current_a);
        ev_of_charger[i] = Some(evs.len());
        evs.push(EvRequest {
            remaining_kwh: remaining,
            max_kw: three_phase_kw(max_a),
            departure_h: c.departure_s.map(|s| (s / 3600.0 - model.departure_buffer_h).max(h.dt_h)),
            efficiency: 1.0,
            dimmable: !(contract && spec.opted_out),
        });
    }

    let heat_pump = model.building.and_then(|b| {
        let (spec, x) = cfg.heat_pumps.first().zip(r.heat_pumps.first().filter(|x| x.online))?;
        Some(HeatPumpModel {
            indoor_c: x.indoor_c?,
            ua_kw_per_k: b.ua_kw_per_k,
            cap_kwh_per_k: b.cap_kwh_per_k,
            max_kw: spec.rated_kw,
            cop: h.cop.clone(),
            gains_kw: h.gains_kw.clone(),
            outdoor_c: h.outdoor_c.clone(),
            t_min_c: h.t_min_c.clone(),
            t_max_c: h.t_max_c.clone(),
            dimmable: !(contract && spec.opted_out),
        })
    });

    let cap = cfg.policy.static_feed_in_cap_pct.unwrap_or(100.0) / 100.0;
    let input = PlanInput {
        dt_h: h.dt_h,
        forecast: h.forecast.clone(),
        import_limit_kw: cfg.connection_kw,
        export_limit_kw: vec![cfg.pv_installed_kw * cap; n],
        dim,
        dim_floor_kw: floor_kw - cfg.margin_kw,
        battery,
        evs,
        heat_pump,
        uncertainty: model.uncertainty,
        recovery_steps: model.recovery_steps,
        demand_charge,
        weights: model.weights,
    };
    (input, ev_of_charger)
}
