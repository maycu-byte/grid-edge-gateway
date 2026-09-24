//! Physics of the demo site and the Modbus register view of every device.
//!
//! The simulation is deterministic for a given seed, so the browser demo,
//! the gateway integration tests and the site simulator all see the same day.

use crate::maps::{evse, heat_pump, regs_to_u32, u32_to_regs};
use crate::sunspec::{self, NI_INT16, common, controls, inverter, meter, nameplate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceId {
    Inverter(usize),
    Meter,
    Charger(usize),
    HeatPump(usize),
}

/// Modbus exceptions a device can answer with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    IllegalDataAddress,
    IllegalDataValue,
    /// The device does not answer (fault injection).
    DeviceOffline,
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
}

#[derive(Debug, Clone)]
pub struct Car {
    pub max_current_a: f64,
    pub needs_kwh: f64,
    pub charged_kwh: f64,
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

#[derive(Debug, Clone)]
pub struct HeatPumpSim {
    pub rated_kw: f64,
    pub min_kw: f64,
    pub limit_kw: f64,
    pub demand_kw: f64,
    pub power_kw: f64,
    pub online: bool,
}

/// A car arriving at a charger during the simulated day.
#[derive(Debug, Clone, Copy)]
pub struct Arrival {
    pub at_s: f64,
    pub charger: usize,
    pub needs_kwh: f64,
    pub max_current_a: f64,
}

#[derive(Debug, Clone)]
pub struct SiteSim {
    /// Seconds since midnight of the simulated day.
    pub t_s: f64,
    pub inverters: Vec<InverterSim>,
    pub chargers: Vec<ChargerSim>,
    pub heat_pumps: Vec<HeatPumpSim>,
    pub meter_online: bool,
    pub base_kw: f64,
    pub arrivals: Vec<Arrival>,
    next_arrival: usize,
    cloud: f64,
    rng: u64,
}

/// Sunrise and sunset of a clear spring day in south-west Germany (local time).
const SUNRISE_S: f64 = 6.5 * 3600.0;
const SUNSET_S: f64 = 19.5 * 3600.0;
/// Inverter output ramp, fraction of rating per second.
const INVERTER_RAMP_PER_S: f64 = 0.1;

impl SiteSim {
    /// The demo depot: 2 × 60 kW inverters (120 kWp), four 22 kW chargers and
    /// a 14 kW heat pump, starting at `start_s` seconds after midnight.
    pub fn depot(start_s: f64, seed: u64) -> Self {
        let inverter = InverterSim {
            rated_kw: 60.0,
            output_kw: 0.0,
            energy_wh: 0.0,
            limit_enabled: false,
            limit_pct: 100.0,
            revert_s: 0.0,
            last_limit_write_s: start_s,
            online: true,
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
        let h = |hh: f64| hh * 3600.0;
        let arrivals = vec![
            Arrival { at_s: h(8.5), charger: 2, needs_kwh: 20.0, max_current_a: 16.0 },
            Arrival { at_s: h(10.25), charger: 3, needs_kwh: 45.0, max_current_a: 32.0 },
            Arrival { at_s: h(11.5), charger: 0, needs_kwh: 30.0, max_current_a: 32.0 },
            Arrival { at_s: h(16.5), charger: 1, needs_kwh: 50.0, max_current_a: 32.0 },
            Arrival { at_s: h(16.75), charger: 2, needs_kwh: 40.0, max_current_a: 32.0 },
            Arrival { at_s: h(17.0), charger: 0, needs_kwh: 45.0, max_current_a: 32.0 },
            Arrival { at_s: h(17.25), charger: 3, needs_kwh: 30.0, max_current_a: 16.0 },
        ];
        let mut sim = SiteSim {
            t_s: start_s,
            inverters: vec![inverter; 2],
            chargers: vec![charger; 4],
            heat_pumps: vec![HeatPumpSim {
                rated_kw: 14.0,
                min_kw: 3.0,
                limit_kw: 14.0,
                demand_kw: 0.0,
                power_kw: 0.0,
                online: true,
            }],
            meter_online: true,
            base_kw: 18.0,
            next_arrival: 0,
            arrivals,
            cloud: 1.0,
            rng: seed.max(1),
        };
        // Cars that arrived before the start are plugged in right away, and
        // the inverters start at the output the sun allows (no ramp from 0).
        sim.step(0.0);
        let solar = sim.solar_fraction();
        for inv in &mut sim.inverters {
            inv.output_kw = inv.rated_kw * solar;
        }
        sim
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

    /// Power at the grid connection point, + = import.
    pub fn grid_kw(&self) -> f64 {
        self.base_kw + self.chargers_kw() + self.heat_pumps_kw() - self.pv_kw()
    }

    /// Outdoor temperature, °C: 3 °C before sunrise to 15 °C mid-afternoon.
    pub fn outdoor_c(&self) -> f64 {
        let day = (self.t_s / 3600.0 - 15.0) / 24.0 * std::f64::consts::TAU;
        9.0 + 6.0 * day.cos()
    }

    /// PV available from the sun (before any limit), fraction of installed power.
    pub fn solar_fraction(&self) -> f64 {
        if self.t_s <= SUNRISE_S || self.t_s >= SUNSET_S {
            return 0.0;
        }
        let x = (self.t_s - SUNRISE_S) / (SUNSET_S - SUNRISE_S);
        0.82 * (std::f64::consts::PI * x).sin().powf(1.4) * self.cloud
    }

    fn noise(&mut self) -> f64 {
        // xorshift64*, mapped to [-1, 1)
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    /// Advances the physics by `dt_s` seconds.
    pub fn step(&mut self, dt_s: f64) {
        self.t_s += dt_s;
        let t = self.t_s;
        let hours = dt_s / 3600.0;

        // Weather: slow cloud random walk between 55% and 100% of clear sky.
        let n = self.noise();
        self.cloud = (self.cloud + n * 0.01 * dt_s.sqrt() + (0.92 - self.cloud) * 0.002 * dt_s).clamp(0.55, 1.0);

        // Base load: office, lighting, cold storage. Higher 07:00–18:00.
        let working = (7.0..18.0).contains(&(t / 3600.0 % 24.0));
        let target = if working { 26.0 } else { 16.0 };
        let n = self.noise();
        self.base_kw += (target - self.base_kw) * (0.01 * dt_s).min(1.0) + n * 0.3 * dt_s.sqrt().min(3.0);
        self.base_kw = self.base_kw.clamp(10.0, 40.0);

        // Inverters follow the sun, their limit and a ramp rate.
        let solar = self.solar_fraction();
        for inv in &mut self.inverters {
            if inv.limit_enabled && inv.revert_s > 0.0 && t - inv.last_limit_write_s > inv.revert_s {
                inv.limit_enabled = false;
            }
            let cap = if inv.limit_enabled { inv.limit_pct / 100.0 } else { 1.0 };
            let target = inv.rated_kw * solar.min(cap);
            let max_step = inv.rated_kw * INVERTER_RAMP_PER_S * dt_s.max(0.0);
            inv.output_kw += (target - inv.output_kw).clamp(-max_step, max_step);
            inv.energy_wh += inv.output_kw * 1000.0 * hours;
        }

        // Cars arrive, charge at min(limit, car maximum), leave the plug when full.
        while self.next_arrival < self.arrivals.len() && self.arrivals[self.next_arrival].at_s <= t {
            let a = self.arrivals[self.next_arrival];
            if let Some(c) = self.chargers.get_mut(a.charger) {
                c.car = Some(Car { max_current_a: a.max_current_a, needs_kwh: a.needs_kwh, charged_kwh: 0.0 });
            }
            self.next_arrival += 1;
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

        // Heat pump: demand from outdoor temperature (never below what the
        // compressor can modulate down to), capped by its limit.
        let outdoor = self.outdoor_c();
        for hp in &mut self.heat_pumps {
            hp.demand_kw = (hp.rated_kw * ((16.0 - outdoor) / 20.0)).clamp(0.25 * hp.rated_kw, hp.rated_kw);
            hp.power_kw = if hp.limit_kw >= hp.min_kw { hp.demand_kw.min(hp.limit_kw) } else { 0.0 };
        }
    }

    fn online(&self, dev: DeviceId) -> bool {
        match dev {
            DeviceId::Inverter(i) => self.inverters.get(i).is_some_and(|d| d.online),
            DeviceId::Meter => self.meter_online,
            DeviceId::Charger(i) => self.chargers.get(i).is_some_and(|d| d.online),
            DeviceId::HeatPump(i) => self.heat_pumps.get(i).is_some_and(|d| d.online),
        }
    }

    pub fn set_online(&mut self, dev: DeviceId, online: bool) {
        match dev {
            DeviceId::Inverter(i) => self.inverters[i].online = online,
            DeviceId::Meter => self.meter_online = online,
            DeviceId::Charger(i) => self.chargers[i].online = online,
            DeviceId::HeatPump(i) => self.heat_pumps[i].online = online,
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

        sunspec::build_image(&[(common::ID, c), (inverter::ID, m), (nameplate::ID, n), (controls::ID, k)])
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
        r
    }

    fn heat_pump_image(&self, i: usize) -> Vec<u16> {
        let hp = &self.heat_pumps[i];
        let mut r = vec![0u16; heat_pump::LEN as usize];
        r[heat_pump::STATUS as usize] = if hp.power_kw == 0.0 {
            heat_pump::STATUS_OFF
        } else if hp.power_kw < hp.demand_kw - 0.05 {
            heat_pump::STATUS_LIMITED
        } else {
            heat_pump::STATUS_RUNNING
        };
        r[heat_pump::POWER_LIMIT as usize] = (hp.limit_kw * 10.0).round() as u16;
        r[heat_pump::POWER as usize] = (hp.power_kw * 10.0).round() as u16;
        r[heat_pump::DEMAND as usize] = (hp.demand_kw * 10.0).round() as u16;
        r[heat_pump::RATED as usize] = (hp.rated_kw * 10.0).round() as u16;
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
