//! Control logic of the edge gateway: consumption dimming (§14a EnWG in DE,
//! flexibility contracts in AT/CH), DSO feed-in limits with country rules
//! (DE 60% cap, AT 70% Spitzenkappung, CH 3% annual budget), batteries, the
//! gradual release, inverters that ignore their limit, and a report of every
//! reduction. No I/O — the same code runs in the gateway, in tests and in the
//! browser.

pub mod accounting;
pub mod compliance;
pub mod controller;
pub mod policy;
pub mod rules;

pub use accounting::{Clock, Totals};
pub use compliance::{DimmingReport, Recorder, Verdict};
pub use controller::{
    BatteryReading, BatterySpec, ChargerReading, ChargerSpec, Controller, DsoCommands, Fallback, FeedInReference,
    Guidance, HeatPumpReading, HeatPumpSpec, InverterReading, Mode, Readings, Refusal, Setpoints, SiteConfig, Status,
};
pub use policy::{ConsumptionRule, Jurisdiction, Policy};
