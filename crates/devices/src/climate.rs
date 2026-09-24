//! Weather, seasons and energy prices of the simulated site.
//!
//! Two periods, each with real day-ahead prices: a spring week-end in April
//! 2025 with deeply negative midday prices, and a January 2025 *Dunkelflaute*
//! (little wind and sun) with a 583 €/MWh evening peak — the situation in
//! which German DSOs are most likely to dim heat pumps and chargers.

use crate::prices_data::{SPRING_EUR_MWH, WINTER_EUR_MWH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    Spring,
    Winter,
}

impl Season {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "spring" => Some(Season::Spring),
            "winter" => Some(Season::Winter),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Winter => "winter",
        }
    }
}

/// Sun and temperature of a season in south-west Germany (local time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    pub season: Season,
    pub sunrise_h: f64,
    pub sunset_h: f64,
    /// PV output under a clear sky at solar noon, fraction of installed power.
    pub clear_peak: f64,
    pub t_mean_c: f64,
    pub t_amp_c: f64,
    pub t_peak_h: f64,
    /// Day-to-day cloudiness (fraction of clear-sky PV actually produced):
    /// climatological mean, standard deviation and the worst day considered.
    pub cloud_mean: f64,
    pub cloud_std: f64,
    pub cloud_worst: f64,
}

impl Climate {
    pub fn of(season: Season) -> Self {
        match season {
            // Moments of the mixture used by `draw_cloudiness`: sunny 35%
            // U(0.9, 1.0), mixed 35% U(0.55, 0.85), overcast 30% U(0.2, 0.45).
            Season::Spring => Climate {
                season,
                sunrise_h: 6.5,
                sunset_h: 19.5,
                clear_peak: 0.82,
                t_mean_c: 9.0,
                t_amp_c: 6.0,
                t_peak_h: 15.0,
                cloud_mean: 0.675,
                cloud_std: 0.261,
                cloud_worst: 0.2,
            },
            // Sunny 25% U(0.85, 1.0), mixed 30% U(0.5, 0.8), overcast 45% U(0.15, 0.4).
            Season::Winter => Climate {
                season,
                sunrise_h: 8.25,
                sunset_h: 16.9,
                clear_peak: 0.40,
                t_mean_c: -0.5,
                t_amp_c: 3.5,
                t_peak_h: 14.0,
                cloud_mean: 0.55,
                cloud_std: 0.278,
                cloud_worst: 0.15,
            },
        }
    }

    /// Clear-sky PV output, fraction of installed power, at `t_s` seconds
    /// after midnight of day 0 (repeats every day).
    pub fn clear_sky_fraction(&self, t_s: f64) -> f64 {
        let h = t_s.rem_euclid(86_400.0) / 3600.0;
        if h <= self.sunrise_h || h >= self.sunset_h {
            return 0.0;
        }
        let x = (h - self.sunrise_h) / (self.sunset_h - self.sunrise_h);
        self.clear_peak * (std::f64::consts::PI * x).sin().powf(1.4)
    }

    pub fn outdoor_c(&self, t_s: f64) -> f64 {
        let day = (t_s / 3600.0 - self.t_peak_h) / 24.0 * std::f64::consts::TAU;
        self.t_mean_c + self.t_amp_c * day.cos()
    }

    /// Day-ahead price, €/MWh, for the hour containing `t_s` (the last known
    /// hour repeats beyond the 72-hour series).
    pub fn day_ahead_eur_mwh(&self, t_s: f64) -> f64 {
        let series = match self.season {
            Season::Spring => &SPRING_EUR_MWH,
            Season::Winter => &WINTER_EUR_MWH,
        };
        let i = (t_s / 3600.0).floor().clamp(0.0, (series.len() - 1) as f64) as usize;
        series[i]
    }

    /// A day's cloudiness from a uniform draw `u` and a second one `r`.
    pub fn draw_cloudiness(&self, u: f64, r: f64) -> f64 {
        match self.season {
            Season::Spring if u < 0.35 => 0.9 + 0.1 * r,
            Season::Spring if u < 0.70 => 0.55 + 0.3 * r,
            Season::Spring => 0.2 + 0.25 * r,
            Season::Winter if u < 0.25 => 0.85 + 0.15 * r,
            Season::Winter if u < 0.55 => 0.5 + 0.3 * r,
            Season::Winter => 0.15 + 0.25 * r,
        }
    }
}

/// Energy prices of a commercial customer on a dynamic, day-ahead-indexed
/// contract, and the market value it gets for exported PV.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tariff {
    /// Grid fees, levies, electricity tax and supplier margin on top of the
    /// day-ahead price, €/kWh. Illustrative; real values depend on the DSO.
    pub import_adder_eur_kwh: f64,
}

impl Default for Tariff {
    fn default() -> Self {
        Tariff { import_adder_eur_kwh: 0.12 }
    }
}

impl Tariff {
    pub fn import_eur_kwh(&self, day_ahead_eur_mwh: f64) -> f64 {
        day_ahead_eur_mwh / 1000.0 + self.import_adder_eur_kwh
    }

    /// Exported PV earns the market price, and nothing while it is negative
    /// (no payment in negative hours: EEG §51, Solarspitzengesetz 2025).
    pub fn export_eur_kwh(&self, day_ahead_eur_mwh: f64) -> f64 {
        day_ahead_eur_mwh.max(0.0) / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_are_in_local_time() {
        let spring = Climate::of(Season::Spring);
        assert_eq!(spring.day_ahead_eur_mwh(14.0 * 3600.0), -114.57);
        let winter = Climate::of(Season::Winter);
        assert_eq!(winter.day_ahead_eur_mwh(17.5 * 3600.0), 583.40);
    }

    #[test]
    fn import_price_stays_above_export_price() {
        let t = Tariff::default();
        for p in [-500.0, -114.57, 0.0, 80.0, 583.4] {
            assert!(t.import_eur_kwh(p) >= t.export_eur_kwh(p) - 1e-12 || p < -120.0, "{p}");
        }
        assert_eq!(t.export_eur_kwh(-60.0), 0.0);
    }

    #[test]
    fn winter_days_are_short_and_cold() {
        let w = Climate::of(Season::Winter);
        assert_eq!(w.clear_sky_fraction(7.0 * 3600.0), 0.0);
        assert!(w.clear_sky_fraction(12.5 * 3600.0) > 0.35);
        assert!(w.clear_sky_fraction(86_400.0 + 12.5 * 3600.0) > 0.35, "repeats the next day");
        assert!(w.outdoor_c(5.0 * 3600.0) < 0.0);
    }

    #[test]
    fn cloudiness_moments_match_the_mixture() {
        for season in [Season::Spring, Season::Winter] {
            let c = Climate::of(season);
            let n = 400;
            let (mut s, mut s2) = (0.0, 0.0);
            for i in 0..n {
                for j in 0..n {
                    let m = c.draw_cloudiness((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64);
                    s += m;
                    s2 += m * m;
                }
            }
            let mean = s / (n * n) as f64;
            let std = (s2 / (n * n) as f64 - mean * mean).sqrt();
            assert!((mean - c.cloud_mean).abs() < 0.002, "{season:?} {mean}");
            assert!((std - c.cloud_std).abs() < 0.002, "{season:?} {std}");
        }
    }
}
