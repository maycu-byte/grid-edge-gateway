//! The glue between the site and the optimiser. It turns what the
//! real-time layer reads from the devices, plus forecasts, into the
//! planner's input, and a plan back into guidance for the real-time layer.
//!
//! The gateway, the closed-loop simulation and the browser demo all plan
//! through this crate, so they plan the same way. It has no I/O: the
//! gateway fetches prices and weather, the simulation makes them up.

mod input;
mod localtime;
mod meters;
mod record;

pub use input::{BuildingModel, Horizon, SiteModel, build_input};
pub use localtime::{Civil, cet_offset_s, civil_from_unix, local_seconds_of_day, unix_from_utc};
pub use meters::{BaseLoadProfile, QuarterPeak};
pub use record::PlanRecord;
