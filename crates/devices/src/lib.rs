//! The field devices of the demo site: their Modbus register maps (SunSpec
//! for inverters and meter, a typical wallbox map for the chargers) and a
//! deterministic physics simulation behind them.

pub mod maps;
pub mod sim;
pub mod sunspec;

/// Three-phase power at 230 V per phase, kW, for a current per phase in A.
pub fn three_phase_kw(current_a: f64) -> f64 {
    3.0 * 230.0 * current_a / 1000.0
}
