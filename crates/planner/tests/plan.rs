use planner::*;

const DT: f64 = 0.25;

fn flat(n: usize, v: f64) -> Vec<f64> {
    vec![v; n]
}

fn forecast(n: usize, pv: f64, base: f64, price: Vec<f64>) -> Forecast {
    Forecast {
        pv_kw: flat(n, pv),
        pv_sigma_kw: flat(n, 0.0),
        pv_worst_kw: flat(n, pv),
        base_kw: flat(n, base),
        base_sigma_kw: 0.0,
        price_export_eur_kwh: price.iter().map(|p| (p - 0.12).max(0.0)).collect(),
        price_import_eur_kwh: price,
    }
}

fn input(n: usize, f: Forecast) -> PlanInput {
    PlanInput {
        dt_h: DT,
        forecast: f,
        import_limit_kw: 250.0,
        export_limit_kw: flat(n, 250.0),
        dim: vec![false; n],
        dim_floor_kw: 0.0,
        battery: None,
        evs: vec![],
        heat_pump: None,
        uncertainty: Uncertainty::Deterministic,
        recovery_steps: 8,
        demand_charge: None,
        weights: Weights { terminal_value_eur_per_kwh: Some(0.0), ..Weights::default() },
    }
}

fn battery(energy: f64, deg: f64) -> BatteryModel {
    BatteryModel {
        energy_kwh: energy,
        capacity_kwh: 100.0,
        min_kwh: 10.0,
        max_kwh: 95.0,
        band_lo_kwh: 10.0,
        band_hi_kwh: 95.0,
        charge_kw: 50.0,
        discharge_kw: 50.0,
        eta_charge: 0.95,
        eta_discharge: 0.95,
        degradation_eur_per_kwh: deg,
        dimmable: true,
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn without_flexibility_the_grid_covers_the_rest() {
    let n = 8;
    let p = plan(&input(n, forecast(n, 4.0, 10.0, flat(n, 0.3)))).unwrap();
    assert!(p.grid_kw.iter().all(|&g| close(g, 6.0, 1e-5)), "{:?}", p.grid_kw);
    assert!(close(p.energy_cost_eur, 8.0 * DT * 6.0 * 0.3, 1e-4));
}

#[test]
fn battery_buys_cheap_and_sells_expensive_unless_ageing_costs_more() {
    let n = 8;
    let prices: Vec<f64> = (0..n).map(|k| if k < 4 { 0.15 } else { 0.55 }).collect();
    let mut inp = input(n, forecast(n, 0.0, 20.0, prices));
    inp.battery = Some(battery(10.0, 0.02));
    let p = plan(&inp).unwrap();
    assert!(p.battery_kw[..4].iter().all(|&b| b > 10.0), "charges while cheap: {:?}", p.battery_kw);
    assert!(p.battery_kw[4..].iter().all(|&b| b < 0.0), "discharges while expensive: {:?}", p.battery_kw);

    // A throughput cost above the price spread makes cycling pointless.
    inp.battery = Some(battery(10.0, 0.5));
    let p = plan(&inp).unwrap();
    assert!(p.battery_kw.iter().all(|&b| b.abs() < 0.5), "{:?}", p.battery_kw);
}

#[test]
fn no_simultaneous_import_and_export_or_charge_and_discharge() {
    let n = 16;
    let prices: Vec<f64> = (0..n).map(|k| 0.1 + 0.05 * (k % 5) as f64).collect();
    let mut inp = input(n, forecast(n, 30.0, 20.0, prices));
    inp.battery = Some(battery(50.0, 0.02));
    let p = plan(&inp).unwrap();
    // grid and battery power are net values; recover the split from the balance
    for k in 0..n {
        let net_site = 20.0 - p.pv_kw[k] + p.battery_kw[k];
        assert!(close(p.grid_kw[k], net_site, 1e-4), "balance at {k}");
    }
}

#[test]
fn ev_gets_its_energy_before_departure_in_the_cheapest_hours() {
    let n = 16; // 4 h
    let prices: Vec<f64> = (0..n).map(|k| if (4..8).contains(&k) { 0.10 } else { 0.40 }).collect();
    let mut inp = input(n, forecast(n, 0.0, 5.0, prices));
    inp.evs =
        vec![EvRequest { remaining_kwh: 10.0, max_kw: 11.0, departure_h: Some(3.0), efficiency: 1.0, dimmable: true }];
    let p = plan(&inp).unwrap();
    let delivered: f64 = p.ev_kw[0].iter().map(|&x| x * DT).sum();
    assert!(close(delivered, 10.0, 1e-3), "{delivered}");
    let cheap: f64 = p.ev_kw[0][4..8].iter().map(|&x| x * DT).sum();
    assert!(cheap > 9.9, "charges in the cheap hour: {:?}", p.ev_kw[0]);
    assert!(p.ev_kw[0][12..].iter().all(|&x| x < 1e-4), "nothing after departure");
    assert!(p.ev_unmet_kwh[0] < 1e-4);
}

#[test]
fn a_demand_charge_spreads_charging_instead_of_raising_the_peak() {
    // Two cars, one cheap hour. Without a demand charge both charge in that
    // hour at full power; with one, the charge-at-the-peak trade-off
    // (0.5 €/kW against 0.10 €/kWh of price spread) flattens the schedule.
    let n = 32; // 8 h
    let prices: Vec<f64> = (0..n).map(|k| if (8..12).contains(&k) { 0.10 } else { 0.20 }).collect();
    let mut inp = input(n, forecast(n, 0.0, 10.0, prices));
    let car = EvRequest { remaining_kwh: 20.0, max_kw: 22.0, departure_h: Some(7.0), efficiency: 1.0, dimmable: true };
    inp.evs = vec![car.clone(), car];
    let max = |p: &Plan| p.grid_kw.iter().copied().fold(0.0, f64::max);
    let free = plan(&inp).unwrap();
    assert!(max(&free) > 45.0, "all in the cheap hour: {:?}", free.grid_kw);

    inp.demand_charge = Some(DemandCharge { eur_per_kw: 0.5, peak_so_far_kw: 0.0 });
    let flat = plan(&inp).unwrap();
    // 40 kWh over the 7 h before departure: 10 kW base + 40/7 kW
    assert!(close(max(&flat), 10.0 + 40.0 / 7.0, 0.05), "{:?}", flat.grid_kw);
    assert!(close(flat.peak_kw.unwrap(), max(&flat), 1e-3));
    assert!(flat.energy_cost_eur > free.energy_cost_eur + 1.0);
    assert!(flat.ev_unmet_kwh.iter().all(|&u| u < 1e-3));

    // A peak the billing period has already paid for is free to use again.
    inp.demand_charge = Some(DemandCharge { eur_per_kw: 0.5, peak_so_far_kw: max(&free) });
    let paid = plan(&inp).unwrap();
    assert!(
        close(paid.energy_cost_eur, free.energy_cost_eur, 0.01),
        "{} vs {}",
        paid.energy_cost_eur,
        free.energy_cost_eur
    );
}

#[test]
fn impossible_request_is_reported_as_unmet_energy() {
    let n = 8;
    let mut inp = input(n, forecast(n, 0.0, 5.0, flat(n, 0.3)));
    inp.evs =
        vec![EvRequest { remaining_kwh: 50.0, max_kw: 11.0, departure_h: Some(1.0), efficiency: 1.0, dimmable: true }];
    let p = plan(&inp).unwrap();
    assert!(close(p.ev_unmet_kwh[0], 39.0, 1e-3), "{}", p.ev_unmet_kwh[0]);
}

#[test]
fn dimming_constraint_holds_and_the_battery_helps() {
    let n = 8;
    let mut inp = input(n, forecast(n, 0.0, 10.0, flat(n, 0.3)));
    inp.dim = vec![true; n];
    inp.dim_floor_kw = 5.0;
    inp.battery = Some(battery(80.0, 0.02));
    inp.evs =
        vec![EvRequest { remaining_kwh: 30.0, max_kw: 22.0, departure_h: Some(2.0), efficiency: 1.0, dimmable: true }];
    let p = plan(&inp).unwrap();
    for k in 0..n {
        let steuve = p.ev_kw[0][k] + p.battery_kw[k];
        assert!(steuve <= 5.0 + 1e-4, "step {k}: {steuve}");
    }
    let delivered: f64 = p.ev_kw[0].iter().map(|&x| x * DT).sum();
    assert!(delivered > 29.9, "the battery makes the car's energy possible: {delivered}");
}

fn winter_building(n: usize) -> HeatPumpModel {
    HeatPumpModel {
        indoor_c: 20.5,
        ua_kw_per_k: 1.2,
        cap_kwh_per_k: 18.0,
        max_kw: 14.0,
        cop: flat(n, 2.8),
        gains_kw: flat(n, 3.0),
        outdoor_c: flat(n, -2.0),
        t_min_c: flat(n, 20.0),
        t_max_c: flat(n, 23.0),
        dimmable: true,
    }
}

#[test]
fn building_is_preheated_before_a_dimming() {
    let n = 24; // 6 h, dimming in hours 3–5
    let mut inp = input(n, forecast(n, 0.0, 10.0, flat(n, 0.3)));
    inp.heat_pump = Some(winter_building(n));
    inp.dim = (0..n).map(|k| (12..20).contains(&k)).collect();
    inp.dim_floor_kw = 2.0; // far below the ~6 kW the building needs at −2 °C
    let p = plan(&inp).unwrap();
    assert!(p.indoor_c[12] > 21.0, "pre-heated before the window: {:?}", &p.indoor_c[..13]);
    assert!(p.indoor_c.iter().all(|&t| t > 20.0 - 1e-3), "{:?}", p.indoor_c);
    for k in 12..20 {
        assert!(p.heat_pump_kw[k] <= 2.0 + 1e-4);
    }
}

#[test]
fn heat_pump_heats_when_energy_is_cheap() {
    let n = 24;
    let prices: Vec<f64> = (0..n).map(|k| if k < 8 { 0.05 } else { 0.60 }).collect();
    let mut inp = input(n, forecast(n, 0.0, 10.0, prices));
    inp.heat_pump = Some(winter_building(n));
    let p = plan(&inp).unwrap();
    let early: f64 = p.heat_pump_kw[..8].iter().sum();
    let late: f64 = p.heat_pump_kw[8..16].iter().sum();
    assert!(early > late, "shifts heat into the cheap hours: {early} vs {late}");
}

#[test]
fn uncertainty_makes_the_plan_keep_more_in_the_battery() {
    let n = 32;
    let mut f = forecast(n, 40.0, 20.0, flat(n, 0.3));
    f.pv_sigma_kw = flat(n, 10.0);
    f.pv_worst_kw = flat(n, 10.0);
    f.base_sigma_kw = 1.0;
    let mut inp = input(n, f);
    inp.battery = Some(battery(40.0, 0.02));
    // an expected dimming at the end: the battery should be ready
    inp.dim = (0..n).map(|k| k >= 28).collect();
    inp.dim_floor_kw = 5.0;
    inp.evs =
        vec![EvRequest { remaining_kwh: 30.0, max_kw: 22.0, departure_h: Some(8.0), efficiency: 1.0, dimmable: true }];

    let lower_bound = |u: Uncertainty, k: usize| {
        let mut i = inp.clone();
        i.uncertainty = u;
        let p = plan(&i).unwrap();
        p.soc_envelope_kwh[k].0
    };
    // In and just before the expected dimming the envelope tightens…
    let det = lower_bound(Uncertainty::Deterministic, 29);
    let cc = lower_bound(Uncertainty::Chance { epsilon: 0.05 }, 29);
    let rob = lower_bound(Uncertainty::Robust, 29);
    assert!(det < cc && cc < rob, "envelope tightens: {det} < {cc} < {rob}");
    // …but not hours before it, where a shortfall is simply bought from the grid.
    assert_eq!(lower_bound(Uncertainty::Chance { epsilon: 0.05 }, 5), det);

    let budget = |u: Uncertainty| {
        let mut i = inp.clone();
        i.uncertainty = u;
        plan(&i).unwrap().dim_budget_kw[30].unwrap()
    };
    assert!(budget(Uncertainty::Deterministic) > budget(Uncertainty::Chance { epsilon: 0.05 }));
    assert!(budget(Uncertainty::Chance { epsilon: 0.05 }) > budget(Uncertainty::Robust) - 1e-9);
}

#[test]
fn a_full_day_with_four_cars_solves_quickly() {
    let n = 96;
    let prices: Vec<f64> = (0..n).map(|k| 0.2 + 0.15 * ((k as f64) / 96.0 * std::f64::consts::TAU).sin()).collect();
    let mut f = forecast(n, 0.0, 20.0, prices);
    f.pv_kw = (0..n).map(|k| (60.0 * ((k as f64 - 28.0) / 52.0 * std::f64::consts::PI).sin()).max(0.0)).collect();
    f.pv_worst_kw = f.pv_kw.iter().map(|p| p * 0.3).collect();
    f.pv_sigma_kw = f.pv_kw.iter().map(|p| p * 0.25).collect();
    f.base_sigma_kw = 1.5;
    let mut inp = input(n, f);
    inp.battery = Some(battery(40.0, 0.03));
    inp.heat_pump = Some(winter_building(n));
    inp.dim = (0..n).map(|k| (70..78).contains(&k)).collect();
    inp.dim_floor_kw = 18.0;
    inp.uncertainty = Uncertainty::Chance { epsilon: 0.05 };
    inp.evs = (0..4)
        .map(|i| EvRequest {
            remaining_kwh: 30.0 + 5.0 * i as f64,
            max_kw: 22.0,
            departure_h: Some(10.0 + 3.0 * i as f64),
            efficiency: 0.95,
            dimmable: true,
        })
        .collect();
    let p = plan(&inp).unwrap();
    assert!(p.ev_unmet_kwh.iter().all(|&u| u < 1e-3), "{:?}", p.ev_unmet_kwh);
    assert!(p.iterations < 100, "{} iterations", p.iterations);
    eprintln!("96-step plan: {:.1} ms, {} iterations", p.solve_ms, p.iterations);

    inp.demand_charge = Some(DemandCharge { eur_per_kw: 0.27, peak_so_far_kw: 30.0 });
    let q = plan(&inp).unwrap();
    assert!(q.ev_unmet_kwh.iter().all(|&u| u < 1e-3), "{:?}", q.ev_unmet_kwh);
    assert!(q.iterations < 100, "{} iterations", q.iterations);
    let max = |p: &Plan| p.grid_kw.iter().copied().fold(0.0, f64::max);
    assert!(max(&q) < max(&p), "the demand charge lowers the peak: {} vs {}", max(&q), max(&p));
    eprintln!("with a demand charge: {:.1} ms, {} iterations", q.solve_ms, q.iterations);
}

#[test]
fn bad_input_is_rejected() {
    let n = 4;
    let mut inp = input(n, forecast(n, 0.0, 5.0, flat(n, 0.3)));
    inp.dim = vec![false; 3];
    assert!(matches!(plan(&inp), Err(PlanError::BadInput(_))));
    let mut inp = input(n, forecast(n, 0.0, 5.0, flat(n, 0.3)));
    inp.uncertainty = Uncertainty::Chance { epsilon: 0.7 };
    assert!(matches!(plan(&inp), Err(PlanError::BadInput(_))));
}
