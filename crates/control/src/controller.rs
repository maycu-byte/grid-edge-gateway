//! The site controller: turns DSO commands and device readings into setpoints.
//!
//! It is a pure function of its inputs plus a little memory (ramp state,
//! rotation, running totals), so the gateway, the browser demo and the tests
//! all run exactly the same code. Country-specific limits come from a
//! [`Policy`]; the controller itself is country-agnostic.

use std::collections::VecDeque;

use crate::accounting::{Accounting, Clock, Totals};
use crate::compliance::{DimmingReport, Recorder};
use crate::policy::{ConsumptionRule, Policy};
use crate::rules::{EV_MIN_CURRENT_A, SteuVE, three_phase_current_a, three_phase_kw};

/// Where the DSO's feed-in limit is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedInReference {
    /// Limit the PV plant's output itself (conservative, simplest).
    PlantOutput,
    /// Limit what flows into the grid at the connection point; PV may
    /// produce more as long as the site consumes (or stores) the difference.
    GridConnectionPoint,
}

#[derive(Debug, Clone)]
pub struct ChargerSpec {
    pub max_current_a: f64,
    /// Current the charger falls back to on its own when the gateway stops
    /// talking to it (charger-side watchdog).
    pub failsafe_current_a: f64,
    /// The owner forbade the DSO to use this device (CH, existing
    /// flexibility; ignored where the law makes control mandatory).
    pub opted_out: bool,
}

#[derive(Debug, Clone)]
pub struct HeatPumpSpec {
    pub rated_kw: f64,
    /// Lowest power the compressor can modulate down to; below it, off.
    pub min_kw: f64,
    pub opted_out: bool,
}

#[derive(Debug, Clone)]
pub struct BatterySpec {
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    /// State-of-charge window the controller keeps to, %.
    pub min_soc_pct: f64,
    pub max_soc_pct: f64,
}

#[derive(Debug, Clone)]
pub struct SiteConfig {
    pub policy: Policy,
    pub pv_installed_kw: f64,
    /// Rating of the grid connection, kW; readings far beyond it are implausible.
    pub connection_kw: f64,
    /// Expected PV yield per year, for curtailment budgets (kWh).
    pub expected_annual_yield_kwh: f64,
    pub chargers: Vec<ChargerSpec>,
    pub heat_pumps: Vec<HeatPumpSpec>,
    pub batteries: Vec<BatterySpec>,
    pub feed_in_reference: FeedInReference,
    /// After a dimming ends, grant power back linearly over this time
    /// (BK6-22-300 4.3: the return to normal must be gradual).
    pub release_ramp_s: f64,
    /// Before that ramp starts, wait a random time up to this long, so that
    /// sites released by the same command do not ramp up together (the UK
    /// Smart Charge Points Regulations 2021 ask for up to 600 s). 0 = off.
    pub release_delay_max_s: f64,
    /// Seed for that random wait; give each site its own.
    pub release_delay_seed: u64,
    /// An inverter still producing above its limit this long after it was
    /// sent is reported (IEC 104 point 2008) and the others make up for it.
    /// 0 = never check.
    pub pv_follow_timeout_s: f64,
    /// Seconds after a dimming starts before the compliance report counts
    /// draw above the floor (the devices' own reaction time).
    pub compliance_grace_s: f64,
    /// Name of the site in compliance reports (e.g. its market location).
    pub site_id: String,
    /// When a feed-in limit is raised or lifted, PV may climb at most this
    /// fast (% of installed power per second); reductions apply at once.
    pub pv_ramp_pct_per_s: f64,
    /// Headroom kept below every limit to absorb measurement noise.
    pub margin_kw: f64,
    /// A charger keeps charging (or waiting) at least this long before the
    /// rotation may switch it, so cars are not toggled every second.
    pub min_dwell_s: f64,
    /// A car whose slack before departure (time left minus time to charge
    /// at full power) is below this charges at full power, plan or not.
    pub deadline_guard_s: f64,
    /// While dimmed, the loads may count on the lowest PV surplus of this
    /// many seconds: the budget follows a drop at once and a rise only once
    /// it has lasted, so a cloud or a load step between two cycles does not
    /// push the devices over the floor.
    pub surplus_hold_s: f64,
    /// Battery self-consumption target: discharge to keep grid import at or
    /// below this (0 = maximise self-consumption).
    pub import_target_kw: f64,
}

impl SiteConfig {
    pub fn steuve(&self) -> SteuVE {
        SteuVE {
            chargers_and_storage: self.chargers.len() + self.batteries.len(),
            heat_pumps_kw: self.heat_pumps.iter().map(|h| h.rated_kw).collect(),
            air_conditioners_kw: vec![],
        }
    }

    fn rated_loads_kw(&self) -> f64 {
        self.chargers.iter().map(|c| three_phase_kw(c.max_current_a)).sum::<f64>()
            + self.heat_pumps.iter().map(|h| h.rated_kw).sum::<f64>()
    }

