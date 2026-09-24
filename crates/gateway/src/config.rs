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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(extra_site: &str, extra: &str) -> Result<Config, String> {
        let base = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../gateway.toml")).unwrap();
        // Line endings may be CRLF on Windows checkouts.
        let text = base.replacen("[site]", &format!("[site]\n{extra_site}"), 1) + extra;
        let dir = std::env::temp_dir().join(format!("gw-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{}.toml", text.len()));
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
    }

    #[test]
    fn mistakes_are_reported() {
        assert!(load("jurisdiction = \"FR\"", "").unwrap_err().contains("unknown jurisdiction"));
        assert!(load("", "\n[policy]\ncontract_min_kw = 8.0\n").unwrap_err().contains("do not apply to DE"));
        assert!(load("", "\n[policy]\nstatic_feed_in_cap_pct = 120.0\n").is_err());
    }
}
