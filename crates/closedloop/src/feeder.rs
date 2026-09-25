//! One site of a feeder around a §14a reduction: the building block of the
//! rebound study (`bin/rebound.rs`) and of the browser's feeder calculator.
//!
//! A site is the demo depot with its own weather (and, optionally, its own
//! van timetable), run from 06:00 so the battery and the building reach the
//! evening in a realistic state. Its grid exchange is recorded as one-minute
//! means from [`FROM_H`] to [`TO_H`]; the feeder is the sum of its sites.

use control::{Controller, Jurisdiction, Mode};
use devices::climate::Season;
use serde::Serialize;

use crate::runner::{ClosedLoop, Scenario, Strategy, site_config};

/// The window recorded, local hours.
pub const FROM_H: f64 = 16.5;
pub const TO_H: f64 = 22.0;
pub const SAMPLE_S: f64 = 60.0;
const SUB_S: f64 = 10.0;
/// Control period of the simulated sites.
pub const CONTROL_DT_S: f64 = 5.0;

#[derive(Debug, Clone, Copy)]
pub struct FeederCase {
    pub season: Season,
    pub strategy: Strategy,
    /// Gradual release after the reduction, s (0 = step back at once).
    pub ramp_s: f64,
    /// Random wait before the ramp, up to this long, s.
    pub delay_max_s: f64,
    /// Reduction window, local hours; `None` = the day without a reduction.
    pub dim: Option<(f64, f64)>,
    /// The grid operator releases the sites in this many groups, 15 min apart.
    pub groups: usize,
    /// A different van timetable at every site.
    pub mixed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteRun {
    /// Grid exchange of the site, kW (+ import), one-minute means from FROM_H.
    pub load_kw: Vec<f64>,
    /// Energy the vans wanted during the reduction but did not get, kWh.
    pub shed_kwh: f64,
    /// Energy the vans left without, kWh.
    pub ev_unmet_kwh: f64,
    pub discomfort_kh: f64,
    /// Energy bought minus sold from FROM_H to TO_H, €.
    pub energy_cost_eur: f64,
}

/// Number of one-minute samples a [`SiteRun`] holds.
pub fn samples() -> usize {
    ((TO_H - FROM_H) * 3600.0 / SAMPLE_S).round() as usize
}

/// A small deterministic generator for the fleet perturbations.
struct Rng(u64);
impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// A different timetable for each site: the whole fleet shifted by up to an
/// hour either way, each van by up to 15 min more, and each van asking for
/// 70–130 % of the energy. Dwell times stay as they are.
pub fn mix_fleet(cl: &mut ClosedLoop, seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let site_shift = rng.range(-3600.0, 3600.0);
    let now = cl.sim.t_s;
    for a in cl.sim.arrivals.iter_mut() {
        let shift = site_shift + rng.range(-900.0, 900.0);
        let dwell = a.departure_s - a.at_s;
        a.at_s = (a.at_s + shift).max(now + 1.0);
        a.departure_s = a.at_s + dwell;
        a.needs_kwh *= rng.range(0.7, 1.3);
    }
    cl.sim.arrivals.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
}

/// Runs one site. `seed` sets its weather (and timetable); `index` its
/// release group.
pub fn run_site(c: &FeederCase, seed: u64, index: usize) -> SiteRun {
    let mut scenario = Scenario::study(c.season, seed);
    scenario.dim_windows_h = match c.dim {
        // Group g is released g × 15 min after the first.
        Some((a, b)) => vec![(a, b + (index % c.groups.max(1)) as f64 * 0.25)],
        None => vec![],
    };
    let mut cl = ClosedLoop::new(scenario, c.strategy, CONTROL_DT_S);
    if c.mixed {
        mix_fleet(&mut cl, seed);
    }
    let mut cfg = site_config(Jurisdiction::De);
    cfg.release_ramp_s = c.ramp_s;
    cfg.release_delay_max_s = c.delay_max_s;
    cfg.release_delay_seed = seed;
    cl.ctl = Controller::new(cfg);
    cl.advance(FROM_H * 3600.0 - cl.sim.t_s);
    let cost0 = cl.metrics.energy_cost_eur;
    let full_ev_kw = control::rules::three_phase_kw(32.0);
    let subs = (SAMPLE_S / SUB_S) as usize;
    let mut load = Vec::with_capacity(samples());
    let mut shed = 0.0;
    for _ in 0..samples() {
        let mut sum = 0.0;
        for _ in 0..subs {
            cl.advance(SUB_S);
            sum += cl.sim.grid_kw();
            if cl.status.as_ref().is_some_and(|s| s.mode == Mode::Dimmed) {
                let cars_wanting = cl
                    .sim
                    .chargers
                    .iter()
                    .filter(|ch| ch.car.as_ref().is_some_and(|car| car.charged_kwh < car.needs_kwh))
                    .count() as f64;
                let want = cars_wanting * full_ev_kw + cl.sim.heat_pumps.iter().map(|h| h.demand_kw).sum::<f64>();
                let got = cl.sim.chargers_kw() + cl.sim.heat_pumps_kw();
                shed += (want - got).max(0.0) * SUB_S / 3600.0;
            }
        }
        load.push(sum / subs as f64);
    }
    let from = FROM_H * 3600.0;
    SiteRun {
        load_kw: load,
        shed_kwh: shed,
        ev_unmet_kwh: cl.sim.departures.iter().filter(|d| d.at_s >= from).map(|d| d.unmet_kwh()).sum(),
        discomfort_kh: cl.metrics.discomfort_kh,
        energy_cost_eur: cl.metrics.energy_cost_eur - cost0,
    }
}
