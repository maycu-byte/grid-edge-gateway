//! Gateway configuration (TOML). See `gateway.toml` in the repository root.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub site: Site,
    #[serde(rename = "inverter")]
    pub inverters: Vec<Device>,
    pub meter: Device,
    #[serde(rename = "charger", default)]
    pub chargers: Vec<Charger>,
    #[serde(rename = "heat_pump", default)]
    pub heat_pumps: Vec<HeatPump>,
    pub iec104: Iec104,
    pub api: Api,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub pv_installed_kw: f64,
    pub feed_in_reference: FeedInReference,
    pub release_ramp_s: f64,
    pub margin_kw: f64,
    pub min_dwell_s: f64,
    pub control_period_ms: u64,
    /// Readings older than this count as "device offline".
    pub stale_after_ms: u64,
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatPump {
    pub address: SocketAddr,
    #[serde(default = "default_unit")]
    pub unit: u8,
    pub rated_kw: f64,
    pub min_kw: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Iec104 {
    pub bind: SocketAddr,
    pub common_address: u16,
    /// Measurement change that triggers a spontaneous report, kW.
    pub deadband_kw: f64,
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

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if cfg.inverters.is_empty() {
            return Err("at least one [[inverter]] is required".into());
        }
        Ok(cfg)
    }

    pub fn site_config(&self) -> control::SiteConfig {
        control::SiteConfig {
            pv_installed_kw: self.site.pv_installed_kw,
            chargers: self
                .chargers
                .iter()
                .map(|c| control::ChargerSpec {
                    max_current_a: c.max_current_a,
                    failsafe_current_a: c.failsafe_current_a,
                })
                .collect(),
            heat_pumps: self
                .heat_pumps
                .iter()
                .map(|h| control::HeatPumpSpec { rated_kw: h.rated_kw, min_kw: h.min_kw })
                .collect(),
            feed_in_reference: match self.site.feed_in_reference {
                FeedInReference::PlantOutput => control::FeedInReference::PlantOutput,
                FeedInReference::GridConnectionPoint => control::FeedInReference::GridConnectionPoint,
            },
            release_ramp_s: self.site.release_ramp_s,
            margin_kw: self.site.margin_kw,
            min_dwell_s: self.site.min_dwell_s,
        }
    }
}
