//! The real-time layer with a plan: it follows the planner's guidance only
//! as far as the rules allow.

use control::rules::three_phase_kw;
use control::*;

include!("common/helpers.rs");

fn with_battery(soc: f64, r: &mut Readings) -> SiteConfig {
    r.batteries = vec![BatteryReading { online: true, soc_pct: soc, power_kw: 0.0 }];
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

fn dim() -> DsoCommands {
    DsoCommands { dim: true, ..Default::default() }
}

#[test]
fn least_laxity_first_when_the_budget_fits_one_car() {
    // A 6 kW contract (CH) and no heat pump: room for one car at 6 A.
    let mut cfg = SiteConfig { policy: Policy::for_country(Jurisdiction::Ch), ..depot() };
    cfg.policy.consumption = ConsumptionRule::Contract { min_kw: 6.0, max_minutes_per_day: 0.0 };
    cfg.heat_pumps.clear();
    let mut r = readings(20.0, 0.0, [true, true, false, false], &[32.0, 32.0, 0.0, 0.0], 0.0, 0.0);
    r.heat_pumps.clear();
    // Car 0 has charged least but leaves tomorrow; car 1 leaves in 2 h and still needs 30 kWh.
    r.chargers[0].remaining_kwh = Some(40.0);
    r.chargers[0].departure_s = Some(12.0 * 3600.0);
    r.chargers[1].remaining_kwh = Some(30.0);
    r.chargers[1].departure_s = Some(2.0 * 3600.0);
    let (s, st) = Controller::new(cfg.clone()).step(&clock(0.0), &dim(), &r);
    assert!((st.steuve_budget_kw.unwrap() - 5.7).abs() < 1e-9);
    assert_eq!(s.charger_current_a[..2], [0.0, 8.0], "the car that leaves first charges");

    // Without departure information, the car that charged least goes first.
    for c in &mut r.chargers {
        c.departure_s = None;
    }
    let (s, _) = Controller::new(cfg).step(&clock(0.0), &dim(), &r);
    assert_eq!(s.charger_current_a[..2], [8.0, 0.0]);
}

#[test]
fn plan_sets_each_chargers_power_in_normal_operation() {
    let mut c = Controller::new(depot());
    let r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 0.0, 0.0);
    c.set_guidance(Some(Guidance { charger_kw: vec![Some(0.0), Some(11.0), Some(1.0), None], ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(
        s.charger_current_a,
        [0.0, 16.0, 0.0, 32.0],
        "11 kW → 15.9 A → 16 A; 1 kW is below half the 6 A minimum"
    );
}

#[test]
fn battery_holds_the_planned_grid_exchange() {
    let mut r = readings(30.0, 0.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let mut c = Controller::new(with_battery(50.0, &mut r));
    c.set_guidance(Some(Guidance { grid_kw: Some(10.0), ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &DsoCommands::default(), &r);
    assert!((s.battery_kw[0] + 20.0).abs() < 1e-9, "30 kW load, 10 kW planned import: {}", s.battery_kw[0]);

    // The plan wants to charge from the grid at night: allowed outside a dimming.
    c.set_guidance(Some(Guidance { grid_kw: Some(70.0), ..Default::default() }));
    let (s, _) = c.step(&clock(1.0), &DsoCommands::default(), &r);
    assert!((s.battery_kw[0] - 40.0).abs() < 1e-9, "{}", s.battery_kw[0]);
}

#[test]
fn guided_battery_still_stores_what_a_feed_in_limit_would_curtail() {
    let mut r = readings(20.0, 100.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    r.pv_available_kw = Some(100.0);
    let mut c = Controller::new(with_battery(50.0, &mut r));
    // A plan that wanted to export 80 kW, but the DSO now allows only 36.
    c.set_guidance(Some(Guidance { grid_kw: Some(-80.0), ..Default::default() }));
    let cmd = DsoCommands { feed_in_limit_pct: 30.0, ..Default::default() };
    let (s, _) = c.step(&clock(0.0), &cmd, &r);
    assert!(s.battery_kw[0] >= 44.0 - 1e-9, "stores 80 − 36 kW: {}", s.battery_kw[0]);
}

#[test]
fn guided_battery_never_charges_from_the_grid_while_dimmed() {
    let mut r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let mut c = Controller::new(with_battery(50.0, &mut r));
    c.set_guidance(Some(Guidance { grid_kw: Some(90.0), dim_expected: true, ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &dim(), &r);
    assert!(s.battery_kw[0] <= 1e-9, "{}", s.battery_kw[0]);
}

#[test]
fn plan_rations_the_battery_through_an_expected_dimming() {
    let mut r = readings(20.0, 0.0, [true; 4], &[6.0, 6.0, 6.0, 0.0], 5.0, 14.0);
    let grid_now = r.grid_kw.unwrap();
    let mut c = Controller::new(with_battery(80.0, &mut r));
    // The plan spreads 20 kW of discharge over the whole window.
    c.set_guidance(Some(Guidance { grid_kw: Some(grid_now - 20.0), dim_expected: true, ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &dim(), &r);
    assert!((s.battery_kw[0] + 20.0).abs() < 1e-6, "follows the plan: {}", s.battery_kw[0]);

    // A dimming the plan did not expect: the rules discharge as needed.
    let mut c = Controller::new(with_battery(80.0, &mut r));
    c.set_guidance(Some(Guidance { grid_kw: Some(grid_now - 20.0), dim_expected: false, ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &dim(), &r);
    assert!(s.battery_kw[0] < -30.0, "rules take over: {}", s.battery_kw[0]);
}

#[test]
fn heat_pump_power_request_passes_through_capped_by_the_dimming() {
    let r = readings(20.0, 0.0, [false; 4], &[0.0; 4], 8.0, 14.0);
    let mut c = Controller::new(depot());
    c.set_guidance(Some(Guidance { heat_pump_kw: vec![Some(12.0)], ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(s.heat_pump_ext_kw, [Some(12.0)]);
    let (s, _) = c.step(&clock(1.0), &dim(), &r);
    let cap = s.heat_pump_limit_kw[0];
    assert!(s.heat_pump_ext_kw[0].unwrap() <= cap + 1e-9, "{:?} vs {cap}", s.heat_pump_ext_kw);

    c.set_guidance(None);
    let (s, _) = c.step(&clock(2.0), &DsoCommands::default(), &r);
    assert_eq!(s.heat_pump_ext_kw, [None], "no plan: the thermostat decides");
}

/// Whatever the plan says, the dimming invariant holds.
#[test]
fn dimming_invariant_holds_under_arbitrary_guidance() {
    let mut seed: u64 = 0xBADC0DE;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..10_000 {
        let cars = [rnd() < 0.7, rnd() < 0.7, rnd() < 0.7, rnd() < 0.7];
        let currents = [rnd() * 32.0, rnd() * 32.0, rnd() * 32.0, rnd() * 32.0];
        let mut r = readings(rnd() * 60.0, rnd() * 120.0, cars, &currents, rnd() * 14.0, rnd() * 14.0);
        for c in &mut r.chargers {
            c.remaining_kwh = Some(rnd() * 60.0);
            c.departure_s = Some(rnd() * 12.0 * 3600.0);
        }
        let mut c = Controller::new(with_battery(rnd() * 100.0, &mut r));
        c.set_guidance(Some(Guidance {
            grid_kw: Some(rnd() * 300.0 - 150.0),
            charger_kw: (0..4).map(|_| if rnd() < 0.5 { Some(rnd() * 30.0) } else { None }).collect(),
            heat_pump_kw: vec![Some(rnd() * 20.0)],
            dim_expected: rnd() < 0.5,
        }));
        let (s, st) = c.step(&clock(0.0), &dim(), &r);
        let budget = st.steuve_budget_kw.unwrap();
        let discharge = (-s.battery_kw[0]).max(0.0);
        let charge = s.battery_kw[0].max(0.0);
        let loads = allocated_kw(&s, cars);
        assert!(loads + charge <= (budget + discharge).max(0.0) + 1e-9, "{loads} + {charge} vs {budget} + {discharge}");
        for &a in &s.charger_current_a {
            assert!(a == 0.0 || ((6.0..=32.0).contains(&a) && a.fract() == 0.0), "{a}");
        }
        let hp_ext = s.heat_pump_ext_kw[0].unwrap();
        assert!(hp_ext <= s.heat_pump_limit_kw[0] + 1e-9);
    }
}

#[test]
fn a_car_about_to_miss_its_departure_charges_at_full_power_whatever_the_plan() {
    let mut c = Controller::new(depot());
    let mut r = readings(20.0, 0.0, [true, false, false, false], &[6.0, 0.0, 0.0, 0.0], 0.0, 0.0);
    // 20 kWh in 1 h at 16 A (11 kW) takes 1.8 h: no slack left.
    r.chargers[0].remaining_kwh = Some(20.0);
    r.chargers[0].departure_s = Some(3600.0);
    r.chargers[0].car_max_current_a = Some(16.0);
    c.set_guidance(Some(Guidance { charger_kw: vec![Some(0.0), None, None, None], ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &DsoCommands::default(), &r);
    assert_eq!(s.charger_current_a[0], 32.0, "the car takes what it can (16 A)");

    // With plenty of slack the plan's "not now" is followed.
    r.chargers[0].departure_s = Some(10.0 * 3600.0);
    let (s, _) = c.step(&clock(1.0), &DsoCommands::default(), &r);
    assert_eq!(s.charger_current_a[0], 0.0);
}

/// Regression: a guided heat pump reports our own request as its demand; a
/// request of 0 just before a dimming used to lock it off for the whole window.
#[test]
fn guided_heat_pump_is_not_locked_off_by_its_own_echo() {
    let mut c = Controller::new(depot());
    // The unit echoes the last request (0 kW) as its demand.
    let r = readings(20.0, 0.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    c.set_guidance(Some(Guidance { heat_pump_kw: vec![Some(9.0)], dim_expected: true, ..Default::default() }));
    let (s, _) = c.step(&clock(0.0), &dim(), &r);
    assert!(s.heat_pump_limit_kw[0] >= 9.0 - 1e-9, "{:?}", s.heat_pump_limit_kw);
    assert_eq!(s.heat_pump_ext_kw, [Some(9.0)]);
}
