//! The closed loop around the gateway's logic: the simulated depot, read and
//! written through its register maps, the real-time controller, and the MPC
//! planner with its forecaster. Used by the browser demo and by the study
//! that compares rule-based control with MPC variants.

pub mod adapter;
pub mod forecast;
pub mod metrics;
pub mod runner;

pub use metrics::Metrics;
pub use runner::{ClosedLoop, PlanRecord, Scenario, Strategy, site_config};
