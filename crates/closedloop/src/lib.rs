//! The closed loop around the gateway's logic: the simulated depot, read and
//! written through its register maps, the real-time controller, and the MPC
//! planner with its forecaster. Used by the browser demo and by the study
//! that compares rule-based control with MPC variants.

pub mod adapter;
pub mod feeder;
pub mod forecast;
pub mod metrics;
pub mod runner;

pub use feeder::{FeederCase, SiteRun, run_site};
pub use metrics::Metrics;
pub use planning::PlanRecord;
pub use runner::{ClosedLoop, Scenario, Strategy, site_config};
