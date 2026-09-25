//! Physics of the demo site and the Modbus register view of every device.
//!
//! The simulation is deterministic for a given seed, so the browser demo,
//! the gateway integration tests, the site simulator and the study all see
//! the same days. Time `t_s` counts seconds from local midnight of day 0 and
//! may run over several days.

use crate::climate::{Climate, Season};
use crate::maps::{battery, evse, heat_pump, regs_to_u32, tenths_i16, u32_to_regs};
use crate::sunspec::{self, NI_INT16, available, common, controls, inverter, meter, nameplate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceId {
    Inverter(usize),
    Meter,
    Charger(usize),
    HeatPump(usize),
    Battery(usize),
}

/// Modbus exceptions a device can answer with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    IllegalDataAddress,
    IllegalDataValue,
    /// The device does not answer (fault injection).
    DeviceOffline,
}

/// How the sky behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    /// A fair day, every day: the demo's default.
    Fair,
    /// Each day's cloudiness is drawn from the season's climatology (the study).
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimConfig {
    /// Seconds after midnight of day 0 the simulation starts at.
    pub start_s: f64,
    pub seed: u64,
    pub season: Season,
    pub weather: Weather,
}

impl SimConfig {
    pub fn new(start_s: f64, seed: u64, season: Season, weather: Weather) -> Self {
        SimConfig { start_s, seed, season, weather }
    }
}

#[derive(Debug, Clone)]
pub struct InverterSim {
    pub rated_kw: f64,
    pub output_kw: f64,
    pub energy_wh: f64,
    pub limit_enabled: bool,
    pub limit_pct: f64,
    /// SunSpec WMaxLimPct_RvrtTms: revert to no limit this many seconds after
    /// the last write. 0 = never revert.
    pub revert_s: f64,
    last_limit_write_s: f64,
    pub online: bool,
    /// Fault injection: stores the limit it is sent, reports it back, and
    /// keeps producing whatever the sun allows.
    pub ignores_limit: bool,
}

#[derive(Debug, Clone)]
pub struct Car {
    pub max_current_a: f64,
    pub needs_kwh: f64,
    pub charged_kwh: f64,
    /// When the car leaves, seconds (same clock as `SiteSim::t_s`).
    pub departure_s: f64,
}

#[derive(Debug, Clone)]
pub struct ChargerSim {
    pub max_current_a: f64,
    pub limit_a: f64,
    pub failsafe_a: f64,
    pub failsafe_timeout_s: f64,
    last_heartbeat_s: f64,
    pub car: Option<Car>,
    pub current_a: f64,
    pub online: bool,
}

impl ChargerSim {
    pub fn in_failsafe(&self, now_s: f64) -> bool {
        self.failsafe_timeout_s > 0.0 && now_s - self.last_heartbeat_s > self.failsafe_timeout_s
    }

    pub fn power_kw(&self) -> f64 {
        crate::three_phase_kw(self.current_a)
    }
}

/// A heat pump with its own thermostat, heating the depot's building.
#[derive(Debug, Clone)]
pub struct HeatPumpSim {
    pub rated_kw: f64,
    pub min_kw: f64,
    pub limit_kw: f64,
    /// External power request from the EMS, kW; `None` = own thermostat.
    pub ext_power_kw: Option<f64>,
    last_ext_write_s: f64,
    /// Active setpoint of the unit's time program, °C.
    pub setpoint_c: f64,
    pub demand_kw: f64,
    pub power_kw: f64,
    pub online: bool,
    cycle_on: bool,
}

impl HeatPumpSim {
    pub fn external(&self, now_s: f64) -> bool {
        self.ext_power_kw.is_some() && now_s - self.last_ext_write_s <= heat_pump::EXT_TIMEOUT_S
    }
}

/// The depot's heated volume as one thermal mass (first-order RC model).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Building {
    pub indoor_c: f64,
    /// Heat loss per kelvin of indoor–outdoor difference, kW/K.
    pub ua_kw_per_k: f64,
    /// Thermal capacity, kWh/K (time constant = cap / ua).
    pub cap_kwh_per_k: f64,
}

impl Building {
    /// Internal gains (people, machines, lighting), kW.
    pub fn gains_kw(t_s: f64) -> f64 {
        if working_hours(t_s) { 3.0 } else { 1.0 }
    }

    /// Occupied hours, when comfort matters: 06:00–22:00.
    pub fn occupied(t_s: f64) -> bool {
        (6.0..22.0).contains(&hour_of_day(t_s))
    }

    /// Lowest acceptable indoor temperature, °C.
    pub fn comfort_min_c(t_s: f64) -> f64 {
        if Self::occupied(t_s) { 20.0 } else { 17.0 }
    }

    pub const COMFORT_MAX_C: f64 = 23.0;
}

/// Coefficient of performance of an air-to-water heat pump, as a simple
/// function of outdoor temperature (illustrative: ~3 at 0 °C).
pub fn cop(outdoor_c: f64) -> f64 {
    (3.0 + 0.08 * outdoor_c).clamp(1.8, 5.0)
}

/// Time program of the heat pump's own thermostat, °C.
pub fn thermostat_setpoint_c(t_s: f64) -> f64 {
    if Building::occupied(t_s) { 21.0 } else { 18.0 }
}

fn hour_of_day(t_s: f64) -> f64 {
    t_s.rem_euclid(86_400.0) / 3600.0
}

fn working_hours(t_s: f64) -> bool {
    (7.0..18.0).contains(&hour_of_day(t_s))
}

