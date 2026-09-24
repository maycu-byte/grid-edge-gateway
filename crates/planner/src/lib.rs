//! Planning layer of the edge gateway: a model-predictive controller that
//! schedules battery, EV charging and heat pump over the next hours as a
//! convex quadratic program, with forecasts, day-ahead prices, battery
//! ageing, comfort, departure deadlines, expected DSO dimming windows and
//! forecast uncertainty (deterministic, chance-constrained or robust).
//!
//! The plan is *guidance*: the real-time controller in the `control` crate
//! follows it only as far as the regulatory and safety rules allow, and falls
//! back to its own rules when there is no plan. Hard guarantees (Pmin,14a,
//! feed-in limits, device minimums) never depend on the plan being right.

mod model;
pub mod normal;
mod qp;

pub use model::{
    BatteryModel, EvRequest, Forecast, HeatPumpModel, Plan, PlanError, PlanInput, Uncertainty, Weights, plan,
};
