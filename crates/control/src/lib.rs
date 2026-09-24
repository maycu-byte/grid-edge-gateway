//! Control logic of the edge gateway: §14a EnWG dimming via an energy
//! management system, the DSO feed-in limit, and the gradual release.
//! No I/O — the same code runs in the gateway, in tests and in the browser.

pub mod controller;
pub mod rules;

pub use controller::{
    ChargerReading, ChargerSpec, Controller, DsoCommands, Fallback, FeedInReference, HeatPumpReading, HeatPumpSpec,
    Mode, Readings, SiteConfig, Setpoints, Status,
};
