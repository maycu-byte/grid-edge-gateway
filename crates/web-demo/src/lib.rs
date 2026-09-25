//! The browser demo: the site physics, the gateway's real-time controller,
//! the MPC planner and the IEC 104 encoder, compiled to WebAssembly.
//! Nothing is mocked in between: the controller reads and writes the
//! simulated devices through their register maps, exactly like the gateway
//! does over Modbus TCP, and every telecontrol frame shown on the page is
//! encoded by the `iec104` crate.
//!
//! A shadow copy of the site runs alongside on rules alone — same weather,
//! same DSO commands, same faults — so the page can show what the plan saved.

use std::collections::HashMap;

use closedloop::{ClosedLoop, Scenario, Strategy, site_config};
use control::{Controller, Jurisdiction, Mode, Totals};
use devices::climate::{Season, Tariff};
use devices::sim::{Building, DeviceId, Weather};
use iec104::describe::describe;
use iec104::{Apdu, Asdu, Cause, Cp56Time2a, Element, Quality, UFunction};
use serde::Serialize;
use wasm_bindgen::prelude::*;

const CA: u16 = 1;
/// Control period in the browser (the gateway uses 1 s; 2 s keeps fast
/// playback smooth and changes nothing a person can see).
const CONTROL_DT_S: f64 = 2.0;
/// Swiss scenario: late in the year most of the free 3% budget is already
/// used, so a noon curtailment runs it out.
const CH_CURTAILED_ALREADY_KWH: f64 = 3_400.0;
/// The window in which the DSO has announced it may dim (preventive §14a
/// control is at most 2 h a day). The planner is told; you decide.
const ANNOUNCED_DIM_H: (f64, f64) = (17.5, 19.5);
/// Longest random wait before power returns after a dimming (the UK Smart
/// Charge Points Regulations 2021 use up to 600 s).
const RELEASE_DELAY_MAX_S: f64 = 600.0;

#[derive(Serialize, Clone)]
pub struct Frame {
    t_s: f64,
    dir: &'static str,
    hex: String,
    text: String,
}

#[derive(Serialize)]
struct ChargerOut {
    online: bool,
    status: &'static str,
    setpoint_a: f64,
    current_a: f64,
    kw: f64,
    session_kwh: f64,
    needs_kwh: f64,
    /// Hours until the car leaves.
    leaves_in_h: Option<f64>,
}

#[derive(Serialize)]
struct InverterOut {
    online: bool,
    kw: f64,
    /// Limit the gateway sent, %.
    limit_pct: f64,
    /// Fault injection: the inverter ignores its limit.
    ignores_limit: bool,
    /// The gateway has noticed and reported it (point 2008).
    reported: bool,
}

/// One finished dimming, for the page's report list.
#[derive(Serialize)]
struct ReportOut {
    seq: u32,
    start_s: f64,
    end_s: f64,
    duration_s: f64,
    floor_kw: f64,
    max_kw: Option<f64>,
    exceeded_s: f64,
    unverified_s: f64,
    verdict: &'static str,
    emergency: bool,
    sha256: String,
    previous_sha256: String,
    file: String,
}

#[derive(Serialize)]
struct BatteryOut {
    online: bool,
    kw: f64,
    setpoint_kw: f64,
    soc_pct: f64,
    watchdog: bool,
}

#[derive(Serialize)]
struct StateOut {
    t_s: f64,
    jurisdiction: &'static str,
    season: &'static str,
    strategy: &'static str,
    mode: &'static str,
    /// While releasing: seconds of random wait left before power returns.
    release_wait_s: Option<f64>,
    /// A dimming is being recorded for its report right now.
    recording: bool,
    dim: bool,
    emergency: bool,
    feed_in_limit_pct: f64,
    feed_in_in_force_pct: f64,
    price_eur_mwh: f64,
    import_eur_kwh: f64,
    grid_kw: Option<f64>,
    pv_kw: f64,
    pv_available_kw: f64,
    base_kw: f64,
    steuve_kw: f64,
    steuve_grid_kw: Option<f64>,
    steuve_budget_kw: Option<f64>,
    floor_kw: f64,
    allowed_export_kw: f64,
    export_kw: f64,
    pv_limit_pct: f64,
    outdoor_c: f64,
    indoor_c: f64,
    comfort_min_c: f64,
    heat_pump_kw: f64,
    heat_pump_demand_kw: f64,
    heat_pump_limit_kw: f64,
    heat_pump_external: bool,
    heat_pump_online: bool,
    heat_pump_opted_out: bool,
    meter_online: bool,
    inverters_online: Vec<bool>,
    inverters: Vec<InverterOut>,
    chargers: Vec<ChargerOut>,
    battery: BatteryOut,
    dimmed_min_today: f64,
    curtailed_kwh_year: f64,
    budget_kwh: Option<f64>,
    budget_used_pct: Option<f64>,
    refusals: Vec<&'static str>,
    fallbacks: Vec<String>,
    /// Running totals since the start, for this site and its rules-only shadow.
    cost_eur: f64,
    ageing_eur: f64,
    demand_eur: f64,
    peak_kw: f64,
    discomfort_kh: f64,
    ev_unmet_kwh: f64,
    shadow_cost_eur: f64,
    shadow_ageing_eur: f64,
    shadow_demand_eur: f64,
    shadow_peak_kw: f64,
    shadow_discomfort_kh: f64,
    shadow_ev_unmet_kwh: f64,
    /// Energy held in the battery and the building's temperature, here and in
    /// the shadow: what a plan bought ahead of a price peak but has not used yet.
    battery_kwh: f64,
    shadow_battery_kwh: f64,
    shadow_indoor_c: f64,
    plan_age_min: Option<f64>,
    plans: u32,
    solve_ms: f64,
}

