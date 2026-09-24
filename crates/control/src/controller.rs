//! The site controller: turns DSO commands and device readings into setpoints.
//!
//! It is a pure function of its inputs plus a little memory (ramp state,
//! last decisions), so the gateway, the browser demo and the tests all run
//! exactly the same code.

use crate::rules::{EV_MIN_CURRENT_A, SteuVE, three_phase_current_a, three_phase_kw};

/// Where the DSO's feed-in limit is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedInReference {
    /// Limit the PV plant's output itself (conservative, simplest).
    PlantOutput,
    /// Limit what flows into the grid at the connection point; PV may
    /// produce more as long as the site consumes the difference.
    GridConnectionPoint,
}

#[derive(Debug, Clone)]
pub struct ChargerSpec {
    pub max_current_a: f64,
    /// Current the charger falls back to on its own when the gateway stops
    /// talking to it (charger-side watchdog).
    pub failsafe_current_a: f64,
}

#[derive(Debug, Clone)]
pub struct HeatPumpSpec {
    pub rated_kw: f64,
    /// Lowest power the compressor can modulate down to; below it, off.
    pub min_kw: f64,
}

#[derive(Debug, Clone)]
pub struct SiteConfig {
    pub pv_installed_kw: f64,
    pub chargers: Vec<ChargerSpec>,
    pub heat_pumps: Vec<HeatPumpSpec>,
    pub feed_in_reference: FeedInReference,
    /// After a dimming ends, grant power back linearly over this time
    /// (BK6-22-300 4.3: the return to normal must be gradual).
    pub release_ramp_s: f64,
    /// Headroom kept below every limit to absorb measurement noise.
    pub margin_kw: f64,
    /// A charger keeps charging (or waiting) at least this long before the
    /// rotation may switch it, so cars are not toggled every second.
    pub min_dwell_s: f64,
}

impl SiteConfig {
    pub fn steuve(&self) -> SteuVE {
        SteuVE {
            chargers_and_storage: self.chargers.len(),
            heat_pumps_kw: self.heat_pumps.iter().map(|h| h.rated_kw).collect(),
            air_conditioners_kw: vec![],
        }
    }

    fn rated_steuve_kw(&self) -> f64 {
        self.chargers.iter().map(|c| three_phase_kw(c.max_current_a)).sum::<f64>()
            + self.heat_pumps.iter().map(|h| h.rated_kw).sum::<f64>()
    }
}

/// What the DSO currently demands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DsoCommands {
    /// §14a EnWG grid-oriented control is active.
    pub dim_14a: bool,
    /// Feed-in limit in % of installed PV power (100 = no limit).
    pub feed_in_limit_pct: f64,
}

