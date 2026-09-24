//! Register maps of the devices that have no SunSpec model: the chargers
//! (EVSE) and the heat pump. Holding registers, 0-based.
//!
//! The EVSE map follows the pattern most Modbus wallboxes share: a current
//! limit in 0.1 A, measured values, and a *failsafe current* with a
//! *failsafe timeout* — if the controller stops writing the heartbeat, the
//! charger falls back to the failsafe current by itself.

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
    pub const LEN: u16 = 11;

    pub const STATUS_AVAILABLE: u16 = 0;
    pub const STATUS_CONNECTED: u16 = 1; // car plugged in, waiting (limit below 6 A)
    pub const STATUS_CHARGING: u16 = 2;
    pub const STATUS_FAILSAFE: u16 = 3; // charging at failsafe current, controller lost
    pub const STATUS_FINISHED: u16 = 4; // car plugged in, battery full
}

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
    pub const LEN: u16 = 5;

    pub const STATUS_OFF: u16 = 0;
    pub const STATUS_RUNNING: u16 = 1;
    pub const STATUS_LIMITED: u16 = 2;
}

pub fn u32_to_regs(v: u32) -> [u16; 2] {
    [(v >> 16) as u16, v as u16]
}

pub fn regs_to_u32(r: &[u16]) -> u32 {
    (r[0] as u32) << 16 | r[1] as u32
}