/// The current plan, for the page's charts: one entry per 15-minute step.
#[derive(Serialize)]
struct PlanOut {
    start_h: f64,
    step_h: f64,
    grid_kw: Vec<f64>,
    battery_kw: Vec<f64>,
    soc_pct: Vec<f64>,
    indoor_c: Vec<f64>,
    heat_pump_kw: Vec<f64>,
    ev_kw: Vec<f64>,
    dim: Vec<bool>,
    price_eur_mwh: Vec<f64>,
}

#[wasm_bindgen]
pub struct Demo {
    cl: ClosedLoop,
    shadow: ClosedLoop,
    epoch_ms: i64,
    frames: Vec<Frame>,
    station_ns: u16,
    dso_ns: u16,
    reported_f: HashMap<u32, Option<f64>>,
    reported_b: HashMap<u32, bool>,
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Normal => "normal",
        Mode::Dimmed => "dimmed",
        Mode::Releasing => "releasing",
    }
}

fn fallback_name(f: &control::Fallback) -> String {
    use control::Fallback::*;
    match f {
        MeterOffline => "meter offline".into(),
        MeterImplausible => "meter implausible".into(),
        PvOffline => "inverter offline".into(),
        PvImplausible => "inverter implausible".into(),
        InverterIgnoresLimit(i) => format!("inverter {} ignores its limit", i + 1),
        ChargerOffline(i) => format!("charger {} offline", i + 1),
        HeatPumpOffline(_) => "heat pump offline".into(),
        BatteryOffline(_) => "battery offline".into(),
    }
}

fn refusal_name(r: &control::Refusal) -> &'static str {
    match r {
        control::Refusal::DimDayLimitReached => "contract day limit reached",
        control::Refusal::CurtailmentBudgetExhausted => "curtailment budget used up",
    }
}

/// Midnight of the simulated first day for CP56Time2a time tags (UTC).
fn epoch_ms(season: Season) -> i64 {
    season.epoch_ms()
}

fn new_loop(j: Jurisdiction, season: Season, strategy: Strategy, start_hour: f64, seed: u64) -> ClosedLoop {
    let scenario = Scenario {
        season,
        jurisdiction: j,
        weather: Weather::Fair,
        seed,
        start_h: start_hour,
        dim_windows_h: vec![ANNOUNCED_DIM_H],
        auto_dso: false,
    };
    let mut cl = ClosedLoop::new(scenario, strategy, CONTROL_DT_S);
    let mut cfg = site_config(j);
    cfg.release_delay_max_s = RELEASE_DELAY_MAX_S;
    cfg.release_delay_seed = seed;
    let mut ctl = Controller::new(cfg).with_report_epoch(epoch_ms(season));
    if j == Jurisdiction::Ch {
        let day = (start_hour * 3600.0 / 86_400.0).floor() as i64;
        ctl = ctl.with_totals(Totals {
            day,
            year: 2026,
            dimmed_s_today: 0.0,
            produced_kwh_year: 0.0,
            curtailed_kwh_year: CH_CURTAILED_ALREADY_KWH,
        });
    }
    cl.ctl = ctl;
    cl
}

