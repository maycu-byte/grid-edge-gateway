// Shared fixtures: the demo depot and readings where every device draws
// what the last setpoints allowed. Included by the test files.

fn clock(t_s: f64) -> Clock {
    Clock::at(t_s, 2026)
}

/// The demo site: a logistics depot with 120 kWp rooftop PV, four 22 kW
/// chargers and one 14 kW heat pump. Pmin,14a = 16.52 kW.
fn depot() -> SiteConfig {
    SiteConfig {
        policy: Policy::for_country(Jurisdiction::De),
        pv_installed_kw: 120.0,
        connection_kw: 250.0,
        expected_annual_yield_kwh: 120_000.0,
        chargers: vec![ChargerSpec { max_current_a: 32.0, failsafe_current_a: 6.0, opted_out: false }; 4],
        heat_pumps: vec![HeatPumpSpec { rated_kw: 14.0, min_kw: 3.0, opted_out: false }],
        batteries: vec![],
        feed_in_reference: FeedInReference::GridConnectionPoint,
        release_ramp_s: 300.0,
        pv_ramp_pct_per_s: 100.0,
        margin_kw: 0.3,
        min_dwell_s: 300.0,
        import_target_kw: 0.0,
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
        pv_available_kw: None,
        chargers,
        heat_pumps: vec![HeatPumpReading { online: true, power_kw: hp_kw, demand_kw: hp_demand }],
        batteries: vec![],
    }
}

fn allocated_kw(s: &Setpoints, cars: [bool; 4]) -> f64 {
    let ev: f64 = s.charger_current_a.iter().zip(cars).filter(|(_, c)| *c).map(|(a, _)| three_phase_kw(*a)).sum();
    ev + s.heat_pump_limit_kw.iter().sum::<f64>()
}
