//! The closed loop: simulated site ↔ register adapter ↔ real-time
//! controller, with an optional MPC planner on top that re-plans every 15
//! minutes and whenever a car arrives or leaves.

use control::rules::three_phase_kw;
use control::{
    BatterySpec, ChargerSpec, Clock, ConsumptionRule, Controller, DsoCommands, FeedInReference, Guidance, HeatPumpSpec,
    Jurisdiction, Mode, Policy, Readings, Setpoints, SiteConfig, Status,
};
use devices::climate::{Climate, Season, Tariff};
use devices::sim::{Building, SimConfig, SiteSim, Weather};
use planner::{BatteryModel, DemandCharge, EvRequest, HeatPumpModel, Plan, PlanInput, Uncertainty, Weights};

use crate::adapter::{self, Details};
use crate::forecast::Forecaster;
use crate::metrics::Metrics;

pub const PLAN_STEP_H: f64 = 0.25;
pub const PLAN_STEPS: usize = 96;
const REPLAN_S: f64 = 900.0;
/// A plan older than this is not followed any more.
const PLAN_MAX_AGE_S: f64 = 3600.0;
/// Battery ageing as a throughput cost: ~€300/kWh, 6,000 cycles at 80% DoD
/// → 300 / (2 · 6000 · 0.8) ≈ €0.03 per kWh in or out. Illustrative.
pub const DEGRADATION_EUR_PER_KWH: f64 = 0.03;
const BATTERY_EFFICIENCY: f64 = 0.95;
/// Plans aim to finish charging this long before a car leaves: a margin
/// against the small losses of tracking a plan with whole amps.
const DEPARTURE_BUFFER_H: f64 = 0.25;
const YEAR: i32 = 2026;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    /// The real-time controller alone: self-consumption battery, cars at
    /// full power, the heat pump's own thermostat.
    Rules,
    /// MPC planner on top of the real-time controller. `dim_forecast`: it
    /// knows the DSO's announced dimming windows; `peak_aware`: its
    /// objective includes the demand charge on the highest quarter-hour.
    Mpc { uncertainty: Uncertainty, dim_forecast: bool, peak_aware: bool },
}

impl Strategy {
    pub fn name(&self) -> &'static str {
        match self {
            Strategy::Rules => "rules",
            Strategy::Mpc { peak_aware: false, .. } => "mpc-no-peak",
            Strategy::Mpc { dim_forecast: false, .. } => "mpc-blind",
            Strategy::Mpc { uncertainty: Uncertainty::Deterministic, .. } => "mpc",
            Strategy::Mpc { uncertainty: Uncertainty::Chance { .. }, .. } => "mpc-cc",
            Strategy::Mpc { uncertainty: Uncertainty::Robust, .. } => "mpc-robust",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mpc = |uncertainty, dim_forecast, peak_aware| Strategy::Mpc { uncertainty, dim_forecast, peak_aware };
        Some(match s {
            "rules" => Strategy::Rules,
            "mpc" => mpc(Uncertainty::Deterministic, true, true),
            "mpc-cc" => mpc(Uncertainty::Chance { epsilon: 0.05 }, true, true),
            "mpc-robust" => mpc(Uncertainty::Robust, true, true),
            "mpc-blind" => mpc(Uncertainty::Deterministic, false, true),
            "mpc-no-peak" => mpc(Uncertainty::Deterministic, true, false),
            _ => return None,
        })
    }

    pub fn study_set() -> Vec<Strategy> {
        ["rules", "mpc-no-peak", "mpc-blind", "mpc", "mpc-cc", "mpc-robust"]
            .iter()
            .filter_map(|s| Strategy::parse(s))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub season: Season,
    pub jurisdiction: Jurisdiction,
    pub weather: Weather,
    pub seed: u64,
    pub start_h: f64,
    /// Daily windows (local hours) in which the DSO dims consumption.
    pub dim_windows_h: Vec<(f64, f64)>,
    /// The scenario's DSO acts by itself (the study); off when a person
    /// plays the DSO (the demo).
    pub auto_dso: bool,
}

impl Scenario {
    /// The study's day: a German depot, preventive §14a control from 17:30
    /// to 19:30 (preventive control is capped at 2 h a day, BK6-22-300 10.5).
    pub fn study(season: Season, seed: u64) -> Self {
        Scenario {
            season,
            jurisdiction: Jurisdiction::De,
            weather: Weather::Random,
            seed,
            start_h: 6.0,
            dim_windows_h: vec![(17.5, 19.5)],
            auto_dso: true,
        }
    }

