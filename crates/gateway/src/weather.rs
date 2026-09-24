//! Weather forecast for the site from Open-Meteo: irradiance on the plane of
//! the modules and outdoor temperature, in 15-minute steps. A JSON file in
//! the same format can stand in for the API (tests, sites without internet).

use crate::market::http_get;

const OPEN_METEO_API: &str = "https://api.open-meteo.com/v1/forecast";

/// Weather over the 15 minutes ending at `end_s` (Open-Meteo gives
/// irradiance as the mean of the preceding 15 minutes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherPoint {
    pub end_s: i64,
    /// Global irradiance on the tilted plane of the modules, W/m².
    pub gti_w_m2: f64,
    pub temp_c: f64,
}

/// Where the modules are and how they face.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    pub latitude: f64,
    pub longitude: f64,
    pub tilt_deg: f64,
    /// 0 = south, −90 = east, 90 = west (Open-Meteo's convention).
    pub azimuth_deg: f64,
}

pub fn open_meteo_url(p: &Plane) -> String {
    format!(
        "{OPEN_METEO_API}?latitude={:.4}&longitude={:.4}&minutely_15=global_tilted_irradiance,temperature_2m\
         &tilt={:.1}&azimuth={:.1}&past_days=1&forecast_days=3&timeformat=unixtime&timezone=GMT",
        p.latitude, p.longitude, p.tilt_deg, p.azimuth_deg
    )
}

/// Parses Open-Meteo's `minutely_15` block. Steps without irradiance are
/// left out; a missing temperature repeats the one before.
pub fn parse_open_meteo(json: &str) -> Result<Vec<WeatherPoint>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Open-Meteo: not JSON ({e})"))?;
    if let Some(reason) = v["reason"].as_str() {
        return Err(format!("Open-Meteo: {reason}"));
    }
    let m = &v["minutely_15"];
    let (Some(t), Some(g), Some(temp)) =
        (m["time"].as_array(), m["global_tilted_irradiance"].as_array(), m["temperature_2m"].as_array())
    else {
        return Err("Open-Meteo: no minutely_15 irradiance and temperature in the answer".into());
    };
    let mut out = Vec::with_capacity(t.len());
    let mut last_temp = None;
    for ((t, g), temp) in t.iter().zip(g).zip(temp) {
        let temp = temp.as_f64().or(last_temp);
        last_temp = temp;
        if let (Some(end_s), Some(gti), Some(temp_c)) = (t.as_i64(), g.as_f64(), temp) {
            out.push(WeatherPoint { end_s, gti_w_m2: gti.max(0.0), temp_c });
        }
    }
    if out.is_empty() {
        return Err("Open-Meteo: empty forecast".into());
    }
    Ok(out)
}

/// Fetches the forecast (blocking): from Open-Meteo, or from `file`.
pub fn fetch(plane: &Plane, file: Option<&std::path::Path>) -> Result<(String, Vec<WeatherPoint>), String> {
    match file {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(("file".into(), parse_open_meteo(&text)?))
        }
        None => {
            let (status, body) = http_get(&open_meteo_url(plane)).map_err(|e| format!("Open-Meteo: {e}"))?;
            if status != 200 {
                return parse_open_meteo(&body).and(Err(format!("Open-Meteo: HTTP {status}")));
            }
            Ok(("open-meteo".into(), parse_open_meteo(&body)?))
        }
    }
}

/// AC power the modules can deliver, kW: installed power × irradiance /
/// 1000 W/m² × performance ratio, less 0.4% per kelvin of cell temperature
/// above 25 °C (the cell runs about 25 K above the air at 1000 W/m²).
pub fn pv_kw(gti_w_m2: f64, temp_c: f64, installed_kw: f64, performance_ratio: f64) -> f64 {
    let gti = gti_w_m2.max(0.0);
    let cell_c = temp_c + 0.025 * gti;
    let derate = (1.0 - 0.004 * (cell_c - 25.0)).clamp(0.75, 1.1);
    (installed_kw * gti / 1000.0 * performance_ratio * derate).clamp(0.0, installed_kw)
}

/// The weather forecast known to the gateway.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherBook {
    points: Vec<WeatherPoint>,
}

