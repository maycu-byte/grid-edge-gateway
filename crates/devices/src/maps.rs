//! Register maps of the devices that have no SunSpec model: the chargers
//! (EVSE), the heat pump and the battery. Holding registers, 0-based.
//!
//! The EVSE map follows the pattern most Modbus wallboxes share: a current
//! limit in 0.1 A, measured values, and a *failsafe current* with a
//! *failsafe timeout* — if the controller stops writing the heartbeat, the
//! charger falls back to the failsafe current by itself. Wallboxes that
//! speak ISO 15118 with the car also learn how much energy the driver wants
//! and when they leave; the map exposes both, so an EMS can plan.

pub mod evse {
    pub const STATUS: u16 = 0;
    /// Current limit advertised to the car, 0.1 A per phase. Writable.
    pub const CURRENT_LIMIT: u16 = 1;
    /// Measured charging current, 0.1 A per phase.
    pub const CURRENT: u16 = 2;
    /// Active power, W (uint32, high word first).
    pub const POWER: u16 = 3;
    /// Energy delivered in the running session, Wh (uint32).
    pub const SESSION_ENERGY: u16 = 5;
    /// Hardware maximum current, 0.1 A.
    pub const MAX_CURRENT: u16 = 7;
    /// Current used when the heartbeat times out, 0.1 A. Writable.
    pub const FAILSAFE_CURRENT: u16 = 8;
    /// Heartbeat timeout, s. Writable. 0 disables the watchdog.
    pub const FAILSAFE_TIMEOUT: u16 = 9;
    /// Any write resets the watchdog.
    pub const HEARTBEAT: u16 = 10;
    /// Energy the car asked for in this session, Wh (uint32) — ISO 15118-2
    /// `EAmount`. 0 when the car did not say.
    pub const ENERGY_REQUEST: u16 = 11;
    /// Minutes until the car leaves — ISO 15118-2 `DepartureTime`, which is
    /// also relative. `NO_DEPARTURE` when unknown.
    pub const DEPARTURE_MIN: u16 = 13;
    pub const LEN: u16 = 14;

    pub const NO_DEPARTURE: u16 = 0xFFFF;

    pub const STATUS_AVAILABLE: u16 = 0;
    pub const STATUS_CONNECTED: u16 = 1; // car plugged in, waiting (limit below 6 A)
    pub const STATUS_CHARGING: u16 = 2;
    pub const STATUS_FAILSAFE: u16 = 3; // charging at failsafe current, controller lost
    pub const STATUS_FINISHED: u16 = 4; // car plugged in, battery full
}

/// A heat pump with its own thermostat (time program: comfort by day, set
/// back at night). An EMS may take over with an *external power request*,
/// as SG Ready state 4 / EEBUS power setpoints allow; the unit keeps its own
/// comfort guard and falls back to the thermostat if the EMS goes quiet.
pub mod heat_pump {
    pub const STATUS: u16 = 0;
    /// Power limit, 0.1 kW. Writable. Below the minimum modulation the unit stops.
    pub const POWER_LIMIT: u16 = 1;
    /// Electrical power, 0.1 kW.
    pub const POWER: u16 = 2;
    /// Power the unit would take without a limit, 0.1 kW.
    pub const DEMAND: u16 = 3;
    /// Rated electrical power, 0.1 kW.
    pub const RATED: u16 = 4;
    /// Indoor temperature, 0.1 °C (int16).
    pub const INDOOR: u16 = 5;
    /// Outdoor temperature, 0.1 °C (int16).
    pub const OUTDOOR: u16 = 6;
    /// Active thermostat setpoint from the time program, 0.1 °C (int16).
    pub const SETPOINT: u16 = 7;
    /// External power request, 0.1 kW. Writable; `EXT_NONE` hands control
    /// back to the thermostat. Reverts on its own after `EXT_TIMEOUT_S`.
    pub const EXT_POWER: u16 = 8;
    pub const LEN: u16 = 9;

    pub const EXT_NONE: u16 = 0xFFFF;
    pub const EXT_TIMEOUT_S: f64 = 900.0;

    pub const STATUS_OFF: u16 = 0;
    pub const STATUS_RUNNING: u16 = 1;
    pub const STATUS_LIMITED: u16 = 2;
    /// Running on the EMS's external power request.
    pub const STATUS_EXTERNAL: u16 = 3;
}

/// A site battery (EMS-facing map of a battery inverter). Power is signed,
/// positive when charging, in 0.1 kW as int16. Like the chargers, the
/// battery has a watchdog: without a setpoint for `WATCHDOG_S` seconds its
/// BMS goes idle (0 kW), the safe state for the grid.
pub mod battery {
    pub const STATUS: u16 = 0;
    /// Power setpoint, 0.1 kW (int16, + charge). Writable; also the heartbeat.
    pub const SETPOINT: u16 = 1;
    /// Measured power, 0.1 kW (int16, + charge).
    pub const POWER: u16 = 2;
    /// State of charge, 0.1 %.
    pub const SOC: u16 = 3;
    /// Usable capacity, 0.1 kWh.
    pub const CAPACITY: u16 = 4;
    pub const MAX_CHARGE: u16 = 5;
    pub const MAX_DISCHARGE: u16 = 6;
    /// Watchdog timeout, s. Writable. 0 disables it.
    pub const WATCHDOG_S: u16 = 7;
    pub const LEN: u16 = 8;

    pub const STATUS_IDLE: u16 = 0;
    pub const STATUS_CHARGING: u16 = 1;
    pub const STATUS_DISCHARGING: u16 = 2;
    pub const STATUS_WATCHDOG: u16 = 3; // idle because the controller went quiet
}

pub fn u32_to_regs(v: u32) -> [u16; 2] {
    [(v >> 16) as u16, v as u16]
}

pub fn regs_to_u32(r: &[u16]) -> u32 {
    (r[0] as u32) << 16 | r[1] as u32
}

/// Signed 0.1-unit register (temperatures).
pub fn tenths_i16(v: f64) -> u16 {
    (v * 10.0).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16
}
