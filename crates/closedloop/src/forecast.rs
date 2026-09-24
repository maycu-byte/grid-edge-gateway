//! Forecasts for the planner, built only from what a real EMS would know:
//! the clear-sky model of the site, the season's climatology of cloudiness,
//! a nowcast of today's cloudiness from the PV the inverters say is
//! available, the depot's usual load profile, and day-ahead prices.

use devices::climate::{Climate, Tariff};
use devices::sim::{Building, cop};

/// Std of today's cloudiness once it has been observed for a while.
const NOWCAST_STD: f64 = 0.08;
/// Time constant of the nowcast's exponential smoothing, s.
const NOWCAST_TAU_S: f64 = 1800.0;
/// Base-load forecast: the depot's schedule and its error.
const BASE_WORK_KW: f64 = 26.0;
const BASE_OFF_KW: f64 = 16.0;
pub const BASE_SIGMA_KW: f64 = 1.5;

/// Everything time-dependent the planner needs for one horizon.
#[derive(Debug, Clone, PartialEq)]
pub struct Horizon {
    pub forecast: planner::Forecast,
    pub outdoor_c: Vec<f64>,
    pub cop: Vec<f64>,
    pub gains_kw: Vec<f64>,
    pub t_min_c: Vec<f64>,
    pub t_max_c: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Forecaster {
    pub climate: Climate,
    pub tariff: Tariff,
    pv_installed_kw: f64,
    /// Today's cloudiness estimate: (day, estimate, last update time).
    today: Option<(i64, f64, f64)>,
}

fn day_of(t_s: f64) -> i64 {
    (t_s / 86_400.0).floor() as i64
}

fn working(t_s: f64) -> bool {
    (7.0..18.0).contains(&(t_s.rem_euclid(86_400.0) / 3600.0))
}

impl Forecaster {
    pub fn new(climate: Climate, tariff: Tariff, pv_installed_kw: f64) -> Self {
        Forecaster { climate, tariff, pv_installed_kw, today: None }
    }

    /// Learns today's cloudiness from the PV the sun currently allows.
    pub fn observe(&mut self, t_s: f64, pv_available_kw: Option<f64>) {
        let clear = self.climate.clear_sky_fraction(t_s) * self.pv_installed_kw;
        let Some(avail) = pv_available_kw else { return };
        if clear < 0.1 * self.pv_installed_kw {
            return; // sun too low to judge the sky
        }
        let ratio = (avail / clear).clamp(0.0, 1.2);
        let day = day_of(t_s);
        self.today = Some(match self.today {
            Some((d, est, last)) if d == day => {
                let alpha = 1.0 - (-(t_s - last).max(0.0) / NOWCAST_TAU_S).exp();
                (d, est + alpha * (ratio - est), t_s)
            }
            _ => (day, ratio, t_s),
        });
    }

    /// Cloudiness expected at `t_s`: (mean, std, worst case).
    pub fn cloudiness(&self, t_s: f64) -> (f64, f64, f64) {
        match self.today {
            Some((d, est, _)) if d == day_of(t_s) => (est, NOWCAST_STD, (est - 0.25).max(0.05)),
            _ => (self.climate.cloud_mean, self.climate.cloud_std, self.climate.cloud_worst),
        }
    }

    pub fn horizon(&self, now_s: f64, steps: usize, dt_h: f64) -> Horizon {
        let mut f = planner::Forecast {
            pv_kw: Vec::with_capacity(steps),
            pv_sigma_kw: Vec::with_capacity(steps),
            pv_worst_kw: Vec::with_capacity(steps),
            base_kw: Vec::with_capacity(steps),
            base_sigma_kw: BASE_SIGMA_KW,
            price_import_eur_kwh: Vec::with_capacity(steps),
            price_export_eur_kwh: Vec::with_capacity(steps),
        };
        let mut h = Horizon {
            forecast: planner::Forecast { ..f.clone() },
            outdoor_c: Vec::with_capacity(steps),
            cop: Vec::with_capacity(steps),
            gains_kw: Vec::with_capacity(steps),
            t_min_c: Vec::with_capacity(steps),
            t_max_c: Vec::with_capacity(steps),
        };
        for k in 0..steps {
            let t_mid = now_s + (k as f64 + 0.5) * dt_h * 3600.0;
            let t_end = now_s + (k as f64 + 1.0) * dt_h * 3600.0;
            let clear = self.climate.clear_sky_fraction(t_mid) * self.pv_installed_kw;
            let (m, s, w) = self.cloudiness(t_mid);
            f.pv_kw.push(m * clear);
            f.pv_sigma_kw.push(s * clear);
            f.pv_worst_kw.push(w * clear);
            f.base_kw.push(if working(t_mid) { BASE_WORK_KW } else { BASE_OFF_KW });
            let da = self.climate.day_ahead_eur_mwh(t_mid);
            f.price_import_eur_kwh.push(self.tariff.import_eur_kwh(da));
            f.price_export_eur_kwh.push(self.tariff.export_eur_kwh(da));
            let t_out = self.climate.outdoor_c(t_mid);
            h.outdoor_c.push(t_out);
            h.cop.push(cop(t_out));
            h.gains_kw.push(Building::gains_kw(t_mid));
            h.t_min_c.push(Building::comfort_min_c(t_end));
            h.t_max_c.push(Building::COMFORT_MAX_C);
        }
        h.forecast = f;
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devices::climate::Season;

    #[test]
    fn nowcast_replaces_climatology_for_the_rest_of_the_day() {
        let c = Climate::of(Season::Spring);
        let mut f = Forecaster::new(c, Tariff::default(), 120.0);
        let noon = 12.0 * 3600.0;
        assert_eq!(f.cloudiness(noon).0, c.cloud_mean);
        let clear = c.clear_sky_fraction(noon) * 120.0;
        for k in 0..400 {
            f.observe(noon + k as f64 * 10.0, Some(0.3 * clear));
        }
        let (m, s, _) = f.cloudiness(noon + 3600.0);
        assert!((m - 0.3).abs() < 0.05, "{m}");
        assert_eq!(s, NOWCAST_STD);
        assert_eq!(f.cloudiness(noon + 86_400.0).0, c.cloud_mean, "tomorrow: climatology again");
    }

    #[test]
    fn horizon_has_prices_comfort_and_no_sun_at_night() {
        let f = Forecaster::new(Climate::of(Season::Winter), Tariff::default(), 120.0);
        let h = f.horizon(16.0 * 3600.0, 96, 0.25);
        assert_eq!(h.forecast.pv_kw.len(), 96);
        assert!(h.forecast.pv_kw[20] == 0.0, "21:00 is dark");
        assert!(h.forecast.price_import_eur_kwh[5] > 0.6, "17:15: the 583 €/MWh hour");
        assert_eq!(h.t_min_c[30], 17.0, "night set-back at 23:45");
        assert_eq!(h.t_min_c[0], 20.0);
    }
}
