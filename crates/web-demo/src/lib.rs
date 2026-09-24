//! The browser demo: the site physics, the gateway's controller and the
//! IEC 104 encoder, compiled to WebAssembly. Nothing is mocked in between:
//! the controller reads and writes the simulated devices through their
//! register maps, exactly like the gateway does over Modbus TCP, and every
//! telecontrol frame shown on the page is encoded by the `iec104` crate.

use std::collections::HashMap;

use control::{
    Clock, ConsumptionRule, Controller, DsoCommands, Jurisdiction, Mode, Policy, Readings, SiteConfig, Totals,
};
use devices::maps::{battery, evse, heat_pump, regs_to_u32};
use devices::sim::{DeviceId, SiteSim};
use devices::sunspec::{self, available, controls, inverter, meter};
use iec104::describe::describe;
use iec104::{Apdu, Asdu, Cause, Cp56Time2a, Element, Quality, UFunction};
use serde::Serialize;
use wasm_bindgen::prelude::*;

const CA: u16 = 1;
const CONTROL_PERIOD_S: f64 = 1.0;
/// Midnight of the simulated day (2026-04-15 UTC) for CP56Time2a time tags.
const DAY_EPOCH_MS: i64 = 1_776_211_200_000;
const YEAR: i32 = 2026;
/// Swiss scenario: expected yield 950 kWh/kWp, and late in the year most of
/// the free 3% budget is already used, so a noon curtailment runs it out.
const CH_YIELD_KWH: f64 = 114_000.0;
const CH_CURTAILED_ALREADY_KWH: f64 = 3_400.0;

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
    mode: &'static str,
    dim: bool,
    emergency: bool,
    feed_in_limit_pct: f64,
    feed_in_in_force_pct: f64,
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
    heat_pump_kw: f64,
    heat_pump_demand_kw: f64,
    heat_pump_limit_kw: f64,
    heat_pump_online: bool,
    heat_pump_opted_out: bool,
    meter_online: bool,
    inverters_online: Vec<bool>,
    chargers: Vec<ChargerOut>,
    battery: BatteryOut,
    dimmed_min_today: f64,
    curtailed_kwh_year: f64,
    budget_kwh: Option<f64>,
    budget_used_pct: Option<f64>,
    refusals: Vec<&'static str>,
    fallbacks: Vec<String>,
}

#[wasm_bindgen]
pub struct Demo {
    sim: SiteSim,
    ctl: Controller,
    cmd: DsoCommands,
    since_control: f64,
    setpoints: control::Setpoints,
    status: Option<control::Status>,
    readings: Readings,
    frames: Vec<Frame>,
    station_ns: u16,
    dso_ns: u16,
    reported_f: HashMap<u32, Option<f64>>,
    reported_b: HashMap<u32, bool>,
}

/// The depot under a country's rules, as the example configurations set it up.
fn depot_config(j: Jurisdiction) -> SiteConfig {
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
        expected_annual_yield_kwh: if j == Jurisdiction::Ch { CH_YIELD_KWH } else { 120_000.0 },
        chargers: vec![control::ChargerSpec { max_current_a: 32.0, failsafe_current_a: 6.0, opted_out: false }; 4],
        heat_pumps: vec![control::HeatPumpSpec { rated_kw: 14.0, min_kw: 3.0, opted_out: hp_opted_out }],
        batteries: vec![control::BatterySpec {
            capacity_kwh: 100.0,
            max_charge_kw: 50.0,
            max_discharge_kw: 50.0,
            min_soc_pct: 10.0,
            max_soc_pct: 95.0,
        }],
        feed_in_reference: control::FeedInReference::GridConnectionPoint,
        release_ramp_s: 300.0,
        pv_ramp_pct_per_s: 10.0 / 60.0,
        margin_kw: 0.3,
        min_dwell_s: 300.0,
        import_target_kw: 0.0,
    }
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

