//! The closed loop: simulated site ↔ register adapter ↔ real-time
//! controller, with an optional MPC planner on top that re-plans every 15
//! minutes and whenever a car arrives or leaves.

use control::{
    BatterySpec, ChargerSpec, Clock, ConsumptionRule, Controller, DsoCommands, FeedInReference, Guidance, HeatPumpSpec,
    Jurisdiction, Mode, Policy, Readings, Setpoints, SiteConfig, Status,
};
use devices::climate::{Climate, Season, Tariff};
use devices::sim::{Building, SimConfig, SiteSim, Weather};
use planner::{DemandCharge, Uncertainty};
use planning::{BuildingModel, PlanRecord, SiteModel, build_input};

use crate::adapter;
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
        let readings = adapter::read(&self.sim);
        self.forecaster.observe(t, readings.pv_available_kw);

        if let Strategy::Mpc { .. } = self.strategy {
            let plugged: Vec<bool> = readings.chargers.iter().map(|c| c.car_waiting).collect();
            let arrivals = plugged.iter().zip(&self.plugged).any(|(now, before)| *now && !before);
            self.plugged = plugged;
            if t >= self.next_plan_s || arrivals {
                self.replan(&readings);
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
        self.plan.as_ref()?.guidance_at(t, PLAN_MAX_AGE_S, self.sim.heat_pumps.len())
    }

    fn replan(&mut self, r: &Readings) {
        let Strategy::Mpc { uncertainty, dim_forecast, peak_aware } = self.strategy else { return };
        let t = self.sim.t_s;
        let h = self.forecaster.horizon(t, PLAN_STEPS, PLAN_STEP_H);
        let model = SiteModel {
            battery_efficiency: BATTERY_EFFICIENCY,
            degradation_eur_per_kwh: DEGRADATION_EUR_PER_KWH,
            building: Some(BuildingModel {
                ua_kw_per_k: self.sim.building.ua_kw_per_k,
                cap_kwh_per_k: self.sim.building.cap_kwh_per_k,
            }),
            departure_buffer_h: DEPARTURE_BUFFER_H,
            uncertainty,
            ..SiteModel::default()
        };
        // Where the planner expects a dimming: the DSO's announced windows
        // (if it knows them), and a dimming in progress for up to 2 hours.
        let dt_s = PLAN_STEP_H * 3600.0;
        let dim: Vec<bool> = (0..PLAN_STEPS)
            .map(|k| {
                let t_mid = t + (k as f64 + 0.5) * dt_s;
                (dim_forecast && self.scenario.dim_at(t_mid)) || (self.cmd.dim && t_mid < t + 2.0 * 3600.0)
            })
            .collect();
        // The plan stands for a day of the billing period (its horizon).
        let demand = peak_aware.then(|| DemandCharge {
            eur_per_kw: Tariff::default().demand_eur_per_kw(PLAN_STEPS as f64 * PLAN_STEP_H),
            peak_so_far_kw: self.metrics.peak_quarter_kw,
        });
        let cfg = self.ctl.config();
        let (input, ev_of_charger) = build_input(cfg, self.ctl.floor_kw(), &model, &h, r, dim, demand);
        match planner::plan(&input) {
            Ok(plan) => {
                self.metrics.record_plan(plan.solve_ms);
                self.plan = Some(PlanRecord { made_at_s: t, start_s: t, step_s: dt_s, plan, ev_of_charger });
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