    pub fn dim_at(&self, t_s: f64) -> bool {
        let h = t_s.rem_euclid(86_400.0) / 3600.0;
        self.dim_windows_h.iter().any(|&(a, b)| h >= a && h < b)
    }
}

/// The depot under a country's rules, as the example configurations set it up.
pub fn site_config(j: Jurisdiction) -> SiteConfig {
    let mut policy = Policy::for_country(j);
    let mut hp_opted_out = false;
    match j {
        Jurisdiction::De => {}
        Jurisdiction::At => {
            policy.consumption = ConsumptionRule::Contract { min_kw: 10.0, max_minutes_per_day: 120.0 };
        }
        Jurisdiction::Ch => {
            policy.consumption = ConsumptionRule::Contract { min_kw: 8.0, max_minutes_per_day: 180.0 };
            hp_opted_out = true;
        }
    }
    SiteConfig {
        policy,
        pv_installed_kw: 120.0,
        connection_kw: 250.0,
        expected_annual_yield_kwh: if j == Jurisdiction::Ch { 114_000.0 } else { 120_000.0 },
        chargers: vec![ChargerSpec { max_current_a: 32.0, failsafe_current_a: 6.0, opted_out: false }; 4],
        heat_pumps: vec![HeatPumpSpec { rated_kw: 14.0, min_kw: 3.0, opted_out: hp_opted_out }],
        batteries: vec![BatterySpec {
            capacity_kwh: 100.0,
            max_charge_kw: 50.0,
            max_discharge_kw: 50.0,
            min_soc_pct: 10.0,
            max_soc_pct: 95.0,
        }],
        feed_in_reference: FeedInReference::GridConnectionPoint,
        release_ramp_s: 300.0,
        pv_ramp_pct_per_s: 10.0 / 60.0,
        margin_kw: 0.3,
        min_dwell_s: 300.0,
        deadline_guard_s: 900.0,
        surplus_hold_s: 30.0,
        import_target_kw: 0.0,
    }
}

/// A plan and when it was made; `ev_of_charger[i]` is the plan's index of
/// the car at charger `i`.
#[derive(Debug, Clone)]
pub struct PlanRecord {
    pub made_at_s: f64,
    pub plan: Plan,
    pub ev_of_charger: Vec<Option<usize>>,
}

pub struct ClosedLoop {
    pub sim: SiteSim,
    pub ctl: Controller,
    pub cmd: DsoCommands,
    pub scenario: Scenario,
    pub strategy: Strategy,
    pub forecaster: Forecaster,
    pub plan: Option<PlanRecord>,
    pub setpoints: Setpoints,
    pub status: Option<Status>,
    pub readings: Readings,
    pub metrics: Metrics,
    pub control_dt_s: f64,
    since_control: f64,
    next_plan_s: f64,
    plugged: Vec<bool>,
}

impl ClosedLoop {
    pub fn new(scenario: Scenario, strategy: Strategy, control_dt_s: f64) -> Self {
        let start_s = scenario.start_h * 3600.0;
        let mut sim = SiteSim::new(SimConfig::new(start_s, scenario.seed, scenario.season, scenario.weather));
        adapter::arm_watchdogs(&mut sim);
        let cfg = site_config(scenario.jurisdiction);
        let installed = cfg.pv_installed_kw;
        let n = sim.chargers.len();
        let mut me = ClosedLoop {
            forecaster: Forecaster::new(Climate::of(scenario.season), Tariff::default(), installed),
            ctl: Controller::new(cfg),
            cmd: DsoCommands::default(),
            sim,
            scenario,
            strategy,
            plan: None,
            setpoints: Setpoints {
                pv_limit_pct: 100.0,
                charger_current_a: vec![],
                heat_pump_limit_kw: vec![],
                battery_kw: vec![],
                heat_pump_ext_kw: vec![],
            },
            status: None,
            readings: Readings::default(),
            metrics: Metrics::default(),
            control_dt_s,
            since_control: 0.0,
            next_plan_s: f64::NEG_INFINITY,
            plugged: vec![false; n],
        };
        me.control_cycle();
        me
    }

