//! The site's energy plan as a convex quadratic program (the optimisation
//! solved at every step of a model-predictive controller).
//!
//! Horizon of `n` steps of `dt` hours. Per step `k`:
//!
//! ```text
//! balance     g⁺ₖ − g⁻ₖ + pvₖ − Σᵢ evᵢₖ − hpₖ − cₖ + dₖ = baseₖ
//! battery     Eₖ₊₁ = Eₖ + η_c·dt·cₖ − dt/η_d·dₖ,   E_min ≤ Eₖ ≤ E_max
//! building    Tₖ₊₁ = a·Tₖ + b·COPₖ·hpₖ + dt/C·(gainsₖ + UA·T_outₖ),  a = 1 − dt·UA/C
//! EV i        Σₖ ηᵢ·dt·evᵢₖ + uᵢ ≥ energy wanted before departure
//! dimming     Σ evᵢₖ + hpₖ + cₖ − dₖ ≤ floor + max(0, PV_lowₖ − base_highₖ)   (when a dimming is expected)
//! ```
//!
//! and the objective sums energy cost (import price ≥ export price keeps
//! the split g⁺/g⁻ exact without binaries), battery degradation (throughput
//! cost plus a penalty outside a comfortable state-of-charge band),
//! penalties on discomfort and on energy a car leaves without, and a small
//! quadratic term that smooths the battery schedule.
//!
//! Uncertainty (PV and base load forecast errors) enters where a shortfall
//! would hurt: the PV that may be counted on during a dimming, and the
//! battery's state-of-charge envelope. The real-time layer lets the battery
//! absorb forecast errors (an affine recourse with gain 1), so its energy
//! deviates from the plan by the accumulated error; the envelope is
//! tightened by that amount — `z·σ` for a chance constraint with
//! `z = Φ⁻¹(1 − ε)`, or the worst case for the robust variant. Errors are
//! accumulated linearly, not in quadrature, because a cloudy day is cloudy
//! all day: forecast errors of neighbouring hours are strongly correlated.

use crate::normal::inverse_cdf;
use crate::qp::{Qp, Terms};

