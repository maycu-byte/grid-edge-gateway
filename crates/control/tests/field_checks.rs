//! The checks around a DSO command: a random wait before power returns, an
//! inverter that ignores its limit, and the report of every reduction.

use control::rules::three_phase_kw;
use control::*;

include!("common/helpers.rs");

fn dim() -> DsoCommands {
    DsoCommands { dim: true, ..Default::default() }
}

/// Readings where the loads draw what the last setpoints allowed.
fn following(s: Option<&Setpoints>) -> Readings {
    match s {
        None => readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0),
        Some(s) => readings(20.0, 0.0, [true; 4], &s.charger_current_a, s.heat_pump_limit_kw[0], 14.0),
    }
}

fn wait_after_release(seed: u64) -> f64 {
    let mut c = Controller::new(SiteConfig { release_delay_max_s: 600.0, release_delay_seed: seed, ..depot() });
    let r = following(None);
    c.step(&clock(0.0), &dim(), &r);
    let (_, st) = c.step(&clock(10.0), &DsoCommands::default(), &r);
    assert_eq!(st.mode, Mode::Releasing);
    st.release_wait_s.expect("a wait")
}

#[test]
fn release_waits_a_random_time_then_ramps() {
    let mut c = Controller::new(SiteConfig { release_delay_max_s: 600.0, release_delay_seed: 42, ..depot() });
    let r = following(None);
    let (_, st) = c.step(&clock(0.0), &dim(), &r);
    let dimmed = st.steuve_budget_kw.unwrap();

    let (_, st) = c.step(&clock(10.0), &DsoCommands::default(), &r);
    let wait = st.release_wait_s.unwrap();
    assert!((0.0..600.0).contains(&wait), "{wait}");
    assert_eq!(st.steuve_budget_kw, Some(dimmed), "power stays at the dimmed budget while waiting");

    let (_, st) = c.step(&clock(10.0 + wait - 1.0), &DsoCommands::default(), &r);
    assert_eq!((st.mode, st.steuve_budget_kw), (Mode::Releasing, Some(dimmed)));
    assert!(st.release_wait_s.unwrap() <= 1.0 + 1e-9);

    let (_, st) = c.step(&clock(10.0 + wait + 150.0), &DsoCommands::default(), &r);
    assert_eq!(st.release_wait_s, None);
    let mid = st.steuve_budget_kw.unwrap();
    assert!(mid > dimmed + 10.0 && mid < 102.32, "halfway up the ramp: {mid}");

    let (s, st) = c.step(&clock(10.0 + wait + 310.0), &DsoCommands::default(), &r);
    assert_eq!(st.mode, Mode::Normal);
    assert_eq!(s.charger_current_a, [32.0; 4]);
}

#[test]
fn each_site_draws_its_own_wait_and_zero_means_none() {
    let waits: Vec<f64> = (1..=20).map(wait_after_release).collect();
    assert_eq!(wait_after_release(7), wait_after_release(7), "same seed, same wait");
    let spread =
        waits.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - waits.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(spread > 300.0, "20 sites spread their restart over the window: {waits:?}");

    // Off by default: the ramp starts at once, as before.
    let mut c = Controller::new(depot());
    let r = following(None);
    c.step(&clock(0.0), &dim(), &r);
    let (_, st) = c.step(&clock(10.0), &DsoCommands::default(), &r);
    assert_eq!(st.release_wait_s, None);
}

/// Two 60 kW inverters with 50 kW of sun each; inverter 1 ignores its limit.
fn plant_readings(sent: &[f64], stuck: bool) -> Readings {
    let obey = |pct: f64| (60.0 * pct / 100.0).min(50.0);
    let kw = [obey(sent[0]), if stuck { 50.0 } else { obey(sent[1]) }];
    let mut r = readings(20.0, kw[0] + kw[1], [false; 4], &[0.0; 4], 0.0, 0.0);
    r.inverters = kw.iter().map(|&kw| InverterReading { online: true, kw, rated_kw: 60.0 }).collect();
    r
}

