//! Gateway configuration (TOML). See `gateway.toml` and `examples/` in the
//! repository root.

use std::net::SocketAddr;
use std::path::PathBuf;

use control::{ConsumptionRule, Jurisdiction, Policy};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub site: Site,
    #[serde(default)]
    pub policy: PolicyOverrides,
    #[serde(rename = "inverter")]
    pub inverters: Vec<Device>,
    pub meter: Device,
    #[serde(rename = "charger", default)]
    pub chargers: Vec<Charger>,
    #[serde(rename = "heat_pump", default)]
    pub heat_pumps: Vec<HeatPump>,
    #[serde(rename = "battery", default)]
    pub batteries: Vec<Battery>,
    pub iec104: Iec104,
    pub api: Api,
    /// The planning layer (MPC); without it the site runs on rules alone.
    pub planner: Option<Planner>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// "DE", "AT" or "CH": which rules the DSO's commands are applied under.
    #[serde(default = "default_jurisdiction")]
    pub jurisdiction: String,
    pub pv_installed_kw: f64,
    /// Rating of the grid connection, kW (plausibility checks).
    pub connection_kw: f64,
    /// Expected PV yield per year, kWh (curtailment budgets). Default:
    /// 1,000 kWh per installed kW.
    pub expected_annual_yield_kwh: Option<f64>,
    pub feed_in_reference: FeedInReference,
    pub release_ramp_s: f64,
    /// Default 0.17 %/s ≈ 10% of installed power per minute.
    #[serde(default = "default_pv_ramp")]
    pub pv_ramp_pct_per_s: f64,
    pub margin_kw: f64,
    pub min_dwell_s: f64,
    /// Cars with less slack than this before departure charge at full power.
    #[serde(default = "default_deadline_guard")]
    pub deadline_guard_s: f64,
    /// While dimmed, count on the lowest PV surplus of this many seconds.
    #[serde(default = "default_surplus_hold")]
    pub surplus_hold_s: f64,
    #[serde(default)]
    pub import_target_kw: f64,
    /// Before handing power back after a dimming, wait a random time up to
    /// this long, so sites released together do not ramp up together. 0 = off.
    #[serde(default)]
    pub release_delay_max_s: f64,
    /// Seed for that wait; by default derived from `site_id`.
    pub release_delay_seed: Option<u64>,
    /// An inverter above its limit this long is reported (point 2008).
    #[serde(default = "default_pv_follow_timeout")]
    pub pv_follow_timeout_s: f64,
    /// Settling time at the start of a dimming, not counted in its report.
    #[serde(default = "default_compliance_grace")]
    pub compliance_grace_s: f64,
    /// The site's name in compliance reports (e.g. its market location ID).
    #[serde(default = "default_site_id")]
    pub site_id: String,
    /// Where compliance reports go; default `reports/` next to this file.
    pub reports_dir: Option<PathBuf>,
    pub control_period_ms: u64,
    /// Readings older than this count as "device offline".
    pub stale_after_ms: u64,
}