#[derive(Debug, Clone)]
pub struct BatterySim {
    pub capacity_kwh: f64,
    /// One-way efficiency (charging and discharging each lose this much).
    pub efficiency: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub soc_pct: f64,
    pub setpoint_kw: f64,
    pub power_kw: f64,
    pub watchdog_s: f64,
    last_setpoint_s: f64,
    pub online: bool,
}

impl BatterySim {
    pub fn in_watchdog(&self, now_s: f64) -> bool {
        self.watchdog_s > 0.0 && now_s - self.last_setpoint_s > self.watchdog_s
    }
}

/// A car arriving at a charger.
#[derive(Debug, Clone, Copy)]
pub struct Arrival {
    pub at_s: f64,
    pub charger: usize,
    pub needs_kwh: f64,
    pub max_current_a: f64,
    pub departure_s: f64,
}

/// What happened when a car left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepartureLog {
    pub at_s: f64,
    pub charger: usize,
    pub needs_kwh: f64,
    pub charged_kwh: f64,
}

impl DepartureLog {
    pub fn unmet_kwh(&self) -> f64 {
        (self.needs_kwh - self.charged_kwh).max(0.0)
    }
}

/// The depot's daily fleet: (arrival h, charger, kWh wanted, car's max A, hours plugged in).
const FLEET: [(f64, usize, f64, f64, f64); 7] = [
    (8.5, 2, 20.0, 16.0, 4.0),     // visitor, leaves 12:30
    (10.25, 3, 45.0, 32.0, 5.5),   // van between tours, 15:45
    (11.5, 0, 30.0, 32.0, 4.5),    // van, 16:00
    (16.5, 1, 50.0, 32.0, 14.0),   // van, overnight until 06:30
    (16.75, 2, 40.0, 32.0, 3.75),  // van for the evening shift, leaves 20:30
    (17.0, 0, 45.0, 32.0, 13.0),   // van, overnight until 06:00
    (17.25, 3, 30.0, 16.0, 13.75), // van, overnight until 07:00
];

const SIM_DAYS: usize = 10;

#[derive(Debug, Clone)]
pub struct SiteSim {
    /// Seconds since local midnight of day 0.
    pub t_s: f64,
    pub climate: Climate,
    pub weather: Weather,
    pub inverters: Vec<InverterSim>,
    pub chargers: Vec<ChargerSim>,
    pub heat_pumps: Vec<HeatPumpSim>,
    pub batteries: Vec<BatterySim>,
    pub building: Building,
    pub meter_online: bool,
    pub base_kw: f64,
    pub arrivals: Vec<Arrival>,
    next_arrival: usize,
    pub departures: Vec<DepartureLog>,
    /// Cloudiness each day aims at (fraction of clear-sky PV).
    pub cloud_day: Vec<f64>,
    cloud: f64,
    rng: u64,
}

/// Inverter output ramp, fraction of rating per second.
const INVERTER_RAMP_PER_S: f64 = 0.1;
/// Thermostat gain on the temperature error, kW (thermal) per K.
const THERMOSTAT_GAIN_KW_PER_K: f64 = 6.0;

impl SiteSim {
    /// The demo depot on a fair spring day: 2 × 60 kW inverters (120 kWp),
    /// four 22 kW chargers, a 14 kW heat pump and a 100 kWh battery.
    pub fn depot(start_s: f64, seed: u64) -> Self {
        Self::new(SimConfig::new(start_s, seed, Season::Spring, Weather::Fair))
    }

