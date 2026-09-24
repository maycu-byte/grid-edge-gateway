//! Country rules (DE, AT, CH), batteries and robustness.

use control::rules::three_phase_kw;
use control::*;

include!("common/helpers.rs");

fn country(j: Jurisdiction) -> SiteConfig {
    SiteConfig { policy: Policy::for_country(j), ..depot() }
}

fn dim_cmd() -> DsoCommands {
    DsoCommands { dim: true, ..Default::default() }
}

fn feed_in(pct: f64) -> DsoCommands {
    DsoCommands { feed_in_limit_pct: pct, ..Default::default() }
}

// --- countries --------------------------------------------------------------

#[test]
fn austria_caps_feed_in_at_70_percent_without_any_command() {
    let mut c = Controller::new(country(Jurisdiction::At));
    // 110 kW PV, empty depot: 20 kW base load. 70% of 120 kWp = 84 kW export.
    let r = readings(20.0, 110.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let (s, st) = c.step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(st.feed_in_limit_pct, 70.0);
    assert_eq!(st.allowed_export_kw, 84.0);
    // PV may make 84 kW of export plus the 20 kW the depot uses, minus the margin.
    assert!((s.pv_limit_pct - (84.0 + 20.0 - 0.3) / 1.2).abs() < 1e-9, "{}", s.pv_limit_pct);
    // A tighter DSO setpoint still wins.
    let (_, st) = c.step(&clock(1.0), &feed_in(30.0), &r);
    assert_eq!(st.feed_in_limit_pct, 30.0);
}

#[test]
fn contract_floor_applies_outside_germany() {
    let mut cfg = country(Jurisdiction::Ch);
    cfg.policy.consumption = ConsumptionRule::Contract { min_kw: 10.0, max_minutes_per_day: 120.0 };
    let mut c = Controller::new(cfg);
    assert_eq!(c.floor_kw(), 10.0);
    let r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let (s, st) = c.step(&clock(0.0), &dim_cmd(), &r);
    assert_eq!(st.mode, Mode::Dimmed);
    assert!(allocated_kw(&s, [true; 4]) <= 9.7 + 1e-9);
}

#[test]
fn contract_day_limit_ends_the_dimming_unless_emergency() {
    let mut cfg = country(Jurisdiction::Ch);
    cfg.policy.consumption = ConsumptionRule::Contract { min_kw: 10.0, max_minutes_per_day: 30.0 };
    let mut c = Controller::new(cfg);
    let r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let mut t = 0.0;
    while t <= 30.0 * 60.0 {
        c.step(&clock(t), &dim_cmd(), &r);
        t += 5.0;
    }
    let (_, st) = c.step(&clock(t), &dim_cmd(), &r);
    assert!(st.refusals.contains(&Refusal::DimDayLimitReached));
    assert_ne!(st.mode, Mode::Dimmed, "dimming ends and power is released gradually");
    let (_, st) = c.step(&clock(t + 1.0), &DsoCommands { emergency: true, ..dim_cmd() }, &r);
    assert_eq!(st.mode, Mode::Dimmed, "an emergency overrides the day limit");
    // The next day the contract minutes are available again.
    let next_day = Clock { t_s: t + 2.0, day: 1, year: 2026 };
    let (_, st) = c.step(&next_day, &dim_cmd(), &r);
    assert!(st.refusals.is_empty());
}

#[test]
fn opted_out_devices_are_not_limited_under_a_contract() {
    let mut cfg = country(Jurisdiction::Ch);
    cfg.policy.consumption = ConsumptionRule::Contract { min_kw: 10.0, max_minutes_per_day: 0.0 };
    cfg.heat_pumps[0].opted_out = true;
    let mut c = Controller::new(cfg);
    let r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let (s, _) = c.step(&clock(0.0), &dim_cmd(), &r);
    assert_eq!(s.heat_pump_limit_kw, [14.0]);
    assert_eq!(s.charger_current_a, [0.0; 4], "the heat pump alone already uses the 10 kW floor");

    // In Germany §14a applies whatever the owner wants.
    let mut de = depot();
    de.heat_pumps[0].opted_out = true;
    let (s, _) = Controller::new(de).step(&clock(0.0), &dim_cmd(), &r);
    assert!(s.heat_pump_limit_kw[0] < 14.0);
}

#[test]
fn swiss_curtailment_budget_is_counted_and_enforced() {
    let mut cfg = country(Jurisdiction::Ch);
    cfg.expected_annual_yield_kwh = 1_000.0; // 3% = 30 kWh, used up within the hour
    let mut c = Controller::new(cfg);
    let mut r = readings(20.0, 36.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    r.pv_available_kw = Some(96.0); // 60 kW curtailed while the limit holds
    let mut t = 0.0;
    let mut last = None;
    while t < 3600.0 {
        last = Some(c.step(&clock(t), &feed_in(0.0), &r).1);
        t += 5.0;
    }
    let st = last.unwrap();
    assert!(st.refusals.contains(&Refusal::CurtailmentBudgetExhausted));
    assert_eq!(st.feed_in_limit_pct, 100.0, "non-emergency curtailment beyond 3% is refused");
    assert!(st.totals.curtailed_kwh_year >= 30.0);
    assert!(st.curtailment_budget_used_pct.unwrap() >= 100.0);
    // An immediate, serious threat still curtails.
    let (_, st) = c.step(&clock(t), &DsoCommands { emergency: true, ..feed_in(0.0) }, &r);
    assert_eq!(st.feed_in_limit_pct, 0.0);
}

#[test]
fn restored_totals_keep_the_budget_across_a_restart() {
    let mut cfg = country(Jurisdiction::Ch);
    cfg.expected_annual_yield_kwh = 1_000.0;
    let totals = Totals { day: 0, year: 2026, dimmed_s_today: 0.0, produced_kwh_year: 500.0, curtailed_kwh_year: 31.0 };
    let mut c = Controller::new(cfg).with_totals(totals);
    let r = readings(20.0, 36.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let (_, st) = c.step(&clock(0.0), &feed_in(30.0), &r);
    assert!(st.refusals.contains(&Refusal::CurtailmentBudgetExhausted));
}

// --- battery ------------------------------------------------------------------

fn with_battery(soc: f64, power_kw: f64, r: &mut Readings) -> SiteConfig {
    r.batteries = vec![BatteryReading { online: true, soc_pct: soc, power_kw }];
    if let Some(g) = r.grid_kw.as_mut() {
        *g += power_kw;
    }
    SiteConfig {
        batteries: vec![BatterySpec {
            capacity_kwh: 100.0,
            max_charge_kw: 50.0,
            max_discharge_kw: 50.0,
            min_soc_pct: 10.0,
            max_soc_pct: 95.0,
        }],
        ..depot()
    }
}

#[test]
fn battery_counts_towards_pmin_as_storage() {
    let mut r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let cfg = with_battery(50.0, 0.0, &mut r);
    // n = 6 (4 chargers, 1 battery, 1 heat pump), GZF 0.6: 5.6 + 5 · 0.6 · 4.2 = 18.2 kW
    assert!((Controller::new(cfg).floor_kw() - 18.2).abs() < 1e-9);
}

#[test]
fn battery_stores_export_before_pv_is_curtailed() {
    let mut r = readings(20.0, 100.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let cfg = with_battery(50.0, 0.0, &mut r);
    let mut c = Controller::new(cfg);
    let (s, _) = c.step(&clock(0.0), &feed_in(30.0), &r);
    // 80 kW would be exported; the battery takes its 50 kW.
    assert!((s.battery_kw[0] - 50.0).abs() < 1e-9);
    // PV may then make 36 kW export + 20 kW base + 50 kW charging − margin.
    assert!((s.pv_limit_pct - (36.0 + 20.0 + 50.0 - 0.3) / 1.2).abs() < 1e-6, "{}", s.pv_limit_pct);
}

#[test]
fn battery_discharges_while_dimmed_and_loads_get_more() {
    let mut r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let cfg = with_battery(80.0, 0.0, &mut r);
    let mut c = Controller::new(cfg);
    let floor = c.floor_kw();
    let (s, st) = c.step(&clock(0.0), &dim_cmd(), &r);
    assert!(s.battery_kw[0] < -30.0, "discharging: {}", s.battery_kw[0]);
    let loads = allocated_kw(&s, [true; 4]);
    assert!(loads > floor, "loads {loads} get more than the floor {floor}");
    assert!(loads <= st.steuve_budget_kw.unwrap() - s.battery_kw[0] + 1e-9);
}

#[test]
fn empty_battery_does_not_discharge_and_full_one_does_not_charge() {
    let mut r = readings(20.0, 0.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let cfg = with_battery(10.0, 0.0, &mut r);
    let (s, _) = Controller::new(cfg).step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(s.battery_kw, [0.0]);
    let mut r = readings(20.0, 100.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let cfg = with_battery(95.0, 0.0, &mut r);
    let (s, _) = Controller::new(cfg).step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(s.battery_kw, [0.0]);
}

// --- robustness ------------------------------------------------------------

#[test]
fn implausible_meter_is_treated_as_lost() {
    let mut c = Controller::new(depot());
    let mut r = readings(20.0, 60.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    r.grid_kw = Some(3_276.7); // an int16 overflow from a wrong scale factor
    let (_, st) = c.step(&clock(0.0), &dim_cmd(), &r);
    assert!(st.fallbacks.contains(&Fallback::MeterImplausible));
    assert!((st.steuve_budget_kw.unwrap() - 16.22).abs() < 1e-9, "blind, so only Pmin");
    r.grid_kw = Some(f64::NAN);
    let (_, st) = c.step(&clock(1.0), &dim_cmd(), &r);
    assert!(st.fallbacks.contains(&Fallback::MeterImplausible));
}

#[test]
fn pv_limit_drops_at_once_and_rises_gently() {
    let mut cfg = depot();
    cfg.pv_ramp_pct_per_s = 0.5;
    cfg.feed_in_reference = FeedInReference::PlantOutput;
    let mut c = Controller::new(cfg);
    let r = readings(20.0, 30.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let (s, _) = c.step(&clock(0.0), &feed_in(30.0), &r);
    assert_eq!(s.pv_limit_pct, 30.0);
    let (s, _) = c.step(&clock(10.0), &feed_in(100.0), &r);
    assert!((s.pv_limit_pct - 35.0).abs() < 1e-9);
    let mut last = 0.0;
    for k in 2..=200 {
        last = c.step(&clock(k as f64 * 10.0), &feed_in(100.0), &r).0.pv_limit_pct;
    }
    assert_eq!(last, 100.0);
}

#[test]
fn site_config_validation_catches_mistakes() {
    depot().validate().unwrap();
    let mut bad = depot();
    bad.chargers[0].failsafe_current_a = 4.0; // below the 6 A a car accepts
    assert!(bad.validate().is_err());
    let mut bad = depot();
    bad.heat_pumps[0].min_kw = 20.0;
    assert!(bad.validate().is_err());
    let mut bad = depot();
    bad.connection_kw = 0.0;
    assert!(bad.validate().is_err());
}

/// The invariant for every country and with a battery: while dimmed, the
/// loads never get more than the floor + PV surplus + battery discharge.
#[test]
fn dimming_invariant_holds_in_every_country_with_a_battery() {
    let mut seed: u64 = 0xC0FFEE;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for k in 0..15_000 {
        let j = [Jurisdiction::De, Jurisdiction::At, Jurisdiction::Ch][k % 3];
        let cars = [rnd() < 0.7, rnd() < 0.7, rnd() < 0.7, rnd() < 0.7];
        let currents = [rnd() * 32.0, rnd() * 32.0, rnd() * 32.0, rnd() * 32.0];
        let mut r = readings(rnd() * 60.0, rnd() * 120.0, cars, &currents, rnd() * 14.0, rnd() * 14.0);
        let mut cfg = with_battery(rnd() * 100.0, 0.0, &mut r);
        cfg.policy = Policy::for_country(j);
        if j != Jurisdiction::De {
            cfg.policy.consumption = ConsumptionRule::Contract { min_kw: rnd() * 20.0, max_minutes_per_day: 0.0 };
        }
        let mut c = Controller::new(cfg);
        let (s, st) = c.step(&clock(0.0), &dim_cmd(), &r);
        let budget = st.steuve_budget_kw.unwrap();
        let discharge = (-s.battery_kw[0]).max(0.0);
        assert!(s.battery_kw[0] <= 1e-9, "never charges while dimmed");
        assert!(allocated_kw(&s, cars) <= (budget + discharge).max(0.0) + 1e-9, "{j:?}");
        for &a in &s.charger_current_a {
            assert!(a == 0.0 || ((6.0..=32.0).contains(&a) && a.fract() == 0.0), "{a}");
        }
    }
}