#[wasm_bindgen]
impl Demo {
    /// `country`: "DE", "AT" or "CH"; `season`: "spring", "winter" or a
    /// date of 2025 ("2025-07-14"), simulated on its real prices and weather;
    /// `strategy`: "rules", "mpc" or "mpc-cc".
    #[wasm_bindgen(constructor)]
    pub fn new(start_hour: f64, seed: u32, country: &str, season: &str, strategy: &str) -> Demo {
        let j = Jurisdiction::parse(country).unwrap_or(Jurisdiction::De);
        let season = Season::parse(season).unwrap_or(Season::Spring);
        let strategy = Strategy::parse(strategy).unwrap_or(Strategy::Rules);
        let cl = new_loop(j, season, strategy, start_hour, seed as u64);
        let shadow = new_loop(j, season, Strategy::Rules, start_hour, seed as u64);
        let mut demo = Demo {
            cl,
            shadow,
            epoch_ms: epoch_ms(season),
            frames: Vec::new(),
            station_ns: 0,
            dso_ns: 0,
            reported_f: HashMap::new(),
            reported_b: HashMap::new(),
        };
        // The control centre connects: STARTDT, then a general interrogation.
        demo.push(true, Apdu::U(UFunction::StartDtAct).encode());
        demo.push(false, Apdu::U(UFunction::StartDtCon).encode());
        let eoi = Asdu::single(Cause::Initialized, CA, 0, Element::EndOfInit { coi: 0 });
        demo.station_send(&eoi);
        let gi = Asdu::single(Cause::Activation, CA, 0, Element::Interrogation { qoi: 20 });
        demo.dso_send(&gi);
        demo.station_send(&gi.mirror(Cause::ActivationCon, false));
        demo.report(true);
        demo.station_send(&gi.mirror(Cause::ActivationTermination, false));
        demo
    }

    /// Advances simulated time on the site and its shadow.
    pub fn advance(&mut self, seconds: f64) {
        self.cl.advance(seconds);
        self.shadow.advance(seconds);
        self.report(false);
    }

    /// DSO: reduce consumption on/off (C_SC_NA_1, IOA 5001).
    pub fn command_dim(&mut self, on: bool) {
        self.single_command(5001, on);
        self.cl.cmd.dim = on;
        self.shadow.cmd.dim = on;
        self.cl.replan_soon();
    }

    /// DSO: emergency on/off (C_SC_NA_1, IOA 5003).
    pub fn command_emergency(&mut self, on: bool) {
        self.single_command(5003, on);
        self.cl.cmd.emergency = on;
        self.shadow.cmd.emergency = on;
    }

    /// DSO: feed-in limit in % (C_SE_NC_1, IOA 5002). Out-of-range values
    /// get a negative confirmation, as from the real station.
    pub fn command_feed_in(&mut self, pct: f64) -> bool {
        let cmd = Asdu::single(
            Cause::Activation,
            CA,
            5002,
            Element::SetpointFloat { value: pct as f32, select: false, qualifier: 0 },
        );
        self.dso_send(&cmd);
        if !(0.0..=100.0).contains(&pct) {
            self.station_send(&cmd.mirror(Cause::ActivationCon, true));
            return false;
        }
        self.cl.cmd.feed_in_limit_pct = pct;
        self.shadow.cmd.feed_in_limit_pct = pct;
        self.cl.replan_soon();
        self.station_send(&cmd.mirror(Cause::ActivationCon, false));
        self.station_send(&cmd.mirror(Cause::ActivationTermination, false));
        true
    }

    /// Switches the planning strategy ("rules", "mpc", "mpc-cc") without
    /// restarting the day.
    pub fn set_strategy(&mut self, name: &str) -> bool {
        match Strategy::parse(name) {
            Some(s) => {
                self.cl.strategy = s;
                self.cl.plan = None;
                self.cl.ctl.set_guidance(None);
                self.cl.replan_soon();
                true
            }
            None => false,
        }
    }

    /// Fault injection: "meter", "inverter0", "charger2", "heatpump0", "battery0".
    pub fn set_device_online(&mut self, name: &str, online: bool) -> bool {
        let sim = &self.cl.sim;
        let idx = |prefix: &str, n: usize| name.strip_prefix(prefix)?.parse::<usize>().ok().filter(|&i| i < n);
        let dev = if name == "meter" {
            DeviceId::Meter
        } else if let Some(i) = idx("inverter", sim.inverters.len()) {
            DeviceId::Inverter(i)
        } else if let Some(i) = idx("charger", sim.chargers.len()) {
            DeviceId::Charger(i)
        } else if let Some(i) = idx("heatpump", sim.heat_pumps.len()) {
            DeviceId::HeatPump(i)
        } else if let Some(i) = idx("battery", sim.batteries.len()) {
            DeviceId::Battery(i)
        } else {
            return false;
        };
        self.cl.sim.set_online(dev, online);
        self.shadow.sim.set_online(dev, online);
        true
    }

    /// Fault injection: inverter `i` keeps producing whatever the sun
    /// allows, whatever limit it is sent (it still reports the limit back).
    pub fn set_inverter_ignores_limit(&mut self, i: usize, on: bool) -> bool {
        if i >= self.cl.sim.inverters.len() {
            return false;
        }
        self.cl.sim.inverters[i].ignores_limit = on;
        self.shadow.sim.inverters[i].ignores_limit = on;
        true
    }