impl Default for DsoCommands {
    fn default() -> Self {
        DsoCommands { dim_14a: false, feed_in_limit_pct: 100.0 }
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
pub struct Readings {
    /// Grid connection point, + = import. `None` if the meter is unreachable.
    pub grid_kw: Option<f64>,
    /// Total PV output. `None` if the inverters are unreachable.
    pub pv_kw: Option<f64>,
    pub chargers: Vec<ChargerReading>,
    pub heat_pumps: Vec<HeatPumpReading>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Setpoints {
    /// Active power limit for the inverters, % of installed power.
    pub pv_limit_pct: f64,
    /// Current limit per charger; 0 = pause.
    pub charger_current_a: Vec<f64>,
    /// Power limit per heat pump; 0 = off.
    pub heat_pump_limit_kw: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    MeterOffline,
    PvOffline,
    ChargerOffline(usize),
    HeatPumpOffline(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Dimmed,
    /// Dimming ended; power is being handed back gradually.
    Releasing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub mode: Mode,
    pub pmin_kw: f64,
    /// Power the controllable devices may draw in total right now (kW),
    /// `None` when unconstrained.
    pub steuve_budget_kw: Option<f64>,
    /// Measured draw of the controllable devices.
    pub steuve_kw: f64,
    /// Part of that draw actually taken from the grid (not covered by PV
    /// surplus) — the quantity §14a limits.
    pub steuve_grid_kw: Option<f64>,
    pub base_load_kw: Option<f64>,
    pub pv_surplus_kw: Option<f64>,
    pub allowed_export_kw: f64,
    pub fallbacks: Vec<Fallback>,
}

pub struct Controller {
    cfg: SiteConfig,
    pmin_kw: f64,
    was_dimmed: bool,
    last_budget_kw: f64,
    release: Option<(f64, f64)>, // (start time, budget at start)
    charger_on: Vec<bool>,
    charger_switched_at: Vec<f64>,
}

impl Controller {
    pub fn new(cfg: SiteConfig) -> Self {
        let n = cfg.chargers.len();
        Controller {
            pmin_kw: cfg.steuve().pmin_kw(),
            cfg,
            was_dimmed: false,
            last_budget_kw: f64::INFINITY,
            release: None,
            charger_on: vec![false; n],
            charger_switched_at: vec![f64::NEG_INFINITY; n],
        }
    }

    pub fn config(&self) -> &SiteConfig {
        &self.cfg
    }

    pub fn pmin_kw(&self) -> f64 {
        self.pmin_kw
    }

    /// One control cycle. `t_s` is a monotonic time in seconds.
    pub fn step(&mut self, t_s: f64, cmd: &DsoCommands, r: &Readings) -> (Setpoints, Status) {
        let cfg = &self.cfg;
        let mut fallbacks = Vec::new();
        if r.grid_kw.is_none() {
            fallbacks.push(Fallback::MeterOffline);
        }
        if r.pv_kw.is_none() {
            fallbacks.push(Fallback::PvOffline);
        }

        // Devices we cannot talk to are assumed to draw what they would on
        // their own: a charger its failsafe current, a heat pump full power.
        let mut offline_reserve = 0.0;
        let mut steuve_kw = 0.0;
        for (i, (spec, c)) in cfg.chargers.iter().zip(&r.chargers).enumerate() {
            if c.online {
                steuve_kw += c.power_kw;
            } else {
                fallbacks.push(Fallback::ChargerOffline(i));
                offline_reserve += three_phase_kw(spec.failsafe_current_a);
            }
        }
        for (i, (spec, h)) in cfg.heat_pumps.iter().zip(&r.heat_pumps).enumerate() {
            if h.online {
                steuve_kw += h.power_kw;
            } else {
                fallbacks.push(Fallback::HeatPumpOffline(i));
                offline_reserve += spec.rated_kw;
            }
        }

        // Base load = everything the meter sees that is neither PV nor steuVE.
        let base_load = match (r.grid_kw, r.pv_kw) {
            (Some(g), Some(pv)) => Some(g + pv - steuve_kw),
            _ => None,
        };
        let pv_surplus = match (base_load, r.pv_kw) {
            (Some(b), Some(pv)) => Some((pv - b).max(0.0)),
            _ => None,
        };

        // --- §14a budget ---------------------------------------------------
        let mode;
        let budget = if cmd.dim_14a {
            mode = Mode::Dimmed;
            self.release = None;
            // Without a meter we cannot see PV surplus: grant only Pmin.
            self.pmin_kw + pv_surplus.unwrap_or(0.0) - cfg.margin_kw
        } else {
            if self.was_dimmed {
                self.release = Some((t_s, self.last_budget_kw));
            }
            match self.release {
                Some((t0, from)) if t_s - t0 < cfg.release_ramp_s => {
                    mode = Mode::Releasing;
                    let full = cfg.rated_steuve_kw();
                    from + (full - from) * (t_s - t0) / cfg.release_ramp_s
                }
                _ => {
                    mode = Mode::Normal;
                    self.release = None;
                    f64::INFINITY
                }
            }
        };
        self.was_dimmed = cmd.dim_14a;
        self.last_budget_kw = budget;

        let (charger_current_a, heat_pump_limit_kw) = self.allocate(t_s, budget - offline_reserve, r);

        // --- feed-in limit -------------------------------------------------
        let cfg = &self.cfg;
        let allowed_export_kw = cfg.pv_installed_kw * cmd.feed_in_limit_pct / 100.0;
        let pv_limit_pct = match (cfg.feed_in_reference, r.grid_kw, r.pv_kw) {
            (_, _, _) if cmd.feed_in_limit_pct >= 100.0 => 100.0,
            (FeedInReference::GridConnectionPoint, Some(g), Some(pv)) if cfg.pv_installed_kw > 0.0 => {
                // Site consumption does not depend on PV: pv + grid import.
                let consumption = (pv + g).max(0.0);
                let pv_max = allowed_export_kw + consumption - cfg.margin_kw;
                (pv_max / cfg.pv_installed_kw * 100.0).clamp(cmd.feed_in_limit_pct, 100.0)
            }
            _ => cmd.feed_in_limit_pct,
        };

        let status = Status {
            mode,
            pmin_kw: self.pmin_kw,
            steuve_budget_kw: budget.is_finite().then_some(budget.max(0.0)),
            steuve_kw,
            steuve_grid_kw: pv_surplus.map(|s| (steuve_kw - s).max(0.0)),
            base_load_kw: base_load,
            pv_surplus_kw: pv_surplus,
            allowed_export_kw,
            fallbacks,
        };
        (Setpoints { pv_limit_pct, charger_current_a, heat_pump_limit_kw }, status)
    }

    /// Splits the budget between heat pumps and chargers.
    ///
    /// Policy (the EMS may split freely, BK6-22-300 4.5.2 sentence 6):
    /// 1. each heat pump first gets up to 40% of its rating — the share the
    ///    regulation itself attributes to it;
    /// 2. then as many waiting cars as fit get the 6 A minimum, those that
    ///    charged least so far first;
    /// 3. what is left tops up the heat pumps to their demand, then the
    ///    chargers evenly up to their maximum.
    ///
    /// Every value is rounded *down* to what the device can do (4.6: when the
    /// exact value is impossible, go to the next lower possible one).
    fn allocate(&mut self, t_s: f64, budget_kw: f64, r: &Readings) -> (Vec<f64>, Vec<f64>) {
        let cfg = &self.cfg;
        let waiting: Vec<usize> =
            (0..cfg.chargers.len()).filter(|&i| r.chargers.get(i).is_some_and(|c| c.online && c.car_waiting)).collect();

        if budget_kw.is_infinite() {
            let currents = cfg.chargers.iter().map(|c| c.max_current_a).collect();
            let hps = cfg.heat_pumps.iter().map(|h| h.rated_kw).collect();
            for i in 0..cfg.chargers.len() {
                self.set_charger_on(i, waiting.contains(&i), t_s);
            }
            return (currents, hps);
        }

        let mut left = budget_kw.max(0.0);
        let demand = |i: usize| {
            r.heat_pumps.get(i).filter(|h| h.online).map_or(0.0, |h| h.demand_kw.min(cfg.heat_pumps[i].rated_kw))
        };

        // 1. heat pumps: guaranteed share
        let mut hp: Vec<f64> = (0..cfg.heat_pumps.len())
            .map(|i| {
                let give = demand(i).min(0.4 * cfg.heat_pumps[i].rated_kw).min(left);
                let give = if give < cfg.heat_pumps[i].min_kw { 0.0 } else { give };
                left -= give;
                give
            })
            .collect();

        // 2. chargers: 6 A minimum for as many cars as fit. Cars inside their
        // dwell time keep their current state first, then least energy first.
        let min_kw = three_phase_kw(EV_MIN_CURRENT_A);
        let mut order = waiting.clone();
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
        let mut currents = vec![0.0; cfg.chargers.len()];
        for &i in &active {
            currents[i] = EV_MIN_CURRENT_A;
            left -= min_kw;
        }

        // 3a. top up heat pumps to their demand
        for (i, give) in hp.iter_mut().enumerate() {
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

        for (i, &a) in currents.iter().enumerate() {
            self.set_charger_on(i, a > 0.0, t_s);
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
