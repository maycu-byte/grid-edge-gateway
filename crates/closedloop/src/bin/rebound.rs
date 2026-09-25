//! Rebound study: what a feeder sees when a §14a reduction ends at the same
//! time for many sites, and what changes that.
//!
//! N copies of the depot (each with its own weather) are reduced together and
//! released. The feeder load is their sum, reported per site. Every case is
//! compared with the same days without a reduction under the same controller
//! (paired by weather and fleet), so the rebound is measured, not guessed.
//!
//! Experiments:
//! * `main`      identical fleets (worst case for synchronisation), winter,
//!   17:30–19:30, six release policies and the planner;
//! * `mixed`     the same with a different van timetable at every site;
//! * `staggered` the grid operator releases groups of sites 15 min apart;
//! * `duration`  reductions of 1, 2 and 3 hours;
//! * `notice`    the planner told the window in advance vs not told;
//! * `spring`    the main policies on the spring day.
//!
//! ```text
//! cargo run --release -p closedloop --bin rebound -- --sites 20 --reps 10 --out docs/study/rebound
//! ```
//!
//! Writes `rebound.json` (the main experiment, for the article's figure),
//! `experiments.json` (every experiment and repetition) and `rebound.md`.

use closedloop::feeder::{self, FROM_H, SAMPLE_S, TO_H};
use closedloop::{FeederCase, Strategy, run_site};
use devices::climate::Season;
use serde::Serialize;

const W: (f64, f64) = (17.5, 19.5);

#[derive(Clone, Copy, Debug)]
struct Case {
    experiment: &'static str,
    name: &'static str,
    strategy: &'static str,
    ramp_s: f64,
    delay_max_s: f64,
    /// Reduction window, local hours; `None` = the day without a reduction.
    dim: Option<(f64, f64)>,
    /// The grid operator releases the sites in this many groups, 15 min apart.
    groups: usize,
    season: Season,
    /// A different van timetable at every site.
    mixed: bool,
    /// The case (same experiment) this one is compared with.
    reference: &'static str,
}

const fn case(experiment: &'static str, reference: &'static str) -> Case {
    Case {
        experiment,
        name: "",
        strategy: "rules",
        ramp_s: 300.0,
        delay_max_s: 0.0,
        dim: Some(W),
        groups: 1,
        season: Season::Winter,
        mixed: false,
        reference,
    }
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (exp, mixed, season) in
        [("main", false, Season::Winter), ("mixed", true, Season::Winter), ("spring", false, Season::Spring)]
    {
        let base = Case { mixed, season, ..case(exp, "no dimming") };
        let full = exp != "spring";
        v.push(Case { name: "no dimming", dim: None, ..base });
        if full {
            v.push(Case { name: "step", ramp_s: 0.0, ..base });
        }
        v.push(Case { name: "ramp 5 min", ..base });
        if full {
            v.push(Case { name: "ramp 5 min + wait ≤10 min", delay_max_s: 600.0, ..base });
        }
        v.push(Case { name: "ramp 5 min + wait ≤30 min", delay_max_s: 1800.0, ..base });
        if full {
            v.push(Case { name: "ramp 30 min", ramp_s: 1800.0, ..base });
        }
        v.push(Case {
            name: "no dimming, planner",
            strategy: "mpc",
            dim: None,
            reference: "no dimming, planner",
            ..base
        });
        v.push(Case { name: "planner + ramp 5 min", strategy: "mpc", reference: "no dimming, planner", ..base });
    }
    let s = case("staggered", "no dimming");
    v.push(Case { name: "no dimming", dim: None, ..s });
    v.push(Case { name: "1 group", ..s });
    v.push(Case { name: "2 groups, 15 min apart", groups: 2, ..s });
    v.push(Case { name: "4 groups, 15 min apart", groups: 4, ..s });
    v.push(Case { name: "4 groups + wait ≤10 min", groups: 4, delay_max_s: 600.0, ..s });
    let d = case("duration", "no dimming");
    v.push(Case { name: "no dimming", dim: None, ..d });
    v.push(Case { name: "1 h", dim: Some((17.5, 18.5)), ..d });
    v.push(Case { name: "2 h", ..d });
    v.push(Case { name: "3 h", dim: Some((17.5, 20.5)), ..d });
    let n = case("notice", "no dimming, planner");
    v.push(Case { name: "no dimming, planner", strategy: "mpc", dim: None, ..n });
    v.push(Case { name: "planner told in advance", strategy: "mpc", ..n });
    v.push(Case { name: "planner not told", strategy: "mpc-blind", ..n });
    v
}