    /// Reports of the dimmings finished so far, oldest first.
    pub fn reports_json(&self) -> String {
        let out: Vec<ReportOut> = self
            .cl
            .ctl
            .reports()
            .iter()
            .map(|r| ReportOut {
                seq: r.seq,
                start_s: r.start_s,
                end_s: r.end_s,
                duration_s: r.duration_s(),
                floor_kw: r.floor_kw,
                max_kw: r.max_after_grace_kw,
                exceeded_s: r.exceeded_s,
                unverified_s: r.unverified_s,
                verdict: r.verdict.name(),
                emergency: r.emergency,
                sha256: r.sha256.clone(),
                previous_sha256: r.previous_sha256.clone(),
                file: r.file_name(),
            })
            .collect();
        serde_json::to_string(&out).unwrap_or_default()
    }

    /// One report as CSV, as the gateway writes it to disk.
    pub fn report_csv(&self, seq: u32) -> String {
        self.cl.ctl.reports().iter().find(|r| r.seq == seq).map(|r| r.to_csv()).unwrap_or_default()
    }

    pub fn time_s(&self) -> f64 {
        self.cl.sim.t_s
    }

    pub fn state_json(&self) -> String {
        let cl = &self.cl;
        let sim = &cl.sim;
        let st = cl.status.as_ref();
        let t = sim.t_s;
        let cfg = cl.ctl.config();
        let chargers = sim
            .chargers
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let status = match &c.car {
                    _ if !c.online => "offline",
                    None => "available",
                    Some(car) if car.charged_kwh >= car.needs_kwh => "finished",
                    Some(_) if c.current_a > 0.0 && c.in_failsafe(t) => "failsafe",
                    Some(_) if c.current_a > 0.0 => "charging",
                    Some(_) => "waiting",
                };
                ChargerOut {
                    online: c.online,
                    status,
                    setpoint_a: cl.setpoints.charger_current_a.get(i).copied().unwrap_or(0.0),
                    current_a: c.current_a,
                    kw: c.power_kw(),
                    session_kwh: c.car.as_ref().map_or(0.0, |car| car.charged_kwh),
                    needs_kwh: c.car.as_ref().map_or(0.0, |car| car.needs_kwh),
                    leaves_in_h: c.car.as_ref().map(|car| (car.departure_s - t) / 3600.0),
                }
            })
            .collect();
        let hp = &sim.heat_pumps[0];
        let b = &sim.batteries[0];
        let da = sim.climate.day_ahead_eur_mwh(t);
        let budget_kwh = cfg.policy.curtailment_budget_pct.map(|p| p / 100.0 * cfg.expected_annual_yield_kwh);
        let stored_kwh = |c: &ClosedLoop| -> f64 {
            c.sim.batteries.iter().filter(|b| b.online).map(|b| b.soc_pct / 100.0 * b.capacity_kwh).sum()
        };
        let unmet = |c: &ClosedLoop| {
            let start = c.scenario.start_h * 3600.0;
            c.sim.departures.iter().filter(|d| d.at_s >= start).map(|d| d.unmet_kwh()).sum::<f64>()
        };
        let out = StateOut {
            t_s: t,
            jurisdiction: cfg.policy.jurisdiction.code(),
            season: sim.climate.season.name(),
            strategy: cl.strategy.name(),
            mode: st.map_or("normal", |s| mode_name(s.mode)),
            release_wait_s: st.and_then(|s| s.release_wait_s),
            recording: st.is_some_and(|s| s.mode == Mode::Dimmed),
            dim: cl.cmd.dim,
            emergency: cl.cmd.emergency,
            feed_in_limit_pct: cl.cmd.feed_in_limit_pct,
            feed_in_in_force_pct: st.map_or(100.0, |s| s.feed_in_limit_pct),
            price_eur_mwh: da,
            import_eur_kwh: Tariff::default().import_eur_kwh(da),
            grid_kw: cl.readings.grid_kw,
            pv_kw: sim.pv_kw(),
            pv_available_kw: sim.pv_available_kw(),
            base_kw: sim.base_kw,
            steuve_kw: sim.chargers_kw() + sim.heat_pumps_kw() + sim.batteries_kw().max(0.0),
            steuve_grid_kw: st.and_then(|s| s.steuve_grid_kw),
            steuve_budget_kw: st.and_then(|s| s.steuve_budget_kw),
            floor_kw: cl.ctl.floor_kw(),
            allowed_export_kw: st.map_or(120.0, |s| s.allowed_export_kw),
            export_kw: (-sim.grid_kw()).max(0.0),
            pv_limit_pct: cl.setpoints.pv_limit_pct,
            outdoor_c: sim.outdoor_c(),
            indoor_c: sim.building.indoor_c,
            comfort_min_c: Building::comfort_min_c(t),
            heat_pump_kw: hp.power_kw,
            heat_pump_demand_kw: hp.demand_kw,
            heat_pump_limit_kw: hp.limit_kw,
            heat_pump_external: hp.external(t),
            heat_pump_online: hp.online,
            heat_pump_opted_out: cfg.heat_pumps[0].opted_out,
            meter_online: sim.meter_online,
            inverters_online: sim.inverters.iter().map(|i| i.online).collect(),
            inverters: sim
                .inverters
                .iter()
                .enumerate()
                .map(|(i, inv)| InverterOut {
                    online: inv.online,
                    kw: inv.output_kw,
                    limit_pct: cl.setpoints.inverter_limit_pct.get(i).copied().unwrap_or(cl.setpoints.pv_limit_pct),
                    ignores_limit: inv.ignores_limit,
                    reported: st.is_some_and(|s| s.fallbacks.contains(&control::Fallback::InverterIgnoresLimit(i))),
                })
                .collect(),
            chargers,
            battery: BatteryOut {
                online: b.online,
                kw: b.power_kw,
                setpoint_kw: cl.setpoints.battery_kw.first().copied().unwrap_or(0.0),
                soc_pct: b.soc_pct,
                watchdog: b.in_watchdog(t),
            },
            dimmed_min_today: st.map_or(0.0, |s| s.totals.dimmed_s_today / 60.0),
            curtailed_kwh_year: st.map_or(0.0, |s| s.totals.curtailed_kwh_year),
            budget_kwh,
            budget_used_pct: st.and_then(|s| s.curtailment_budget_used_pct),
            refusals: st.map_or(vec![], |s| s.refusals.iter().map(refusal_name).collect()),
            fallbacks: st.map_or(vec![], |s| s.fallbacks.iter().map(fallback_name).collect()),
            cost_eur: cl.metrics.energy_cost_eur,
            ageing_eur: cl.metrics.degradation_eur(),
            demand_eur: cl.metrics.demand_eur(),
            peak_kw: cl.metrics.peak_quarter_kw,
            discomfort_kh: cl.metrics.discomfort_kh,
            ev_unmet_kwh: unmet(cl),
            shadow_cost_eur: self.shadow.metrics.energy_cost_eur,
            shadow_ageing_eur: self.shadow.metrics.degradation_eur(),
            shadow_demand_eur: self.shadow.metrics.demand_eur(),
            shadow_peak_kw: self.shadow.metrics.peak_quarter_kw,
            shadow_discomfort_kh: self.shadow.metrics.discomfort_kh,
            shadow_ev_unmet_kwh: unmet(&self.shadow),
            battery_kwh: stored_kwh(cl),
            shadow_battery_kwh: stored_kwh(&self.shadow),
            shadow_indoor_c: self.shadow.sim.building.indoor_c,
            plan_age_min: cl.plan.as_ref().map(|p| (t - p.made_at_s) / 60.0),
            plans: cl.metrics.plans,
            solve_ms: cl.metrics.solve_ms_mean,
        };
        serde_json::to_string(&out).unwrap_or_default()
    }

    /// The plan in force, or `null` when the site runs on rules.
    pub fn plan_json(&self) -> String {
        let Some(rec) = &self.cl.plan else { return "null".into() };
        let p = &rec.plan;
        let n = p.grid_kw.len();
        let step_h = closedloop::runner::PLAN_STEP_H;
        let cap = self.cl.sim.batteries[0].capacity_kwh;
        let out = PlanOut {
            start_h: rec.start_s / 3600.0,
            step_h,
            grid_kw: p.grid_kw.clone(),
            battery_kw: p.battery_kw.clone(),
            soc_pct: p.soc_kwh.iter().map(|e| e / cap * 100.0).collect(),
            indoor_c: p.indoor_c.clone(),
            heat_pump_kw: p.heat_pump_kw.clone(),
            ev_kw: (0..n).map(|k| p.ev_kw.iter().map(|v| v[k]).sum()).collect(),
            dim: p.dim_budget_kw.iter().map(Option::is_some).collect(),
            price_eur_mwh: (0..n)
                .map(|k| self.cl.sim.climate.day_ahead_eur_mwh(rec.start_s + (k as f64 + 0.5) * step_h * 3600.0))
                .collect(),
        };
        serde_json::to_string(&out).unwrap_or_default()
    }

    /// Day-ahead prices, €/MWh, for hours `from_h`..`to_h` (for the price strip).
    pub fn prices_json(&self, from_h: f64, to_h: f64) -> String {
        let hours: Vec<(f64, f64)> = ((from_h.floor() as i64)..(to_h.ceil() as i64))
            .map(|h| (h as f64, self.cl.sim.climate.day_ahead_eur_mwh(h as f64 * 3600.0 + 1.0)))
            .collect();
        serde_json::to_string(&hours).unwrap_or_default()
    }

    /// Frames produced since the last call, oldest first.
    pub fn take_frames_json(&mut self) -> String {
        let f = std::mem::take(&mut self.frames);
        serde_json::to_string(&f).unwrap_or_default()
    }
}

