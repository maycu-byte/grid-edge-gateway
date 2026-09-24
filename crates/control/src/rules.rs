//! Regulatory numbers, kept apart from the control loop so each can be
//! checked against its source.

/// Minimum power per controllable device that a §14a dimming may never go
/// below (BNetzA BK6-22-300, Anlage 1, 4.5.1).
pub const PMIN_PER_DEVICE_KW: f64 = 4.2;

/// Scaling factor for heat pumps / air conditioners above 11 kW (4.5.1, 4.5.2).
pub const LARGE_DEVICE_FACTOR: f64 = 0.4;
pub const LARGE_DEVICE_THRESHOLD_KW: f64 = 11.0;

/// Simultaneity factor (GZF) for `n` devices behind one EMS (4.5.2 table).
pub fn simultaneity_factor(n: usize) -> f64 {
    match n {
        0 | 1 => 0.0, // the (n - 1) term vanishes anyway
        2 => 0.8,
        3 => 0.75,
        4 => 0.7,
        5 => 0.65,
        6 => 0.6,
        7 => 0.55,
        8 => 0.5,
        _ => 0.45,
    }
}

/// The controllable devices (steuVE) behind one grid connection that are
/// dimmed through an energy management system.
#[derive(Debug, Clone, Default)]
pub struct SteuVE {
    /// Wallboxes and batteries: counted, their size does not matter.
    pub chargers_and_storage: usize,
    /// Rated grid connection power of each heat pump, kW.
    pub heat_pumps_kw: Vec<f64>,
    /// Rated grid connection power of each air conditioner, kW.
    pub air_conditioners_kw: Vec<f64>,
}

impl SteuVE {
    pub fn count(&self) -> usize {
        self.chargers_and_storage + self.heat_pumps_kw.len() + self.air_conditioners_kw.len()
    }

    /// Minimum grid-effective power the operator must still grant while
    /// dimming (Pmin,14a for control via EMS, BK6-22-300 Anlage 1, 4.5.2):
    ///
    /// * with a heat pump or air conditioner above 11 kW:
    ///   `max(0.4 · ΣP_WP, 0.4 · ΣP_Klima) + (n − 1) · GZF · 4.2 kW`
    /// * otherwise: `4.2 kW + (n − 1) · GZF · 4.2 kW`
    pub fn pmin_kw(&self) -> f64 {
        let n = self.count();
        if n == 0 {
            return 0.0;
        }
        let large = |v: &[f64]| v.iter().any(|&p| p > LARGE_DEVICE_THRESHOLD_KW);
        let base = if large(&self.heat_pumps_kw) || large(&self.air_conditioners_kw) {
            let wp: f64 = self.heat_pumps_kw.iter().sum();
            let klima: f64 = self.air_conditioners_kw.iter().sum();
            (LARGE_DEVICE_FACTOR * wp).max(LARGE_DEVICE_FACTOR * klima)
        } else {
            PMIN_PER_DEVICE_KW
        };
        base + (n - 1) as f64 * simultaneity_factor(n) * PMIN_PER_DEVICE_KW
    }
}

/// Mode-3 AC charging (IEC 61851-1): the charger advertises a current limit
/// to the car, and below 6 A the car must stop charging.
pub const EV_MIN_CURRENT_A: f64 = 6.0;
pub const PHASE_VOLTAGE_V: f64 = 230.0;

/// Three-phase charging power in kW for a per-phase current in A.
pub fn three_phase_kw(current_a: f64) -> f64 {
    3.0 * PHASE_VOLTAGE_V * current_a / 1000.0
}

pub fn three_phase_current_a(kw: f64) -> f64 {
    kw * 1000.0 / (3.0 * PHASE_VOLTAGE_V)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn single_wallbox_keeps_4_2_kw() {
        approx(SteuVE { chargers_and_storage: 1, ..Default::default() }.pmin_kw(), 4.2);
    }

    #[test]
    fn wallbox_and_small_heat_pump() {
        // n = 2, GZF 0.8: 4.2 + 1 · 0.8 · 4.2 = 7.56 kW
        let s = SteuVE { chargers_and_storage: 1, heat_pumps_kw: vec![9.0], ..Default::default() };
        approx(s.pmin_kw(), 7.56);
    }

    #[test]
    fn depot_with_four_chargers_and_a_14_kw_heat_pump() {
        // n = 5, GZF 0.65, heat pump > 11 kW:
        // 0.4 · 14 + 4 · 0.65 · 4.2 = 5.6 + 10.92 = 16.52 kW
        let s = SteuVE { chargers_and_storage: 4, heat_pumps_kw: vec![14.0], ..Default::default() };
        approx(s.pmin_kw(), 16.52);
    }

    #[test]
    fn large_air_conditioning_uses_the_larger_group() {
        // n = 2, GZF 0.8: max(0.4 · 12, 0.4 · 30) + 0.8 · 4.2 = 12 + 3.36
        let s = SteuVE { heat_pumps_kw: vec![12.0], air_conditioners_kw: vec![30.0], ..Default::default() };
        approx(s.pmin_kw(), 15.36);
    }

    #[test]
    fn gzf_floor_from_nine_devices() {
        assert_eq!(simultaneity_factor(9), 0.45);
        assert_eq!(simultaneity_factor(40), 0.45);
    }

    #[test]
    fn six_amps_three_phase_is_4_14_kw() {
        approx(three_phase_kw(6.0), 4.14);
        approx(three_phase_kw(32.0), 22.08);
    }
}