    /// Checks the values a configuration file could get wrong.
    pub fn validate(&self) -> Result<(), String> {
        self.policy.validate()?;
        let pos = |v: f64, what: &str| if v > 0.0 { Ok(()) } else { Err(format!("{what} must be > 0")) };
        pos(self.pv_installed_kw, "pv_installed_kw")?;
        pos(self.connection_kw, "connection_kw")?;
        pos(self.pv_ramp_pct_per_s, "pv_ramp_pct_per_s")?;
        let non_negative = [
            self.margin_kw,
            self.release_ramp_s,
            self.min_dwell_s,
            self.surplus_hold_s,
            self.release_delay_max_s,
            self.pv_follow_timeout_s,
            self.compliance_grace_s,
        ];
        if non_negative.iter().any(|v| v.is_nan() || *v < 0.0) {
            return Err("margin_kw, release_ramp_s, min_dwell_s, surplus_hold_s, release_delay_max_s, \
                        pv_follow_timeout_s and compliance_grace_s must be ≥ 0"
                .into());
        }
        for (i, c) in self.chargers.iter().enumerate() {
            if !(EV_MIN_CURRENT_A..=63.0).contains(&c.max_current_a) {
                return Err(format!("charger {i}: max_current_a must be within 6–63 A"));
            }
            if c.failsafe_current_a > c.max_current_a || (c.failsafe_current_a > 0.0 && c.failsafe_current_a < 6.0) {
                return Err(format!("charger {i}: failsafe_current_a must be 0 or 6 A up to max_current_a"));
            }
        }
        for (i, h) in self.heat_pumps.iter().enumerate() {
            if h.rated_kw <= 0.0 || h.min_kw < 0.0 || h.min_kw > h.rated_kw {
                return Err(format!("heat pump {i}: need 0 ≤ min_kw ≤ rated_kw and rated_kw > 0"));
            }
        }
        for (i, b) in self.batteries.iter().enumerate() {
            if b.capacity_kwh <= 0.0 || b.max_charge_kw < 0.0 || b.max_discharge_kw < 0.0 {
                return Err(format!("battery {i}: capacity must be > 0 and powers ≥ 0"));
            }
            if !(0.0..=100.0).contains(&b.min_soc_pct) || b.max_soc_pct > 100.0 || b.min_soc_pct >= b.max_soc_pct {
                return Err(format!("battery {i}: need 0 ≤ min_soc_pct < max_soc_pct ≤ 100"));
            }
        }
        Ok(())
    }
}

/// What the DSO currently demands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DsoCommands {
    /// Reduce consumption of the controllable devices (§14a in DE, the
    /// flexibility contract elsewhere).
    pub dim: bool,
    /// Feed-in limit in % of installed PV power (100 = no limit).
    pub feed_in_limit_pct: f64,
    /// Immediate, serious threat to grid operation (CH StromVG 17c 4b): the
    /// DSO's commands apply regardless of day limits, budgets and opt-outs.
    pub emergency: bool,
    /// Consumption limit sent with the dimming, kW (EEBUS LPC). The floor
    /// while dimmed is this value, but never below the legal minimum
    /// (Pmin,14a in DE): `None` dims to that minimum.
    pub limit_kw: Option<f64>,
}

