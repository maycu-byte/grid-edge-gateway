//! Weather, seasons and energy prices of the simulated site.
//!
//! Two periods, each with real day-ahead prices: a spring week-end in April
//! 2025 with deeply negative midday prices, and a January 2025 *Dunkelflaute*
//! (little wind and sun) with a 583 €/MWh evening peak — the situation in
//! which German DSOs are most likely to dim heat pumps and chargers.
//!
//! Any day of 2025 can also be simulated on its real data: the day-ahead
//! prices of that day in the country's bidding zone and the measured sun and
//! temperature of Stuttgart, Vienna or Zurich (`year2025_data`).

use crate::prices_data::{SPRING_EUR_MWH, WINTER_EUR_MWH};
use crate::year2025_data::{GHI_AT, GHI_CH, GHI_DE, PRICE_AT, PRICE_CH, PRICE_DE, TEMP_AT, TEMP_CH, TEMP_DE};

/// Days in each month of 2025.
const MONTH_DAYS: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
/// Days of 2025 (0 = 1 January) whose local midnight is in summer time (CEST):
/// 31 March to 26 October.
const CEST_DAYS: std::ops::RangeInclusive<u16> = 89..=298;
/// Unix time of 1 January 2025, 00:00 CET, ms.
const EPOCH_2025_MS: i64 = 1_735_686_000_000;
/// Hours in the 2025 series.
const HOURS_2025: usize = 8760;

/// Where a real day of 2025 is taken from: the bidding zone for the prices
/// and a city of that country for the weather.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// DE-LU, weather of Stuttgart.
    De,
    /// AT, weather of Vienna.
    At,
    /// CH, weather of Zurich.
    Ch,
}

impl Zone {
    /// "DE", "AT" or "CH".
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "DE" => Some(Zone::De),
            "AT" => Some(Zone::At),
            "CH" => Some(Zone::Ch),
            _ => None,
        }
    }

    /// Latitude and longitude of the weather point, degrees.
    fn coordinates(self) -> (f64, f64) {
        match self {
            Zone::De => (48.78, 9.18),
            Zone::At => (48.21, 16.37),
            Zone::Ch => (47.38, 8.54),
        }
    }

    fn series(self) -> (&'static [f32; HOURS_2025], &'static [f32; HOURS_2025], &'static [u16; HOURS_2025]) {
        match self {
            Zone::De => (&PRICE_DE, &TEMP_DE, &GHI_DE),
            Zone::At => (&PRICE_AT, &TEMP_AT, &GHI_AT),
            Zone::Ch => (&PRICE_CH, &TEMP_CH, &GHI_CH),
        }
    }
}
/// Share of the horizontal irradiance the rooftop PV turns into AC power
/// (performance ratio, per 1000 W/m²).
const PERFORMANCE_RATIO: f64 = 0.85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    Spring,
    Winter,
    /// A real day of 2025 (0 = 1 January) in a country.
    Day(u16, Zone),
}

impl Season {
    /// "spring", "winter" or a date of 2025 such as "2025-01-20".
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "spring" => Some(Season::Spring),
            "winter" => Some(Season::Winter),
            d => Self::parse_date(d),
        }
    }

    fn parse_date(s: &str) -> Option<Self> {
        let mut it = s.split('-');
        let (y, m, d) = (it.next()?, it.next()?.parse::<u16>().ok()?, it.next()?.parse::<u16>().ok()?);
        if y != "2025" || it.next().is_some() || !(1..=12).contains(&m) || d == 0 || d > MONTH_DAYS[m as usize - 1] {
            return None;
        }
        Some(Season::Day(MONTH_DAYS[..m as usize - 1].iter().sum::<u16>() + d - 1, Zone::De))
    }

    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Winter => "winter",
            Season::Day(..) => "2025",
        }
    }

    /// The same day in another country; the two study days stay as they are.
    pub fn in_zone(self, zone: Zone) -> Self {
        match self {
            Season::Day(d, _) => Season::Day(d, zone),
            s => s,
        }
    }

    /// "spring", "winter" or the date, e.g. "2025-01-20".
    pub fn label(self) -> String {
        match self {
            Season::Day(d, _) => {
                let (m, day) = month_day(d);
                format!("2025-{:02}-{:02}", m + 1, day + 1)
            }
            s => s.name().to_string(),
        }
    }

    /// Unix time of local midnight of the simulated day 0, ms.
    pub fn epoch_ms(self) -> i64 {
        match self {
            Season::Spring => 1_743_890_400_000, // 2025-04-06 00:00 CEST
            Season::Winter => 1_737_327_600_000, // 2025-01-20 00:00 CET
            Season::Day(d, _) => {
                let summer = if CEST_DAYS.contains(&d) { 3_600_000 } else { 0 };
                EPOCH_2025_MS + d as i64 * 86_400_000 - summer
            }
        }
    }
}