#[test]
fn an_inverter_that_ignores_its_limit_is_reported_and_the_other_makes_up_for_it() {
    let mut cfg = depot();
    cfg.feed_in_reference = FeedInReference::PlantOutput;
    let mut c = Controller::new(cfg);
    let cmd = DsoCommands { feed_in_limit_pct: 60.0, ..Default::default() };
    let mut sent = vec![100.0, 100.0];
    let mut flagged_at = None;
    for t in 0..60 {
        let r = plant_readings(&sent, true);
        let (s, st) = c.step(&clock(t as f64), &cmd, &r);
        sent = s.inverter_limit_pct.clone();
        if flagged_at.is_none() && st.fallbacks.contains(&Fallback::InverterIgnoresLimit(1)) {
            flagged_at = Some(t);
        }
        assert!(!st.fallbacks.contains(&Fallback::InverterIgnoresLimit(0)), "inverter 0 follows its limit");
    }
    let at = flagged_at.expect("inverter 1 is reported");
    assert!((30..=32).contains(&at), "reported after the 30 s timeout, at {at} s");
    // 72 kW allowed, inverter 1 stuck at 50 kW: inverter 0 gets 22 of its 60 kW.
    assert!((sent[0] - 22.0 / 60.0 * 100.0).abs() < 1e-9, "{sent:?}");
    assert_eq!(sent[1], 60.0, "the stuck inverter keeps being sent the plant limit");
    let r = plant_readings(&sent, true);
    assert!((r.pv_kw.unwrap() - 72.0).abs() < 1e-9, "the plant as a whole is back at the limit");

    // When it follows again, the report clears and both get the same limit.
    let (s, st) = c.step(&clock(61.0), &cmd, &plant_readings(&sent, false));
    assert!(!st.fallbacks.iter().any(|f| matches!(f, Fallback::InverterIgnoresLimit(_))));
    assert_eq!(s.inverter_limit_pct, [60.0, 60.0]);
}

#[test]
fn a_slow_inverter_is_not_reported_before_the_timeout_and_without_readings_nothing_changes() {
    let mut cfg = depot();
    cfg.feed_in_reference = FeedInReference::PlantOutput;
    let mut c = Controller::new(cfg);
    let cmd = DsoCommands { feed_in_limit_pct: 30.0, ..Default::default() };
    for t in 0..29 {
        let (_, st) = c.step(&clock(t as f64), &cmd, &plant_readings(&[100.0, 100.0], true));
        assert!(st.fallbacks.is_empty(), "{t}: {:?}", st.fallbacks);
    }
    let (s, _) =
        Controller::new(depot()).step(&clock(0.0), &cmd, &readings(20.0, 100.0, [false; 4], &[0.0; 4], 0.0, 0.0));
    assert!(s.inverter_limit_pct.is_empty(), "no per-inverter data: use pv_limit_pct");
}

#[test]
fn every_dimming_leaves_a_chained_report() {
    let mut c = Controller::new(depot()).with_report_epoch(1_737_327_600_000);
    // 1: the loads follow the setpoints.
    let mut last: Option<Setpoints> = None;
    for t in 0..600 {
        let (s, _) = c.step(&clock(t as f64), &dim(), &following(last.as_ref()));
        assert!(allocated_kw(&s, [true; 4]) <= 16.52);
        last = Some(s);
    }
    c.step(&clock(600.0), &DsoCommands::default(), &following(last.as_ref()));
    // 2: the loads ignore them and keep drawing full power.
    for t in 1000..1300 {
        c.step(&clock(t as f64), &dim(), &following(None));
    }
    c.step(&clock(1300.0), &DsoCommands::default(), &following(None));

    let reps = c.reports().to_vec();
    assert_eq!(reps.len(), 2);
    let (ok, bad) = (&reps[0], &reps[1]);
    assert_eq!(ok.verdict, Verdict::Followed, "max {:?}", ok.max_after_grace_kw);
    assert!((ok.floor_kw - 16.52).abs() < 1e-9);
    assert_eq!(ok.samples.len(), 600);
    assert!(ok.max_after_grace_kw.unwrap() <= 16.52);
    assert_eq!(bad.verdict, Verdict::Exceeded);
    assert!((bad.exceeded_s - 240.0).abs() < 1e-9, "300 s minus the 60 s grace: {}", bad.exceeded_s);
    assert_eq!(bad.previous_sha256, ok.sha256);
    assert!(DimmingReport::verify_csv(&bad.to_csv()));
    assert!(ok.to_csv().contains("# start: 2025-01-19T23:00:00Z"));
    assert_eq!(c.report_chain(), (2, bad.sha256.clone()));
    assert_eq!(c.take_reports().len(), 2);
    assert!(c.reports().is_empty());

    // After a restart the chain continues.
    let mut c2 = Controller::new(depot()).with_report_chain(2, &bad.sha256);
    c2.step(&clock(0.0), &dim(), &following(None));
    c2.step(&clock(5.0), &DsoCommands::default(), &following(None));
    assert_eq!(c2.reports()[0].seq, 3);
    assert_eq!(c2.reports()[0].previous_sha256, bad.sha256);
}