/// Body address of a SunSpec model in a device's chain, found the way a
/// Modbus client finds it: by walking the chain from register 40000.
fn model_body(sim: &SiteSim, dev: DeviceId, id: u16) -> Option<u16> {
    let (_, image) = sim.image(dev);
    sunspec::locate_models(image.get(2..)?)?.into_iter().find(|m| m.id == id).map(|m| m.body)
}

#[wasm_bindgen]
impl Demo {
    /// `country`: "DE", "AT" or "CH".
    #[wasm_bindgen(constructor)]
    pub fn new(start_hour: f64, seed: u32, country: &str) -> Demo {
        let j = Jurisdiction::parse(country).unwrap_or(Jurisdiction::De);
        let mut sim = SiteSim::depot(start_hour * 3600.0, seed as u64);
        // Arm the watchdogs, as the gateway does on connect.
        for i in 0..sim.chargers.len() {
            let _ = sim.write(DeviceId::Charger(i), evse::FAILSAFE_CURRENT, &[60]);
            let _ = sim.write(DeviceId::Charger(i), evse::FAILSAFE_TIMEOUT, &[30]);
        }
        for i in 0..sim.batteries.len() {
            let _ = sim.write(DeviceId::Battery(i), battery::WATCHDOG_S, &[30]);
        }
        let mut ctl = Controller::new(depot_config(j));
        if j == Jurisdiction::Ch {
            let day = (start_hour * 3600.0 / 86_400.0).floor() as i64;
            ctl = ctl.with_totals(Totals {
                day,
                year: YEAR,
                dimmed_s_today: 0.0,
                produced_kwh_year: 0.0,
                curtailed_kwh_year: CH_CURTAILED_ALREADY_KWH,
            });
        }
        let mut demo = Demo {
            sim,
            ctl,
            cmd: DsoCommands::default(),
            since_control: 0.0,
            setpoints: control::Setpoints {
                pv_limit_pct: 100.0,
                charger_current_a: vec![],
                heat_pump_limit_kw: vec![],
                battery_kw: vec![],
            },
            status: None,
            readings: Readings::default(),
            frames: Vec::new(),
            station_ns: 0,
            dso_ns: 0,
            reported_f: HashMap::new(),
            reported_b: HashMap::new(),
        };
        demo.control_cycle();
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

    /// Advances simulated time, one second of physics per step.
    pub fn advance(&mut self, seconds: f64) {
        let mut left = seconds;
        while left > 1e-9 {
            let dt = left.min(1.0);
            self.sim.step(dt);
            self.since_control += dt;
            left -= dt;
            if self.since_control >= CONTROL_PERIOD_S - 1e-9 {
                self.since_control = 0.0;
                self.control_cycle();
                self.report(false);
            }
        }
    }

    /// DSO: reduce consumption on/off (C_SC_NA_1, IOA 5001).
    pub fn command_dim(&mut self, on: bool) {
        self.single_command(5001, on);
        self.cmd.dim = on;
    }

    /// DSO: emergency on/off (C_SC_NA_1, IOA 5003).
    pub fn command_emergency(&mut self, on: bool) {
        self.single_command(5003, on);
        self.cmd.emergency = on;
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
        self.cmd.feed_in_limit_pct = pct;
        self.station_send(&cmd.mirror(Cause::ActivationCon, false));
        self.station_send(&cmd.mirror(Cause::ActivationTermination, false));
        true
    }

    /// Fault injection: "meter", "inverter0", "charger2", "heatpump0", "battery0".
    pub fn set_device_online(&mut self, name: &str, online: bool) -> bool {
        let idx = |prefix: &str, n: usize| name.strip_prefix(prefix)?.parse::<usize>().ok().filter(|&i| i < n);
        let dev = if name == "meter" {
            DeviceId::Meter
        } else if let Some(i) = idx("inverter", self.sim.inverters.len()) {
            DeviceId::Inverter(i)
        } else if let Some(i) = idx("charger", self.sim.chargers.len()) {
            DeviceId::Charger(i)
        } else if let Some(i) = idx("heatpump", self.sim.heat_pumps.len()) {
            DeviceId::HeatPump(i)
        } else if let Some(i) = idx("battery", self.sim.batteries.len()) {
            DeviceId::Battery(i)
        } else {
            return false;
        };
        self.sim.set_online(dev, online);
        true
    }

    pub fn time_s(&self) -> f64 {
        self.sim.t_s
    }

    pub fn state_json(&self) -> String {
        let st = self.status.as_ref();
        let r = &self.readings;
        let t = self.sim.t_s;
        let cfg = self.ctl.config();
        let chargers = self
            .sim
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
                    setpoint_a: self.setpoints.charger_current_a.get(i).copied().unwrap_or(0.0),
                    current_a: c.current_a,
                    kw: c.power_kw(),
                    session_kwh: c.car.as_ref().map_or(0.0, |car| car.charged_kwh),
                    needs_kwh: c.car.as_ref().map_or(0.0, |car| car.needs_kwh),
                }
            })
            .collect();
        let hp = &self.sim.heat_pumps[0];
        let b = &self.sim.batteries[0];
        let budget_kwh = cfg.policy.curtailment_budget_pct.map(|p| p / 100.0 * cfg.expected_annual_yield_kwh);
        let out = StateOut {
            t_s: t,
            jurisdiction: cfg.policy.jurisdiction.code(),
            mode: st.map_or("normal", |s| mode_name(s.mode)),
            dim: self.cmd.dim,
            emergency: self.cmd.emergency,
            feed_in_limit_pct: self.cmd.feed_in_limit_pct,
            feed_in_in_force_pct: st.map_or(100.0, |s| s.feed_in_limit_pct),
            grid_kw: r.grid_kw,
            pv_kw: self.sim.pv_kw(),
            pv_available_kw: self.sim.pv_available_kw(),
            base_kw: self.sim.base_kw,
            steuve_kw: self.sim.chargers_kw() + self.sim.heat_pumps_kw() + self.sim.batteries_kw().max(0.0),
            steuve_grid_kw: st.and_then(|s| s.steuve_grid_kw),
            steuve_budget_kw: st.and_then(|s| s.steuve_budget_kw),
            floor_kw: self.ctl.floor_kw(),
            allowed_export_kw: st.map_or(120.0, |s| s.allowed_export_kw),
            export_kw: (-self.sim.grid_kw()).max(0.0),
            pv_limit_pct: self.setpoints.pv_limit_pct,
            outdoor_c: self.sim.outdoor_c(),
            heat_pump_kw: hp.power_kw,
            heat_pump_demand_kw: hp.demand_kw,
            heat_pump_limit_kw: hp.limit_kw,
            heat_pump_online: hp.online,
            heat_pump_opted_out: cfg.heat_pumps[0].opted_out,
            meter_online: self.sim.meter_online,
            inverters_online: self.sim.inverters.iter().map(|i| i.online).collect(),
            chargers,
            battery: BatteryOut {
                online: b.online,
                kw: b.power_kw,
                setpoint_kw: self.setpoints.battery_kw.first().copied().unwrap_or(0.0),
                soc_pct: b.soc_pct,
                watchdog: b.in_watchdog(t),
            },
            dimmed_min_today: st.map_or(0.0, |s| s.totals.dimmed_s_today / 60.0),
            curtailed_kwh_year: st.map_or(0.0, |s| s.totals.curtailed_kwh_year),
            budget_kwh,
            budget_used_pct: st.and_then(|s| s.curtailment_budget_used_pct),
            refusals: st.map_or(vec![], |s| s.refusals.iter().map(refusal_name).collect()),
            fallbacks: st.map_or(vec![], |s| s.fallbacks.iter().map(fallback_name).collect()),
        };
        serde_json::to_string(&out).unwrap_or_default()
    }

    /// Frames produced since the last call, oldest first.
    pub fn take_frames_json(&mut self) -> String {
        let f = std::mem::take(&mut self.frames);
        serde_json::to_string(&f).unwrap_or_default()
    }
}