/// Feeder calculator: one site of a feeder around a reduction, the same code
/// as the rebound study (`closedloop::feeder`). The page runs it once per
/// site and sums the loads. `dim_from`/`dim_to` in local hours; a NaN
/// `dim_from` means the day without a reduction. Returns the site's
/// one-minute grid exchange and its customer-side results as JSON.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn feeder_site(
    season: &str,
    strategy: &str,
    ramp_s: f64,
    delay_max_s: f64,
    dim_from: f64,
    dim_to: f64,
    groups: u32,
    mixed: bool,
    seed: u32,
    index: u32,
) -> String {
    let case = closedloop::FeederCase {
        season: Season::parse(season).unwrap_or(Season::Winter),
        strategy: Strategy::parse(strategy).unwrap_or(Strategy::Rules),
        ramp_s: ramp_s.max(0.0),
        delay_max_s: delay_max_s.max(0.0),
        dim: (!dim_from.is_nan()).then_some((dim_from, dim_to)),
        groups: groups.max(1) as usize,
        mixed,
    };
    serde_json::to_string(&closedloop::run_site(&case, seed as u64, index as usize)).unwrap_or_default()
}

/// The window and resolution of `feeder_site`'s load curve.
#[wasm_bindgen]
pub fn feeder_meta() -> String {
    serde_json::json!({ "from_h": closedloop::feeder::FROM_H, "to_h": closedloop::feeder::TO_H, "sample_s": closedloop::feeder::SAMPLE_S })
        .to_string()
}