    pub fn t_s(&self) -> f64 {
        self.sim.t_s
    }

    /// Advances simulated time; physics in steps of at most the control period.
    pub fn advance(&mut self, seconds: f64) {
        let mut left = seconds;
        while left > 1e-9 {
            let dt = left.min(self.control_dt_s - self.since_control).max(1e-6);
            self.sim.step(dt);
            let dimmed = self.status.as_ref().is_some_and(|s| s.mode == Mode::Dimmed);
            self.metrics.accumulate(&self.sim, dt, dimmed, self.ctl.floor_kw());
            self.since_control += dt;
            left -= dt;
            if self.since_control >= self.control_dt_s - 1e-9 {
                self.since_control = 0.0;
                self.control_cycle();
            }
        }
    }

    /// Forces a new plan at the next control cycle (after a DSO command).
    pub fn replan_soon(&mut self) {
        self.next_plan_s = f64::NEG_INFINITY;
    }

    fn control_cycle(&mut self) {
        let t = self.sim.t_s;
        if self.scenario.auto_dso {
            self.cmd.dim = self.scenario.dim_at(t);
        }
        let (readings, details) = adapter::read(&self.sim);
        self.forecaster.observe(t, readings.pv_available_kw);

        if let Strategy::Mpc { .. } = self.strategy {
            let plugged: Vec<bool> = readings.chargers.iter().map(|c| c.car_waiting).collect();
            let arrivals = plugged.iter().zip(&self.plugged).any(|(now, before)| *now && !before);
            self.plugged = plugged;
            if t >= self.next_plan_s || arrivals {
                self.replan(&readings, &details);
                self.next_plan_s = ((t / REPLAN_S).floor() + 1.0) * REPLAN_S;
            }
        }
        let guidance = self.guidance_at(t);
        self.ctl.set_guidance(guidance);

        let clock = Clock::at(t, YEAR);
        let (sp, st) = self.ctl.step(&clock, &self.cmd, &readings);
        adapter::write(&mut self.sim, &sp);
        self.setpoints = sp;
        self.status = Some(st);
        self.readings = readings;
    }

    fn guidance_at(&self, t: f64) -> Option<Guidance> {
        let rec = self.plan.as_ref()?;
        let age = t - rec.made_at_s;
        if !(0.0..PLAN_MAX_AGE_S).contains(&age) {
            return None;
        }
        let k = (age / (PLAN_STEP_H * 3600.0)).floor() as usize;
        let p = &rec.plan;
        if k >= p.grid_kw.len() {
            return None;
        }
        Some(Guidance {
            grid_kw: Some(p.grid_kw[k]),
            charger_kw: rec.ev_of_charger.iter().map(|e| e.map(|i| p.ev_kw[i][k])).collect(),
            heat_pump_kw: vec![p.heat_pump_kw.get(k).copied()],
            dim_expected: p.dim_budget_kw[k].is_some(),
        })
    }