impl Demo {
    fn single_command(&mut self, ioa: u32, on: bool) {
        let cmd = Asdu::single(Cause::Activation, CA, ioa, Element::SingleCommand { on, select: false, qualifier: 0 });
        self.dso_send(&cmd);
        self.station_send(&cmd.mirror(Cause::ActivationCon, false));
        self.station_send(&cmd.mirror(Cause::ActivationTermination, false));
    }

    /// Reads every device through its registers, runs the controller and
    /// writes the setpoints back — one gateway cycle.
    fn control_cycle(&mut self) {
        let sim = &mut self.sim;

        let mut pv = Some(0.0);
        let mut avail = Some(0.0);
        let mut control_bodies = Vec::new();
        for i in 0..sim.inverters.len() {
            let dev = DeviceId::Inverter(i);
            let kw = model_body(sim, dev, inverter::ID)
                .and_then(|b| sim.read(dev, b, inverter::LEN as u16).ok())
                .map(|m| sunspec::scaled(m[inverter::W] as i16, m[inverter::W_SF] as i16) / 1000.0);
            let av = model_body(sim, dev, available::ID)
                .and_then(|b| sim.read(dev, b, available::LEN as u16).ok())
                .map(|m| sunspec::scaled(m[available::W_AVAIL] as i16, m[available::W_AVAIL_SF] as i16) / 1000.0);
            pv = pv.zip(kw).map(|(a, b)| a + b);
            avail = avail.zip(av).map(|(a, b)| a + b);
            control_bodies.push(model_body(sim, dev, controls::ID));
        }
        let grid = model_body(sim, DeviceId::Meter, meter::ID)
            .and_then(|b| sim.read(DeviceId::Meter, b, meter::LEN as u16).ok())
            .map(|m| sunspec::scaled(m[meter::W] as i16, m[meter::W_SF] as i16) / 1000.0);

        let chargers = (0..sim.chargers.len())
            .map(|i| match sim.read(DeviceId::Charger(i), 0, evse::LEN) {
                Ok(r) => control::ChargerReading {
                    online: true,
                    car_waiting: matches!(
                        r[evse::STATUS as usize],
                        evse::STATUS_CONNECTED | evse::STATUS_CHARGING | evse::STATUS_FAILSAFE
                    ),
                    current_a: r[evse::CURRENT as usize] as f64 / 10.0,
                    power_kw: regs_to_u32(&r[evse::POWER as usize..]) as f64 / 1000.0,
                    session_kwh: regs_to_u32(&r[evse::SESSION_ENERGY as usize..]) as f64 / 1000.0,
                },
                Err(_) => control::ChargerReading::default(),
            })
            .collect();
        let heat_pumps = (0..sim.heat_pumps.len())
            .map(|i| match sim.read(DeviceId::HeatPump(i), 0, heat_pump::LEN) {
                Ok(r) => control::HeatPumpReading {
                    online: true,
                    power_kw: r[heat_pump::POWER as usize] as f64 / 10.0,
                    demand_kw: r[heat_pump::DEMAND as usize] as f64 / 10.0,
                },
                Err(_) => control::HeatPumpReading::default(),
            })
            .collect();
        let batteries = (0..sim.batteries.len())
            .map(|i| match sim.read(DeviceId::Battery(i), 0, battery::LEN) {
                Ok(r) => control::BatteryReading {
                    online: true,
                    soc_pct: r[battery::SOC as usize] as f64 / 10.0,
                    power_kw: r[battery::POWER as usize] as i16 as f64 / 10.0,
                },
                Err(_) => control::BatteryReading::default(),
            })
            .collect();

        let readings = Readings { grid_kw: grid, pv_kw: pv, pv_available_kw: avail, chargers, heat_pumps, batteries };
        let clock = Clock::at(sim.t_s, YEAR);
        let (sp, st) = self.ctl.step(&clock, &self.cmd, &readings);

        for (i, body) in control_bodies.iter().enumerate() {
            if let Some(b) = body {
                let raw = sunspec::unscaled(sp.pv_limit_pct, -1) as u16;
                let _ = sim.write(DeviceId::Inverter(i), b + controls::W_MAX_LIM_PCT as u16, &[raw]);
                let _ = sim.write(DeviceId::Inverter(i), b + controls::W_MAX_LIM_ENA as u16, &[1]);
            }
        }
        for (i, &a) in sp.charger_current_a.iter().enumerate() {
            let _ = sim.write(DeviceId::Charger(i), evse::CURRENT_LIMIT, &[(a * 10.0).round() as u16]);
        }
        for (i, &kw) in sp.heat_pump_limit_kw.iter().enumerate() {
            let _ = sim.write(DeviceId::HeatPump(i), heat_pump::POWER_LIMIT, &[(kw * 10.0).floor() as u16]);
        }
        for (i, &kw) in sp.battery_kw.iter().enumerate() {
            let _ = sim.write(DeviceId::Battery(i), battery::SETPOINT, &[(kw * 10.0).round() as i16 as u16]);
        }
        self.readings = readings;
        self.setpoints = sp;
        self.status = Some(st);
    }