impl WeatherBook {
    pub fn points(&self) -> &[WeatherPoint] {
        &self.points
    }

    /// Replaces the forecast with a fresh one (keeping older observations
    /// that the fresh one no longer carries).
    pub fn merge(&mut self, fresh: Vec<WeatherPoint>, now_s: i64) {
        let Some(lo) = fresh.first().map(|p| p.end_s) else { return };
        self.points.retain(|p| p.end_s < lo && p.end_s > now_s - 2 * 86_400);
        self.points.extend(fresh);
        self.points.sort_by_key(|p| p.end_s);
    }

    /// The step ending at `end_s`, or the nearest one within 15 minutes.
    pub fn at_end(&self, end_s: i64) -> Option<WeatherPoint> {
        let i = self.points.partition_point(|p| p.end_s < end_s);
        [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|j| self.points.get(j))
            .filter(|p| (p.end_s - end_s).abs() <= 900)
            .min_by_key(|p| (p.end_s - end_s).abs())
            .copied()
    }

    pub fn until_s(&self) -> Option<i64> {
        self.points.last().map(|p| p.end_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_meteo_minutely_block_is_parsed() {
        let json = r#"{"latitude":49.22,"longitude":7.0,"utc_offset_seconds":0,"timezone":"GMT",
            "minutely_15_units":{"time":"unixtime","global_tilted_irradiance":"W/m²","temperature_2m":"°C"},
            "minutely_15":{"time":[1790208000,1790208900,1790209800],
                           "global_tilted_irradiance":[0.0,null,512.5],
                           "temperature_2m":[11.2,11.0,null]}}"#;
        let p = parse_open_meteo(json).unwrap();
        assert_eq!(p.len(), 2, "the step without irradiance is left out");
        assert_eq!(p[1], WeatherPoint { end_s: 1_790_209_800, gti_w_m2: 512.5, temp_c: 11.0 });
        let err = r#"{"error":true,"reason":"Latitude must be in range of -90 to 90°."}"#;
        assert!(parse_open_meteo(err).unwrap_err().contains("Latitude must be"));
    }

    #[test]
    fn url_asks_for_the_plane_and_unix_times() {
        let url = open_meteo_url(&Plane { latitude: 49.23, longitude: 7.0, tilt_deg: 15.0, azimuth_deg: -90.0 });
        assert!(url.contains("latitude=49.2300&longitude=7.0000"));
        assert!(url.contains("minutely_15=global_tilted_irradiance,temperature_2m&tilt=15.0&azimuth=-90.0"));
        assert!(url.contains("timeformat=unixtime"));
    }

    #[test]
    fn pv_power_follows_irradiance_and_loses_some_to_heat() {
        assert_eq!(pv_kw(0.0, 10.0, 120.0, 0.85), 0.0);
        let cold = pv_kw(800.0, 0.0, 120.0, 0.85);
        let hot = pv_kw(800.0, 35.0, 120.0, 0.85);
        assert!(cold > 0.8 * 120.0 * 0.85 && hot < cold, "{cold} {hot}");
        assert!((pv_kw(1000.0, 0.0, 120.0, 0.85) - 120.0 * 0.85).abs() < 1e-9, "cell at 25 °C");
        assert_eq!(pv_kw(1400.0, -20.0, 120.0, 1.0), 120.0, "never above the installed power");
    }

    #[test]
    fn book_finds_the_step_and_keeps_recent_history() {
        let mut b = WeatherBook::default();
        let p = |end_s| WeatherPoint { end_s, gti_w_m2: end_s as f64, temp_c: 5.0 };
        b.merge((0..8).map(|k| p(1_000_000 + k * 900)).collect(), 1_000_000);
        assert_eq!(b.at_end(1_000_900).unwrap().end_s, 1_000_900);
        assert_eq!(b.at_end(1_001_000).unwrap().end_s, 1_000_900, "nearest within 15 minutes");
        assert!(b.at_end(1_000_000 + 9 * 900 + 1).is_none());
        b.merge(vec![p(1_000_000 + 4 * 900)], 1_000_000);
        assert_eq!(b.points().len(), 5, "the fresh forecast replaces the old one from its first step");
    }
}
