//! The whole loop — simulated depot, register maps, real-time controller and
//! planner — over an evening with a §14a dimming, for every strategy.

use closedloop::{ClosedLoop, Metrics, Scenario, Strategy};
use devices::climate::Season;

fn run(strategy: &str, from_h: f64, to_h: f64) -> Metrics {
    let mut scenario = Scenario::study(Season::Winter, 1);
    scenario.start_h = from_h;
    let mut cl = ClosedLoop::new(scenario, Strategy::parse(strategy).unwrap(), 10.0);
    cl.advance((to_h - from_h) * 3600.0);
    cl.finish()
}

#[test]
fn every_strategy_keeps_the_hard_rules_through_a_dimming() {
    // 16:00 to 21:00: the vans arrive, the price peaks, the DSO dims 17:30–19:30.
    for s in Strategy::study_set() {
        let m = run(s.name(), 16.0, 21.0);
        assert!(m.dimmed_hours > 1.9, "{}: the dimming happened", s.name());
        assert!(m.dim_excess_kwh < 1e-9, "{}: drew {} kWh above the floor", s.name(), m.dim_excess_kwh);
        assert_eq!(m.plan_failures, 0, "{}", s.name());
        if s != Strategy::Rules {
            assert!(m.plans >= 20, "{}: re-plans every 15 min", s.name());
        }
    }
}

#[test]
fn the_planner_saves_money_without_raising_the_peak() {
    // Overnight, so the vans that arrived in the evening leave inside the run.
    let rules = run("rules", 16.0, 30.0);
    let blind_to_peak = run("mpc-no-peak", 16.0, 30.0);
    let mpc = run("mpc", 16.0, 30.0);
    for (name, m) in [("rules", &rules), ("mpc-no-peak", &blind_to_peak), ("mpc", &mpc)] {
        assert!(m.departures >= 2, "{name}: cars left during the run");
        assert_eq!(m.cars_short, 0, "{name}: {} kWh missing", m.ev_unmet_kwh);
        assert!(m.dim_excess_kwh < 1e-9, "{name}");
    }
    assert!(mpc.total_eur() < rules.total_eur() - 10.0, "{} vs {}", mpc.total_eur(), rules.total_eur());
    assert!(
        mpc.peak_quarter_kw < 0.8 * rules.peak_quarter_kw,
        "peak {} vs {} kW on rules",
        mpc.peak_quarter_kw,
        rules.peak_quarter_kw
    );
    assert!(
        blind_to_peak.peak_quarter_kw > mpc.peak_quarter_kw + 20.0,
        "without the demand charge the plan piles load into the cheapest hours: {} vs {} kW",
        blind_to_peak.peak_quarter_kw,
        mpc.peak_quarter_kw
    );
}
