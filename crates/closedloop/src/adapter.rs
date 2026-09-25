//! Reads and writes the simulated devices through their register maps —
//! exactly what the gateway does over Modbus TCP — so the closed loop tests
//! the same decoding and the same setpoint path.

use control::{InverterReading, Readings, Setpoints};
use devices::maps::{battery, evse, heat_pump, regs_to_u32};
use devices::sim::{DeviceId, SiteSim};
use devices::sunspec::{self, available, controls, inverter, meter, nameplate};

/// Body address of a SunSpec model in a device's chain, found the way a
/// Modbus client finds it: by walking the chain from register 40000.
fn model_body(sim: &SiteSim, dev: DeviceId, id: u16) -> Option<u16> {
    let (_, image) = sim.image(dev);
    sunspec::locate_models(image.get(2..)?)?.into_iter().find(|m| m.id == id).map(|m| m.body)
}

pub fn arm_watchdogs(sim: &mut SiteSim) {
    for i in 0..sim.chargers.len() {
        let _ = sim.write(DeviceId::Charger(i), evse::FAILSAFE_CURRENT, &[60]);
        let _ = sim.write(DeviceId::Charger(i), evse::FAILSAFE_TIMEOUT, &[30]);
    }
    for i in 0..sim.batteries.len() {
        let _ = sim.write(DeviceId::Battery(i), battery::WATCHDOG_S, &[30]);
    }
}

pub fn read(sim: &SiteSim) -> Readings {
    let mut pv = Some(0.0);
    let mut avail = Some(0.0);
    let mut inverters = Vec::new();
    for i in 0..sim.inverters.len() {
        let dev = DeviceId::Inverter(i);
        let kw = model_body(sim, dev, inverter::ID)
            .and_then(|b| sim.read(dev, b, inverter::LEN as u16).ok())
            .map(|m| sunspec::scaled(m[inverter::W] as i16, m[inverter::W_SF] as i16) / 1000.0);
        let av = model_body(sim, dev, available::ID)
            .and_then(|b| sim.read(dev, b, available::LEN as u16).ok())
            .map(|m| sunspec::scaled(m[available::W_AVAIL] as i16, m[available::W_AVAIL_SF] as i16) / 1000.0);
        let rated = model_body(sim, dev, nameplate::ID)
            .and_then(|b| sim.read(dev, b, nameplate::LEN as u16).ok())
            .map(|m| sunspec::scaled(m[nameplate::W_RTG] as i16, m[nameplate::W_RTG_SF] as i16) / 1000.0);
        inverters.push(match (kw, rated) {
            (Some(kw), Some(rated_kw)) => InverterReading { online: true, kw, rated_kw },
            _ => InverterReading::default(),
        });
        pv = pv.zip(kw).map(|(a, b)| a + b);
        avail = avail.zip(av).map(|(a, b)| a + b);
    }
    let grid = model_body(sim, DeviceId::Meter, meter::ID)
        .and_then(|b| sim.read(DeviceId::Meter, b, meter::LEN as u16).ok())
        .map(|m| sunspec::scaled(m[meter::W] as i16, m[meter::W_SF] as i16) / 1000.0);

    let chargers = (0..sim.chargers.len())
        .map(|i| match sim.read(DeviceId::Charger(i), 0, evse::LEN) {
            Ok(r) => {
                let session_kwh = regs_to_u32(&r[evse::SESSION_ENERGY as usize..]) as f64 / 1000.0;
                let request = regs_to_u32(&r[evse::ENERGY_REQUEST as usize..]) as f64 / 1000.0;
                let dep = r[evse::DEPARTURE_MIN as usize];
                let car_max = r[evse::CAR_MAX_CURRENT as usize];
                control::ChargerReading {
                    online: true,
                    car_waiting: matches!(
                        r[evse::STATUS as usize],
                        evse::STATUS_CONNECTED | evse::STATUS_CHARGING | evse::STATUS_FAILSAFE
                    ),
                    current_a: r[evse::CURRENT as usize] as f64 / 10.0,
                    power_kw: regs_to_u32(&r[evse::POWER as usize..]) as f64 / 1000.0,
                    session_kwh,
                    remaining_kwh: (request > 0.0).then(|| (request - session_kwh).max(0.0)),
                    departure_s: (dep != evse::NO_DEPARTURE).then_some(dep as f64 * 60.0),
                    car_max_current_a: (car_max > 0).then(|| car_max as f64 / 10.0),
                }
            }
            Err(_) => control::ChargerReading::default(),
        })
        .collect();
    let heat_pumps = (0..sim.heat_pumps.len())
        .map(|i| match sim.read(DeviceId::HeatPump(i), 0, heat_pump::LEN) {
            Ok(r) => control::HeatPumpReading {
                online: true,
                power_kw: r[heat_pump::POWER as usize] as f64 / 10.0,
                demand_kw: r[heat_pump::DEMAND as usize] as f64 / 10.0,
                indoor_c: Some(r[heat_pump::INDOOR as usize] as i16 as f64 / 10.0),
                outdoor_c: Some(r[heat_pump::OUTDOOR as usize] as i16 as f64 / 10.0),
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

    Readings { grid_kw: grid, pv_kw: pv, pv_available_kw: avail, inverters, chargers, heat_pumps, batteries }
}

pub fn write(sim: &mut SiteSim, sp: &Setpoints) {
    for i in 0..sim.inverters.len() {
        if let Some(b) = model_body(sim, DeviceId::Inverter(i), controls::ID) {
            let pct = sp.inverter_limit_pct.get(i).copied().unwrap_or(sp.pv_limit_pct);
            let raw = sunspec::unscaled(pct, -1) as u16;
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
    for (i, ext) in sp.heat_pump_ext_kw.iter().enumerate() {
        let raw = ext.map_or(heat_pump::EXT_NONE, |kw| (kw * 10.0).round().clamp(0.0, 65_000.0) as u16);
        let _ = sim.write(DeviceId::HeatPump(i), heat_pump::EXT_POWER, &[raw]);
    }
    for (i, &kw) in sp.battery_kw.iter().enumerate() {
        let _ = sim.write(DeviceId::Battery(i), battery::SETPOINT, &[(kw * 10.0).round() as i16 as u16]);
    }
}