    pub fn new(cfg: SimConfig) -> Self {
        let start_s = cfg.start_s;
        let climate = Climate::of(cfg.season);
        let inverter = InverterSim {
            rated_kw: 60.0,
            output_kw: 0.0,
            energy_wh: 0.0,
            limit_enabled: false,
            limit_pct: 100.0,
            revert_s: 0.0,
            last_limit_write_s: start_s,
            online: true,
            ignores_limit: false,
        };
        let charger = ChargerSim {
            max_current_a: 32.0,
            limit_a: 32.0,
            failsafe_a: 32.0,
            failsafe_timeout_s: 0.0,
            last_heartbeat_s: start_s,
            car: None,
            current_a: 0.0,
            online: true,
        };
        let mut arrivals = Vec::new();
        for day in 0..SIM_DAYS {
            for &(h, charger, needs_kwh, max_current_a, dwell_h) in &FLEET {
                let at_s = day as f64 * 86_400.0 + h * 3600.0;
                arrivals.push(Arrival {
                    at_s,
                    charger,
                    needs_kwh,
                    max_current_a,
                    departure_s: at_s + dwell_h * 3600.0,
                });
            }
        }
        arrivals.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));

        let mut sim = SiteSim {
            t_s: start_s,
            climate,
            weather: cfg.weather,
            inverters: vec![inverter; 2],
            chargers: vec![charger; 4],
            heat_pumps: vec![HeatPumpSim {
                rated_kw: 14.0,
                min_kw: 3.0,
                limit_kw: 14.0,
                ext_power_kw: None,
                last_ext_write_s: f64::NEG_INFINITY,
                setpoint_c: thermostat_setpoint_c(start_s),
                demand_kw: 0.0,
                power_kw: 0.0,
                online: true,
                cycle_on: false,
            }],
            batteries: vec![BatterySim {
                capacity_kwh: 100.0,
                efficiency: 0.95,
                max_charge_kw: 50.0,
                max_discharge_kw: 50.0,
                soc_pct: 40.0,
                setpoint_kw: 0.0,
                power_kw: 0.0,
                watchdog_s: 0.0,
                last_setpoint_s: start_s,
                online: true,
            }],
            building: Building { indoor_c: thermostat_setpoint_c(start_s), ua_kw_per_k: 1.2, cap_kwh_per_k: 18.0 },
            meter_online: true,
            base_kw: 18.0,
            next_arrival: 0,
            arrivals,
            departures: Vec::new(),
            cloud_day: vec![0.92; SIM_DAYS],
            cloud: 1.0,
            rng: cfg.seed.max(1),
        };
        if cfg.weather == Weather::Random {
            for d in 0..SIM_DAYS {
                let (u, r) = (sim.uniform(), sim.uniform());
                sim.cloud_day[d] = sim.climate.draw_cloudiness(u, r);
            }
        }
        sim.cloud = sim.cloud_day[sim.day()];
        // Cars already plugged in at the start, inverters at the output the
        // sun allows (no ramp from 0).
        sim.step(0.0);
        let solar = sim.solar_fraction();
        for inv in &mut sim.inverters {
            inv.output_kw = inv.rated_kw * solar;
        }
        sim
    }

    pub fn day(&self) -> usize {
        ((self.t_s / 86_400.0).floor().max(0.0) as usize).min(SIM_DAYS - 1)
    }

    pub fn pv_installed_kw(&self) -> f64 {
        self.inverters.iter().map(|i| i.rated_kw).sum()
    }

    pub fn pv_kw(&self) -> f64 {
        self.inverters.iter().map(|i| i.output_kw).sum()
    }

    pub fn chargers_kw(&self) -> f64 {
        self.chargers.iter().map(|c| c.power_kw()).sum()
    }

    pub fn heat_pumps_kw(&self) -> f64 {
        self.heat_pumps.iter().map(|h| h.power_kw).sum()
    }

    /// Battery power, + charging.
    pub fn batteries_kw(&self) -> f64 {
        self.batteries.iter().map(|b| b.power_kw).sum()
    }

    /// PV the sun allows right now, before any limit.
    pub fn pv_available_kw(&self) -> f64 {
        self.solar_fraction() * self.pv_installed_kw()
    }

    /// Power at the grid connection point, + = import.
    pub fn grid_kw(&self) -> f64 {
        self.base_kw + self.chargers_kw() + self.heat_pumps_kw() + self.batteries_kw() - self.pv_kw()
    }

    pub fn outdoor_c(&self) -> f64 {
        self.climate.outdoor_c(self.t_s)
    }

    /// PV available from the sun (before any limit), fraction of installed power.
    pub fn solar_fraction(&self) -> f64 {
        self.climate.clear_sky_fraction(self.t_s) * self.cloud
    }

    /// Current cloudiness factor (1 = clear sky).
    pub fn cloudiness(&self) -> f64 {
        self.cloud
    }

    fn noise(&mut self) -> f64 {
        // xorshift64*, mapped to [-1, 1)
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    fn uniform(&mut self) -> f64 {
        (self.noise() + 1.0) / 2.0
    }

    /// Advances the physics by `dt_s` seconds.
    pub fn step(&mut self, dt_s: f64) {
        self.t_s += dt_s;
        let t = self.t_s;
        let hours = dt_s / 3600.0;

        // Weather: a slow cloud random walk around the day's cloudiness.
        let aim = self.cloud_day[self.day()];
        let (lo, hi) = match self.weather {
            Weather::Fair => (0.55, 1.0),
            Weather::Random => ((aim - 0.3).max(0.05), (aim + 0.2).min(1.0)),
        };
        let n = self.noise();
        self.cloud = (self.cloud + n * 0.01 * dt_s.sqrt() + (aim - self.cloud) * 0.002 * dt_s).clamp(lo, hi);

        // Base load: office, lighting, cold storage. Higher 07:00–18:00.
        let target = if working_hours(t) { 26.0 } else { 16.0 };
        let n = self.noise();
        self.base_kw += (target - self.base_kw) * (0.01 * dt_s).min(1.0) + n * 0.3 * dt_s.sqrt().min(3.0);
        self.base_kw = self.base_kw.clamp(10.0, 40.0);

        // Inverters follow the sun, their limit and a ramp rate.
        let solar = self.solar_fraction();
        for inv in &mut self.inverters {
            if inv.limit_enabled && inv.revert_s > 0.0 && t - inv.last_limit_write_s > inv.revert_s {
                inv.limit_enabled = false;
            }
            let cap = if inv.limit_enabled && !inv.ignores_limit { inv.limit_pct / 100.0 } else { 1.0 };
            let target = inv.rated_kw * solar.min(cap);
            let max_step = inv.rated_kw * INVERTER_RAMP_PER_S * dt_s.max(0.0);
            inv.output_kw += (target - inv.output_kw).clamp(-max_step, max_step);
            inv.energy_wh += inv.output_kw * 1000.0 * hours;
        }

        // Cars leave at their departure time, full or not, and arrive at free chargers.
        for (i, c) in self.chargers.iter_mut().enumerate() {
            if let Some(car) = &c.car
                && t >= car.departure_s
            {
                self.departures.push(DepartureLog {
                    at_s: car.departure_s,
                    charger: i,
                    needs_kwh: car.needs_kwh,
                    charged_kwh: car.charged_kwh,
                });
                c.car = None;
            }
        }
        while self.next_arrival < self.arrivals.len() && self.arrivals[self.next_arrival].at_s <= t {
            let a = self.arrivals[self.next_arrival];
            self.next_arrival += 1;
            if a.departure_s <= t {
                continue; // left before the simulation reached it
            }
            if let Some(c) = self.chargers.get_mut(a.charger)
                && c.car.is_none()
            {
                c.car = Some(Car {
                    max_current_a: a.max_current_a,
                    needs_kwh: a.needs_kwh,
                    charged_kwh: 0.0,
                    departure_s: a.departure_s,
                });
            }
        }
        for c in &mut self.chargers {
            let limit = if c.in_failsafe(t) { c.failsafe_a } else { c.limit_a };
            c.current_a = match &mut c.car {
                Some(car) if car.charged_kwh < car.needs_kwh && limit >= 6.0 => limit.min(car.max_current_a),
                _ => 0.0,
            };
            if let Some(car) = &mut c.car {
                car.charged_kwh += crate::three_phase_kw(c.current_a) * hours;
            }
        }

        // Batteries follow their setpoint within power and SoC limits; the
        // BMS goes idle when the controller stops writing.
        for b in &mut self.batteries {
            let sp = if b.in_watchdog(t) { 0.0 } else { b.setpoint_kw };
            let mut p = sp.clamp(-b.max_discharge_kw, b.max_charge_kw);
            if (p > 0.0 && b.soc_pct >= 100.0) || (p < 0.0 && b.soc_pct <= 0.0) {
                p = 0.0;
            }
            b.power_kw = p;
            let stored = if p > 0.0 { p * b.efficiency } else { p / b.efficiency };
            b.soc_pct = (b.soc_pct + stored * hours / b.capacity_kwh * 100.0).clamp(0.0, 100.0);
        }

        // Heat pump and building.
        let outdoor = self.outdoor_c();
        let gains = Building::gains_kw(t);
        let cop_now = cop(outdoor);
        let b = self.building;
        let mut heat_kw = 0.0;
        for hp in &mut self.heat_pumps {
            hp.setpoint_c = thermostat_setpoint_c(t);
            let sp = hp.setpoint_c;
            // The unit's own comfort guard overrides the EMS below
            // setpoint − 3 K and above 24 °C.
            let guard = b.indoor_c < sp - 3.0 || b.indoor_c > 24.0;
            let external = hp.external(t) && !guard;
            let thermostat_kw =
                ((b.ua_kw_per_k * (sp - outdoor) - gains + THERMOSTAT_GAIN_KW_PER_K * (sp - b.indoor_c)) / cop_now)
                    .clamp(0.0, hp.rated_kw);
            hp.demand_kw = if external { hp.ext_power_kw.unwrap_or(0.0).min(hp.rated_kw) } else { thermostat_kw };
            // Below the minimum modulation the compressor cycles.
            let run = if hp.demand_kw >= hp.min_kw {
                hp.demand_kw
            } else if external {
                if hp.demand_kw >= hp.min_kw / 2.0 { hp.min_kw } else { 0.0 }
            } else {
                if b.indoor_c < sp - 0.3 {
                    hp.cycle_on = true;
                } else if b.indoor_c > sp + 0.3 {
                    hp.cycle_on = false;
                }
                if hp.cycle_on { hp.min_kw } else { 0.0 }
            };
            hp.power_kw = if hp.limit_kw >= hp.min_kw { run.min(hp.limit_kw) } else { 0.0 };
            heat_kw += cop_now * hp.power_kw;
        }
        let bld = &mut self.building;
        bld.indoor_c += hours / bld.cap_kwh_per_k * (heat_kw + gains - bld.ua_kw_per_k * (bld.indoor_c - outdoor));
    }

    fn online(&self, dev: DeviceId) -> bool {
        match dev {
            DeviceId::Inverter(i) => self.inverters.get(i).is_some_and(|d| d.online),
            DeviceId::Meter => self.meter_online,
            DeviceId::Charger(i) => self.chargers.get(i).is_some_and(|d| d.online),
            DeviceId::HeatPump(i) => self.heat_pumps.get(i).is_some_and(|d| d.online),
            DeviceId::Battery(i) => self.batteries.get(i).is_some_and(|d| d.online),
        }
    }

    pub fn set_online(&mut self, dev: DeviceId, online: bool) {
        match dev {
            DeviceId::Inverter(i) => self.inverters[i].online = online,
            DeviceId::Meter => self.meter_online = online,
            DeviceId::Charger(i) => self.chargers[i].online = online,
            DeviceId::HeatPump(i) => self.heat_pumps[i].online = online,
            DeviceId::Battery(i) => self.batteries[i].online = online,
        }
    }

    /// The device's holding registers starting at the address its map begins
    /// with (SunSpec devices at 40000, the others at 0).
    pub fn image(&self, dev: DeviceId) -> (u16, Vec<u16>) {
        match dev {
            DeviceId::Inverter(i) => (sunspec::BASE, self.inverter_image(i)),
            DeviceId::Meter => (sunspec::BASE, self.meter_image()),
            DeviceId::Charger(i) => (0, self.charger_image(i)),
            DeviceId::HeatPump(i) => (0, self.heat_pump_image(i)),
            DeviceId::Battery(i) => (0, self.battery_image(i)),
        }
    }

    pub fn read(&self, dev: DeviceId, addr: u16, count: u16) -> Result<Vec<u16>, Exception> {
        if !self.online(dev) {
            return Err(Exception::DeviceOffline);
        }
        let (start, image) = self.image(dev);
        let from = addr.checked_sub(start).ok_or(Exception::IllegalDataAddress)? as usize;
        image.get(from..from + count as usize).map(<[u16]>::to_vec).ok_or(Exception::IllegalDataAddress)
    }

    pub fn write(&mut self, dev: DeviceId, addr: u16, values: &[u16]) -> Result<(), Exception> {
        if !self.online(dev) {
            return Err(Exception::DeviceOffline);
        }
        let t = self.t_s;
        for (k, &v) in values.iter().enumerate() {
            let a = addr + k as u16;
            match dev {
                DeviceId::Inverter(i) => {
                    let body = inverter_controls_body();
                    let inv = &mut self.inverters[i];
                    match a.checked_sub(body).map(usize::from) {
                        Some(controls::W_MAX_LIM_PCT) => {
                            if v > 1000 {
                                return Err(Exception::IllegalDataValue);
                            }
                            inv.limit_pct = v as f64 / 10.0;
                            inv.last_limit_write_s = t;
                        }
                        Some(controls::W_MAX_LIM_PCT_RVRT_TMS) => inv.revert_s = v as f64,
                        Some(controls::W_MAX_LIM_ENA) => {
                            inv.limit_enabled = v == 1;
                            inv.last_limit_write_s = t;
                        }
                        Some(controls::CONN) => {}
                        _ => return Err(Exception::IllegalDataAddress),
                    }
                }
                DeviceId::Meter => return Err(Exception::IllegalDataAddress),
                DeviceId::Charger(i) => {
                    let c = &mut self.chargers[i];
                    match a {
                        evse::CURRENT_LIMIT => {
                            c.limit_a = (v as f64 / 10.0).min(c.max_current_a);
                            c.last_heartbeat_s = t;
                        }
                        evse::FAILSAFE_CURRENT => c.failsafe_a = (v as f64 / 10.0).min(c.max_current_a),
                        evse::FAILSAFE_TIMEOUT => c.failsafe_timeout_s = v as f64,
                        evse::HEARTBEAT => c.last_heartbeat_s = t,
                        _ => return Err(Exception::IllegalDataAddress),
                    }
                }
                DeviceId::HeatPump(i) => {
                    let hp = &mut self.heat_pumps[i];
                    match a {
                        heat_pump::POWER_LIMIT => hp.limit_kw = (v as f64 / 10.0).min(hp.rated_kw),
                        heat_pump::EXT_POWER => {
                            hp.ext_power_kw = (v != heat_pump::EXT_NONE).then(|| v as f64 / 10.0);
                            hp.last_ext_write_s = t;
                        }
                        _ => return Err(Exception::IllegalDataAddress),
                    }
                }
                DeviceId::Battery(i) => {
                    let b = &mut self.batteries[i];
                    match a {
                        battery::SETPOINT => {
                            b.setpoint_kw = v as i16 as f64 / 10.0;
                            b.last_setpoint_s = t;
                        }
                        battery::WATCHDOG_S => b.watchdog_s = v as f64,
                        _ => return Err(Exception::IllegalDataAddress),
                    }
                }
            }
        }
        Ok(())
    }

    fn inverter_image(&self, i: usize) -> Vec<u16> {
        let inv = &self.inverters[i];
        let mut c = common_body("GridEdge Sim", "PV-60K3", &format!("INV-{:04}", i + 1));
        c[common::DEVICE_ADDRESS] = 1;

        let mut m = vec![NI_INT16; inverter::LEN];
        m[inverter::W] = sunspec::unscaled(inv.output_kw * 1000.0, 1) as u16;
        m[inverter::W_SF] = 1;
        m[inverter::HZ] = 5000;
        m[inverter::HZ_SF] = (-2i16) as u16;
        let wh = u32_to_regs(inv.energy_wh as u32);
        m[inverter::WH..inverter::WH + 2].copy_from_slice(&wh);
        m[inverter::WH_SF] = 0;
        m[inverter::ST] = if inv.output_kw < 0.05 {
            inverter::ST_OFF
        } else if inv.limit_enabled && inv.limit_pct < 100.0 {
            inverter::ST_THROTTLED
        } else {
            inverter::ST_MPPT
        };

        let mut n = vec![NI_INT16; nameplate::LEN];
        n[nameplate::DER_TYPE] = nameplate::DER_TYPE_PV;
        n[nameplate::W_RTG] = sunspec::unscaled(inv.rated_kw * 1000.0, 1) as u16;
        n[nameplate::W_RTG_SF] = 1;

        let mut k = vec![0u16; controls::LEN];
        k[controls::CONN] = 1;
        k[controls::W_MAX_LIM_PCT] = (inv.limit_pct * 10.0).round() as u16;
        k[controls::W_MAX_LIM_PCT_RVRT_TMS] = inv.revert_s as u16;
        k[controls::W_MAX_LIM_ENA] = inv.limit_enabled as u16;
        k[controls::W_MAX_LIM_PCT_SF] = (-1i16) as u16;

        let mut av = vec![0u16; available::LEN];
        av[available::W_AVAIL] = sunspec::unscaled(inv.rated_kw * self.solar_fraction() * 1000.0, 1) as u16;
        av[available::W_AVAIL_SF] = 1;

        sunspec::build_image(&[
            (common::ID, c),
            (inverter::ID, m),
            (nameplate::ID, n),
            (controls::ID, k),
            (available::ID, av),
        ])
    }

    fn meter_image(&self) -> Vec<u16> {
        let c = common_body("GridEdge Sim", "EM-3P", "MTR-0001");
        let mut m = vec![NI_INT16; meter::LEN];
        m[meter::HZ] = 5000;
        m[meter::HZ_SF] = (-2i16) as u16;
        m[meter::W] = sunspec::unscaled(self.grid_kw() * 1000.0, 1) as u16;
        m[meter::W_SF] = 1;
        sunspec::build_image(&[(common::ID, c), (meter::ID, m)])
    }

    fn charger_image(&self, i: usize) -> Vec<u16> {
        let c = &self.chargers[i];
        let mut r = vec![0u16; evse::LEN as usize];
        r[evse::STATUS as usize] = match &c.car {
            None => evse::STATUS_AVAILABLE,
            Some(car) if car.charged_kwh >= car.needs_kwh => evse::STATUS_FINISHED,
            Some(_) if c.current_a > 0.0 && c.in_failsafe(self.t_s) => evse::STATUS_FAILSAFE,
            Some(_) if c.current_a > 0.0 => evse::STATUS_CHARGING,
            Some(_) => evse::STATUS_CONNECTED,
        };
        r[evse::CURRENT_LIMIT as usize] = (c.limit_a * 10.0).round() as u16;
        r[evse::CURRENT as usize] = (c.current_a * 10.0).round() as u16;
        let p = u32_to_regs((c.power_kw() * 1000.0).round() as u32);
        r[evse::POWER as usize..evse::POWER as usize + 2].copy_from_slice(&p);
        let e = c.car.as_ref().map_or(0.0, |car| car.charged_kwh * 1000.0);
        r[evse::SESSION_ENERGY as usize..evse::SESSION_ENERGY as usize + 2].copy_from_slice(&u32_to_regs(e as u32));
        r[evse::MAX_CURRENT as usize] = (c.max_current_a * 10.0) as u16;
        r[evse::FAILSAFE_CURRENT as usize] = (c.failsafe_a * 10.0) as u16;
        r[evse::FAILSAFE_TIMEOUT as usize] = c.failsafe_timeout_s as u16;
        let request = c.car.as_ref().map_or(0.0, |car| car.needs_kwh * 1000.0);
        r[evse::ENERGY_REQUEST as usize..evse::ENERGY_REQUEST as usize + 2]
            .copy_from_slice(&u32_to_regs(request as u32));
        r[evse::DEPARTURE_MIN as usize] = c.car.as_ref().map_or(evse::NO_DEPARTURE, |car| {
            ((car.departure_s - self.t_s) / 60.0).floor().clamp(0.0, (evse::NO_DEPARTURE - 1) as f64) as u16
        });
        r[evse::CAR_MAX_CURRENT as usize] = c.car.as_ref().map_or(0, |car| (car.max_current_a * 10.0) as u16);
        r
    }

    fn heat_pump_image(&self, i: usize) -> Vec<u16> {
        let hp = &self.heat_pumps[i];
        let mut r = vec![0u16; heat_pump::LEN as usize];
        r[heat_pump::STATUS as usize] = if hp.power_kw == 0.0 {
            heat_pump::STATUS_OFF
        } else if hp.external(self.t_s) {
            heat_pump::STATUS_EXTERNAL
        } else if hp.power_kw < hp.demand_kw - 0.05 {
            heat_pump::STATUS_LIMITED
        } else {
            heat_pump::STATUS_RUNNING
        };
        r[heat_pump::POWER_LIMIT as usize] = (hp.limit_kw * 10.0).round() as u16;
        r[heat_pump::POWER as usize] = (hp.power_kw * 10.0).round() as u16;
        r[heat_pump::DEMAND as usize] = (hp.demand_kw * 10.0).round() as u16;
        r[heat_pump::RATED as usize] = (hp.rated_kw * 10.0).round() as u16;
        r[heat_pump::INDOOR as usize] = tenths_i16(self.building.indoor_c);
        r[heat_pump::OUTDOOR as usize] = tenths_i16(self.outdoor_c());
        r[heat_pump::SETPOINT as usize] = tenths_i16(hp.setpoint_c);
        r[heat_pump::EXT_POWER as usize] = match hp.ext_power_kw {
            Some(p) if hp.external(self.t_s) => (p * 10.0).round() as u16,
            _ => heat_pump::EXT_NONE,
        };
        r
    }

    fn battery_image(&self, i: usize) -> Vec<u16> {
        let b = &self.batteries[i];
        let mut r = vec![0u16; battery::LEN as usize];
        r[battery::STATUS as usize] = if b.in_watchdog(self.t_s) {
            battery::STATUS_WATCHDOG
        } else if b.power_kw > 0.05 {
            battery::STATUS_CHARGING
        } else if b.power_kw < -0.05 {
            battery::STATUS_DISCHARGING
        } else {
            battery::STATUS_IDLE
        };
        r[battery::SETPOINT as usize] = (b.setpoint_kw * 10.0).round() as i16 as u16;
        r[battery::POWER as usize] = (b.power_kw * 10.0).round() as i16 as u16;
        r[battery::SOC as usize] = (b.soc_pct * 10.0).round() as u16;
        r[battery::CAPACITY as usize] = (b.capacity_kwh * 10.0).round() as u16;
        r[battery::MAX_CHARGE as usize] = (b.max_charge_kw * 10.0).round() as u16;
        r[battery::MAX_DISCHARGE as usize] = (b.max_discharge_kw * 10.0).round() as u16;
        r[battery::WATCHDOG_S as usize] = b.watchdog_s as u16;
        r
    }
}