/// (month 0–11, day of month 0–30) of a day of 2025.
fn month_day(mut d: u16) -> (usize, u16) {
    for (m, &n) in MONTH_DAYS.iter().enumerate() {
        if d < n {
            return (m, d);
        }
        d -= n;
    }
    (11, 30)
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
    /// A real day of 2025: sun, temperature and prices come from the data.
    pub actual_day: Option<(u16, Zone)>,
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
                actual_day: None,
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
                actual_day: None,
            },
            Season::Day(d, z) => Self::of_day(d, z),
        }
    }

    /// A real day: the sun's path at the site, the cloudiness climatology of
    /// the half-year (for the forecaster), measured weather for the rest.
    fn of_day(d: u16, zone: Zone) -> Self {
        let (lat_deg, lon_deg) = zone.coordinates();
        let (_, temps, _) = zone.series();
        let (month, _) = month_day(d);
        let base = Self::of(if (3..=8).contains(&month) { Season::Spring } else { Season::Winter });
        let n = d as f64 + 1.0;
        let decl = (-23.44f64).to_radians() * (std::f64::consts::TAU * (n + 10.0) / 365.0).cos();
        let lat = lat_deg.to_radians();
        let half_day_h = (-lat.tan() * decl.tan()).clamp(-1.0, 1.0).acos().to_degrees() / 15.0;
        // Solar noon on the local clock: longitude, and an hour later in summer time.
        let summer = if (88..=297).contains(&d) { 1.0 } else { 0.0 };
        let noon_h = 12.0 + (15.0 - lon_deg) / 15.0 + summer;
        let elevation = (90.0 - lat_deg).to_radians() + decl;
        let hours = (d as usize * 24..d as usize * 24 + 24).map(|i| temps[i] as f64);
        let (lo, hi) = hours.fold((f64::MAX, f64::MIN), |(a, b), t| (a.min(t), b.max(t)));
        Climate {
            season: Season::Day(d, zone),
            sunrise_h: noon_h - half_day_h,
            sunset_h: noon_h + half_day_h,
            // Clear-sky horizontal irradiance ~ 1100 W/m² · sin(elevation)^1.15.
            clear_peak: PERFORMANCE_RATIO * 1.1 * elevation.sin().powf(1.15),
            t_mean_c: (lo + hi) / 2.0,
            t_amp_c: (hi - lo) / 2.0,
            actual_day: Some((d, zone)),
            ..base
        }
    }

    /// Index into the 2025 series and the fraction towards the next hour,
    /// for hourly means centred on the half hour.
    fn hour_at(d: u16, t_s: f64) -> (usize, usize, f64) {
        let x = (d as f64 * 24.0 + t_s / 3600.0 - 0.5).clamp(0.0, (HOURS_2025 - 1) as f64);
        let i = x.floor() as usize;
        (i, (i + 1).min(HOURS_2025 - 1), x - i as f64)
    }

    /// PV available from the sun, fraction of installed power, with the
    /// simulator's cloudiness `cloud` (ignored on a real day: the measured
    /// irradiance already has its clouds).
    pub fn solar_fraction(&self, t_s: f64, cloud: f64) -> f64 {
        match self.actual_day {
            Some((d, zone)) => {
                let (_, _, ghis) = zone.series();
                let (i, j, f) = Self::hour_at(d, t_s);
                let ghi = ghis[i] as f64 * (1.0 - f) + ghis[j] as f64 * f;
                (ghi / 1000.0 * PERFORMANCE_RATIO).clamp(0.0, 1.0)
            }
            None => self.clear_sky_fraction(t_s) * cloud,
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
        if let Some((d, zone)) = self.actual_day {
            let (_, temps, _) = zone.series();
            let (i, j, f) = Self::hour_at(d, t_s);
            return temps[i] as f64 * (1.0 - f) + temps[j] as f64 * f;
        }
        let day = (t_s / 3600.0 - self.t_peak_h) / 24.0 * std::f64::consts::TAU;
        self.t_mean_c + self.t_amp_c * day.cos()
    }

    /// Day-ahead price, €/MWh, for the hour containing `t_s` (the last known
    /// hour repeats beyond the 72-hour series).
    pub fn day_ahead_eur_mwh(&self, t_s: f64) -> f64 {
        let series = match self.season {
            Season::Spring => &SPRING_EUR_MWH,
            Season::Winter => &WINTER_EUR_MWH,
            Season::Day(d, zone) => {
                let (prices, _, _) = zone.series();
                let i = (d as f64 * 24.0 + (t_s / 3600.0).floor()).clamp(0.0, (HOURS_2025 - 1) as f64);
                return prices[i as usize] as f64;
            }
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
            // A real day has measured sun; the draw is not used.
            Season::Day(..) => 1.0,
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
    /// Demand charge (Leistungspreis) on the highest quarter-hour of import
    /// in the billing period, € per kW and year. Illustrative, of the order
    /// of German tariffs for metered commercial customers.
    pub demand_eur_per_kw_year: f64,
}

impl Default for Tariff {
    fn default() -> Self {
        Tariff { import_adder_eur_kwh: 0.12, demand_eur_per_kw_year: 100.0 }
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

    /// The demand charge a stretch of `hours` carries on its own peak when
    /// it stands for every day of the billing period (a representative day).
    pub fn demand_eur_per_kw(&self, hours: f64) -> f64 {
        self.demand_eur_per_kw_year * hours / 8760.0
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
    fn a_real_day_matches_the_two_study_days() {
        let jan20 = Season::parse("2025-01-20").unwrap();
        assert_eq!(jan20, Season::Day(19, Zone::De));
        assert_eq!(jan20.label(), "2025-01-20");
        assert_eq!(jan20.epoch_ms(), Season::Winter.epoch_ms());
        assert_eq!(Season::parse("2025-04-06").unwrap().epoch_ms(), Season::Spring.epoch_ms());
        let w = Climate::of(jan20);
        assert!((w.day_ahead_eur_mwh(17.5 * 3600.0) - 583.40).abs() < 0.01);
        let s = Climate::of(Season::parse("2025-04-06").unwrap());
        assert!((s.day_ahead_eur_mwh(14.5 * 3600.0) + 114.57).abs() < 0.01);
        assert!(Season::parse("2025-02-29").is_none() && Season::parse("2024-01-01").is_none());
    }

    #[test]
    fn each_country_has_its_own_prices_and_weather() {
        let jan20 = Season::parse("2025-01-20").unwrap();
        let de = Climate::of(jan20);
        let at = Climate::of(jan20.in_zone(Zone::At));
        let ch = Climate::of(jan20.in_zone(Zone::Ch));
        let evening = 17.5 * 3600.0;
        assert!((at.day_ahead_eur_mwh(evening) - 561.75).abs() < 0.01, "AT peak of 2025");
        assert!(ch.day_ahead_eur_mwh(evening) < de.day_ahead_eur_mwh(evening));
        assert_ne!(at.outdoor_c(12.0 * 3600.0), de.outdoor_c(12.0 * 3600.0));
        // Vienna lies 7° east of Stuttgart: its sun rises about half an hour earlier
        assert!(de.sunrise_h - at.sunrise_h > 0.4);
        assert_eq!(Season::Winter.in_zone(Zone::At), Season::Winter);
    }

    #[test]
    fn a_real_day_has_measured_sun_and_temperature() {
        let jun = Climate::of(Season::parse("2025-06-21").unwrap());
        let jan = Climate::of(Season::parse("2025-01-15").unwrap());
        assert_eq!(jun.solar_fraction(1.0 * 3600.0, 1.0), 0.0, "night");
        assert!(jun.sunset_h - jun.sunrise_h > 15.5 && jan.sunset_h - jan.sunrise_h < 9.0);
        assert!(jun.sunset_h > 21.0, "summer time: the sun sets after 21:00");
        let noon = |c: &Climate| (10..16).map(|h| c.solar_fraction(h as f64 * 3600.0, 0.3)).fold(0.0, f64::max);
        assert!(noon(&jun) > noon(&jan));
        assert!(noon(&jun) <= 1.0);
        let t = |c: &Climate| c.outdoor_c(14.0 * 3600.0);
        assert!(t(&jun) > t(&jan) + 10.0);
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