/// Site-specific deviations from the country defaults in `control::policy`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyOverrides {
    /// AT: the cap the DSO set for this connection (up to 70); DE: 60 for a
    /// new system without smart meter (Solarspitzengesetz).
    pub static_feed_in_cap_pct: Option<f64>,
    pub curtailment_budget_pct: Option<f64>,
    pub enforce_curtailment_budget: Option<bool>,
    /// AT/CH flexibility contract.
    pub contract_min_kw: Option<f64>,
    pub contract_max_minutes_per_day: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedInReference {
    PlantOutput,
    GridConnectionPoint,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub address: SocketAddr,
    #[serde(default = "default_unit")]
    pub unit: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charger {
    pub address: SocketAddr,
    #[serde(default = "default_unit")]
    pub unit: u8,
    pub max_current_a: f64,
    pub failsafe_current_a: f64,
    pub failsafe_timeout_s: u16,
    /// The owner forbade the DSO to use this device (CH existing flexibility).
    #[serde(default)]
    pub opted_out: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatPump {
    pub address: SocketAddr,
    #[serde(default = "default_unit")]
    pub unit: u8,
    pub rated_kw: f64,
    pub min_kw: f64,
    #[serde(default)]
    pub opted_out: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Battery {
    pub address: SocketAddr,
    #[serde(default = "default_unit")]
    pub unit: u8,
    pub capacity_kwh: f64,
    pub max_charge_kw: f64,
    pub max_discharge_kw: f64,
    pub min_soc_pct: f64,
    pub max_soc_pct: f64,
    /// The battery goes idle on its own if the gateway is silent this long.
    pub watchdog_s: u16,
}

/// What happens to DSO commands when no control centre is connected.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LinkLossPolicy {
    /// Keep the last commands until the DSO changes them (typical in DE).
    Hold,
    /// Lift the commands (except an emergency) after `link_loss_release_s`.
    Release,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Iec104 {
    pub bind: SocketAddr,
    pub common_address: u16,
    /// Measurement change that triggers a spontaneous report, kW.
    pub deadband_kw: f64,
    #[serde(default = "default_link_loss")]
    pub on_link_loss: LinkLossPolicy,
    #[serde(default = "default_link_loss_s")]
    pub link_loss_release_s: u64,
    pub tls: Option<Tls>,
}

/// IEC 62351-3 style TLS: the station presents its certificate and only
/// accepts control centres whose client certificate is signed by `client_ca`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub client_ca: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Api {
    pub bind: SocketAddr,
    pub web_root: Option<PathBuf>,
}

/// The planning layer: where prices and weather come from, and what the
/// planner should weigh.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Planner {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// "deterministic", "chance" (with `epsilon`) or "robust".
    #[serde(default = "default_uncertainty")]
    pub uncertainty: String,
    #[serde(default = "default_epsilon")]
    pub epsilon: f64,
    /// Dimming windows the DSO has announced as recurring, local time,
    /// e.g. `["17:30-19:30"]`. A dimming in progress is always expected to
    /// last up to 2 hours.
    #[serde(default)]
    pub dim_windows: Vec<String>,
    /// Cycle ageing of the battery, € per kWh in or out.
    #[serde(default = "default_degradation")]
    pub battery_degradation_eur_per_kwh: f64,
    pub prices: Prices,
    pub weather: Option<Weather>,
    pub demand_charge: Option<DemandCharge>,
    /// Plan the (first) heat pump with a thermal model of the building.
    pub building: Option<Building>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    /// Tried in order: "entsoe" (needs the ENTSOE_TOKEN environment
    /// variable), "energy-charts", "file".
    pub sources: Vec<String>,
    /// "DE-LU", "AT" or "CH"; by default the jurisdiction's zone.
    pub bidding_zone: Option<String>,
    /// Grid fees, levies and taxes on top of the day-ahead price, €/kWh.
    pub import_adder_eur_kwh: f64,
    /// A fixed feed-in payment, €/kWh. Without it, exports earn the
    /// day-ahead price (and nothing when it is negative).
    pub export_fixed_eur_kwh: Option<f64>,
    /// CSV of `start,eur_mwh` for the "file" source.
    pub file: Option<PathBuf>,
    #[serde(default = "default_refresh")]
    pub refresh_s: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weather {
    pub latitude: f64,
    pub longitude: f64,
    pub tilt_deg: f64,
    /// 0 = south, −90 = east, 90 = west.
    pub azimuth_deg: f64,
    #[serde(default = "default_performance_ratio")]
    pub performance_ratio: f64,
    /// Open-Meteo JSON to read instead of the API.
    pub file: Option<PathBuf>,
    #[serde(default = "default_refresh")]
    pub refresh_s: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemandCharge {
    /// Leistungspreis, € per kW of the billed peak and year.
    pub eur_per_kw_year: f64,
    /// "year" or "month": the period whose highest quarter-hour is billed.
    #[serde(default = "default_billing")]
    pub billing: String,
    /// A peak the site reaches this period anyway: below it, new peaks cost
    /// nothing (e.g. last year's peak, early in the year).
    #[serde(default)]
    pub peak_floor_kw: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Building {
    pub ua_kw_per_k: f64,
    pub cap_kwh_per_k: f64,
    pub gains_day_kw: f64,
    pub gains_night_kw: f64,
    pub comfort_day_min_c: f64,
    pub comfort_night_min_c: f64,
    pub comfort_max_c: f64,
    /// Local time the day's comfort band starts and ends, "HH:MM".
    pub day_start: String,
    pub day_end: String,
    /// Heat pump efficiency: COP = cop_at_0c + cop_per_k × outdoor °C
    /// (within 1.5–6).
    pub cop_at_0c: f64,
    pub cop_per_k: f64,
}

/// "HH:MM" → seconds after midnight.
pub fn parse_hhmm(s: &str) -> Option<f64> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h <= 24 && m < 60 && h * 60 + m <= 1440).then(|| f64::from(h * 3600 + m * 60))
}

/// "17:30-19:30" → local seconds after midnight.
pub fn parse_window(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once('-')?;
    let (a, b) = (parse_hhmm(a)?, parse_hhmm(b)?);
    (a < b).then_some((a, b))
}

impl Planner {
    pub fn validate(&self, jurisdiction: Jurisdiction) -> Result<(), String> {
        let bad = |s: String| Err(format!("[planner] {s}"));
        if !matches!(self.uncertainty.as_str(), "deterministic" | "chance" | "robust") {
            return bad(format!("uncertainty must be deterministic, chance or robust, not {:?}", self.uncertainty));
        }
        if !(self.epsilon > 0.0 && self.epsilon < 0.5) {
            return bad("epsilon must be within (0, 0.5)".into());
        }
        for w in &self.dim_windows {
            if parse_window(w).is_none() {
                return bad(format!("dim window {w:?} is not like \"17:30-19:30\""));
            }
        }
        let p = &self.prices;
        if p.sources.is_empty() {
            return bad("prices.sources is empty".into());
        }
        for s in &p.sources {
            match s.as_str() {
                "entsoe" | "energy-charts" => {}
                "file" if p.file.is_some() => {}
                "file" => return bad("prices source \"file\" needs prices.file".into()),
                other => return bad(format!("unknown price source {other:?} (entsoe, energy-charts, file)")),
            }
        }
        let zone = self.bidding_zone(jurisdiction);
        if crate::market::eic(&zone).is_none() {
            return bad(format!("unknown bidding zone {zone:?} (DE-LU, AT, CH)"));
        }
        if let Some(w) = &self.weather {
            if !(-90.0..=90.0).contains(&w.latitude) || !(-180.0..=180.0).contains(&w.longitude) {
                return bad("weather latitude/longitude out of range".into());
            }
            if !(0.0..=90.0).contains(&w.tilt_deg) || !(-180.0..=180.0).contains(&w.azimuth_deg) {
                return bad("weather tilt must be 0–90° and azimuth −180–180°".into());
            }
            if !(w.performance_ratio > 0.3 && w.performance_ratio <= 1.0) {
                return bad("weather performance_ratio must be within (0.3, 1]".into());
            }
        }
        if let Some(d) = &self.demand_charge {
            if d.eur_per_kw_year.is_nan() || d.eur_per_kw_year < 0.0 || d.peak_floor_kw < 0.0 {
                return bad("demand_charge values must be ≥ 0".into());
            }
            if !matches!(d.billing.as_str(), "year" | "month") {
                return bad("demand_charge.billing must be \"year\" or \"month\"".into());
            }
        }
        if let Some(b) = &self.building {
            if b.ua_kw_per_k <= 0.0 || b.cap_kwh_per_k <= 4.0 * b.ua_kw_per_k * 0.25 {
                return bad("building needs ua_kw_per_k > 0 and a capacity well above ua × 15 min".into());
            }
            if !(b.comfort_night_min_c <= b.comfort_day_min_c && b.comfort_day_min_c < b.comfort_max_c) {
                return bad("building comfort needs night min ≤ day min < max".into());
            }
            let (Some(a), Some(z)) = (parse_hhmm(&b.day_start), parse_hhmm(&b.day_end)) else {
                return bad("building day_start/day_end must be \"HH:MM\"".into());
            };
            if a >= z {
                return bad("building day_start must be before day_end".into());
            }
        }
        Ok(())
    }

    /// The day-ahead zone: the configured one, else the jurisdiction's.
    pub fn bidding_zone(&self, jurisdiction: Jurisdiction) -> String {
        self.prices.bidding_zone.clone().unwrap_or_else(|| {
            match jurisdiction {
                Jurisdiction::De => "DE-LU",
                Jurisdiction::At => "AT",
                Jurisdiction::Ch => "CH",
            }
            .into()
        })
    }
}

fn default_true() -> bool {
    true
}
fn default_uncertainty() -> String {
    "deterministic".into()
}
fn default_epsilon() -> f64 {
    0.05
}
fn default_degradation() -> f64 {
    0.03
}
fn default_refresh() -> u64 {
    3600
}
fn default_performance_ratio() -> f64 {
    0.85
}
fn default_billing() -> String {
    "year".into()
}

fn default_unit() -> u8 {
    1
}
fn default_jurisdiction() -> String {
    "DE".into()
}
fn default_pv_ramp() -> f64 {
    10.0 / 60.0
}
fn default_deadline_guard() -> f64 {
    900.0
}
fn default_surplus_hold() -> f64 {
    30.0
}
fn default_link_loss() -> LinkLossPolicy {
    LinkLossPolicy::Hold
}
fn default_link_loss_s() -> u64 {
    900
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if cfg.inverters.is_empty() {
            return Err("at least one [[inverter]] is required".into());
        }
        if cfg.site.control_period_ms < 100 || cfg.site.stale_after_ms < 2 * cfg.site.control_period_ms {
            return Err("control_period_ms must be ≥ 100 and stale_after_ms at least twice that".into());
        }
        cfg.site_config()?.validate()?;
        if let Some(p) = &cfg.planner {
            p.validate(cfg.jurisdiction()?)?;
        }
        Ok(cfg)
    }

    pub fn jurisdiction(&self) -> Result<Jurisdiction, String> {
        Jurisdiction::parse(&self.site.jurisdiction)
            .ok_or_else(|| format!("unknown jurisdiction {:?} (use DE, AT or CH)", self.site.jurisdiction))
    }

    pub fn policy(&self) -> Result<Policy, String> {
        let mut p = Policy::for_country(self.jurisdiction()?);
        let o = &self.policy;
        if o.static_feed_in_cap_pct.is_some() {
            p.static_feed_in_cap_pct = o.static_feed_in_cap_pct;
        }
        if o.curtailment_budget_pct.is_some() {
            p.curtailment_budget_pct = o.curtailment_budget_pct;
        }
        if let Some(e) = o.enforce_curtailment_budget {
            p.enforce_curtailment_budget = e;
        }
        if let ConsumptionRule::Contract { min_kw, max_minutes_per_day } = p.consumption {
            p.consumption = ConsumptionRule::Contract {
                min_kw: o.contract_min_kw.unwrap_or(min_kw),
                max_minutes_per_day: o.contract_max_minutes_per_day.unwrap_or(max_minutes_per_day),
            };
        } else if o.contract_min_kw.is_some() || o.contract_max_minutes_per_day.is_some() {
            return Err("contract_* settings do not apply to DE (§14a)".into());
        }
        Ok(p)
    }

    pub fn site_config(&self) -> Result<control::SiteConfig, String> {
        Ok(control::SiteConfig {
            policy: self.policy()?,
            pv_installed_kw: self.site.pv_installed_kw,
            connection_kw: self.site.connection_kw,
            expected_annual_yield_kwh: self
                .site
                .expected_annual_yield_kwh
                .unwrap_or(self.site.pv_installed_kw * 1000.0),
            chargers: self
                .chargers
                .iter()
                .map(|c| control::ChargerSpec {
                    max_current_a: c.max_current_a,
                    failsafe_current_a: c.failsafe_current_a,
                    opted_out: c.opted_out,
                })
                .collect(),
            heat_pumps: self
                .heat_pumps
                .iter()
                .map(|h| control::HeatPumpSpec { rated_kw: h.rated_kw, min_kw: h.min_kw, opted_out: h.opted_out })
                .collect(),
            batteries: self
                .batteries
                .iter()
                .map(|b| control::BatterySpec {
                    capacity_kwh: b.capacity_kwh,
                    max_charge_kw: b.max_charge_kw,
                    max_discharge_kw: b.max_discharge_kw,
                    min_soc_pct: b.min_soc_pct,
                    max_soc_pct: b.max_soc_pct,
                })
                .collect(),
            feed_in_reference: match self.site.feed_in_reference {
                FeedInReference::PlantOutput => control::FeedInReference::PlantOutput,
                FeedInReference::GridConnectionPoint => control::FeedInReference::GridConnectionPoint,
            },
            release_ramp_s: self.site.release_ramp_s,
            pv_ramp_pct_per_s: self.site.pv_ramp_pct_per_s,
            margin_kw: self.site.margin_kw,
            min_dwell_s: self.site.min_dwell_s,
            deadline_guard_s: self.site.deadline_guard_s,
            surplus_hold_s: self.site.surplus_hold_s,
            import_target_kw: self.site.import_target_kw,
            release_delay_max_s: self.site.release_delay_max_s,
            release_delay_seed: self.site.release_delay_seed.unwrap_or_else(|| fnv1a(&self.site.site_id)),
            pv_follow_timeout_s: self.site.pv_follow_timeout_s,
            compliance_grace_s: self.site.compliance_grace_s,
            site_id: self.site.site_id.clone(),
        })
    }
}

fn default_pv_follow_timeout() -> f64 {
    30.0
}

fn default_compliance_grace() -> f64 {
    60.0
}

fn default_site_id() -> String {
    "site".into()
}

/// A stable seed from a name, so every site draws its own release wait.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(extra_site: &str, extra: &str) -> Result<Config, String> {
        let base = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../gateway.toml")).unwrap();
        // Line endings may be CRLF on Windows checkouts.
        let text = base.replacen("[site]", &format!("[site]\n{extra_site}"), 1) + extra;
        // One file per call: tests run in parallel.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("gw-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{n}.toml"));
        std::fs::write(&p, text).unwrap();
        Config::load(&p)
    }

    #[test]
    fn repository_config_is_valid() {
        let c = load("", "").unwrap();
        assert_eq!(c.jurisdiction().unwrap(), Jurisdiction::De);
    }

    #[test]
    fn country_and_overrides_are_applied() {
        let c = load(
            "jurisdiction = \"AT\"",
            "\n[policy]\nstatic_feed_in_cap_pct = 60.0\ncontract_min_kw = 8.0\ncontract_max_minutes_per_day = 120.0\n",
        )
        .unwrap();
        let p = c.policy().unwrap();
        assert_eq!(p.static_feed_in_cap_pct, Some(60.0));
        assert_eq!(p.consumption, ConsumptionRule::Contract { min_kw: 8.0, max_minutes_per_day: 120.0 });
    }

    #[test]
    fn example_configs_are_valid() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
        let at = Config::load(std::path::Path::new(&format!("{dir}/gateway-at.toml"))).unwrap();
        assert_eq!(at.policy().unwrap().static_feed_in_cap_pct, Some(70.0));
        let ch = Config::load(std::path::Path::new(&format!("{dir}/gateway-ch.toml"))).unwrap();
        assert_eq!(ch.policy().unwrap().curtailment_budget_pct, Some(3.0));
        assert!(ch.heat_pumps[0].opted_out);
        let mpc = Config::load(std::path::Path::new(&format!("{dir}/gateway-de-planner.toml"))).unwrap();
        let p = mpc.planner.unwrap();
        assert_eq!(p.prices.sources, ["entsoe", "energy-charts"]);
        assert!(p.building.is_some() && p.weather.is_some());
    }

    const PLANNER: &str = "\n[planner]\ndim_windows = [\"17:30-19:30\"]\n\
        [planner.prices]\nsources = [\"entsoe\", \"energy-charts\"]\nimport_adder_eur_kwh = 0.12\n\
        [planner.weather]\nlatitude = 49.23\nlongitude = 7.0\ntilt_deg = 15.0\nazimuth_deg = 0.0\n\
        [planner.demand_charge]\neur_per_kw_year = 100.0\n";

    #[test]
    fn planner_section_is_read_and_checked() {
        let c = load("", PLANNER).unwrap();
        let p = c.planner.as_ref().unwrap();
        assert!(p.enabled);
        assert_eq!(p.bidding_zone(Jurisdiction::De), "DE-LU");
        assert_eq!(p.demand_charge.as_ref().unwrap().billing, "year");
        assert_eq!(parse_window(&p.dim_windows[0]), Some((17.5 * 3600.0, 19.5 * 3600.0)));
        let ch = load("jurisdiction = \"CH\"", PLANNER).unwrap();
        assert_eq!(ch.planner.unwrap().bidding_zone(Jurisdiction::Ch), "CH", "hourly Swiss day-ahead zone");
        assert!(
            load("", &PLANNER.replace("\"energy-charts\"", "\"nordpool\""))
                .unwrap_err()
                .contains("unknown price source")
        );
        assert!(
            load("", &PLANNER.replace("[\"17:30-19:30\"]", "[\"19:30-17:30\"]")).unwrap_err().contains("dim window")
        );
        assert!(load("", &(PLANNER.to_owned() + "billing = \"week\"\n")).unwrap_err().contains("billing"));
    }

    #[test]
    fn mistakes_are_reported() {
        assert!(load("jurisdiction = \"FR\"", "").unwrap_err().contains("unknown jurisdiction"));
        assert!(load("", "\n[policy]\ncontract_min_kw = 8.0\n").unwrap_err().contains("do not apply to DE"));
        assert!(load("", "\n[policy]\nstatic_feed_in_cap_pct = 120.0\n").is_err());
    }
}