    fn replan(&mut self, r: &Readings, details: &Details) {
        let Strategy::Mpc { uncertainty, dim_forecast, peak_aware } = self.strategy else { return };
        let t = self.sim.t_s;
        let cfg = self.ctl.config().clone();
        let h = self.forecaster.horizon(t, PLAN_STEPS, PLAN_STEP_H);
        let contract = matches!(cfg.policy.consumption, ConsumptionRule::Contract { .. });

        let battery = cfg.batteries.first().zip(r.batteries.first().filter(|b| b.online)).map(|(spec, b)| {
            let cap = spec.capacity_kwh;
            BatteryModel {
                energy_kwh: b.soc_pct / 100.0 * cap,
                capacity_kwh: cap,
                min_kwh: spec.min_soc_pct / 100.0 * cap,
                max_kwh: spec.max_soc_pct / 100.0 * cap,
                band_lo_kwh: 0.2 * cap,
                band_hi_kwh: 0.8 * cap,
                charge_kw: spec.max_charge_kw,
                discharge_kw: spec.max_discharge_kw,
                eta_charge: BATTERY_EFFICIENCY,
                eta_discharge: BATTERY_EFFICIENCY,
                degradation_eur_per_kwh: DEGRADATION_EUR_PER_KWH,
                dimmable: true,
            }
        });

        let mut evs = Vec::new();
        let mut ev_of_charger = vec![None; cfg.chargers.len()];
        for (i, c) in r.chargers.iter().enumerate() {
            let remaining = c.remaining_kwh.unwrap_or(0.0);
            if !(c.online && c.car_waiting && remaining > 0.05) {
                continue;
            }
            let max_a = details.car_max_current_a.get(i).copied().flatten().unwrap_or(cfg.chargers[i].max_current_a);
            ev_of_charger[i] = Some(evs.len());
            evs.push(EvRequest {
                remaining_kwh: remaining,
                max_kw: three_phase_kw(max_a.min(cfg.chargers[i].max_current_a)),
                departure_h: c.departure_s.map(|s| (s / 3600.0 - DEPARTURE_BUFFER_H).max(PLAN_STEP_H)),
                efficiency: 1.0,
                dimmable: !(contract && cfg.chargers[i].opted_out),
            });
        }

        let heat_pump = cfg.heat_pumps.first().zip(r.heat_pumps.first().filter(|x| x.online)).and_then(|(spec, x)| {
            Some(HeatPumpModel {
                indoor_c: x.indoor_c?,
                ua_kw_per_k: self.sim.building.ua_kw_per_k,
                cap_kwh_per_k: self.sim.building.cap_kwh_per_k,
                max_kw: spec.rated_kw,
                cop: h.cop.clone(),
                gains_kw: h.gains_kw.clone(),
                outdoor_c: h.outdoor_c.clone(),
                t_min_c: h.t_min_c.clone(),
                t_max_c: h.t_max_c.clone(),
                dimmable: !(contract && spec.opted_out),
            })
        });

        // Where the planner expects a dimming: the DSO's announced windows
        // (if it knows them), and a dimming in progress for up to 2 hours.
        let dt_s = PLAN_STEP_H * 3600.0;
        let dim: Vec<bool> = (0..PLAN_STEPS)
            .map(|k| {
                let t_mid = t + (k as f64 + 0.5) * dt_s;
                (dim_forecast && self.scenario.dim_at(t_mid)) || (self.cmd.dim && t_mid < t + 2.0 * 3600.0)
            })
            .collect();
        let cap = cfg.policy.static_feed_in_cap_pct.unwrap_or(100.0) / 100.0;

        let input = PlanInput {
            dt_h: PLAN_STEP_H,
            forecast: h.forecast.clone(),
            import_limit_kw: cfg.connection_kw,
            export_limit_kw: vec![cfg.pv_installed_kw * cap; PLAN_STEPS],
            dim,
            dim_floor_kw: self.ctl.floor_kw() - cfg.margin_kw,
            battery,
            evs,
            heat_pump,
            uncertainty,
            recovery_steps: 8,
            // The plan stands for a day of the billing period (its horizon).
            demand_charge: peak_aware.then(|| DemandCharge {
                eur_per_kw: Tariff::default().demand_eur_per_kw(PLAN_STEPS as f64 * PLAN_STEP_H),
                peak_so_far_kw: self.metrics.peak_quarter_kw,
            }),
            weights: Weights::default(),
        };
        match planner::plan(&input) {
            Ok(plan) => {
                self.metrics.record_plan(plan.solve_ms);
                self.plan = Some(PlanRecord { made_at_s: t, plan, ev_of_charger });
            }
            Err(_) => {
                self.metrics.plan_failures += 1;
                self.plan = None; // rules only until the next plan
            }
        }
    }

    /// Ends the run: counts the cars that left since the start.
    pub fn finish(&mut self) -> Metrics {
        let start = self.scenario.start_h * 3600.0;
        let mut m = self.metrics.clone();
        m.finish(&self.sim, start);
        m
    }
}

impl Scenario {
    /// Comfort floor at `t_s`, for callers that show it.
    pub fn comfort_min_c(t_s: f64) -> f64 {
        Building::comfort_min_c(t_s)
    }
}
