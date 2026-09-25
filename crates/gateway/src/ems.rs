//! The planning layer inside the gateway.
//!
//! A background task fetches day-ahead prices and the weather forecast and
//! re-plans every quarter-hour, and also when a car plugs in or leaves, when
//! a dimming starts or ends, and when new prices come in. The control loop
//! only reads the plan's guidance for the current quarter-hour and reports
//! what it measured. It never waits for the network or the solver. Without a
//! plan (no prices yet, a failed solve, a plan older than an hour) the site
//! runs on rules alone.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use control::{Guidance, Jurisdiction, Readings, SiteConfig};
use planner::{DemandCharge, Forecast, Plan, PlanInput, Uncertainty};
use planning::{
    BaseLoadProfile, BuildingModel, Horizon, PlanRecord, QuarterPeak, SiteModel, build_input, cet_offset_s,
    civil_from_unix, local_seconds_of_day,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::config::{self, Planner as PlannerCfg};
use crate::market::{self, Fetched, PriceBook, PriceJob, PricePoint};
use crate::snapshot::now_ms;
use crate::weather::{self, Plane, WeatherBook, WeatherPoint};

const STEP_S: f64 = 900.0;
const STEPS: usize = 96;
/// A plan older than this is not followed any more.
const PLAN_MAX_AGE_S: f64 = 3600.0;
/// A failed fetch is retried after this long; a successful one is not
/// repeated within a minute.
const RETRY_S: f64 = 300.0;
const MIN_REFETCH_S: f64 = 60.0;
/// Weight of each new day in the base-load profile.
const PROFILE_ALPHA: f64 = 0.2;
/// How far into the plan today's observed PV-to-forecast ratio carries.
const NOWCAST_TAU_S: f64 = 7200.0;
/// A dimming in progress is expected to last this long.
const DIM_EXPECTED_S: f64 = 7200.0;

fn unix_now() -> f64 {
    now_ms() as f64 / 1000.0
}

/// Local calendar year, or year·12 + month, for the billing period.
fn billing_period(cfg: &PlannerCfg, unix_s: f64) -> i64 {
    let c = civil_from_unix(unix_s + cet_offset_s(unix_s));
    match cfg.demand_charge.as_ref().map(|d| d.billing.as_str()) {
        Some("month") => i64::from(c.year) * 12 + i64::from(c.month) - 1,
        _ => i64::from(c.year),
    }
}

#[derive(Debug, Clone, Default)]
struct Feed {
    source: Option<String>,
    fetched_s: Option<f64>,
    tried_s: Option<f64>,
    error: Option<String>,
}

impl Feed {
    /// Time to fetch: never fetched, older than `refresh_s`, or `short` (it
    /// does not reach far enough ahead) and older than a quarter-hour.
    fn due(&self, now: f64, refresh_s: f64, short: bool) -> bool {
        let wait = if self.error.is_some() { RETRY_S } else { MIN_REFETCH_S };
        if self.tried_s.is_some_and(|t| now - t < wait) {
            return false;
        }
        match self.fetched_s {
            None => true,
            Some(f) => now - f >= refresh_s || (short && now - f >= STEP_S),
        }
    }

    fn view(&self, until_s: Option<i64>) -> FeedView {
        FeedView {
            source: self.source.clone(),
            fetched_ms: self.fetched_s.map(|t| (t * 1000.0) as i64),
            until_ms: until_s.map(|t| t * 1000),
            error: self.error.clone(),
        }
    }
}

/// The last control cycle, as the planner needs it.
#[derive(Debug, Clone)]
struct Latest {
    unix_s: f64,
    readings: Readings,
    dimmed: bool,
    /// Load the planner does not control, kW.
    uncontrolled_kw: Option<f64>,
}

/// A planning problem taken out of the lock to be solved.
pub struct Prepared {
    pub input: PlanInput,
    ev_of_charger: Vec<Option<usize>>,
    start_s: f64,
    made_at_s: f64,
    why: &'static str,
}

pub struct Ems {
    cfg: PlannerCfg,
    site: SiteConfig,
    floor_kw: f64,
    zone: String,
    model: SiteModel,
    dim_windows: Vec<(f64, f64)>,
    /// The building's day, local seconds after midnight.
    day: Option<(f64, f64)>,
    prices: PriceBook,
    prices_feed: Feed,
    prices_skipped: Vec<String>,
    weather: WeatherBook,
    weather_feed: Feed,
    base: BaseLoadProfile,
    peak: QuarterPeak,
    pv_ratio: Option<f64>,
    latest: Option<Latest>,
    plan: Option<PlanRecord>,
    plans: u32,
    solve_ms: Option<f64>,
    plan_error: Option<String>,
    planned_s: Option<f64>,
    wanted: Option<&'static str>,
    plugged: Vec<bool>,
    dimmed: bool,
}

impl Ems {
    pub fn new(
        cfg: PlannerCfg,
        site: SiteConfig,
        floor_kw: f64,
        jurisdiction: Jurisdiction,
        saved: Option<Saved>,
        now_s: f64,
    ) -> Self {
        let uncertainty = match cfg.uncertainty.as_str() {
            "chance" => Uncertainty::Chance { epsilon: cfg.epsilon },
            "robust" => Uncertainty::Robust,
            _ => Uncertainty::Deterministic,
        };
        let model = SiteModel {
            degradation_eur_per_kwh: cfg.battery_degradation_eur_per_kwh,
            building: cfg
                .building
                .as_ref()
                .map(|b| BuildingModel { ua_kw_per_k: b.ua_kw_per_k, cap_kwh_per_k: b.cap_kwh_per_k }),
            uncertainty,
            ..SiteModel::default()
        };
        let period = billing_period(&cfg, now_s);
        let mut ems = Ems {
            zone: cfg.bidding_zone(jurisdiction),
            dim_windows: cfg.dim_windows.iter().filter_map(|w| config::parse_window(w)).collect(),
            day: cfg
                .building
                .as_ref()
                .and_then(|b| Some((config::parse_hhmm(&b.day_start)?, config::parse_hhmm(&b.day_end)?))),
            model,
            floor_kw,
            prices: PriceBook::default(),
            prices_feed: Feed::default(),
            prices_skipped: Vec::new(),
            weather: WeatherBook::default(),
            weather_feed: Feed::default(),
            base: BaseLoadProfile::new(PROFILE_ALPHA),
            peak: QuarterPeak::new(period, 0.0),
            pv_ratio: None,
            latest: None,
            plan: None,
            plans: 0,
            solve_ms: None,
            plan_error: None,
            planned_s: None,
            wanted: None,
            plugged: vec![false; site.chargers.len()],
            dimmed: false,
            site,
            cfg,
        };
        if let Some(s) = saved {
            ems.base = BaseLoadProfile::restore(PROFILE_ALPHA, s.base_mean, s.base_miss_sq);
            if s.peak_period == period {
                ems.peak = QuarterPeak::new(period, s.peak_kw);
            }
            let now = now_s as i64;
            ems.prices.merge(
                s.prices.into_iter().map(|(start_s, dur_s, eur_mwh)| PricePoint { start_s, dur_s, eur_mwh }).collect(),
                now,
            );
            ems.weather.merge(
                s.weather
                    .into_iter()
                    .map(|(end_s, gti_w_m2, temp_c)| WeatherPoint { end_s, gti_w_m2, temp_c })
                    .collect(),
                now,
            );
        }
        ems
    }

    /// Called by the control loop after every cycle, with what it measured.
    pub fn observe(&mut self, unix_s: f64, readings: &Readings, base_load_kw: Option<f64>, dimmed: bool) {
        let dt = self.latest.as_ref().map_or(0.0, |l| (unix_s - l.unix_s).clamp(0.0, 10.0));
        // A heat pump the plan does not schedule is load it cannot control.
        let unplanned_hp: f64 = if self.model.building.is_some() {
            0.0
        } else {
            readings.heat_pumps.iter().filter(|h| h.online).map(|h| h.power_kw).sum()
        };
        let uncontrolled = base_load_kw.map(|b| (b + unplanned_hp).max(0.0));
        if let Some(u) = uncontrolled {
            self.base.observe(unix_s, dt, u);
        }
        if let Some(g) = readings.grid_kw {
            let period = billing_period(&self.cfg, unix_s);
            self.peak.observe(unix_s, dt, g, period);
        }
        // Nowcast: how today's sky compares with the forecast.
        if let Some(f) = self.pv_forecast_raw(unix_s) {
            match readings.pv_available_kw.or(readings.pv_kw) {
                Some(seen) if f > 0.05 * self.site.pv_installed_kw => {
                    let r = (seen / f).clamp(0.1, 1.6);
                    let a = 1.0 - (-dt / 1200.0).exp();
                    self.pv_ratio = Some(self.pv_ratio.map_or(r, |old| old + a * (r - old)));
                }
                _ => self.pv_ratio = None, // sun too low to judge the sky
            }
        }
        let plugged: Vec<bool> = readings.chargers.iter().map(|c| c.online && c.car_waiting).collect();
        if plugged.len() == self.plugged.len() {
            if plugged.iter().zip(&self.plugged).any(|(now, before)| *now && !before) {
                self.wanted = Some("a car plugged in");
            } else if plugged != self.plugged {
                self.wanted.get_or_insert("a car left");
            }
        }
        self.plugged = plugged;
        if dimmed != self.dimmed {
            self.wanted = Some(if dimmed { "the DSO started a dimming" } else { "the dimming ended" });
            self.dimmed = dimmed;
        }
        self.latest = Some(Latest { unix_s, readings: readings.clone(), dimmed, uncontrolled_kw: uncontrolled });
    }

    /// What the plan asks of the real-time layer now, if a plan is in force.
    pub fn guidance(&self, unix_s: f64) -> Option<Guidance> {
        self.plan.as_ref()?.guidance_at(unix_s, PLAN_MAX_AGE_S, self.site.heat_pumps.len())
    }

    fn pv_forecast_raw(&self, t_s: f64) -> Option<f64> {
        let w = self.cfg.weather.as_ref()?;
        let end = ((t_s / STEP_S).floor() + 1.0) * STEP_S;
        let p = self.weather.at_end(end as i64)?;
        Some(weather::pv_kw(p.gti_w_m2, p.temp_c, self.site.pv_installed_kw, w.performance_ratio))
    }

    fn nowcast(&self, t_s: f64, now: f64) -> f64 {
        self.pv_ratio.map_or(1.0, |r| 1.0 + (r - 1.0) * (-(t_s - now).max(0.0) / NOWCAST_TAU_S).exp())
    }

    fn in_day(&self, t_s: f64) -> bool {
        let l = local_seconds_of_day(t_s);
        self.day.is_some_and(|(a, b)| (a..b).contains(&l))
    }

    fn in_dim_window(&self, t_s: f64) -> bool {
        let l = local_seconds_of_day(t_s);
        self.dim_windows.iter().any(|&(a, b)| (a..b).contains(&l))
    }

    /// Prices to fetch now, if any: from yesterday to two days ahead.
    pub fn price_job(&mut self, now: f64) -> Option<PriceJob> {
        let short = self.prices.until_s().is_none_or(|u| (u as f64) < now + 12.0 * 3600.0);
        if !self.prices_feed.due(now, self.cfg.prices.refresh_s as f64, short) {
            return None;
        }
        self.prices_feed.tried_s = Some(now);
        Some(PriceJob {
            sources: self.cfg.prices.sources.clone(),
            zone: self.zone.clone(),
            file: self.cfg.prices.file.clone(),
            from_s: now as i64 - 86_400,
            to_s: now as i64 + 2 * 86_400,
        })
    }

    pub fn store_prices(&mut self, now: f64, result: Result<Fetched, String>) {
        match result {
            Ok(Fetched { source, points, skipped }) => {
                if skipped != self.prices_skipped {
                    for e in &skipped {
                        info!("{e}; prices from {source} instead");
                    }
                    self.prices_skipped = skipped;
                }
                let before = self.prices.until_s();
                self.prices.merge(points, now as i64);
                if self.prices.until_s() != before {
                    info!(
                        "day-ahead prices for {} from {source}, known until {}",
                        self.zone,
                        self.prices.until_s().map_or("?".into(), |u| format!("+{:.1} h", (u as f64 - now) / 3600.0))
                    );
                    self.wanted.get_or_insert("new day-ahead prices");
                }
                self.prices_feed = Feed { source: Some(source), fetched_s: Some(now), tried_s: Some(now), error: None };
            }
            Err(e) => {
                if self.prices_feed.error.as_deref() != Some(e.as_str()) {
                    warn!("day-ahead prices: {e}");
                }
                self.prices_feed.error = Some(e);
            }
        }
    }

    /// Weather to fetch now, if any.
    pub fn weather_job(&mut self, now: f64) -> Option<(Plane, Option<PathBuf>)> {
        let w = self.cfg.weather.as_ref()?;
        let short = self.weather.until_s().is_none_or(|u| (u as f64) < now + 24.0 * 3600.0);
        if !self.weather_feed.due(now, w.refresh_s as f64, short) {
            return None;
        }
        self.weather_feed.tried_s = Some(now);
        let plane =
            Plane { latitude: w.latitude, longitude: w.longitude, tilt_deg: w.tilt_deg, azimuth_deg: w.azimuth_deg };
        Some((plane, w.file.clone()))
    }

    pub fn store_weather(&mut self, now: f64, result: Result<(String, Vec<WeatherPoint>), String>) {
        match result {
            Ok((source, points)) => {
                if self.weather_feed.fetched_s.is_none() || self.weather_feed.error.is_some() {
                    info!(
                        "weather forecast from {source}, until {}",
                        points.last().map_or("?".into(), |p| format!("+{:.1} h", (p.end_s as f64 - now) / 3600.0))
                    );
                }
                self.weather.merge(points, now as i64);
                self.weather_feed =
                    Feed { source: Some(source), fetched_s: Some(now), tried_s: Some(now), error: None };
            }
            Err(e) => {
                if self.weather_feed.error.as_deref() != Some(e.as_str()) {
                    warn!("weather forecast: {e}");
                }
                self.weather_feed.error = Some(e);
            }
        }
    }

    /// A new plan is due: something changed, or a new quarter-hour began.
    pub fn replan_due(&self, now: f64) -> bool {
        self.latest.is_some()
            && (self.wanted.is_some() || self.planned_s.is_none_or(|t| (now / STEP_S).floor() > (t / STEP_S).floor()))
    }

    /// The planning problem for the site as it is now. Cheap: the solve
    /// happens outside the lock.
    pub fn prepare(&mut self, now: f64) -> Result<Prepared, String> {
        self.planned_s = Some(now);
        let why = self.wanted.take().unwrap_or("new quarter-hour");
        let latest = self.latest.clone().ok_or("no readings yet")?;
        let start = (now / STEP_S).floor() * STEP_S;
        let h = self.horizon(start, now, &latest)?;
        let dim: Vec<bool> = (0..STEPS)
            .map(|k| {
                let t_mid = start + (k as f64 + 0.5) * STEP_S;
                (latest.dimmed && t_mid < now + DIM_EXPECTED_S) || self.in_dim_window(t_mid)
            })
            .collect();
        let demand = self.cfg.demand_charge.as_ref().map(|d| DemandCharge {
            eur_per_kw: if d.billing == "month" { d.eur_per_kw_year / 12.0 } else { d.eur_per_kw_year },
            peak_so_far_kw: self.peak.peak_kw.max(d.peak_floor_kw),
        });
        let (input, ev_of_charger) =
            build_input(&self.site, self.floor_kw, &self.model, &h, &latest.readings, dim, demand);
        Ok(Prepared { input, ev_of_charger, start_s: start, made_at_s: now, why })
    }

    fn horizon(&self, start: f64, now: f64, latest: &Latest) -> Result<Horizon, String> {
        let p = &self.cfg.prices;
        let installed = self.site.pv_installed_kw;
        let mut f = Forecast {
            pv_kw: Vec::with_capacity(STEPS),
            pv_sigma_kw: Vec::with_capacity(STEPS),
            pv_worst_kw: Vec::with_capacity(STEPS),
            base_kw: Vec::with_capacity(STEPS),
            base_sigma_kw: self.base.sigma_kw().unwrap_or(1.0).max(0.5),
            price_import_eur_kwh: Vec::with_capacity(STEPS),
            price_export_eur_kwh: Vec::with_capacity(STEPS),
        };
        let mut h = Horizon {
            dt_h: STEP_S / 3600.0,
            forecast: f.clone(),
            outdoor_c: Vec::with_capacity(STEPS),
            cop: Vec::with_capacity(STEPS),
            gains_kw: Vec::with_capacity(STEPS),
            t_min_c: Vec::with_capacity(STEPS),
            t_max_c: Vec::with_capacity(STEPS),
        };
        let outdoor_now = latest.readings.heat_pumps.first().and_then(|x| x.outdoor_c);
        let base_now = latest.uncontrolled_kw.unwrap_or(0.0);
        for k in 0..STEPS {
            let t_mid = start + (k as f64 + 0.5) * STEP_S;
            let t_end = start + (k as f64 + 1.0) * STEP_S;
            let (da, _) = self.prices.at_or_before(t_mid as i64).ok_or("no day-ahead prices yet")?;
            f.price_import_eur_kwh.push(da / 1000.0 + p.import_adder_eur_kwh);
            f.price_export_eur_kwh.push(p.export_fixed_eur_kwh.unwrap_or(da.max(0.0) / 1000.0));
            let w = self.weather.at_end(t_end as i64);
            let pv = match (&self.cfg.weather, w) {
                (Some(c), Some(w)) => (weather::pv_kw(w.gti_w_m2, w.temp_c, installed, c.performance_ratio)
                    * self.nowcast(t_mid, now))
                .min(installed),
                _ => 0.0,
            };
            f.pv_kw.push(pv);
            f.pv_sigma_kw.push(if pv > 0.0 { 0.15 * pv + 0.02 * installed } else { 0.0 });
            f.pv_worst_kw.push(0.35 * pv);
            f.base_kw.push(self.base.expected(t_mid).unwrap_or(base_now).max(0.0));
            let outdoor = w.map(|w| w.temp_c).or(outdoor_now).unwrap_or(10.0);
            h.outdoor_c.push(outdoor);
            match &self.cfg.building {
                Some(b) => {
                    h.cop.push((b.cop_at_0c + b.cop_per_k * outdoor).clamp(1.5, 6.0));
                    h.gains_kw.push(if self.in_day(t_mid) { b.gains_day_kw } else { b.gains_night_kw });
                    h.t_min_c.push(if self.in_day(t_end) { b.comfort_day_min_c } else { b.comfort_night_min_c });
                    h.t_max_c.push(b.comfort_max_c);
                }
                None => {
                    h.cop.push(3.0);
                    h.gains_kw.push(0.0);
                    h.t_min_c.push(f64::NEG_INFINITY);
                    h.t_max_c.push(f64::INFINITY);
                }
            }
        }
        h.forecast = f;
        Ok(h)
    }

    /// Takes the solver's answer.
    pub fn accept(&mut self, p: Prepared, result: Result<Plan, String>) {
        match result {
            Ok(plan) => {
                self.plans += 1;
                self.solve_ms = Some(plan.solve_ms);
                self.plan_error = None;
                let cars = p.ev_of_charger.iter().flatten().count();
                info!(
                    "plan ({}): {:.2} € of energy over 24 h; now grid {:+.1} kW, battery {:+.1} kW; peak {:.0} kW; {} car(s); {:.0} ms",
                    p.why,
                    plan.energy_cost_eur,
                    plan.grid_kw.first().copied().unwrap_or(0.0),
                    plan.battery_kw.first().copied().unwrap_or(0.0),
                    plan.peak_kw.unwrap_or_else(|| plan.grid_kw.iter().copied().fold(0.0, f64::max)),
                    cars,
                    plan.solve_ms,
                );
                self.plan = Some(PlanRecord {
                    made_at_s: p.made_at_s,
                    start_s: p.start_s,
                    step_s: STEP_S,
                    plan,
                    ev_of_charger: p.ev_of_charger,
                });
            }
            Err(e) => self.not_planned(e),
        }
    }

    pub fn not_planned(&mut self, e: String) {
        if self.plan_error.as_deref() != Some(e.as_str()) {
            warn!("no plan: {e}");
        }
        self.plan_error = Some(e);
    }

    /// The planner's state for the dashboard.
    pub fn view(&self, now: f64) -> PlannerView {
        let guidance = self.guidance(now);
        let status = match (&guidance, &self.plan_error, &self.plan) {
            (Some(_), _, _) => "following the plan".to_string(),
            (None, Some(e), _) => format!("rules only: {e}"),
            (None, None, Some(_)) => "rules only: the plan is too old".into(),
            (None, None, None) => "rules only: no plan yet".into(),
        };
        PlannerView {
            status,
            plans: self.plans,
            plan_made_ms: self.plan.as_ref().map(|p| (p.made_at_s * 1000.0) as i64),
            solve_ms: self.solve_ms,
            plan_energy_cost_eur: self.plan.as_ref().map(|p| p.plan.energy_cost_eur),
            bidding_zone: self.zone.clone(),
            prices: self.prices_feed.view(self.prices.until_s()),
            weather: self.cfg.weather.as_ref().map(|_| self.weather_feed.view(self.weather.until_s())),
            price_now_eur_mwh: self.prices.at(now as i64),
            pv_forecast_now_kw: self.pv_forecast_raw(now).map(|f| f * self.nowcast(now, now)),
            base_forecast_now_kw: self.base.expected(now).or(self.latest.as_ref().and_then(|l| l.uncontrolled_kw)),
            billing_peak_kw: self.peak.peak_kw,
            guidance: guidance.map(|g| GuidanceView {
                grid_kw: g.grid_kw,
                charger_kw: g.charger_kw,
                heat_pump_kw: g.heat_pump_kw,
                dim_expected: g.dim_expected,
            }),
        }
    }

    /// The plan in force, step by step, for the API.
    pub fn plan_json(&self) -> Value {
        let Some(r) = &self.plan else { return Value::Null };
        let p = &r.plan;
        let price: Vec<Option<f64>> = (0..p.grid_kw.len())
            .map(|k| self.prices.at_or_before((r.start_s + (k as f64 + 0.5) * r.step_s) as i64).map(|x| x.0))
            .collect();
        json!({
            "made_at_ms": (r.made_at_s * 1000.0) as i64,
            "start_ms": (r.start_s * 1000.0) as i64,
            "step_s": r.step_s,
            "energy_cost_eur": p.energy_cost_eur,
            "peak_kw": p.peak_kw,
            "price_eur_mwh": price,
            "grid_kw": p.grid_kw,
            "pv_kw": p.pv_kw,
            "battery_kw": p.battery_kw,
            "soc_kwh": p.soc_kwh,
            "heat_pump_kw": p.heat_pump_kw,
            "indoor_c": p.indoor_c,
            "ev_kw": p.ev_kw,
            "ev_of_charger": r.ev_of_charger,
            "dim": p.dim_budget_kw.iter().map(Option::is_some).collect::<Vec<_>>(),
        })
    }

    pub fn saved(&self) -> Saved {
        Saved {
            base_mean: self.base.means().to_vec(),
            base_miss_sq: self.base.miss_sq(),
            peak_period: self.peak.period,
            peak_kw: self.peak.peak_kw,
            prices: self.prices.points().iter().map(|p| (p.start_s, p.dur_s, p.eur_mwh)).collect(),
            weather: self.weather.points().iter().map(|w| (w.end_s, w.gti_w_m2, w.temp_c)).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FeedView {
    pub source: Option<String>,
    pub fetched_ms: Option<i64>,
    /// End of the last value known.
    pub until_ms: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuidanceView {
    pub grid_kw: Option<f64>,
    pub charger_kw: Vec<Option<f64>>,
    pub heat_pump_kw: Vec<Option<f64>>,
    pub dim_expected: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PlannerView {
    pub status: String,
    pub plans: u32,
    pub plan_made_ms: Option<i64>,
    pub solve_ms: Option<f64>,
    /// Energy cost of the plan in force over its 24 hours, €.
    pub plan_energy_cost_eur: Option<f64>,
    pub bidding_zone: String,
    pub prices: FeedView,
    pub weather: Option<FeedView>,
    pub price_now_eur_mwh: Option<f64>,
    pub pv_forecast_now_kw: Option<f64>,
    pub base_forecast_now_kw: Option<f64>,
    /// Highest quarter-hour of import in the billing period so far, kW.
    pub billing_peak_kw: f64,
    pub guidance: Option<GuidanceView>,
}

/// What the planner keeps across restarts: the learned base load, the
/// billing peak, and the last prices and forecast (a restart without
/// internet can still plan).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    base_mean: Vec<Option<f64>>,
    base_miss_sq: Option<f64>,
    peak_period: i64,
    peak_kw: f64,
    prices: Vec<(i64, i64, f64)>,
    weather: Vec<(i64, f64, f64)>,
}

pub fn load(path: &Path) -> Option<Saved> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn save(path: &Path, s: &Saved) {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(s).unwrap_or_default();
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("could not persist the planner's state: {e}");
    }
}

/// Runs the planner's fetching and solving in the background.
pub fn spawn(ems: Arc<Mutex<Ems>>, state_file: PathBuf) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut saved_at = f64::NEG_INFINITY;
        loop {
            tick.tick().await;
            let job = ems.lock().unwrap().price_job(unix_now());
            if let Some(job) = job {
                let r = tokio::task::spawn_blocking(move || market::fetch(&job))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                ems.lock().unwrap().store_prices(unix_now(), r);
            }
            let job = ems.lock().unwrap().weather_job(unix_now());
            if let Some((plane, file)) = job {
                let r = tokio::task::spawn_blocking(move || weather::fetch(&plane, file.as_deref()))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                ems.lock().unwrap().store_weather(unix_now(), r);
            }
            let now = unix_now();
            let prepared = {
                let mut e = ems.lock().unwrap();
                e.replan_due(now).then(|| e.prepare(now))
            };
            match prepared {
                Some(Ok(p)) => {
                    let input = p.input.clone();
                    let r = tokio::task::spawn_blocking(move || planner::plan(&input).map_err(|e| e.to_string()))
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()));
                    ems.lock().unwrap().accept(p, r);
                }
                Some(Err(e)) => ems.lock().unwrap().not_planned(e),
                None => {}
            }
            if now - saved_at >= 300.0 {
                let s = ems.lock().unwrap().saved();
                let path = state_file.clone();
                let _ = tokio::task::spawn_blocking(move || save(&path, &s)).await;
                saved_at = now;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use control::{ChargerReading, HeatPumpReading};

    fn cfg(extra: &str) -> PlannerCfg {
        let text = format!(
            "[prices]\nsources = [\"file\"]\nfile = \"x.csv\"\nimport_adder_eur_kwh = 0.12\n\
             [weather]\nlatitude = 49.2\nlongitude = 7.0\ntilt_deg = 15.0\nazimuth_deg = 0.0\n{extra}"
        );
        toml::from_str(&text).unwrap()
    }

    /// The depot, as `gateway.toml` sets it up.
    fn site() -> SiteConfig {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../gateway.toml")).unwrap();
        toml::from_str::<crate::config::Config>(&text).unwrap().site_config().unwrap()
    }

    // Monday 20 January 2025, 12:00 UTC.
    const T0: f64 = 1_737_374_400.0;

    fn readings(car: bool) -> Readings {
        Readings {
            grid_kw: Some(40.0),
            pv_kw: Some(30.0),
            pv_available_kw: Some(30.0),
            inverters: vec![],
            chargers: (0..4)
                .map(|i| ChargerReading {
                    online: true,
                    car_waiting: car && i == 0,
                    remaining_kwh: (car && i == 0).then_some(30.0),
                    departure_s: (car && i == 0).then_some(6.0 * 3600.0),
                    ..Default::default()
                })
                .collect(),
            heat_pumps: vec![HeatPumpReading {
                online: true,
                power_kw: 5.0,
                demand_kw: 5.0,
                indoor_c: Some(20.5),
                outdoor_c: Some(2.0),
            }],
            batteries: vec![control::BatteryReading { online: true, soc_pct: 50.0, power_kw: 0.0 }],
        }
    }

    fn with_prices(e: &mut Ems) {
        let start = (T0 as i64 / 86_400) * 86_400 - 86_400;
        let pts = (0..3 * 96)
            .map(|k| PricePoint {
                start_s: start + k * 900,
                dur_s: 900,
                // expensive from 16:00 to 19:00 UTC
                eur_mwh: if (64..76).contains(&(k % 96)) { 400.0 } else { 90.0 },
            })
            .collect();
        e.store_prices(T0, Ok(Fetched { source: "file".into(), points: pts, skipped: vec![] }));
    }

    #[test]
    fn no_plan_until_prices_arrive_then_a_plan_that_avoids_the_price_peak() {
        let mut e = Ems::new(cfg(""), site(), 18.2, Jurisdiction::De, None, T0);
        e.observe(T0, &readings(true), Some(25.0), false);
        assert!(e.replan_due(T0));
        assert_eq!(e.prepare(T0).err().unwrap(), "no day-ahead prices yet");
        assert!(e.guidance(T0).is_none());
        assert!(e.view(T0).status.starts_with("rules only"));

        with_prices(&mut e);
        assert!(e.replan_due(T0 + 1.0), "new prices call for a plan");
        let p = e.prepare(T0 + 1.0).unwrap();
        assert_eq!(p.input.forecast.price_import_eur_kwh.len(), STEPS);
        assert_eq!(p.input.evs.len(), 1);
        assert!(p.input.heat_pump.is_none(), "no building configured");
        let plan = planner::plan(&p.input).unwrap();
        e.accept(p, Ok(plan));
        let g = e.guidance(T0 + 60.0).unwrap();
        assert_eq!(g.charger_kw.len(), 4);
        assert!(g.charger_kw[0].is_some() && g.charger_kw[1].is_none());
        assert_eq!(e.view(T0 + 60.0).status, "following the plan");
        // the plan charges the car outside the 16:00–19:00 UTC peak
        let json = e.plan_json();
        let ev = json["ev_kw"][0].as_array().unwrap();
        let start = json["start_ms"].as_i64().unwrap() as f64 / 1000.0;
        let in_peak: f64 = ev
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let t = start + *k as f64 * STEP_S;
                (16.0..19.0).contains(&(t.rem_euclid(86_400.0) / 3600.0))
            })
            .map(|(_, v)| v.as_f64().unwrap())
            .sum();
        assert!(in_peak < 1e-3, "{in_peak}");
        // the plan expires
        assert!(e.guidance(T0 + PLAN_MAX_AGE_S + 2.0).is_none());
    }

    #[test]
    fn events_and_new_quarter_hours_call_for_plans() {
        let mut e = Ems::new(cfg(""), site(), 18.2, Jurisdiction::De, None, T0);
        with_prices(&mut e);
        e.observe(T0, &readings(false), Some(25.0), false);
        let _ = e.prepare(T0);
        assert!(!e.replan_due(T0 + 10.0));
        e.observe(T0 + 11.0, &readings(true), Some(25.0), false);
        assert!(e.replan_due(T0 + 11.0), "a car plugged in");
        let _ = e.prepare(T0 + 11.0);
        e.observe(T0 + 12.0, &readings(true), Some(25.0), true);
        assert!(e.replan_due(T0 + 12.0), "the DSO dims");
        let p = e.prepare(T0 + 12.0).unwrap();
        assert!(p.input.dim[0] && p.input.dim[7] && !p.input.dim[8], "a dimming in progress is expected for 2 h");
        assert!(!e.replan_due(T0 + 13.0));
        assert!(e.replan_due(T0 + STEP_S), "the next quarter-hour");
    }

    #[test]
    fn weather_forecast_and_nowcast_shape_the_pv_the_plan_counts_on() {
        let mut e = Ems::new(cfg(""), site(), 18.2, Jurisdiction::De, None, T0);
        with_prices(&mut e);
        let q = (T0 / STEP_S).floor() as i64 * 900;
        let pts = (0..200).map(|k| WeatherPoint { end_s: q + k * 900, gti_w_m2: 500.0, temp_c: 5.0 }).collect();
        e.store_weather(T0, Ok(("file".into(), pts)));
        let clear = weather::pv_kw(500.0, 5.0, 120.0, 0.85);
        // the inverters see half of what the forecast expected
        let mut r = readings(false);
        r.pv_available_kw = Some(0.5 * clear);
        for s in 0..600 {
            e.observe(T0 + s as f64, &r, Some(25.0), false);
        }
        let p = e.prepare(T0 + 600.0).unwrap();
        assert!((p.input.forecast.pv_kw[0] / clear - 0.5).abs() < 0.1, "now: {}", p.input.forecast.pv_kw[0]);
        assert!(p.input.forecast.pv_kw[40] / clear > 0.9, "in 10 h the forecast counts again");
        assert!(p.input.forecast.pv_sigma_kw[0] > 0.0);
    }

    #[test]
    fn heat_pump_is_planned_with_a_building_and_learned_base_load_excludes_it() {
        let building = "[building]\nua_kw_per_k = 1.2\ncap_kwh_per_k = 18.0\ngains_day_kw = 3.0\ngains_night_kw = 1.0\n\
                        comfort_day_min_c = 20.0\ncomfort_night_min_c = 17.0\ncomfort_max_c = 23.0\n\
                        day_start = \"06:00\"\nday_end = \"22:00\"\ncop_at_0c = 3.0\ncop_per_k = 0.08\n";
        let mut e = Ems::new(cfg(building), site(), 18.2, Jurisdiction::De, None, T0);
        with_prices(&mut e);
        e.observe(T0, &readings(false), Some(25.0), false);
        let p = e.prepare(T0).unwrap();
        let hp = p.input.heat_pump.as_ref().unwrap();
        assert_eq!(hp.indoor_c, 20.5);
        // 12:00 UTC is 13:00 in Germany: daytime comfort
        assert_eq!(hp.t_min_c[0], 20.0);
        // 21:00 UTC is 22:00 local: night set-back from there
        assert_eq!(hp.t_min_c[35], 17.0);
        assert_eq!(p.input.forecast.base_kw[0], 25.0, "the heat pump's 5 kW is not in the base load");

        let mut plain = Ems::new(cfg(""), site(), 18.2, Jurisdiction::De, None, T0);
        with_prices(&mut plain);
        plain.observe(T0, &readings(false), Some(25.0), false);
        assert_eq!(plain.prepare(T0).unwrap().input.forecast.base_kw[0], 30.0, "unplanned heat pump counts as load");
    }

    #[test]
    fn billing_peak_and_demand_charge_reach_the_planner_and_survive_a_restart() {
        let dc = "[demand_charge]\neur_per_kw_year = 100.0\npeak_floor_kw = 20.0\n";
        let mut e = Ems::new(cfg(dc), site(), 18.2, Jurisdiction::De, None, T0);
        with_prices(&mut e);
        let mut r = readings(false);
        r.grid_kw = Some(60.0);
        for s in (0..=1800).step_by(5) {
            e.observe(T0 + s as f64, &r, Some(25.0), false);
        }
        assert!((e.view(T0 + 1800.0).billing_peak_kw - 60.0).abs() < 1e-6);
        let d = e.prepare(T0 + 1800.0).unwrap().input.demand_charge.unwrap();
        assert_eq!(d.eur_per_kw, 100.0);
        assert!((d.peak_so_far_kw - 60.0).abs() < 1e-6);

        let saved = e.saved();
        let back = Ems::new(cfg(dc), site(), 18.2, Jurisdiction::De, Some(saved.clone()), T0 + 3600.0);
        assert!((back.view(T0).billing_peak_kw - 60.0).abs() < 1e-6);
        assert_eq!(back.prices.points().len(), e.prices.points().len());
        // next year the peak starts over
        let next_year = Ems::new(cfg(dc), site(), 18.2, Jurisdiction::De, Some(saved), T0 + 366.0 * 86_400.0);
        assert_eq!(next_year.view(T0).billing_peak_kw, 0.0);
    }
}