impl Default for DsoCommands {
    fn default() -> Self {
        DsoCommands { dim: false, feed_in_limit_pct: 100.0, emergency: false, limit_kw: None }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChargerReading {
    pub online: bool,
    /// A car is plugged in and wants energy.
    pub car_waiting: bool,
    /// Measured charging current per phase.
    pub current_a: f64,
    pub power_kw: f64,
    pub session_kwh: f64,
    /// Energy the car still wants, kWh, when it told the charger.
    pub remaining_kwh: Option<f64>,
    /// Seconds until the car leaves, when it told the charger.
    pub departure_s: Option<f64>,
    /// Most current the car accepts, A, when it told the charger (ISO 15118).
    pub car_max_current_a: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct HeatPumpReading {
    pub online: bool,
    pub power_kw: f64,
    /// Power the heat pump would take without any limit.
    pub demand_kw: f64,
    pub indoor_c: Option<f64>,
    pub outdoor_c: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct InverterReading {
    pub online: bool,
    pub kw: f64,
    pub rated_kw: f64,
}

#[derive(Debug, Clone, Default)]
pub struct BatteryReading {
    pub online: bool,
    pub soc_pct: f64,
    /// + charging, − discharging.
    pub power_kw: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Readings {
    /// Grid connection point, + = import. `None` if the meter is unreachable.
    pub grid_kw: Option<f64>,
    /// Total PV output. `None` if an inverter is unreachable.
    pub pv_kw: Option<f64>,
    /// PV the sun would allow without any limit, if the inverters report it.
    pub pv_available_kw: Option<f64>,
    /// Each inverter on its own, to check that it follows its limit. Empty
    /// = not known; every inverter then gets the same limit.
    pub inverters: Vec<InverterReading>,
    pub chargers: Vec<ChargerReading>,
    pub heat_pumps: Vec<HeatPumpReading>,
    pub batteries: Vec<BatteryReading>,
}

/// What a planner (the MPC in the `planner` crate) suggests for the current
/// step. The controller follows it only as far as the rules allow: the
/// consumption floor, feed-in limits, device minimums and the battery's
/// state-of-charge window never depend on the plan being right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guidance {
    /// Grid exchange the plan expects, kW (+ import). The battery holds it,
    /// absorbing forecast errors — the plan's recourse.
    pub grid_kw: Option<f64>,
    /// Planned power per charger, kW; `None` = no opinion (as much as allowed).
    pub charger_kw: Vec<Option<f64>>,
    /// Planned electrical power per heat pump, kW; `None` = its own thermostat.
    pub heat_pump_kw: Vec<Option<f64>>,
    /// The plan expected a dimming now and has prepared for it. If a
    /// dimming arrives that the plan did not expect, the rules take over.
    pub dim_expected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Setpoints {
    /// Active power limit for the inverters, % of installed power.
    pub pv_limit_pct: f64,
    /// Limit per inverter, % of its rating: `pv_limit_pct` for all, except
    /// that the others take over what an inverter ignoring its limit
    /// produces too much. Empty when the readings had no per-inverter data;
    /// use `pv_limit_pct` then.
    pub inverter_limit_pct: Vec<f64>,
    /// Current limit per charger; 0 = pause.
    pub charger_current_a: Vec<f64>,
    /// Power limit per heat pump; 0 = off.
    pub heat_pump_limit_kw: Vec<f64>,
    /// Power per battery, + charge, − discharge.
    pub battery_kw: Vec<f64>,
    /// External power request per heat pump, kW; `None` = its own thermostat.
    pub heat_pump_ext_kw: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    MeterOffline,
    /// The meter answers, but with a value no real site could produce.
    MeterImplausible,
    PvOffline,
    PvImplausible,
    /// The inverter keeps producing above the limit it was sent.
    InverterIgnoresLimit(usize),
    ChargerOffline(usize),
    HeatPumpOffline(usize),
    BatteryOffline(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Dimmed,
    /// Dimming ended; power is being handed back gradually.
    Releasing,
}

/// A DSO command the controller did not (fully) apply, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The contract's daily dimming time is used up.
    DimDayLimitReached,
    /// The free curtailment budget (CH 3%) is used up.
    CurtailmentBudgetExhausted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub mode: Mode,
    /// While releasing: seconds of random wait left before power returns.
    pub release_wait_s: Option<f64>,
    /// Minimum grid power the controllable devices keep while dimmed.
    pub floor_kw: f64,
    /// Power the controllable devices may draw from the grid right now,
    /// `None` when unconstrained.
    pub steuve_budget_kw: Option<f64>,
    /// Measured draw of the controllable devices (loads + battery charging).
    pub steuve_kw: f64,
    /// Part of that draw taken from the grid (not covered by PV surplus or
    /// the battery) — the quantity a consumption limit applies to.
    pub steuve_grid_kw: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub pv_surplus_kw: Option<f64>,
    /// Feed-in limit in force after country rules, %.
    pub feed_in_limit_pct: f64,
    pub allowed_export_kw: f64,
    pub emergency: bool,
    pub refusals: Vec<Refusal>,
    pub totals: Totals,
    /// Share of the year's free curtailment budget used, % (CH).
    pub curtailment_budget_used_pct: Option<f64>,
    pub fallbacks: Vec<Fallback>,
}

pub struct Controller {
    cfg: SiteConfig,
    floor_kw: f64,
    was_dimmed: bool,
    last_budget_kw: f64,
    release: Option<(f64, f64)>, // (start time, budget at start)
    last_pv_pct: f64,
    last_t: Option<f64>,
    charger_on: Vec<bool>,
    charger_switched_at: Vec<f64>,
    accounting: Accounting,
    guidance: Option<Guidance>,
    /// PV surplus measured over the last `surplus_hold_s`: (time, kW).
    surplus_seen: VecDeque<(f64, f64)>,
    /// xorshift state for the release wait.
    rng: u64,
    /// Limit last sent to each inverter, %, and since when it has been
    /// producing above it.
    inverter_sent_pct: Vec<f64>,
    inverter_over_since: Vec<Option<f64>>,
    compliance: Recorder,
}

impl Controller {
    pub fn new(cfg: SiteConfig) -> Self {
        let n = cfg.chargers.len();
        let floor_kw = match cfg.policy.consumption {
            ConsumptionRule::De14a => cfg.steuve().pmin_kw(),
            ConsumptionRule::Contract { min_kw, .. } => min_kw,
        };
        // xorshift must not start at 0.
        let rng = cfg.release_delay_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        Controller {
            floor_kw,
            was_dimmed: false,
            last_budget_kw: f64::INFINITY,
            release: None,
            last_pv_pct: 100.0,
            last_t: None,
            charger_on: vec![false; n],
            charger_switched_at: vec![f64::NEG_INFINITY; n],
            accounting: Accounting::default(),
            guidance: None,
            surplus_seen: VecDeque::new(),
            rng,
            inverter_sent_pct: Vec::new(),
            inverter_over_since: Vec::new(),
            compliance: Recorder::new(&cfg.site_id, cfg.compliance_grace_s, 0),
            cfg,
        }
    }

    /// Timestamps for compliance reports: Unix time of this controller's
    /// clock zero, ms (the gateway: now − t_s; the demo: the simulated day).
    pub fn with_report_epoch(mut self, epoch_ms: i64) -> Self {
        self.compliance.set_epoch(epoch_ms);
        self
    }

    /// Continues the report chain after a restart.
    pub fn with_report_chain(mut self, seq: u32, last_sha256: &str) -> Self {
        self.compliance.resume(seq, last_sha256);
        self
    }

    /// Reports of finished dimmings not yet taken, oldest first.
    pub fn reports(&self) -> &[DimmingReport] {
        self.compliance.reports()
    }

    /// Takes the finished reports (to write them somewhere).
    pub fn take_reports(&mut self) -> Vec<DimmingReport> {
        self.compliance.take_reports()
    }

    /// Number of reports so far and the digest of the last one.
    pub fn report_chain(&self) -> (u32, String) {
        let (seq, last) = self.compliance.chain();
        (seq, last.to_string())
    }

    /// Guidance for the next cycles, or `None` to run on rules alone (no
    /// plan, a failed solve, a plan grown too old).
    pub fn set_guidance(&mut self, guidance: Option<Guidance>) {
        self.guidance = guidance;
    }

    pub fn guidance(&self) -> Option<&Guidance> {
        self.guidance.as_ref()
    }

    /// Continues from persisted totals (after a gateway restart).
    pub fn with_totals(mut self, totals: Totals) -> Self {
        self.accounting = Accounting::restore(totals);
        self
    }

    pub fn config(&self) -> &SiteConfig {
        &self.cfg
    }

    /// The minimum power kept while dimmed: Pmin,14a in DE, the contract elsewhere.
    pub fn floor_kw(&self) -> f64 {
        self.floor_kw
    }

    /// Kept for callers written against the DE-only controller.
    pub fn pmin_kw(&self) -> f64 {
        self.floor_kw
    }

    /// The floor for these commands: the limit sent with them, never below
    /// the legal minimum.
    pub fn floor_for(&self, cmd: &DsoCommands) -> f64 {
        cmd.limit_kw.filter(|l| l.is_finite()).map_or(self.floor_kw, |l| l.max(self.floor_kw))
    }

    /// One control cycle.
    pub fn step(&mut self, clock: &Clock, cmd: &DsoCommands, r: &Readings) -> (Setpoints, Status) {
        let t_s = clock.t_s;
        let floor_kw = self.floor_for(cmd);
        let dt = self.last_t.map_or(0.0, |l| (t_s - l).clamp(0.0, 10.0));
        self.last_t = Some(t_s);
        let mut fallbacks = Vec::new();
        let mut refusals = Vec::new();

        // --- plausibility --------------------------------------------------
        let cfg = &self.cfg;
        let grid_kw = match r.grid_kw {
            None => {
                fallbacks.push(Fallback::MeterOffline);
                None
            }
            Some(g) if !g.is_finite() || g.abs() > 1.5 * cfg.connection_kw => {
                fallbacks.push(Fallback::MeterImplausible);
                None
            }
            g => g,
        };
        let pv_kw = match r.pv_kw {
            None => {
                fallbacks.push(Fallback::PvOffline);
                None
            }
            Some(p) if !p.is_finite() || p < -1.0 || p > 1.25 * cfg.pv_installed_kw => {
                fallbacks.push(Fallback::PvImplausible);
                None
            }
            p => p.map(|p| p.max(0.0)),
        };

        // --- what the controllable devices draw ------------------------------
        // Devices we cannot talk to are assumed to draw what they would on
        // their own: a charger its failsafe current, a heat pump full power,
        // a battery nothing (its BMS stops on a lost link).
        let contract = matches!(cfg.policy.consumption, ConsumptionRule::Contract { .. });
        let mut offline_reserve = 0.0;
        let mut loads_kw = 0.0;
        for (i, (spec, c)) in cfg.chargers.iter().zip(&r.chargers).enumerate() {
            if c.online {
                loads_kw += c.power_kw;
            } else {
                fallbacks.push(Fallback::ChargerOffline(i));
                if !(contract && spec.opted_out) {
                    offline_reserve += three_phase_kw(spec.failsafe_current_a);
                }
            }
        }
        for (i, (spec, h)) in cfg.heat_pumps.iter().zip(&r.heat_pumps).enumerate() {
            if h.online {
                loads_kw += h.power_kw;
            } else {
                fallbacks.push(Fallback::HeatPumpOffline(i));
                if !(contract && spec.opted_out) {
                    offline_reserve += spec.rated_kw;
                }
            }
        }
        let mut battery_kw = 0.0;
        for (i, b) in r.batteries.iter().enumerate().take(cfg.batteries.len()) {
            if b.online {
                battery_kw += b.power_kw;
            } else {
                fallbacks.push(Fallback::BatteryOffline(i));
            }
        }
        let steuve_kw = loads_kw + battery_kw.max(0.0);

        // Base load = everything the meter sees that is neither PV nor a
        // controllable device.
        let base_load = grid_kw.zip(pv_kw).map(|(g, pv)| g + pv - loads_kw - battery_kw);
        let pv_surplus = base_load.zip(pv_kw).map(|(b, pv)| (pv - b).max(0.0));
        let hold = cfg.surplus_hold_s;
        self.surplus_seen.retain(|&(t, _)| t <= t_s && t_s - t <= hold);
        let held_surplus = pv_surplus.map(|s| {
            self.surplus_seen.push_back((t_s, s));
            self.surplus_seen.iter().map(|&(_, v)| v).fold(s, f64::min)
        });
        let cfg = &self.cfg;

        // --- consumption: the budget for the controllable loads ------------
        self.accounting.roll(clock);
        let totals = self.accounting.totals();
        let mut dim = cmd.dim;
        if let ConsumptionRule::Contract { max_minutes_per_day, .. } = cfg.policy.consumption
            && dim
            && !cmd.emergency
            && max_minutes_per_day > 0.0
            && totals.dimmed_s_today >= max_minutes_per_day * 60.0
        {
            refusals.push(Refusal::DimDayLimitReached);
            dim = false;
        }
        let mode;
        let mut release_wait_s = None;
        let budget = if dim {
            mode = Mode::Dimmed;
            self.release = None;
            // Without a meter we cannot see PV surplus: grant only the floor.
            floor_kw + held_surplus.unwrap_or(0.0) - cfg.margin_kw
        } else {
            if self.was_dimmed {
                // The ramp starts after a random wait (0 when switched off).
                let wait = cfg.release_delay_max_s * random_unit(&mut self.rng);
                self.release = Some((t_s + wait, self.last_budget_kw));
            }
            match self.release {
                Some((t0, from)) if t_s < t0 => {
                    mode = Mode::Releasing;
                    release_wait_s = Some(t0 - t_s);
                    from
                }
                Some((t0, from)) if t_s - t0 < cfg.release_ramp_s => {
                    mode = Mode::Releasing;
                    let full = cfg.rated_loads_kw();
                    from + (full - from) * (t_s - t0) / cfg.release_ramp_s
                }
                _ => {
                    mode = Mode::Normal;
                    self.release = None;
                    f64::INFINITY
                }
            }
        };
        self.was_dimmed = dim;
        self.last_budget_kw = budget;

        // --- feed-in: the limit in force after country rules ----------------
        let static_cap = cfg.policy.static_feed_in_cap_pct.unwrap_or(100.0);
        let mut feed_in_pct = cmd.feed_in_limit_pct.clamp(0.0, 100.0);
        let budget_used = cfg.policy.curtailment_budget_pct.map(|pct| {
            let allowed = pct / 100.0 * cfg.expected_annual_yield_kwh.max(1.0);
            totals.curtailed_kwh_year / allowed * 100.0
        });
        if feed_in_pct < 100.0 && !cmd.emergency && budget_used.is_some_and(|u| u >= 100.0) {
            refusals.push(Refusal::CurtailmentBudgetExhausted);
            if cfg.policy.enforce_curtailment_budget {
                feed_in_pct = 100.0;
            }
        }
        let feed_in_pct = feed_in_pct.min(static_cap);
        let allowed_export_kw = cfg.pv_installed_kw * feed_in_pct / 100.0;

        // --- battery --------------------------------------------------------
        let battery_sp = self.plan_batteries(
            r,
            grid_kw,
            battery_kw,
            dim || mode == Mode::Releasing,
            floor_kw,
            base_load,
            pv_kw,
            loads_kw,
            (feed_in_pct < 100.0).then_some(allowed_export_kw),
        );
        let battery_discharge: f64 = battery_sp.iter().map(|&p| (-p).max(0.0)).sum();
        let battery_charge: f64 = battery_sp.iter().map(|&p| p.max(0.0)).sum();

        // While dimmed, discharging the battery offsets the grid draw of the
        // controllable devices, so the loads may use that much more; charging
        // (only from PV surplus, see plan_batteries) uses up part of it.
        let loads_budget = if budget.is_finite() { budget + battery_discharge - battery_charge } else { budget };
        let (charger_current_a, heat_pump_limit_kw) = self.allocate(t_s, loads_budget - offline_reserve, r);

        // --- PV limit -------------------------------------------------------
        let cfg = &self.cfg;
        let target_pct = match (cfg.feed_in_reference, grid_kw, pv_kw) {
            (_, _, _) if feed_in_pct >= 100.0 => 100.0,
            (FeedInReference::GridConnectionPoint, Some(g), Some(pv)) => {
                // Site consumption does not depend on PV: pv + grid import,
                // corrected for the battery's change of plan.
                let consumption = (pv + g - battery_kw + battery_charge - battery_discharge).max(0.0);
                let pv_max = allowed_export_kw + consumption - cfg.margin_kw;
                (pv_max / cfg.pv_installed_kw * 100.0).clamp(feed_in_pct, 100.0)
            }
            _ => feed_in_pct,
        };
        // Down at once, up gently.
        let pv_limit_pct = if target_pct > self.last_pv_pct {
            (self.last_pv_pct + cfg.pv_ramp_pct_per_s * dt).min(target_pct)
        } else {
            target_pct
        };
        self.last_pv_pct = pv_limit_pct;

        // --- inverters that ignore their limit ------------------------------
        // Compare each inverter's output with the limit it was sent last
        // cycle. One still above it after `pv_follow_timeout_s` is reported;
        // its output is taken as given and the others are limited further so
        // the plant as a whole stays at the limit.
        let n_inv = r.inverters.len();
        if self.inverter_sent_pct.len() != n_inv {
            self.inverter_sent_pct = vec![100.0; n_inv];
            self.inverter_over_since = vec![None; n_inv];
        }
        let mut ignoring = vec![false; n_inv];
        for (i, inv) in r.inverters.iter().enumerate() {
            let sent = self.inverter_sent_pct[i];
            let tolerance_kw = cfg.margin_kw.max(0.02 * inv.rated_kw);
            let over = inv.online && sent < 100.0 && inv.kw > inv.rated_kw * sent / 100.0 + tolerance_kw;
            if !over {
                self.inverter_over_since[i] = None;
                continue;
            }
            let since = *self.inverter_over_since[i].get_or_insert(t_s);
            if cfg.pv_follow_timeout_s > 0.0 && t_s - since >= cfg.pv_follow_timeout_s {
                ignoring[i] = true;
                fallbacks.push(Fallback::InverterIgnoresLimit(i));
            }
        }
        let inverter_limit_pct: Vec<f64> = if pv_limit_pct >= 100.0 || !ignoring.contains(&true) {
            vec![pv_limit_pct; n_inv]
        } else {
            let target_kw = cfg.pv_installed_kw * pv_limit_pct / 100.0;
            let stuck_kw: f64 = r.inverters.iter().zip(&ignoring).filter(|(_, g)| **g).map(|(v, _)| v.kw).sum();
            let free_rated: f64 =
                r.inverters.iter().zip(&ignoring).filter(|(v, g)| !**g && v.online).map(|(v, _)| v.rated_kw).sum();
            let free_pct = if free_rated > 0.0 {
                ((target_kw - stuck_kw) / free_rated * 100.0).clamp(0.0, pv_limit_pct)
            } else {
                pv_limit_pct
            };
            ignoring.iter().map(|&g| if g { pv_limit_pct } else { free_pct }).collect()
        };
        self.inverter_sent_pct.clone_from(&inverter_limit_pct);

        // --- running totals -------------------------------------------------
        let curtailed_kw = match (r.pv_available_kw, pv_kw) {
            (Some(avail), Some(pv)) if pv_limit_pct < 100.0 => (avail - pv).max(0.0),
            _ => 0.0,
        };
        self.accounting.advance(clock, dim, pv_kw, curtailed_kw);

        let status = Status {
            mode,
            release_wait_s,
            floor_kw,
            steuve_budget_kw: budget.is_finite().then_some(budget.max(0.0)),
            steuve_kw,
            steuve_grid_kw: pv_surplus.map(|s| (steuve_kw - s - battery_kw.min(0.0).abs()).max(0.0)),
            base_load_kw: base_load,
            pv_surplus_kw: pv_surplus,
            feed_in_limit_pct: feed_in_pct,
            allowed_export_kw,
            emergency: cmd.emergency,
            refusals,
            totals: self.accounting.totals(),
            curtailment_budget_used_pct: budget_used,
            fallbacks,
        };
        let heat_pump_ext_kw = (0..cfg.heat_pumps.len())
            .map(|i| {
                let planned = self.guidance.as_ref().and_then(|g| g.heat_pump_kw.get(i).copied().flatten())?;
                Some(planned.min(heat_pump_limit_kw[i]).max(0.0))
            })
            .collect();
        self.compliance.observe(
            t_s,
            status.mode == Mode::Dimmed,
            status.emergency,
            status.floor_kw,
            status.steuve_grid_kw,
            status.steuve_budget_kw,
        );
        (
            Setpoints {
                pv_limit_pct,
                inverter_limit_pct,
                charger_current_a,
                heat_pump_limit_kw,
                battery_kw: battery_sp,
                heat_pump_ext_kw,
            },
            status,
        )
    }

    /// Battery plan.
    /// * Normal operation: store what would be exported, discharge to keep
    ///   grid import at `import_target_kw` — self-consumption, which also
    ///   soaks up a feed-in limit before any PV is curtailed.
    /// * While dimmed or releasing: never charge from the grid; discharge to
    ///   cover the site's import so the loads keep more of their budget.
    ///
    /// The balance uses the PV the sun *allows* where the inverters report
    /// it. With measured PV only, a curtailment would look like a deficit:
    /// the battery would discharge, the controller would curtail PV further
    /// to hold the export limit, and the two would chase each other down.
    ///
    /// With guidance, the battery holds the planned grid exchange instead
    /// (it absorbs forecast errors), within the same safety rules: it still
    /// stores whatever a feed-in limit would curtail, never discharges into
    /// curtailed PV it cannot see, and never charges from the grid while the
    /// consumption is dimmed. A dimming the plan did not expect falls back to
    /// the rules above.
    #[allow(clippy::too_many_arguments)]
    fn plan_batteries(
        &self,
        r: &Readings,
        grid_kw: Option<f64>,
        battery_now_kw: f64,
        constrained: bool,
        floor_kw: f64,
        base_load: Option<f64>,
        pv_kw: Option<f64>,
        loads_kw: f64,
        allowed_export_kw: Option<f64>,
    ) -> Vec<f64> {
        let cfg = &self.cfg;
        let feed_limited = allowed_export_kw.is_some();
        // Grid power as it would be with the batteries idle.
        let Some(grid_idle) = grid_kw.map(|g| g - battery_now_kw) else {
            return vec![0.0; cfg.batteries.len()]; // blind: hold still
        };
        let pv_ref = r.pv_available_kw.or(pv_kw);
        // + import / − export with the batteries idle and PV unconstrained.
        let unconstrained = base_load.zip(r.pv_available_kw).map(|(b, av)| b + loads_kw - av);
        let surplus_now = base_load.zip(pv_ref).map_or(0.0, |(b, pv)| (pv - b - loads_kw).max(0.0));
        let guided = self.guidance.as_ref().and_then(|g| g.grid_kw.map(|target| (target, g.dim_expected)));
        let mut want = match guided {
            Some((target, expected)) if !constrained || expected => {
                let mut w = target - grid_idle;
                if let (Some(unc), Some(ax)) = (unconstrained, allowed_export_kw) {
                    w = w.max(-unc - ax); // store what the limit would otherwise curtail
                }
                if feed_limited && unconstrained.is_none() {
                    w = w.max(0.0);
                }
                if constrained {
                    w = w.min(surplus_now); // no grid charging while dimmed
                }
                w
            }
            _ if constrained => {
                // Import the site would have while dimmed ≈ base load − PV + a
                // floor's worth of loads; cover as much as the battery can.
                let base_minus_pv = base_load.zip(pv_ref).map_or(0.0, |(b, pv)| b - pv);
                -(base_minus_pv + floor_kw).max(0.0)
            }
            _ => {
                let net = unconstrained.unwrap_or(grid_idle);
                if net < 0.0 {
                    -net // surplus → charge
                } else if feed_limited && unconstrained.is_none() {
                    0.0 // cannot tell a real deficit from our own curtailment
                } else {
                    -(net - cfg.import_target_kw).max(0.0) // deficit → discharge
                }
            }
        };
        cfg.batteries
            .iter()
            .enumerate()
            .map(|(i, spec)| {
                let Some(b) = r.batteries.get(i).filter(|b| b.online) else { return 0.0 };
                let room_kwh = (spec.max_soc_pct - b.soc_pct).max(0.0) / 100.0 * spec.capacity_kwh;
                let avail_kwh = (b.soc_pct - spec.min_soc_pct).max(0.0) / 100.0 * spec.capacity_kwh;
                // Taper near the SoC limits instead of hitting them at full power.
                let max_ch = spec.max_charge_kw.min(room_kwh * 4.0);
                let max_dis = spec.max_discharge_kw.min(avail_kwh * 4.0);
                let p = want.clamp(-max_dis, max_ch);
                want -= p;
                p
            })
            .collect()
    }

    /// Splits the budget between heat pumps and chargers.
    ///
    /// Policy (the EMS may split freely, BK6-22-300 4.5.2 sentence 6):
    /// 1. each heat pump first gets up to 40% of its rating — the share the
    ///    German regulation itself attributes to it;
    /// 2. then as many waiting cars as fit get the 6 A minimum — cars that
    ///    must leave soonest relative to the energy they still need (least
    ///    laxity) first, then those that charged least so far;
    /// 3. what is left tops up the heat pumps to their demand, then the
    ///    chargers evenly up to their maximum.
    ///
    /// Devices whose owner opted out of a contract are never limited. Every
    /// value is rounded *down* to what the device can do (4.6: when the exact
    /// value is impossible, go to the next lower possible one).
    fn allocate(&mut self, t_s: f64, budget_kw: f64, r: &Readings) -> (Vec<f64>, Vec<f64>) {
        let cfg = &self.cfg;
        let contract = matches!(cfg.policy.consumption, ConsumptionRule::Contract { .. });
        let exempt_c = |i: usize| contract && cfg.chargers[i].opted_out;
        let exempt_h = |i: usize| contract && cfg.heat_pumps[i].opted_out;
        // Slack before departure: time left minus time to charge at full power.
        let laxity = |i: usize| {
            let c = &r.chargers[i];
            let max_a = c.car_max_current_a.unwrap_or(f64::INFINITY).min(cfg.chargers[i].max_current_a);
            match (c.departure_s, c.remaining_kwh) {
                (Some(dep), Some(rem)) if max_a > 0.0 => dep - rem / three_phase_kw(max_a) * 3600.0,
                _ => f64::INFINITY,
            }
        };
        // What the plan wants for each charger, as a current cap. A plan of
        // less than half the 6 A minimum means "not now". A car about to miss
        // its departure charges at full power whatever the plan says.
        let min_kw = three_phase_kw(EV_MIN_CURRENT_A);
        let planned_a = |i: usize| -> Option<f64> {
            let kw = self.guidance.as_ref().and_then(|g| g.charger_kw.get(i).copied().flatten())?;
            if laxity(i) <= cfg.deadline_guard_s {
                return None;
            }
            Some(if kw < min_kw / 2.0 {
                0.0
            } else {
                three_phase_current_a(kw).round().clamp(EV_MIN_CURRENT_A, cfg.chargers[i].max_current_a)
            })
        };
        let cap_a = |i: usize| planned_a(i).unwrap_or(cfg.chargers[i].max_current_a);
        let waiting: Vec<usize> = (0..cfg.chargers.len())
            .filter(|&i| r.chargers.get(i).is_some_and(|c| c.online && c.car_waiting) && cap_a(i) > 0.0)
            .collect();

        let mut currents: Vec<f64> = (0..cfg.chargers.len()).map(cap_a).collect();
        let mut hp: Vec<f64> = cfg.heat_pumps.iter().map(|h| h.rated_kw).collect();
        if budget_kw.is_infinite() {
            for i in 0..cfg.chargers.len() {
                self.set_charger_on(i, waiting.contains(&i), t_s);
            }
            return (currents, hp);
        }

        // A guided heat pump reports our own request as its demand; use the
        // plan itself, or a request of 0 would lock the unit off while dimmed.
        let demand = |i: usize| {
            let planned = self.guidance.as_ref().and_then(|g| g.heat_pump_kw.get(i).copied().flatten());
            r.heat_pumps
                .get(i)
                .filter(|h| h.online)
                .map_or(0.0, |h| planned.unwrap_or(h.demand_kw).min(cfg.heat_pumps[i].rated_kw))
        };
        let mut left = budget_kw.max(0.0);
        // Exempt devices run free but their draw still reaches the grid.
        for i in (0..cfg.heat_pumps.len()).filter(|&i| exempt_h(i)) {
            left -= demand(i);
        }
        for &i in waiting.iter().filter(|&&i| exempt_c(i)) {
            left -= r.chargers[i].power_kw;
        }
        left = left.max(0.0);

        // 1. heat pumps: guaranteed share
        for (i, give) in hp.iter_mut().enumerate().filter(|(i, _)| !exempt_h(*i)) {
            let g = demand(i).min(0.4 * cfg.heat_pumps[i].rated_kw).min(left);
            *give = if g < cfg.heat_pumps[i].min_kw { 0.0 } else { g };
            left -= *give;
        }

        // 2. chargers: 6 A minimum for as many cars as fit. Cars inside their
        // dwell time keep their current state first; then least laxity (time
        // to departure minus time to charge at full power), then least energy.
        let mut order: Vec<usize> = waiting.iter().copied().filter(|&i| !exempt_c(i)).collect();
        order.sort_by(|&a, &b| {
            let key = |i: usize| {
                let locked = t_s - self.charger_switched_at[i] < cfg.min_dwell_s;
                let rank = match (locked, self.charger_on[i]) {
                    (true, true) => 0,
                    (false, _) => 1,
                    (true, false) => 2,
                };
                (rank, laxity(i), r.chargers[i].session_kwh)
            };
            let (ra, la, ea) = key(a);
            let (rb, lb, eb) = key(b);
            ra.cmp(&rb).then(la.total_cmp(&lb)).then(ea.total_cmp(&eb))
        });
        let fit = ((left / min_kw).floor() as usize).min(order.len());
        let active: Vec<usize> = order[..fit].to_vec();
        for (i, c) in currents.iter_mut().enumerate() {
            if !exempt_c(i) {
                *c = 0.0;
            }
        }
        for &i in &active {
            currents[i] = EV_MIN_CURRENT_A;
            left -= min_kw;
        }

        // 3a. top up heat pumps to their demand
        for (i, give) in hp.iter_mut().enumerate().filter(|(i, _)| !exempt_h(*i)) {
            let extra = (demand(i) - *give).max(0.0).min(left);
            if *give + extra >= cfg.heat_pumps[i].min_kw {
                *give += extra;
                left -= extra;
            }
        }

        // 3b. water-fill the active chargers in whole amps
        let mut spare_a = three_phase_current_a(left.max(0.0));
        let mut open: Vec<usize> = active.clone();
        while spare_a >= 1.0 && !open.is_empty() {
            let share = (spare_a / open.len() as f64).floor().max(1.0);
            let mut next = Vec::new();
            for &i in &open {
                let room = (cap_a(i) - currents[i]).floor();
                let add = share.min(room).min(spare_a.floor());
                currents[i] += add;
                spare_a -= add;
                if room - add >= 1.0 {
                    next.push(i);
                }
            }
            open = next;
        }

        for (i, &a) in currents.clone().iter().enumerate() {
            self.set_charger_on(i, a > 0.0 && waiting.contains(&i), t_s);
        }
        (currents, hp)
    }

    fn set_charger_on(&mut self, i: usize, on: bool, t_s: f64) {
        if self.charger_on[i] != on {
            self.charger_on[i] = on;
            self.charger_switched_at[i] = t_s;
        }
    }
}

/// Uniform in [0, 1) from a xorshift64 state.
fn random_unit(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}