/// Forecasts for each step of the horizon.
#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    /// Expected PV the sun allows, kW.
    pub pv_kw: Vec<f64>,
    /// Standard deviation of the PV forecast error, kW.
    pub pv_sigma_kw: Vec<f64>,
    /// PV on the worst day the robust variant considers, kW.
    pub pv_worst_kw: Vec<f64>,
    /// Expected uncontrollable load, kW.
    pub base_kw: Vec<f64>,
    pub base_sigma_kw: f64,
    pub price_import_eur_kwh: Vec<f64>,
    pub price_export_eur_kwh: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatteryModel {
    pub energy_kwh: f64,
    pub capacity_kwh: f64,
    pub min_kwh: f64,
    pub max_kwh: f64,
    /// State of charge the cells like: outside it, `soc_band` penalties apply.
    pub band_lo_kwh: f64,
    pub band_hi_kwh: f64,
    pub charge_kw: f64,
    pub discharge_kw: f64,
    pub eta_charge: f64,
    pub eta_discharge: f64,
    /// Cycle ageing as a throughput cost, € per kWh charged or discharged:
    /// replacement cost / (2 · cycles · capacity · depth of discharge).
    pub degradation_eur_per_kwh: f64,
    /// Counts as a controllable device while dimmed (a battery is one in DE).
    pub dimmable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvRequest {
    pub remaining_kwh: f64,
    pub max_kw: f64,
    /// Hours until the car leaves; `None` = unknown (plan for the horizon's end).
    pub departure_h: Option<f64>,
    pub efficiency: f64,
    pub dimmable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeatPumpModel {
    pub indoor_c: f64,
    pub ua_kw_per_k: f64,
    pub cap_kwh_per_k: f64,
    pub max_kw: f64,
    pub cop: Vec<f64>,
    pub gains_kw: Vec<f64>,
    pub outdoor_c: Vec<f64>,
    /// Comfort band at the end of each step, °C.
    pub t_min_c: Vec<f64>,
    pub t_max_c: Vec<f64>,
    pub dimmable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Uncertainty {
    /// Plan on the expected values.
    Deterministic,
    /// Hold each constraint with probability at least 1 − ε (Gaussian errors).
    Chance { epsilon: f64 },
    /// Hold every constraint on the worst day of the uncertainty set.
    Robust,
}

/// Preference weights of the objective, in € per unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub comfort_eur_per_kh: f64,
    pub ev_unmet_eur_per_kwh: f64,
    pub soc_band_eur_per_kwh_h: f64,
    pub soft_envelope_eur_per_kwh: f64,
    pub smoothing_eur_per_kw2: f64,
    /// Value of energy left in the battery at the horizon's end, €/kWh.
    /// `None`: 90% of the mean import price, times the round-trip efficiency.
    pub terminal_value_eur_per_kwh: Option<f64>,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            comfort_eur_per_kh: 20.0,
            ev_unmet_eur_per_kwh: 1.0,
            soc_band_eur_per_kwh_h: 0.01,
            soft_envelope_eur_per_kwh: 0.5,
            smoothing_eur_per_kw2: 1e-4,
            terminal_value_eur_per_kwh: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanInput {
    pub dt_h: f64,
    pub forecast: Forecast,
    pub import_limit_kw: f64,
    /// Export allowed per step (static caps, known DSO limits), kW.
    pub export_limit_kw: Vec<f64>,
    /// Steps in which the DSO is expected to dim consumption.
    pub dim: Vec<bool>,
    /// What the controllable devices may still draw from the grid then
    /// (Pmin,14a or the contract, minus the controller's margin), kW.
    pub dim_floor_kw: f64,
    pub battery: Option<BatteryModel>,
    pub evs: Vec<EvRequest>,
    pub heat_pump: Option<HeatPumpModel>,
    pub uncertainty: Uncertainty,
    pub weights: Weights,
}

impl PlanInput {
    pub fn steps(&self) -> usize {
        self.forecast.pv_kw.len()
    }
}

/// The optimal schedule. Powers are per step; `soc_kwh` and `indoor_c`
/// have one more entry (the state at the start of each step, then the end).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub grid_kw: Vec<f64>,
    pub pv_kw: Vec<f64>,
    pub battery_kw: Vec<f64>,
    pub soc_kwh: Vec<f64>,
    /// State-of-charge envelope after tightening for uncertainty, per step end.
    pub soc_envelope_kwh: Vec<(f64, f64)>,
    pub ev_kw: Vec<Vec<f64>>,
    pub ev_unmet_kwh: Vec<f64>,
    pub heat_pump_kw: Vec<f64>,
    pub indoor_c: Vec<f64>,
    /// Right-hand side of the dimming constraint, where one applies.
    pub dim_budget_kw: Vec<Option<f64>>,
    /// Energy cost of the plan (import − export), €.
    pub energy_cost_eur: f64,
    pub objective: f64,
    pub iterations: u32,
    pub solve_ms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    BadInput(String),
    Solver(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::BadInput(s) => write!(f, "bad planner input: {s}"),
            PlanError::Solver(s) => write!(f, "solver: {s}"),
        }
    }
}

impl std::error::Error for PlanError {}

fn check(inp: &PlanInput) -> Result<(), PlanError> {
    let n = inp.steps();
    let f = &inp.forecast;
    let bad = |s: &str| Err(PlanError::BadInput(s.into()));
    if n == 0 || inp.dt_h <= 0.0 {
        return bad("empty horizon or non-positive step");
    }
    for (name, len) in [
        ("pv_sigma_kw", f.pv_sigma_kw.len()),
        ("pv_worst_kw", f.pv_worst_kw.len()),
        ("base_kw", f.base_kw.len()),
        ("price_import", f.price_import_eur_kwh.len()),
        ("price_export", f.price_export_eur_kwh.len()),
        ("export_limit_kw", inp.export_limit_kw.len()),
        ("dim", inp.dim.len()),
    ] {
        if len != n {
            return Err(PlanError::BadInput(format!("{name} has {len} entries, expected {n}")));
        }
    }
    if let Some(h) = &inp.heat_pump {
        for (name, len) in [
            ("cop", h.cop.len()),
            ("gains_kw", h.gains_kw.len()),
            ("outdoor_c", h.outdoor_c.len()),
            ("t_min_c", h.t_min_c.len()),
            ("t_max_c", h.t_max_c.len()),
        ] {
            if len != n {
                return Err(PlanError::BadInput(format!("heat pump {name} has {len} entries, expected {n}")));
            }
        }
        if h.cap_kwh_per_k <= 0.0 || inp.dt_h * h.ua_kw_per_k >= h.cap_kwh_per_k {
            return bad("building capacity must be positive and the step shorter than its time constant");
        }
    }
    if let Some(b) = &inp.battery
        && (b.capacity_kwh <= 0.0 || b.eta_charge <= 0.0 || b.eta_discharge <= 0.0)
    {
        return bad("battery capacity and efficiencies must be positive");
    }
    if let Uncertainty::Chance { epsilon } = inp.uncertainty
        && !(epsilon > 0.0 && epsilon < 0.5)
    {
        return bad("chance constraint epsilon must be in (0, 0.5)");
    }
    Ok(())
}

pub fn plan(inp: &PlanInput) -> Result<Plan, PlanError> {
    check(inp)?;
    let n = inp.steps();
    let dt = inp.dt_h;
    let f = &inp.forecast;
    let w = &inp.weights;

    let z = match inp.uncertainty {
        Uncertainty::Chance { epsilon } => inverse_cdf(1.0 - epsilon),
        _ => 0.0,
    };
    // What may be counted on where a shortfall would hurt.
    let pv_low: Vec<f64> = (0..n)
        .map(|k| match inp.uncertainty {
            Uncertainty::Deterministic => f.pv_kw[k],
            Uncertainty::Chance { .. } => (f.pv_kw[k] - z * f.pv_sigma_kw[k]).max(0.0),
            Uncertainty::Robust => f.pv_worst_kw[k].min(f.pv_kw[k]),
        })
        .collect();
    let base_high: Vec<f64> = (0..n)
        .map(|k| {
            f.base_kw[k]
                + match inp.uncertainty {
                    Uncertainty::Deterministic => 0.0,
                    Uncertainty::Chance { .. } => z * f.base_sigma_kw,
                    Uncertainty::Robust => 2.0 * f.base_sigma_kw,
                }
        })
        .collect();
    // Convexity of the import/export split: never price imports below exports.
    let p_imp: Vec<f64> = (0..n).map(|k| f.price_import_eur_kwh[k].max(f.price_export_eur_kwh[k])).collect();
    let p_exp = &f.price_export_eur_kwh;

    let mut qp = Qp::default();
    let mut gi = Vec::with_capacity(n);
    let mut ge = Vec::with_capacity(n);
    let mut pv = Vec::with_capacity(n);
    let mut bc = Vec::new();
    let mut bd = Vec::new();
    let mut e = Vec::new();
    let mut envelope = Vec::new();
    let mut hp = Vec::new();
    let mut tin = Vec::new();
    let mut ev: Vec<Vec<usize>> = vec![Vec::with_capacity(n); inp.evs.len()];
    let mut dim_budget = vec![None; n];

    let (mut acc_sigma, mut acc_low, mut acc_high) = (0.0, 0.0, 0.0);

    for k in 0..n {
        let g_in = qp.var(0.0, inp.import_limit_kw);
        qp.cost(g_in, dt * p_imp[k]);
        let g_out = qp.var(0.0, inp.export_limit_kw[k].max(0.0));
        qp.cost(g_out, -dt * p_exp[k]);
        let p = qp.var(0.0, f.pv_kw[k].max(0.0));
        qp.cost(p, -1e-4 * dt); // use PV rather than curtail it when indifferent
        gi.push(g_in);
        ge.push(g_out);
        pv.push(p);

        // Σ over consumers, entering the balance with −1
        let mut balance: Terms = vec![(g_in, 1.0), (g_out, -1.0), (p, 1.0)];
        let mut dim_terms: Terms = Vec::new();

        if let Some(b) = &inp.battery {
            let c = qp.var(0.0, b.charge_kw);
            let d = qp.var(0.0, b.discharge_kw);
            qp.cost(c, dt * b.degradation_eur_per_kwh);
            qp.cost(d, dt * b.degradation_eur_per_kwh);
            let lo = b.min_kwh.min(b.energy_kwh);
            let hi = b.max_kwh.max(b.energy_kwh);
            let ek = qp.var(lo, hi);
            // dynamics
            let mut dyn_terms: Terms = vec![(ek, 1.0), (c, -b.eta_charge * dt), (d, dt / b.eta_discharge)];
            let rhs = if k == 0 {
                b.energy_kwh
            } else {
                dyn_terms.push((e[k - 1], -1.0));
                0.0
            };
            qp.eq(dyn_terms, rhs);
            // comfortable band for the cells
            let band_lo = qp.var(0.0, f64::INFINITY);
            let band_hi = qp.var(0.0, f64::INFINITY);
            qp.cost(band_lo, dt * w.soc_band_eur_per_kwh_h);
            qp.cost(band_hi, dt * w.soc_band_eur_per_kwh_h);
            qp.ge(vec![(ek, 1.0), (band_lo, 1.0)], b.band_lo_kwh);
            qp.le(vec![(ek, 1.0), (band_hi, -1.0)], b.band_hi_kwh);
            // envelope tightened by the accumulated forecast error the
            // battery will have absorbed by the end of this step
            acc_sigma += dt * (f.pv_sigma_kw[k] + f.base_sigma_kw);
            acc_low += dt * ((f.pv_kw[k] - f.pv_worst_kw[k]).max(0.0) + 2.0 * f.base_sigma_kw);
            acc_high += dt * 2.0 * (f.pv_sigma_kw[k] + f.base_sigma_kw);
            let (t_lo, t_hi) = match inp.uncertainty {
                Uncertainty::Deterministic => (0.0, 0.0),
                Uncertainty::Chance { .. } => (z * acc_sigma, z * acc_sigma),
                Uncertainty::Robust => (acc_low, acc_high),
            };
            let env = (b.min_kwh + t_lo, b.max_kwh - t_hi);
            if t_lo > 0.0 {
                let s = qp.var(0.0, f64::INFINITY);
                qp.cost(s, w.soft_envelope_eur_per_kwh);
                qp.ge(vec![(ek, 1.0), (s, 1.0)], env.0);
            }
            if t_hi > 0.0 {
                let s = qp.var(0.0, f64::INFINITY);
                qp.cost(s, w.soft_envelope_eur_per_kwh);
                qp.le(vec![(ek, 1.0), (s, -1.0)], env.1);
            }
            envelope.push(env);
            // smooth schedule: w·(Pₖ − Pₖ₋₁)²
            if k > 0 && w.smoothing_eur_per_kw2 > 0.0 {
                qp.square(&[(c, 1.0), (d, -1.0), (bc[k - 1], -1.0), (bd[k - 1], 1.0)], w.smoothing_eur_per_kw2);
            }
            balance.push((c, -1.0));
            balance.push((d, 1.0));
            if b.dimmable {
                dim_terms.push((c, 1.0));
                dim_terms.push((d, -1.0));
            }
            bc.push(c);
            bd.push(d);
            e.push(ek);
        }

        if let Some(h) = &inp.heat_pump {
            let x = qp.var(0.0, h.max_kw);
            let t = qp.var(f64::NEG_INFINITY, f64::INFINITY);
            let a = 1.0 - dt * h.ua_kw_per_k / h.cap_kwh_per_k;
            let b = dt * h.cop[k] / h.cap_kwh_per_k;
            let c = dt * (h.gains_kw[k] + h.ua_kw_per_k * h.outdoor_c[k]) / h.cap_kwh_per_k;
            let mut terms: Terms = vec![(t, 1.0), (x, -b)];
            let rhs = if k == 0 {
                c + a * h.indoor_c
            } else {
                terms.push((tin[k - 1], -a));
                c
            };
            qp.eq(terms, rhs);
            let cold = qp.var(0.0, f64::INFINITY);
            let warm = qp.var(0.0, f64::INFINITY);
            qp.cost(cold, dt * w.comfort_eur_per_kh);
            qp.cost(warm, dt * w.comfort_eur_per_kh);
            qp.ge(vec![(t, 1.0), (cold, 1.0)], h.t_min_c[k]);
            qp.le(vec![(t, 1.0), (warm, -1.0)], h.t_max_c[k]);
            balance.push((x, -1.0));
            if h.dimmable {
                dim_terms.push((x, 1.0));
            }
            hp.push(x);
            tin.push(t);
        }

        for (i, r) in inp.evs.iter().enumerate() {
            let avail = r.departure_h.map_or(1.0, |d| (d / dt - k as f64).clamp(0.0, 1.0));
            let x = qp.var(0.0, r.max_kw * avail);
            // when it makes no difference, charge sooner rather than later
            qp.cost(x, 1e-5 * dt * k as f64);
            balance.push((x, -1.0));
            if r.dimmable {
                dim_terms.push((x, 1.0));
            }
            ev[i].push(x);
        }

        qp.eq(balance, f.base_kw[k]);

        if inp.dim[k] {
            let rhs = inp.dim_floor_kw + (pv_low[k] - base_high[k]).max(0.0);
            dim_budget[k] = Some(rhs);
            if !dim_terms.is_empty() {
                qp.le(dim_terms, rhs);
            }
        }
    }

    // Energy each car should get before it leaves.
    let mut unmet = Vec::with_capacity(inp.evs.len());
    for (i, r) in inp.evs.iter().enumerate() {
        let dep_steps = r.departure_h.map(|d| d / dt);
        let need = match dep_steps {
            Some(d) if d > n as f64 => r.remaining_kwh * n as f64 / d, // leaves after the horizon: pro rata
            _ => r.remaining_kwh,
        };
        let u = qp.var(0.0, f64::INFINITY);
        qp.cost(u, w.ev_unmet_eur_per_kwh);
        let mut terms: Terms = ev[i].iter().map(|&x| (x, r.efficiency * dt)).collect();
        terms.push((u, 1.0));
        qp.ge(terms, need.max(0.0));
        unmet.push(u);
    }

    // Energy left in the battery is worth something after the horizon.
    if let (Some(b), Some(&last)) = (&inp.battery, e.last()) {
        let mean = p_imp.iter().sum::<f64>() / n as f64;
        let v = w.terminal_value_eur_per_kwh.unwrap_or(0.9 * mean * b.eta_charge * b.eta_discharge);
        qp.cost(last, -v);
    }

    let sol = qp.solve().map_err(PlanError::Solver)?;
    let x = &sol.x;
    let val = |i: usize| x[i];

    let grid_kw: Vec<f64> = (0..n).map(|k| val(gi[k]) - val(ge[k])).collect();
    let energy_cost_eur = (0..n).map(|k| dt * (p_imp[k] * val(gi[k]) - p_exp[k] * val(ge[k]))).sum();
    let battery_kw = (0..bc.len()).map(|k| val(bc[k]) - val(bd[k])).collect();
    let soc_kwh = match &inp.battery {
        Some(b) => std::iter::once(b.energy_kwh).chain(e.iter().map(|&i| val(i))).collect(),
        None => Vec::new(),
    };
    let indoor_c = match &inp.heat_pump {
        Some(h) => std::iter::once(h.indoor_c).chain(tin.iter().map(|&i| val(i))).collect(),
        None => Vec::new(),
    };
    Ok(Plan {
        grid_kw,
        pv_kw: pv.iter().map(|&i| val(i)).collect(),
        battery_kw,
        soc_kwh,
        soc_envelope_kwh: envelope,
        ev_kw: ev.iter().map(|v| v.iter().map(|&i| val(i)).collect()).collect(),
        ev_unmet_kwh: unmet.iter().map(|&i| val(i)).collect(),
        heat_pump_kw: hp.iter().map(|&i| val(i)).collect(),
        indoor_c,
        dim_budget_kw: dim_budget,
        energy_cost_eur,
        objective: sol.objective,
        iterations: sol.iterations,
        solve_ms: sol.solve_ms,
    })
}
