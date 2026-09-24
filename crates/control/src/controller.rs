//! The site controller: turns DSO commands and device readings into setpoints.
//!
//! It is a pure function of its inputs plus a little memory (ramp state,
//! rotation, running totals), so the gateway, the browser demo and the tests
//! all run exactly the same code. Country-specific limits come from a
//! [`Policy`]; the controller itself is country-agnostic.

use crate::accounting::{Accounting, Clock, Totals};
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
    /// When a feed-in limit is raised or lifted, PV may climb at most this
    /// fast (% of installed power per second); reductions apply at once.
    pub pv_ramp_pct_per_s: f64,
    /// Headroom kept below every limit to absorb measurement noise.
    pub margin_kw: f64,
    /// A charger keeps charging (or waiting) at least this long before the
    /// rotation may switch it, so cars are not toggled every second.
    pub min_dwell_s: f64,
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
        if self.margin_kw < 0.0 || self.release_ramp_s < 0.0 || self.min_dwell_s < 0.0 {
            return Err("margin_kw, release_ramp_s and min_dwell_s must be ≥ 0".into());
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
}

impl Default for DsoCommands {
    fn default() -> Self {
        DsoCommands { dim: false, feed_in_limit_pct: 100.0, emergency: false }
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
}

#[derive(Debug, Clone, Default)]
pub struct HeatPumpReading {
    pub online: bool,
    pub power_kw: f64,
    /// Power the heat pump would take without any limit.
    pub demand_kw: f64,
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
    pub chargers: Vec<ChargerReading>,
    pub heat_pumps: Vec<HeatPumpReading>,
    pub batteries: Vec<BatteryReading>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Setpoints {
    /// Active power limit for the inverters, % of installed power.
    pub pv_limit_pct: f64,
    /// Current limit per charger; 0 = pause.
    pub charger_current_a: Vec<f64>,
    /// Power limit per heat pump; 0 = off.
    pub heat_pump_limit_kw: Vec<f64>,
    /// Power per battery, + charge, − discharge.
    pub battery_kw: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    MeterOffline,
    /// The meter answers, but with a value no real site could produce.
    MeterImplausible,
    PvOffline,
    PvImplausible,
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
}

impl Controller {
    pub fn new(cfg: SiteConfig) -> Self {
        let n = cfg.chargers.len();
        let floor_kw = match cfg.policy.consumption {
            ConsumptionRule::De14a => cfg.steuve().pmin_kw(),
            ConsumptionRule::Contract { min_kw, .. } => min_kw,
        };
        Controller {
            floor_kw,
            cfg,
            was_dimmed: false,
            last_budget_kw: f64::INFINITY,
            release: None,
            last_pv_pct: 100.0,
            last_t: None,
            charger_on: vec![false; n],
            charger_switched_at: vec![f64::NEG_INFINITY; n],
            accounting: Accounting::default(),
        }
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

    /// One control cycle.
    pub fn step(&mut self, clock: &Clock, cmd: &DsoCommands, r: &Readings) -> (Setpoints, Status) {
        let t_s = clock.t_s;
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
        let budget = if dim {
            mode = Mode::Dimmed;
            self.release = None;
            // Without a meter we cannot see PV surplus: grant only the floor.
            self.floor_kw + pv_surplus.unwrap_or(0.0) - cfg.margin_kw
        } else {
            if self.was_dimmed {
                self.release = Some((t_s, self.last_budget_kw));
            }
            match self.release {
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
            base_load,
            pv_kw,
            loads_kw,
            feed_in_pct < 100.0,
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

        // --- running totals -------------------------------------------------
        let curtailed_kw = match (r.pv_available_kw, pv_kw) {
            (Some(avail), Some(pv)) if pv_limit_pct < 100.0 => (avail - pv).max(0.0),
            _ => 0.0,
        };
        self.accounting.advance(clock, dim, pv_kw, curtailed_kw);

        let status = Status {
            mode,
            floor_kw: self.floor_kw,
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
        (Setpoints { pv_limit_pct, charger_current_a, heat_pump_limit_kw, battery_kw: battery_sp }, status)
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
    #[allow(clippy::too_many_arguments)]
    fn plan_batteries(
        &self,
        r: &Readings,
        grid_kw: Option<f64>,
        battery_now_kw: f64,
        constrained: bool,
        base_load: Option<f64>,
        pv_kw: Option<f64>,
        loads_kw: f64,
        feed_limited: bool,
    ) -> Vec<f64> {
        let cfg = &self.cfg;
        // Grid power as it would be with the batteries idle.
        let Some(grid_idle) = grid_kw.map(|g| g - battery_now_kw) else {
            return vec![0.0; cfg.batteries.len()]; // blind: hold still
        };
        let pv_ref = r.pv_available_kw.or(pv_kw);
        let mut want = if constrained {
            // Import the site would have while dimmed ≈ base load − PV + a
            // floor's worth of loads; cover as much as the battery can.
            let base_minus_pv = base_load.zip(pv_ref).map_or(0.0, |(b, pv)| b - pv);
            -(base_minus_pv + self.floor_kw).max(0.0)
        } else {
            // + import / − export with the batteries idle and PV unconstrained.
            let unconstrained = base_load.zip(r.pv_available_kw).map(|(b, av)| b + loads_kw - av);
            let net = unconstrained.unwrap_or(grid_idle);
            if net < 0.0 {
                -net // surplus → charge
            } else if feed_limited && unconstrained.is_none() {
                0.0 // cannot tell a real deficit from our own curtailment
            } else {
                -(net - cfg.import_target_kw).max(0.0) // deficit → discharge
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
    /// 2. then as many waiting cars as fit get the 6 A minimum, those that
    ///    charged least so far first;
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
        let waiting: Vec<usize> =
            (0..cfg.chargers.len()).filter(|&i| r.chargers.get(i).is_some_and(|c| c.online && c.car_waiting)).collect();

        let mut currents: Vec<f64> = cfg.chargers.iter().map(|c| c.max_current_a).collect();
        let mut hp: Vec<f64> = cfg.heat_pumps.iter().map(|h| h.rated_kw).collect();
        if budget_kw.is_infinite() {
            for i in 0..cfg.chargers.len() {
                self.set_charger_on(i, waiting.contains(&i), t_s);
            }
            return (currents, hp);
        }

        let demand = |i: usize| {
            r.heat_pumps.get(i).filter(|h| h.online).map_or(0.0, |h| h.demand_kw.min(cfg.heat_pumps[i].rated_kw))
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
        // dwell time keep their current state first, then least energy first.
        let min_kw = three_phase_kw(EV_MIN_CURRENT_A);
        let mut order: Vec<usize> = waiting.iter().copied().filter(|&i| !exempt_c(i)).collect();
        order.sort_by(|&a, &b| {
            let key = |i: usize| {
                let locked = t_s - self.charger_switched_at[i] < cfg.min_dwell_s;
                let rank = match (locked, self.charger_on[i]) {
                    (true, true) => 0,
                    (false, _) => 1,
                    (true, false) => 2,
                };
                (rank, r.chargers[i].session_kwh)
            };
            let (ra, ea) = key(a);
            let (rb, eb) = key(b);
            ra.cmp(&rb).then(ea.total_cmp(&eb))
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
                let room = (cfg.chargers[i].max_current_a - currents[i]).floor();
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