impl Demo {
    fn single_command(&mut self, ioa: u32, on: bool) {
        let cmd = Asdu::single(Cause::Activation, CA, ioa, Element::SingleCommand { on, select: false, qualifier: 0 });
        self.dso_send(&cmd);
        self.station_send(&cmd.mirror(Cause::ActivationCon, false));
        self.station_send(&cmd.mirror(Cause::ActivationTermination, false));
    }

    fn time_tag(&self) -> Cp56Time2a {
        Cp56Time2a::from_unix_ms(self.epoch_ms + (self.cl.sim.t_s * 1000.0) as i64)
    }

    /// Spontaneous reports, or every point for a general interrogation.
    fn report(&mut self, all: bool) {
        let Some(st) = self.cl.status.clone() else { return };
        let time = self.time_tag();
        let r = &self.cl.readings;
        let b = r.batteries.first().filter(|b| b.online);
        let floats = [
            (1001, r.grid_kw),
            (1002, r.pv_kw),
            (1003, Some(st.steuve_kw)),
            (1004, st.steuve_grid_kw),
            (1005, Some(st.floor_kw)),
            (1006, Some(self.cl.cmd.feed_in_limit_pct)),
            (1007, Some(self.cl.setpoints.pv_limit_pct)),
            (1008, Some(st.feed_in_limit_pct)),
            (1009, b.map(|b| b.power_kw)),
            (1010, b.map(|b| b.soc_pct)),
            (1011, Some(st.totals.dimmed_s_today / 60.0)),
            (1012, Some(st.totals.curtailed_kwh_year)),
            (1013, st.curtailment_budget_used_pct),
        ];
        let points = [
            (2001, st.mode == Mode::Dimmed),
            (2002, st.mode == Mode::Releasing),
            (2003, st.fallbacks.contains(&control::Fallback::MeterOffline)),
            (2004, !st.fallbacks.is_empty()),
            (2005, st.emergency),
            (2006, st.refusals.contains(&control::Refusal::DimDayLimitReached)),
            (2007, st.refusals.contains(&control::Refusal::CurtailmentBudgetExhausted)),
            (2008, st.fallbacks.iter().any(|f| matches!(f, control::Fallback::InverterIgnoresLimit(_)))),
        ];
        let cause = if all { Cause::Interrogated } else { Cause::Spontaneous };
        for (ioa, v) in floats {
            // A wider deadband than the gateway's keeps the page log readable.
            let deadband = match ioa {
                1011 | 1012 => 15.0,
                1006..=1008 | 1010 | 1013 => 1.0,
                _ => 3.0,
            };
            let moved = match (self.reported_f.get(&ioa).copied().flatten(), v) {
                (Some(a), Some(b)) => (a - b).abs() >= deadband,
                (None, None) => false,
                _ => true,
            };
            if all || moved || !self.reported_f.contains_key(&ioa) {
                self.reported_f.insert(ioa, v);
                let e = match v {
                    Some(v) => Element::FloatTime { value: v as f32, quality: Quality::GOOD, time },
                    None => Element::FloatTime { value: 0.0, quality: Quality::invalid(), time },
                };
                self.station_send(&Asdu::single(cause, CA, ioa, e));
            }
        }
        for (ioa, on) in points {
            if all || self.reported_b.get(&ioa) != Some(&on) {
                self.reported_b.insert(ioa, on);
                let e = Element::SinglePointTime { on, quality: Quality::GOOD, time };
                self.station_send(&Asdu::single(cause, CA, ioa, e));
            }
        }
    }

