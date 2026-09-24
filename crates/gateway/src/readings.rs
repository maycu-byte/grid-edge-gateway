//! Field state → controller readings and dashboard views. A device whose
//! last reading is older than the staleness limit counts as offline.

use std::time::Duration;

use control::{Fallback, Readings, Refusal};
use devices::maps::{battery, evse};

use crate::field::{self, FieldState};
use crate::snapshot::{BatteryView, ChargerView, HeatPumpView, InverterView};

pub struct Views {
    pub inverters: Vec<InverterView>,
    pub chargers: Vec<ChargerView>,
    pub heat_pumps: Vec<HeatPumpView>,
    pub batteries: Vec<BatteryView>,
}

pub fn collect(f: &FieldState, stale: Duration) -> (Readings, Views) {
    let inverters: Vec<Option<field::InverterReading>> = f.inverters.iter().map(|s| s.fresh(stale)).collect();
    // Totals are only known if every inverter answers.
    let pv_kw = inverters.iter().map(|r| r.as_ref().map(|r| r.kw)).sum::<Option<f64>>();
    let pv_available_kw = inverters.iter().map(|r| r.as_ref().and_then(|r| r.available_kw)).sum::<Option<f64>>();
    let chargers: Vec<Option<field::ChargerReading>> = f.chargers.iter().map(|s| s.fresh(stale)).collect();
    let heat_pumps: Vec<Option<field::HeatPumpReading>> = f.heat_pumps.iter().map(|s| s.fresh(stale)).collect();
    let batteries: Vec<Option<field::BatteryReading>> = f.batteries.iter().map(|s| s.fresh(stale)).collect();

    let readings = Readings {
        grid_kw: f.meter_kw.fresh(stale),
        pv_kw,
        pv_available_kw,
        chargers: chargers
            .iter()
            .map(|c| match c {
                Some(c) => control::ChargerReading {
                    online: true,
                    car_waiting: matches!(
                        c.status,
                        evse::STATUS_CONNECTED | evse::STATUS_CHARGING | evse::STATUS_FAILSAFE
                    ),
                    current_a: c.current_a,
                    power_kw: c.kw,
                    session_kwh: c.session_kwh,
                },
                None => control::ChargerReading::default(),
            })
            .collect(),
        heat_pumps: heat_pumps
            .iter()
            .map(|h| match h {
                Some(h) => control::HeatPumpReading { online: true, power_kw: h.kw, demand_kw: h.demand_kw },
                None => control::HeatPumpReading::default(),
            })
            .collect(),
        batteries: batteries
            .iter()
            .map(|b| match b {
                Some(b) => control::BatteryReading { online: true, soc_pct: b.soc_pct, power_kw: b.kw },
                None => control::BatteryReading::default(),
            })
            .collect(),
    };

    let views = Views {
        inverters: inverters
            .iter()
            .map(|r| InverterView {
                online: r.is_some(),
                kw: r.as_ref().map(|r| r.kw),
                rated_kw: r.as_ref().map(|r| r.rated_kw),
            })
            .collect(),
        chargers: chargers
            .iter()
            .map(|c| ChargerView {
                online: c.is_some(),
                status: match c.as_ref().map(|c| c.status) {
                    None => "offline",
                    Some(evse::STATUS_AVAILABLE) => "available",
                    Some(evse::STATUS_CONNECTED) => "waiting",
                    Some(evse::STATUS_CHARGING) => "charging",
                    Some(evse::STATUS_FAILSAFE) => "failsafe",
                    Some(evse::STATUS_FINISHED) => "finished",
                    Some(_) => "unknown",
                },
                current_a: c.as_ref().map(|c| c.current_a),
                setpoint_a: 0.0,
                kw: c.as_ref().map(|c| c.kw),
                session_kwh: c.as_ref().map(|c| c.session_kwh),
            })
            .collect(),
        heat_pumps: heat_pumps
            .iter()
            .map(|h| HeatPumpView {
                online: h.is_some(),
                kw: h.as_ref().map(|h| h.kw),
                demand_kw: h.as_ref().map(|h| h.demand_kw),
                limit_kw: 0.0,
            })
            .collect(),
        batteries: batteries
            .iter()
            .map(|b| BatteryView {
                online: b.is_some(),
                status: match b.as_ref().map(|b| b.status) {
                    None => "offline",
                    Some(battery::STATUS_CHARGING) => "charging",
                    Some(battery::STATUS_DISCHARGING) => "discharging",
                    Some(battery::STATUS_WATCHDOG) => "watchdog",
                    Some(_) => "idle",
                },
                kw: b.as_ref().map(|b| b.kw),
                soc_pct: b.as_ref().map(|b| b.soc_pct),
                setpoint_kw: 0.0,
            })
            .collect(),
    };
    (readings, views)
}

pub fn fallback_name(f: &Fallback) -> String {
    match f {
        Fallback::MeterOffline => "meter offline".into(),
        Fallback::MeterImplausible => "meter implausible".into(),
        Fallback::PvOffline => "inverter offline".into(),
        Fallback::PvImplausible => "inverter implausible".into(),
        Fallback::ChargerOffline(i) => format!("charger{i} offline"),
        Fallback::HeatPumpOffline(i) => format!("heatpump{i} offline"),
        Fallback::BatteryOffline(i) => format!("battery{i} offline"),
    }
}

pub fn refusal_name(r: &Refusal) -> &'static str {
    match r {
        Refusal::DimDayLimitReached => "contract day limit reached",
        Refusal::CurtailmentBudgetExhausted => "curtailment budget used up",
    }
}