fn common_body(manufacturer: &str, model: &str, serial: &str) -> Vec<u16> {
    let mut c = vec![0u16; common::LEN];
    sunspec::put_string(&mut c[common::MANUFACTURER..common::MANUFACTURER + 16], manufacturer);
    sunspec::put_string(&mut c[common::MODEL..common::MODEL + 16], model);
    sunspec::put_string(&mut c[common::SERIAL..common::SERIAL + 16], serial);
    c
}

/// Absolute address of the model 123 body in the simulated inverter's chain.
fn inverter_controls_body() -> u16 {
    sunspec::BASE + 2 + (2 + common::LEN + 2 + inverter::LEN + 2 + nameplate::LEN) as u16 + 2
}

/// Session energy from an EVSE image, kWh.
pub fn session_kwh(regs: &[u16]) -> f64 {
    regs_to_u32(&regs[evse::SESSION_ENERGY as usize..]) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverter_limit_is_written_through_model_123() {
        let mut sim = SiteSim::depot(12.0 * 3600.0, 7);
        let (_, image) = sim.image(DeviceId::Inverter(0));
        let models = sunspec::locate_models(&image[2..]).unwrap();
        let ctl = models.iter().find(|m| m.id == controls::ID).unwrap();
        assert_eq!(ctl.body, inverter_controls_body());

        sim.write(DeviceId::Inverter(0), ctl.body + controls::W_MAX_LIM_PCT as u16, &[300]).unwrap();
        sim.write(DeviceId::Inverter(0), ctl.body + controls::W_MAX_LIM_ENA as u16, &[1]).unwrap();
        for _ in 0..30 {
            sim.step(1.0);
        }
        assert!((sim.inverters[0].output_kw - 18.0).abs() < 1e-6, "{}", sim.inverters[0].output_kw);
        assert!(sim.inverters[1].output_kw > 30.0, "the other inverter is not limited");
    }

    #[test]
    fn meter_reports_import_positive() {
        let sim = SiteSim::depot(2.0 * 3600.0, 7); // night: no PV
        let regs = sim.read(DeviceId::Meter, sunspec::BASE + 2 + 2 + 66 + 2, meter::LEN as u16).unwrap();
        let w = sunspec::scaled(regs[meter::W] as i16, regs[meter::W_SF] as i16);
        assert!(w > 10_000.0, "{w}");
    }

    #[test]
    fn charger_falls_back_to_failsafe_current_without_heartbeat() {
        let mut sim = SiteSim::depot(17.5 * 3600.0, 7);
        let c = DeviceId::Charger(1);
        sim.write(c, evse::FAILSAFE_CURRENT, &[60]).unwrap();
        sim.write(c, evse::FAILSAFE_TIMEOUT, &[30]).unwrap();
        sim.write(c, evse::CURRENT_LIMIT, &[320]).unwrap();
        sim.step(1.0);
        assert_eq!(sim.chargers[1].current_a, 32.0);
        for _ in 0..31 {
            sim.step(1.0);
        }
        assert_eq!(sim.chargers[1].current_a, 6.0);
        let regs = sim.read(c, 0, evse::LEN).unwrap();
        assert_eq!(regs[evse::STATUS as usize], evse::STATUS_FAILSAFE);
    }

    #[test]
    fn chargers_report_energy_request_and_departure() {
        let sim = SiteSim::depot(17.5 * 3600.0, 7);
        let regs = sim.read(DeviceId::Charger(2), 0, evse::LEN).unwrap(); // evening-shift van
        assert_eq!(regs_to_u32(&regs[evse::ENERGY_REQUEST as usize..]), 40_000);
        assert_eq!(regs[evse::DEPARTURE_MIN as usize], 180, "leaves at 20:30");
        let empty = sim.read(DeviceId::Charger(0), 0, evse::LEN);
        assert!(empty.is_ok());
    }

    #[test]
    fn cars_leave_at_departure_and_unmet_energy_is_logged() {
        let mut sim = SiteSim::depot(16.7 * 3600.0, 7);
        for i in 0..4 {
            sim.write(DeviceId::Charger(i), evse::CURRENT_LIMIT, &[0]).unwrap(); // nobody charges
        }
        while sim.t_s < 21.0 * 3600.0 {
            sim.step(10.0);
        }
        let van = sim.departures.iter().find(|d| d.charger == 2).expect("evening van left");
        assert_eq!(van.needs_kwh, 40.0);
        assert!((van.unmet_kwh() - 40.0).abs() < 1e-9);
        assert!(sim.chargers[2].car.is_none());
    }

    #[test]
    fn the_fleet_comes_back_the_next_day() {
        let mut sim = SiteSim::depot(6.0 * 3600.0, 7);
        while sim.t_s < 86_400.0 + 9.0 * 3600.0 {
            sim.step(30.0);
        }
        assert!(sim.chargers[2].car.is_some(), "the 08:30 visitor on day 1");
        assert!(sim.departures.len() >= 7, "{}", sim.departures.len());
    }

    #[test]
    fn battery_follows_setpoint_and_goes_idle_without_controller() {
        let mut sim = SiteSim::depot(20.0 * 3600.0, 7);
        let b = DeviceId::Battery(0);
        sim.write(b, battery::WATCHDOG_S, &[20]).unwrap();
        sim.write(b, battery::SETPOINT, &[(-300i16) as u16]).unwrap(); // discharge 30 kW
        let soc0 = sim.batteries[0].soc_pct;
        for _ in 0..10 {
            sim.step(1.0);
        }
        assert_eq!(sim.batteries[0].power_kw, -30.0);
        assert!(sim.batteries[0].soc_pct < soc0);
        let regs = sim.read(b, 0, battery::LEN).unwrap();
        assert_eq!(regs[battery::POWER as usize] as i16, -300);
        for _ in 0..15 {
            sim.step(1.0);
        }
        assert_eq!(sim.batteries[0].power_kw, 0.0, "watchdog: idle");
        assert_eq!(sim.read(b, 0, 1).unwrap()[0], battery::STATUS_WATCHDOG);
    }

    #[test]
    fn inverter_reports_available_power_in_a_vendor_model() {
        let mut sim = SiteSim::depot(12.0 * 3600.0, 7);
        let (_, image) = sim.image(DeviceId::Inverter(0));
        let models = sunspec::locate_models(&image[2..]).unwrap();
        let av = models.iter().find(|m| m.id == available::ID).unwrap();
        let body = inverter_controls_body();
        sim.write(DeviceId::Inverter(0), body + controls::W_MAX_LIM_PCT as u16, &[200]).unwrap();
        sim.write(DeviceId::Inverter(0), body + controls::W_MAX_LIM_ENA as u16, &[1]).unwrap();
        for _ in 0..30 {
            sim.step(1.0);
        }
        let r = sim.read(DeviceId::Inverter(0), av.body, 2).unwrap();
        let avail = sunspec::scaled(r[0] as i16, r[1] as i16) / 1000.0;
        assert!(avail > 30.0 && sim.inverters[0].output_kw <= 12.0 + 1e-9, "{avail}");
    }

    #[test]
    fn thermostat_holds_the_building_and_ems_can_take_over() {
        let mut sim = SiteSim::new(SimConfig::new(9.0 * 3600.0, 7, Season::Winter, Weather::Fair));
        for _ in 0..(3 * 360) {
            sim.step(10.0);
        }
        let t = sim.building.indoor_c;
        assert!((t - 21.0).abs() < 0.5, "thermostat keeps ~21 °C: {t}");
        let hp_kw = sim.heat_pumps[0].power_kw;
        assert!(hp_kw > 3.0, "winter needs heat: {hp_kw}");

        // The EMS pre-heats at full power for an hour.
        for _ in 0..360 {
            sim.write(DeviceId::HeatPump(0), heat_pump::EXT_POWER, &[140]).unwrap();
            sim.step(10.0);
        }
        assert!(sim.building.indoor_c > t + 0.7, "{}", sim.building.indoor_c);
        let regs = sim.read(DeviceId::HeatPump(0), 0, heat_pump::LEN).unwrap();
        assert_eq!(regs[heat_pump::STATUS as usize], heat_pump::STATUS_EXTERNAL);
        assert!(regs[heat_pump::INDOOR as usize] as i16 > 215);

        // A silent EMS hands control back to the thermostat after 15 minutes.
        for _ in 0..100 {
            sim.step(10.0);
        }
        assert!(!sim.heat_pumps[0].external(sim.t_s));
    }

    #[test]
    fn comfort_guard_overrides_an_ems_that_starves_the_building() {
        let mut sim = SiteSim::new(SimConfig::new(9.0 * 3600.0, 7, Season::Winter, Weather::Fair));
        for _ in 0..(8 * 360) {
            sim.write(DeviceId::HeatPump(0), heat_pump::EXT_POWER, &[0]).unwrap();
            sim.step(10.0);
        }
        assert!(sim.building.indoor_c > 17.5, "guard at setpoint − 3 K: {}", sim.building.indoor_c);
    }

    #[test]
    fn random_weather_varies_from_day_to_day() {
        let sim = SiteSim::new(SimConfig::new(0.0, 11, Season::Spring, Weather::Random));
        let spread =
            sim.cloud_day.iter().cloned().fold(0.0f64, f64::max) - sim.cloud_day.iter().cloned().fold(1.0f64, f64::min);
        assert!(spread > 0.2, "{:?}", sim.cloud_day);
    }

    #[test]
    fn offline_devices_do_not_answer() {
        let mut sim = SiteSim::depot(0.0, 7);
        sim.set_online(DeviceId::HeatPump(0), false);
        assert_eq!(sim.read(DeviceId::HeatPump(0), 0, 1), Err(Exception::DeviceOffline));
        assert_eq!(sim.read(DeviceId::Charger(0), 100, 1), Err(Exception::IllegalDataAddress));
    }

    #[test]
    fn the_day_has_a_solar_peak_and_evening_charging() {
        let mut sim = SiteSim::depot(6.0 * 3600.0, 7);
        let mut peak_pv: f64 = 0.0;
        let mut peak_ev: f64 = 0.0;
        while sim.t_s < 20.0 * 3600.0 {
            sim.step(10.0);
            peak_pv = peak_pv.max(sim.pv_kw());
            if sim.t_s > 17.5 * 3600.0 {
                peak_ev = peak_ev.max(sim.chargers_kw());
            }
        }
        assert!((70.0..100.0).contains(&peak_pv), "{peak_pv}");
        assert!(peak_ev > 60.0, "{peak_ev}");
    }
}