#[derive(Serialize, Clone)]
struct Rep {
    experiment: &'static str,
    case: &'static str,
    rep: u64,
    /// Feeder load per site, kW, one-minute means from 16:30.
    load_kw: Vec<f64>,
    before_peak_kw: f64,
    dimmed_mean_kw: f64,
    after_peak_kw: f64,
    /// Highest quarter-hour mean after the release (what a transformer's
    /// thermal rating and a demand charge respond to).
    after_peak_15_kw: f64,
    max_rise_kw_per_min: f64,
    rebound_kw: f64,
    rebound_kwh: f64,
    /// Energy the vans and the heat pump wanted during the reduction but did not get.
    shed_kwh: f64,
    ev_unmet_kwh: f64,
    discomfort_kh: f64,
    /// Energy bought minus sold, 16:30–22:00, €.
    energy_cost_eur: f64,
}

#[derive(Serialize)]
struct Row {
    case: &'static str,
    strategy: &'static str,
    ramp_s: f64,
    delay_max_s: f64,
    groups: usize,
    dim_h: Option<(f64, f64)>,
    before_peak_kw: (f64, f64),
    dimmed_mean_kw: (f64, f64),
    after_peak_kw: (f64, f64),
    after_peak_15_kw: (f64, f64),
    max_rise_kw_per_min: (f64, f64),
    rebound_kw: (f64, f64),
    rebound_kwh: (f64, f64),
    shed_kwh: (f64, f64),
    ev_unmet_kwh: (f64, f64),
    discomfort_kh: (f64, f64),
    energy_cost_eur: (f64, f64),
    mean_load_kw: Vec<f64>,
}

#[derive(Serialize)]
struct Experiment {
    name: &'static str,
    season: &'static str,
    mixed: bool,
    rows: Vec<Row>,
}

/// Mean and half-width of the 95% confidence interval, Student's t.
fn mean_ci(v: &[f64]) -> (f64, f64) {
    let n = v.len();
    let m = v.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return (m, 0.0);
    }
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64;
    // Two-sided 97.5% quantiles for 1..=29 degrees of freedom, then ~1.96.
    const T: [f64; 29] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131,
        2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045,
    ];
    let t = T.get(n - 2).copied().unwrap_or(1.96);
    (m, t * (var / n as f64).sqrt())
}

fn run(c: Case, rep: u64, sites: usize) -> Rep {
    let fc = FeederCase {
        season: c.season,
        strategy: Strategy::parse(c.strategy).expect("strategy"),
        ramp_s: c.ramp_s,
        delay_max_s: c.delay_max_s,
        dim: c.dim,
        groups: c.groups,
        mixed: c.mixed,
    };
    let n_samples = feeder::samples();
    let mut load = vec![0.0; n_samples];
    let (mut unmet, mut discomfort, mut cost, mut shed) = (0.0, 0.0, 0.0, 0.0);
    for site in 0..sites {
        let s = run_site(&fc, rep * 1000 + site as u64 + 1, site);
        for (a, b) in load.iter_mut().zip(&s.load_kw) {
            *a += b;
        }
        unmet += s.ev_unmet_kwh;
        discomfort += s.discomfort_kh;
        cost += s.energy_cost_eur;
        shed += s.shed_kwh;
    }
    let n = sites as f64;
    for v in load.iter_mut() {
        *v /= n;
    }
    let idx = |h: f64| (((h - FROM_H) * 3600.0 / SAMPLE_S).round() as usize).min(n_samples);
    let (d0, d1) = c.dim.unwrap_or(W);
    let before = &load[idx(d0 - 0.5)..idx(d0)];
    let dimmed = &load[idx(d0) + 1..idx(d1)];
    let after_from = idx(d1);
    let after = &load[after_from..];
    Rep {
        experiment: c.experiment,
        case: c.name,
        rep,
        before_peak_kw: before.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        dimmed_mean_kw: dimmed.iter().sum::<f64>() / dimmed.len().max(1) as f64,
        after_peak_kw: after.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        after_peak_15_kw: after.windows(15).map(|w| w.iter().sum::<f64>() / 15.0).fold(f64::NEG_INFINITY, f64::max),
        max_rise_kw_per_min: load[after_from.saturating_sub(1)..].windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max),
        rebound_kw: 0.0,
        rebound_kwh: 0.0,
        shed_kwh: shed / n,
        ev_unmet_kwh: unmet / n,
        discomfort_kh: discomfort / n,
        energy_cost_eur: cost / n,
        load_kw: load,
    }
}