    fn dso_send(&mut self, asdu: &Asdu) {
        let apdu = Apdu::I { ns: self.dso_ns, nr: self.station_ns, asdu: asdu.encode() };
        self.dso_ns = (self.dso_ns + 1) % 32_768;
        self.push(true, apdu.encode());
    }

    fn station_send(&mut self, asdu: &Asdu) {
        let apdu = Apdu::I { ns: self.station_ns, nr: self.dso_ns, asdu: asdu.encode() };
        self.station_ns = (self.station_ns + 1) % 32_768;
        self.push(false, apdu.encode());
    }

    fn push(&mut self, from_dso: bool, bytes: Vec<u8>) {
        self.frames.push(Frame {
            t_s: self.cl.sim.t_s,
            dir: if from_dso { "dso" } else { "site" },
            text: describe(&bytes),
            hex: bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
        });
        if self.frames.len() > 2_000 {
            self.frames.drain(..1_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(d: &Demo) -> serde_json::Value {
        serde_json::from_str(&d.state_json()).unwrap()
    }

    #[test]
    fn dimming_in_the_browser_demo_respects_the_floor() {
        let mut d = Demo::new(17.75, 7, "DE", "spring", "rules");
        d.advance(30.0);
        assert!(state(&d)["steuve_kw"].as_f64().unwrap() > 40.0);
        d.command_dim(true);
        d.advance(10.0);
        let s = state(&d);
        assert_eq!(s["mode"], "dimmed");
        assert!(s["steuve_grid_kw"].as_f64().unwrap() <= s["floor_kw"].as_f64().unwrap(), "{s}");
        let frames: Vec<serde_json::Value> = serde_json::from_str(&d.take_frames_json()).unwrap();
        let texts: Vec<&str> = frames.iter().map(|f| f["text"].as_str().unwrap()).collect();
        assert!(texts.iter().any(|t| t.contains("C_SC_NA_1 act CA=1 | IOA 5001 ON")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("IOA 2001 ON")), "{texts:?}");
    }

    #[test]
    fn feed_in_limit_at_noon() {
        let mut d = Demo::new(12.5, 7, "DE", "spring", "rules");
        d.advance(60.0);
        assert!(d.command_feed_in(30.0));
        assert!(!d.command_feed_in(120.0));
        d.advance(120.0);
        let s = state(&d);
        assert!(s["export_kw"].as_f64().unwrap() <= 36.0 + 1.0, "{s}");
        assert!(s["pv_kw"].as_f64().unwrap() > 36.0, "the site and battery use PV above the export limit: {s}");
    }

    #[test]
    fn charger_offline_falls_back_to_its_own_failsafe() {
        let mut d = Demo::new(17.75, 7, "DE", "spring", "rules");
        d.advance(10.0);
        d.set_device_online("charger1", false);
        d.advance(40.0);
        let s = state(&d);
        assert_eq!(s["chargers"][1]["current_a"].as_f64().unwrap(), 6.0);
        assert!(s["fallbacks"].as_array().unwrap().iter().any(|f| f == "charger 2 offline"));
    }

    #[test]
    fn austria_caps_export_at_70_percent_by_itself() {
        let mut d = Demo::new(12.5, 7, "AT", "spring", "rules");
        d.set_device_online("battery0", false);
        d.advance(600.0);
        let s = state(&d);
        assert_eq!(s["feed_in_in_force_pct"], 70.0);
        assert!(s["export_kw"].as_f64().unwrap() <= 84.0 + 1.0, "{s}");
    }

    #[test]
    fn swiss_budget_runs_out_and_an_emergency_still_curtails() {
        let mut d = Demo::new(12.5, 7, "CH", "spring", "rules");
        d.set_device_online("battery0", false);
        d.advance(30.0);
        d.command_feed_in(0.0);
        let mut refused = false;
        for _ in 0..240 {
            d.advance(30.0);
            if state(&d)["refusals"].as_array().unwrap().iter().any(|r| r == "curtailment budget used up") {
                refused = true;
                break;
            }
        }
        assert!(refused, "{}", state(&d));
        d.advance(10.0);
        assert_eq!(state(&d)["feed_in_in_force_pct"], 100.0);
        d.command_emergency(true);
        d.advance(5.0);
        assert_eq!(state(&d)["feed_in_in_force_pct"], 0.0);
    }

    #[test]
    fn mpc_saves_money_against_its_rules_shadow_on_a_winter_evening() {
        let mut d = Demo::new(14.0, 7, "DE", "winter", "mpc");
        assert_ne!(d.plan_json(), "null", "a plan from the first cycle");
        for _ in 0..(8 * 4) {
            d.advance(900.0);
        }
        let s = state(&d);
        let total = |p: &str| {
            ["cost_eur", "ageing_eur", "demand_eur"].iter().map(|k| s[format!("{p}{k}")].as_f64().unwrap()).sum::<f64>()
        };
        let mpc = total("");
        let rules = total("shadow_");
        assert!(mpc < rules, "MPC {mpc:.1} € vs rules {rules:.1} €");
        let plan: serde_json::Value = serde_json::from_str(&d.plan_json()).unwrap();
        assert_eq!(plan["soc_pct"].as_array().unwrap().len(), 97);
    }

    #[test]
    fn a_dimming_leaves_a_report_and_power_returns_after_a_random_wait() {
        let mut d = Demo::new(17.75, 7, "DE", "spring", "rules");
        d.advance(30.0);
        d.command_dim(true);
        d.advance(2.0);
        assert!(state(&d)["recording"].as_bool().unwrap());
        d.advance(898.0);
        d.command_dim(false);
        d.advance(4.0);
        let s = state(&d);
        assert_eq!(s["mode"], "releasing");
        let wait = s["release_wait_s"].as_f64().expect("a random wait");
        assert!((0.0..600.0).contains(&wait), "{wait}");
        let reps: serde_json::Value = serde_json::from_str(&d.reports_json()).unwrap();
        assert_eq!(reps.as_array().unwrap().len(), 1);
        assert_eq!(reps[0]["verdict"], "followed", "{reps}");
        assert!((reps[0]["duration_s"].as_f64().unwrap() - 900.0).abs() <= 4.0);
        let csv = d.report_csv(1);
        assert!(csv.starts_with("# Consumption reduction report"));
        assert!(csv.contains("# start: 2025-04-06T15:45:32Z"), "{}", &csv[..300]);
        assert!(control::DimmingReport::verify_csv(&csv));
        assert_eq!(d.report_csv(2), "");
    }

    #[test]
    fn an_inverter_ignoring_its_limit_is_reported_and_the_export_still_holds() {
        let mut d = Demo::new(12.5, 7, "DE", "spring", "rules");
        d.set_device_online("battery0", false);
        d.advance(60.0);
        assert!(d.set_inverter_ignores_limit(1, true));
        assert!(!d.set_inverter_ignores_limit(5, true));
        // Zero export: the limit binds even with the depot's own load.
        d.command_feed_in(0.0);
        d.advance(10.0);
        let ignoring =
            |d: &Demo| state(d)["fallbacks"].as_array().unwrap().iter().any(|f| f == "inverter 2 ignores its limit");
        assert!(!ignoring(&d), "not before the timeout");
        d.advance(120.0);
        let s = state(&d);
        assert!(ignoring(&d));
        assert!(s["inverters"][1]["reported"].as_bool().unwrap(), "{s}");
        assert!(!s["inverters"][0]["reported"].as_bool().unwrap());
        let (l0, l1) =
            (s["inverters"][0]["limit_pct"].as_f64().unwrap(), s["inverters"][1]["limit_pct"].as_f64().unwrap());
        assert!(l0 < l1, "the other inverter makes up for it: {s}");
        assert!(s["export_kw"].as_f64().unwrap() <= 1.0, "{s}");
        let frames: Vec<serde_json::Value> = serde_json::from_str(&d.take_frames_json()).unwrap();
        assert!(frames.iter().any(|f| f["text"].as_str().unwrap().contains("IOA 2008 ON")));
    }

    #[test]
    fn feeder_site_matches_the_study() {
        let with: serde_json::Value =
            serde_json::from_str(&feeder_site("winter", "rules", 300.0, 0.0, 17.5, 19.5, 1, false, 1, 0)).unwrap();
        let without: serde_json::Value =
            serde_json::from_str(&feeder_site("winter", "rules", 300.0, 0.0, f64::NAN, 0.0, 1, false, 1, 0)).unwrap();
        let a = with["load_kw"].as_array().unwrap();
        let b = without["load_kw"].as_array().unwrap();
        assert_eq!(a.len(), 330);
        let after = |v: &Vec<serde_json::Value>| v[180..].iter().map(|x| x.as_f64().unwrap()).fold(0.0, f64::max);
        assert!(after(a) > after(b) + 50.0, "a rebound after 19:30: {} vs {}", after(a), after(b));
        assert_eq!(with["ev_unmet_kwh"], 0.0);
    }

    #[test]
    fn strategy_can_be_switched_mid_day() {
        let mut d = Demo::new(10.0, 7, "DE", "spring", "rules");
        assert_eq!(d.plan_json(), "null");
        assert!(d.set_strategy("mpc"));
        d.advance(10.0);
        assert_ne!(d.plan_json(), "null");
        assert!(!d.set_strategy("magic"));
    }
}
