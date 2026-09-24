use control::rules::three_phase_kw;
use control::*;

/// The demo site: a logistics depot with 120 kWp rooftop PV, four 22 kW
/// chargers and one 14 kW heat pump. Pmin,14a = 16.52 kW.
fn depot() -> SiteConfig {
    SiteConfig {
        pv_installed_kw: 120.0,
        chargers: vec![ChargerSpec { max_current_a: 32.0, failsafe_current_a: 6.0 }; 4],
        heat_pumps: vec![HeatPumpSpec { rated_kw: 14.0, min_kw: 3.0 }],
        feed_in_reference: FeedInReference::GridConnectionPoint,
        release_ramp_s: 300.0,
        margin_kw: 0.3,
        min_dwell_s: 300.0,
    }
}

/// Readings where every device draws what the last setpoints allowed.
fn readings(base_kw: f64, pv_kw: f64, cars: [bool; 4], currents: &[f64], hp_kw: f64, hp_demand: f64) -> Readings {
    let chargers: Vec<ChargerReading> = (0..4)
        .map(|i| ChargerReading {
            online: true,
            car_waiting: cars[i],
            current_a: if cars[i] { currents[i] } else { 0.0 },
            power_kw: if cars[i] { three_phase_kw(currents[i]) } else { 0.0 },
            session_kwh: i as f64, // charger 0 has charged least
        })
        .collect();
    let steuve: f64 = chargers.iter().map(|c| c.power_kw).sum::<f64>() + hp_kw;
    Readings {
        grid_kw: Some(base_kw + steuve - pv_kw),
        pv_kw: Some(pv_kw),
        chargers,
        heat_pumps: vec![HeatPumpReading { online: true, power_kw: hp_kw, demand_kw: hp_demand }],
    }
}

fn allocated_kw(s: &Setpoints, cars: [bool; 4]) -> f64 {
    let ev: f64 = s.charger_current_a.iter().zip(cars).filter(|(_, c)| *c).map(|(a, _)| three_phase_kw(*a)).sum();
    ev + s.heat_pump_limit_kw.iter().sum::<f64>()
}

#[test]
fn pmin_of_the_depot() {
    assert!((Controller::new(depot()).pmin_kw() - 16.52).abs() < 1e-9);
}

#[test]
fn without_dso_commands_everything_runs_at_full_power() {
    let mut c = Controller::new(depot());
    let r = readings(20.0, 0.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let (s, st) = c.step(0.0, &DsoCommands::default(), &r);
    assert_eq!(s.charger_current_a, [32.0; 4]);
    assert_eq!(s.heat_pump_limit_kw, [14.0]);
    assert_eq!(s.pv_limit_pct, 100.0);
    assert_eq!(st.mode, Mode::Normal);
    assert_eq!(st.steuve_budget_kw, None);
}

#[test]
fn dimming_at_night_keeps_grid_draw_of_steuve_under_pmin() {
    let mut c = Controller::new(depot());
    let cars = [true; 4];
    let r = readings(20.0, 0.0, cars, &[32.0; 4], 14.0, 14.0);
    let dim = DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 };
    let (s, st) = c.step(0.0, &dim, &r);

    // Budget 16.52 - 0.3 = 16.22 kW:
    // heat pump 5.6 (40%), two cars at 6 A (8.28), rest 2.34 back to the heat pump.
    assert_eq!(st.mode, Mode::Dimmed);
    assert!((st.steuve_budget_kw.unwrap() - 16.22).abs() < 1e-9);
    assert_eq!(s.charger_current_a, [6.0, 6.0, 0.0, 0.0], "least-charged cars first");
    assert!((s.heat_pump_limit_kw[0] - 7.94).abs() < 1e-9);
    assert!(allocated_kw(&s, cars) <= 16.22 + 1e-9);
}

#[test]
fn pv_surplus_can_be_used_on_top_of_pmin() {
    let mut c = Controller::new(depot());
    let cars = [true; 4];
    // 60 kW PV, 20 kW base load: 40 kW surplus, budget 56.22 kW
    let r = readings(20.0, 60.0, cars, &[32.0; 4], 14.0, 14.0);
    let (s, st) = c.step(0.0, &DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 }, &r);
    assert!((st.steuve_budget_kw.unwrap() - 56.22).abs() < 1e-9);
    assert_eq!(s.heat_pump_limit_kw, [14.0]);
    // 42.22 kW left for cars = 61.2 A: 4 cars, 15 A each (whole amps, rounded down)
    assert_eq!(s.charger_current_a, [16.0, 15.0, 15.0, 15.0]);
    assert!(allocated_kw(&s, cars) <= 56.22 + 1e-9);
}

#[test]
fn release_is_gradual() {
    let mut c = Controller::new(depot());
    let cars = [true; 4];
    let dim = DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 };
    let r = readings(20.0, 0.0, cars, &[6.0, 6.0, 0.0, 0.0], 7.94, 14.0);
    c.step(0.0, &dim, &r);
    let (_, st) = c.step(10.0, &DsoCommands::default(), &r);
    assert_eq!(st.mode, Mode::Releasing);
    let b10 = st.steuve_budget_kw.unwrap();
    let (_, st) = c.step(160.0, &DsoCommands::default(), &r);
    let b160 = st.steuve_budget_kw.unwrap();
    assert!(b10 > 16.0 && b160 > b10 && b160 < 102.32, "{b10} {b160}");
    let (s, st) = c.step(310.0, &DsoCommands::default(), &r);
    assert_eq!(st.mode, Mode::Normal);
    assert_eq!(s.charger_current_a, [32.0; 4]);
}