fn main() {
    let mut sites = 20usize;
    let mut reps = 10u64;
    let mut out = String::from("docs/study/rebound");
    let mut only: Option<String> = None;
    let argv: Vec<String> = std::env::args().skip(1).collect();
    for pair in argv.chunks(2) {
        match (pair[0].as_str(), pair.get(1)) {
            ("--sites", Some(v)) => sites = v.parse().expect("--sites N"),
            ("--reps", Some(v)) => reps = v.parse().expect("--reps N"),
            ("--out", Some(v)) => out = v.clone(),
            ("--only", Some(v)) => only = Some(v.clone()),
            _ => {
                eprintln!("usage: rebound [--sites N] [--reps N] [--out DIR] [--only EXPERIMENT]");
                std::process::exit(2)
            }
        }
    }
    let all: Vec<Case> = cases().into_iter().filter(|c| only.as_deref().is_none_or(|o| o == c.experiment)).collect();
    let mut jobs: Vec<(Case, u64)> = all.iter().flat_map(|&c| (0..reps).map(move |r| (c, r))).collect();
    // The planner cases are the slow ones: spread them over all threads.
    jobs.sort_by_key(|(c, _)| c.strategy == "rules");
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunks: Vec<Vec<(Case, u64)>> =
        (0..threads).map(|t| jobs.iter().skip(t).step_by(threads).copied().collect()).collect();
    let mut runs: Vec<Rep> = std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|c| s.spawn(move || c.into_iter().map(|(case, r)| run(case, r, sites)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("worker")).collect()
    });

    // Rebound, paired with the same repetition of the reference case, from
    // the first release onwards.
    let find =
        |exp: &str, name: &str| all.iter().find(|c| c.experiment == exp && c.name == name).copied().expect("case");
    let bases: Vec<Vec<f64>> = runs
        .iter()
        .map(|r| {
            let reference = find(r.experiment, r.case).reference;
            runs.iter()
                .find(|x| x.experiment == r.experiment && x.case == reference && x.rep == r.rep)
                .expect("reference run")
                .load_kw
                .clone()
        })
        .collect();
    for (r, base) in runs.iter_mut().zip(&bases) {
        let first_release = find(r.experiment, r.case).dim.unwrap_or(W).1;
        let from = (((first_release - FROM_H) * 3600.0 / SAMPLE_S).round() as usize).min(base.len());
        let diff: Vec<f64> = r.load_kw[from..].iter().zip(&base[from..]).map(|(a, b)| a - b).collect();
        r.rebound_kw = diff.iter().cloned().fold(0.0, f64::max);
        r.rebound_kwh = diff.iter().map(|d| d.max(0.0)).sum::<f64>() * SAMPLE_S / 3600.0;
    }

    let mut experiments: Vec<Experiment> = Vec::new();
    for c in &all {
        if !experiments.iter().any(|e| e.name == c.experiment) {
            experiments.push(Experiment {
                name: c.experiment,
                season: c.season.name(),
                mixed: c.mixed,
                rows: Vec::new(),
            });
        }
        let rs: Vec<&Rep> = runs.iter().filter(|r| r.experiment == c.experiment && r.case == c.name).collect();
        let col = |f: &dyn Fn(&Rep) -> f64| mean_ci(&rs.iter().map(|r| f(r)).collect::<Vec<_>>());
        let len = rs[0].load_kw.len();
        let row = Row {
            case: c.name,
            strategy: c.strategy,
            ramp_s: c.ramp_s,
            delay_max_s: c.delay_max_s,
            groups: c.groups,
            dim_h: c.dim,
            before_peak_kw: col(&|r| r.before_peak_kw),
            dimmed_mean_kw: col(&|r| r.dimmed_mean_kw),
            after_peak_kw: col(&|r| r.after_peak_kw),
            after_peak_15_kw: col(&|r| r.after_peak_15_kw),
            max_rise_kw_per_min: col(&|r| r.max_rise_kw_per_min),
            rebound_kw: col(&|r| r.rebound_kw),
            rebound_kwh: col(&|r| r.rebound_kwh),
            shed_kwh: col(&|r| r.shed_kwh),
            ev_unmet_kwh: col(&|r| r.ev_unmet_kwh),
            discomfort_kh: col(&|r| r.discomfort_kh),
            energy_cost_eur: col(&|r| r.energy_cost_eur),
            mean_load_kw: (0..len).map(|k| rs.iter().map(|r| r.load_kw[k]).sum::<f64>() / rs.len() as f64).collect(),
        };
        experiments.iter_mut().find(|e| e.name == c.experiment).expect("experiment").rows.push(row);
    }

    let mut md = format!(
        "# Rebound after a §14a reduction: {sites} sites, {reps} repetitions per case\n\n\
         Loads per site (feeder load ÷ {sites}), one-minute means from {FROM_H} h to {TO_H} h; ± is the half-width of the \
         95% confidence interval (Student's t) over the repetitions. Rebound: how far the load after the first release rises \
         above the same days without a reduction under the same controller (paired by weather and fleet). Shed: energy the \
         vans and the heat pump wanted during the reduction but did not get.\n"
    );
    for e in &experiments {
        md.push_str(&format!(
            "\n## {} ({}{})\n\n| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |\n|---|---|---|---|---|---|---|---|---|---|\n",
            e.name,
            e.season,
            if e.mixed { ", mixed fleets" } else { "" }
        ));
        for r in &e.rows {
            let f = |(m, h): (f64, f64), d: usize| format!("{m:.d$} ± {h:.d$}");
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                r.case,
                f(r.after_peak_kw, 1),
                f(r.after_peak_15_kw, 1),
                f(r.max_rise_kw_per_min, 1),
                f(r.rebound_kw, 1),
                f(r.rebound_kwh, 1),
                f(r.shed_kwh, 1),
                f(r.ev_unmet_kwh, 2),
                f(r.discomfort_kh, 2),
                f(r.energy_cost_eur, 2),
            ));
        }
    }
    std::fs::create_dir_all(&out).expect("create output directory");
    runs.sort_by(|a, b| a.experiment.cmp(b.experiment).then(a.case.cmp(b.case)).then(a.rep.cmp(&b.rep)));
    #[derive(Serialize)]
    struct All<'a> {
        sites: usize,
        reps: u64,
        from_h: f64,
        sample_s: f64,
        experiments: &'a [Experiment],
        runs: &'a [Rep],
    }
    let all_out = All { sites, reps, from_h: FROM_H, sample_s: SAMPLE_S, experiments: &experiments, runs: &runs };
    std::fs::write(format!("{out}/experiments.json"), serde_json::to_string(&all_out).expect("json"))
        .expect("write experiments.json");
    // The main experiment in the shape the article's Figure 2 reads.
    if let Some(main) = experiments.iter().find(|e| e.name == "main") {
        let summary: Vec<serde_json::Value> = main
            .rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "policy": r.case, "after_peak_kw": r.after_peak_kw, "rebound_kw": r.rebound_kw,
                    "max_rise_kw_per_min": r.max_rise_kw_per_min, "rebound_kwh": r.rebound_kwh,
                    "ev_unmet_kwh": r.ev_unmet_kwh,
                    "mean_load_kw": r.mean_load_kw.iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(),
                })
            })
            .collect();
        let fig = serde_json::json!({
            "sites": sites, "reps": reps, "season": "winter", "dim_h": W, "from_h": FROM_H,
            "sample_s": SAMPLE_S, "summary": summary,
        });
        std::fs::write(format!("{out}/rebound.json"), fig.to_string()).expect("write rebound.json");
    }
    std::fs::write(format!("{out}/rebound.md"), &md).expect("write md");
    print!("{md}");
}