    fn time_tag(&self) -> Cp56Time2a {
        Cp56Time2a::from_unix_ms(DAY_EPOCH_MS + (self.sim.t_s * 1000.0) as i64)
    }

    /// Spontaneous reports, or every point for a general interrogation.
    fn report(&mut self, all: bool) {
        let Some(st) = self.status.clone() else { return };
        let time = self.time_tag();
        let b = self.readings.batteries.first().filter(|b| b.online);
        let floats = [
            (1001, self.readings.grid_kw),
            (1002, self.readings.pv_kw),
            (1003, Some(st.steuve_kw)),
            (1004, st.steuve_grid_kw),
            (1005, Some(st.floor_kw)),
            (1006, Some(self.cmd.feed_in_limit_pct)),
            (1007, Some(self.setpoints.pv_limit_pct)),
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
        ];
        let cause = if all { Cause::Interrogated } else { Cause::Spontaneous };
        for (ioa, v) in floats {
            // A wider deadband than the gateway's keeps the page log readable.
            let deadband = match ioa {
                1011 | 1012 => 15.0, // minutes / kWh counters: every quarter of an hour is enough here
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
            t_s: self.sim.t_s,
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
    fn dimming_in_the_browser_demo_respects_pmin() {
        let mut d = Demo::new(17.75, 7, "DE");
        d.advance(30.0);
        assert!(state(&d)["steuve_kw"].as_f64().unwrap() > 60.0);
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
        let mut d = Demo::new(12.5, 7, "DE");
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
        let mut d = Demo::new(17.75, 7, "DE");
        d.advance(10.0);
        d.set_device_online("charger1", false);
        d.advance(40.0);
        let s = state(&d);
        assert_eq!(s["chargers"][1]["current_a"].as_f64().unwrap(), 6.0);
        assert!(s["fallbacks"].as_array().unwrap().iter().any(|f| f == "charger 2 offline"));
    }

    #[test]
    fn austria_caps_export_at_70_percent_by_itself() {
        let mut d = Demo::new(12.5, 7, "AT");
        d.set_device_online("battery0", false);
        d.advance(600.0);
        let s = state(&d);
        assert_eq!(s["feed_in_in_force_pct"], 70.0);
        assert!(s["export_kw"].as_f64().unwrap() <= 84.0 + 1.0, "{s}");
    }

    #[test]
    fn swiss_budget_runs_out_and_an_emergency_still_curtails() {
        let mut d = Demo::new(12.5, 7, "CH");
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
    fn battery_stores_solar_and_its_watchdog_idles_it() {
        let mut d = Demo::new(12.5, 7, "DE");
        d.advance(120.0);
        let s = state(&d);
        assert!(s["battery"]["kw"].as_f64().unwrap() > 1.0, "charging from surplus: {s}");
        d.set_device_online("battery0", false);
        d.advance(40.0);
        let s = state(&d);
        assert_eq!(s["battery"]["kw"].as_f64().unwrap(), 0.0);
        assert!(s["fallbacks"].as_array().unwrap().iter().any(|f| f == "battery offline"));
    }
}