#[test]
fn offline_charger_is_counted_at_its_failsafe_current() {
    let mut c = Controller::new(depot());
    let cars = [true; 4];
    let mut r = readings(20.0, 0.0, cars, &[32.0; 4], 14.0, 14.0);
    r.chargers[3].online = false;
    let (s, st) = c.step(0.0, &DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 }, &r);
    assert!(st.fallbacks.contains(&Fallback::ChargerOffline(3)));
    // 16.22 - 4.14 reserve = 12.08: heat pump 5.6, one car 4.14, rest to heat pump
    assert_eq!(s.charger_current_a[..3], [6.0, 0.0, 0.0]);
    assert!(allocated_kw(&s, [true, true, true, false]) + three_phase_kw(6.0) <= 16.22 + 1e-9);
}

#[test]
fn without_a_meter_dimming_grants_only_pmin_and_feed_in_limit_is_plant_based() {
    let mut c = Controller::new(depot());
    let mut r = readings(20.0, 60.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    r.grid_kw = None;
    let (s, st) = c.step(0.0, &DsoCommands { dim_14a: true, feed_in_limit_pct: 60.0 }, &r);
    assert!(st.fallbacks.contains(&Fallback::MeterOffline));
    assert!((st.steuve_budget_kw.unwrap() - 16.22).abs() < 1e-9);
    assert_eq!(s.pv_limit_pct, 60.0);
}

#[test]
fn feed_in_limit_at_the_grid_point_lets_the_site_use_more_pv() {
    let mut c = Controller::new(depot());
    // 100 kW PV, 60% limit = 72 kW export allowed. Site consumes 20 base + 88.32 cars + 14 hp.
    let r = readings(20.0, 100.0, [true; 4], &[32.0; 4], 14.0, 14.0);
    let (s, st) = c.step(0.0, &DsoCommands { dim_14a: false, feed_in_limit_pct: 60.0 }, &r);
    assert_eq!(st.allowed_export_kw, 72.0);
    assert_eq!(s.pv_limit_pct, 100.0, "consumption absorbs everything above 72 kW");

    // Empty depot: only 20 kW base load -> PV may produce 72 + 20 - 0.3 kW.
    let r = readings(20.0, 100.0, [false; 4], &[0.0; 4], 0.0, 0.0);
    let (s, _) = c.step(1.0, &DsoCommands { dim_14a: false, feed_in_limit_pct: 60.0 }, &r);
    assert!((s.pv_limit_pct - 91.7 / 1.2).abs() < 1e-9, "{}", s.pv_limit_pct);

    let mut plant = depot();
    plant.feed_in_reference = FeedInReference::PlantOutput;
    let (s, _) = Controller::new(plant).step(0.0, &DsoCommands { dim_14a: false, feed_in_limit_pct: 60.0 }, &r);
    assert_eq!(s.pv_limit_pct, 60.0);
}

#[test]
fn rotation_waits_for_the_dwell_time() {
    let mut c = Controller::new(depot());
    let cars = [true; 4];
    let dim = DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 };
    let mut r = readings(20.0, 0.0, cars, &[6.0, 6.0, 0.0, 0.0], 7.94, 14.0);
    let (s, _) = c.step(0.0, &dim, &r);
    assert_eq!(s.charger_current_a, [6.0, 6.0, 0.0, 0.0]);
    // Cars 0 and 1 charge and overtake cars 2 and 3 in session energy.
    r.chargers[0].session_kwh = 10.0;
    r.chargers[1].session_kwh = 11.0;
    let (s, _) = c.step(100.0, &dim, &r);
    assert_eq!(s.charger_current_a, [6.0, 6.0, 0.0, 0.0], "still inside dwell time");
    let (s, _) = c.step(301.0, &dim, &r);
    assert_eq!(s.charger_current_a, [0.0, 0.0, 6.0, 6.0], "rotated to the cars that waited");
}

/// Invariant over many random situations: while dimmed, what the controller
/// grants never exceeds Pmin + PV surplus (minus devices it cannot control),
/// every charger current is 0 or 6..=32 A in whole amps, and no heat pump
/// runs below its minimum modulation.
#[test]
fn dimming_invariant_holds_for_random_states() {
    let mut seed: u64 = 0x5EED;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..20_000 {
        let mut c = Controller::new(depot());
        let cars = [rnd() < 0.7, rnd() < 0.7, rnd() < 0.7, rnd() < 0.7];
        let currents = [rnd() * 32.0, rnd() * 32.0, rnd() * 32.0, rnd() * 32.0];
        let base = rnd() * 60.0;
        let pv = rnd() * 120.0;
        let hp = rnd() * 14.0;
        let mut r = readings(base, pv, cars, &currents, hp, rnd() * 14.0);
        let offline = rnd() < 0.2;
        if offline {
            r.chargers[0].online = false;
        }
        let (s, st) = c.step(0.0, &DsoCommands { dim_14a: true, feed_in_limit_pct: 100.0 }, &r);
        let budget = st.steuve_budget_kw.unwrap();
        // An offline charger's draw is invisible to the controller, so it lands in base load.
        let seen_base = base + if offline { r.chargers[0].power_kw } else { 0.0 };
        let expected = 16.52 + (pv - seen_base).max(0.0) - 0.3;
        assert!((budget - expected).abs() < 1e-6);
        let reserve = if offline { three_phase_kw(6.0) } else { 0.0 };
        let online_cars = [cars[0] && !offline, cars[1], cars[2], cars[3]];
        assert!(allocated_kw(&s, online_cars) <= (budget - reserve).max(0.0) + 1e-9);
        for &a in &s.charger_current_a {
            assert!(a == 0.0 || ((6.0..=32.0).contains(&a) && a.fract() == 0.0), "{a}");
        }
        assert!(s.heat_pump_limit_kw[0] == 0.0 || s.heat_pump_limit_kw[0] >= 3.0);
    }
}
