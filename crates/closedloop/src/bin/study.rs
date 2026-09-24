//! Monte Carlo study: rule-based control vs MPC variants on the simulated
//! depot, over many days of random weather, in spring and winter.
//!
//! ```text
//! cargo run --release -p closedloop --bin study -- --seeds 30 --out docs/study
//! cargo run --release -p closedloop --bin study -- --dim 13-15 --out docs/study/afternoon
//! ```
//!
//! Writes `study.json` (every run) and `study.md` (summary tables).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use closedloop::{ClosedLoop, Metrics, Scenario, Strategy};
use devices::climate::Season;
use serde::Serialize;

#[derive(Serialize, Clone)]
struct Run {
    season: &'static str,
    seed: u64,
    strategy: &'static str,
    cloudiness_day0: f64,
    degradation_eur: f64,
    demand_eur: f64,
    total_eur: f64,
    curtailed_kwh: f64,
    metrics: Metrics,
}

struct Args {
    seeds: u64,
    seasons: Vec<Season>,
    strategies: Vec<Strategy>,
    hours: f64,
    dt_s: f64,
    /// The DSO's daily dimming window, local hours.
    dim: (f64, f64),
    out: String,
}

fn parse() -> Args {
    let mut a = Args {
        seeds: 30,
        seasons: vec![Season::Spring, Season::Winter],
        strategies: Strategy::study_set(),
        // 06:00 on day 0 to 07:30 on day 1: the overnight vans leave inside the run.
        hours: 25.5,
        dt_s: 5.0,
        dim: (17.5, 19.5),
        out: "docs/study".into(),
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let usage = || -> ! {
        eprintln!(
            "usage: study [--seeds N] [--seasons spring,winter] [--strategies rules,mpc,...] [--hours H] [--dt S] [--dim FROM-TO] [--out DIR]"
        );
        std::process::exit(2)
    };
    for pair in argv.chunks(2) {
        let [k, v] = pair else { usage() };
        match k.as_str() {
            "--seeds" => a.seeds = v.parse().unwrap_or_else(|_| usage()),
            "--hours" => a.hours = v.parse().unwrap_or_else(|_| usage()),
            "--dt" => a.dt_s = v.parse().unwrap_or_else(|_| usage()),
            "--out" => a.out = v.clone(),
            "--dim" => {
                let (x, y) = v.split_once('-').unwrap_or_else(|| usage());
                a.dim = (x.parse().unwrap_or_else(|_| usage()), y.parse().unwrap_or_else(|_| usage()));
            }
            "--seasons" => {
                a.seasons = v.split(',').map(|s| Season::parse(s).unwrap_or_else(|| usage())).collect();
            }
            "--strategies" => {
                a.strategies = v.split(',').map(|s| Strategy::parse(s).unwrap_or_else(|| usage())).collect();
            }
            _ => usage(),
        }
    }
    a
}

fn run_one(season: Season, seed: u64, strategy: Strategy, a: &Args) -> Run {
    let mut scenario = Scenario::study(season, seed);
    scenario.dim_windows_h = vec![a.dim];
    let mut cl = ClosedLoop::new(scenario, strategy, a.dt_s);
    let cloudiness_day0 = cl.sim.cloud_day[0];
    let mut left = a.hours * 3600.0;
    while left > 0.0 {
        let chunk = left.min(900.0);
        cl.advance(chunk);
        left -= chunk;
    }
    let m = cl.finish();
    Run {
        season: season.name(),
        seed,
        strategy: strategy.name(),
        cloudiness_day0,
        degradation_eur: m.degradation_eur(),
        demand_eur: m.demand_eur(),
        total_eur: m.total_eur(),
        curtailed_kwh: m.curtailed_kwh(),
        metrics: m,
    }
}

fn mean_ci(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if n < 2.0 {
        return (xs.first().copied().unwrap_or(0.0), 0.0);
    }
    let m = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    // 95% interval of the mean, normal approximation (n ≥ 20 in the study).
    (m, 1.96 * (var / n).sqrt())
}

/// `study trace <season> <seed> <strategy> [file.csv]`: one run, sampled every 5 minutes.
fn trace(argv: &[String]) {
    let season = argv.first().and_then(|s| Season::parse(s)).expect("season");
    let seed: u64 = argv.get(1).and_then(|s| s.parse().ok()).expect("seed");
    let strategy = argv.get(2).and_then(|s| Strategy::parse(s)).expect("strategy");
    let out = argv.get(3).cloned().unwrap_or_else(|| "trace.csv".into());
    let mut cl = ClosedLoop::new(Scenario::study(season, seed), strategy, 5.0);
    let mut csv = String::from(
        "hour,price_eur_mwh,dim,grid_kw,pv_kw,pv_avail_kw,base_kw,ev_kw,ev0,ev1,ev2,ev3,hp_kw,battery_kw,soc_pct,indoor_c,t_min_c,plan_grid_kw,plan_soc_kwh\n",
    );
    for _ in 0..(25.5 * 12.0) as usize {
        cl.advance(300.0);
        let s = &cl.sim;
        let (pg, ps) = cl.plan.as_ref().map_or((f64::NAN, f64::NAN), |p| {
            let k = ((s.t_s - p.made_at_s) / 900.0).floor() as usize;
            (p.plan.grid_kw.get(k).copied().unwrap_or(f64::NAN), p.plan.soc_kwh.get(k + 1).copied().unwrap_or(f64::NAN))
        });
        csv.push_str(&format!(
            "{:.3},{:.2},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.1},{:.2},{:.1},{:.2},{:.2}\n",
            s.t_s / 3600.0,
            s.climate.day_ahead_eur_mwh(s.t_s),
            cl.cmd.dim as u8,
            s.grid_kw(),
            s.pv_kw(),
            s.pv_available_kw(),
            s.base_kw,
            s.chargers_kw(),
            s.chargers[0].power_kw(),
            s.chargers[1].power_kw(),
            s.chargers[2].power_kw(),
            s.chargers[3].power_kw(),
            s.heat_pumps_kw(),
            s.batteries_kw(),
            s.batteries[0].soc_pct,
            s.building.indoor_c,
            devices::sim::Building::comfort_min_c(s.t_s),
            pg,
            ps
        ));
    }
    let m = cl.finish();
    std::fs::write(&out, csv).unwrap();
    println!("{}", serde_json::to_string_pretty(&m).unwrap());
    for d in cl.sim.departures.iter() {
        println!(
            "departure {:.2} h charger {} wanted {:.1} got {:.1}",
            d.at_s / 3600.0,
            d.charger,
            d.needs_kwh,
            d.charged_kwh
        );
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("trace") {
        trace(&argv[1..]);
        return;
    }
    let a = parse();
    let mut jobs = Vec::new();
    for &season in &a.seasons {
        for seed in 1..=a.seeds {
            for &strategy in &a.strategies {
                jobs.push((season, seed, strategy));
            }
        }
    }
    let total = jobs.len();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(total));
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(&(season, seed, strategy)) = jobs.get(i) else { break };
                    let r = run_one(season, seed, strategy, &a);
                    let mut res = results.lock().unwrap();
                    res.push(r);
                    eprint!("\r{}/{} runs", res.len(), total);
                }
            });
        }
    });
    eprintln!("\r{total} runs in {:.0} s on {threads} threads", started.elapsed().as_secs_f64());
    let mut runs = results.into_inner().unwrap();
    runs.sort_by(|x, y| (x.season, x.seed, x.strategy).cmp(&(y.season, y.seed, y.strategy)));

    std::fs::create_dir_all(&a.out).expect("create output directory");
    std::fs::write(format!("{}/study.json", a.out), serde_json::to_string_pretty(&runs).unwrap()).unwrap();

    let mut md = String::new();
    md.push_str(&format!(
        "Generated by `cargo run --release -p closedloop --bin study -- --seeds {} --dim {}-{}`: {} seeds × {} seasons × {} strategies, {} h each from 06:00, §14a dimming {} to {} (announced a day ahead), control every {} s, MPC every 15 min (96 × 15 min horizon).\n\n",
        a.seeds,
        a.dim.0,
        a.dim.1,
        a.seeds,
        a.seasons.len(),
        a.strategies.len(),
        a.hours,
        clock(a.dim.0),
        clock(a.dim.1),
        a.dt_s
    ));
    for &season in &a.seasons {
        let name = season.name();
        md.push_str(&format!("### {}\n\n", capitalize(name)));
        md.push_str("| Strategy | Energy € | Battery ageing € | Peak charge € | Total € | Δ total vs rules € | Peak kW (15 min) | Discomfort K·h | EV energy missing kWh | Cars short | Above the dimming floor: kWh · s · max kW | Solve ms (mean / max) |\n");
        md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        let of = |strategy: &str| -> Vec<&Run> {
            runs.iter().filter(|r| r.season == name && r.strategy == strategy).collect()
        };
        let base = of("rules");
        for &strategy in &a.strategies {
            let rs = of(strategy.name());
            if rs.is_empty() {
                continue;
            }
            let col = |f: &dyn Fn(&Run) -> f64| mean_ci(&rs.iter().map(|r| f(r)).collect::<Vec<_>>());
            let cost = col(&|r| r.metrics.energy_cost_eur);
            let deg = col(&|r| r.degradation_eur);
            let demand = col(&|r| r.demand_eur);
            let total = col(&|r| r.total_eur);
            // paired by seed: same weather for both strategies
            let delta: Vec<f64> = rs
                .iter()
                .filter_map(|r| {
                    let b = base.iter().find(|b| b.seed == r.seed)?;
                    Some(r.total_eur - b.total_eur)
                })
                .collect();
            let d = mean_ci(&delta);
            let disc = col(&|r| r.metrics.discomfort_kh);
            let unmet = col(&|r| r.metrics.ev_unmet_kwh);
            let short = col(&|r| r.metrics.cars_short as f64);
            let excess = col(&|r| r.metrics.dim_excess_kwh);
            let excess_s = col(&|r| r.metrics.dim_excess_s);
            let excess_max = rs.iter().map(|r| r.metrics.dim_excess_max_kw).fold(0.0, f64::max);
            let peak = col(&|r| r.metrics.peak_quarter_kw);
            let ms_mean = rs.iter().map(|r| r.metrics.solve_ms_mean).sum::<f64>() / rs.len() as f64;
            let ms_max = rs.iter().map(|r| r.metrics.solve_ms_max).fold(0.0, f64::max);
            let pm = |(m, ci): (f64, f64), digits: usize| format!("{m:.digits$} ± {ci:.digits$}");
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.2} | {:.3} · {:.0} · {:.1} | {} |\n",
                strategy.name(),
                pm(cost, 1),
                pm(deg, 2),
                pm(demand, 1),
                pm(total, 1),
                if strategy.name() == "rules" { "—".into() } else { pm(d, 1) },
                pm(peak, 0),
                pm(disc, 2),
                pm(unmet, 1),
                short.0,
                excess.0,
                excess_s.0,
                excess_max,
                if strategy.name() == "rules" { "—".into() } else { format!("{ms_mean:.0} / {ms_max:.0}") },
            ));
        }
        md.push('\n');
    }
    md.push_str("Values are means over the seeds ± the half-width of a 95% confidence interval of the mean (the maximum above the floor is the largest in any run). The dimming check allows 0.5 kW of tolerance and starts 60 s after a dimming begins. \"Δ total vs rules\" pairs each run with the rule-based run on the same weather. The peak charge is a Leistungspreis of 100 €/kW a year on the highest quarter-hour of import, of which the run carries its share of the year (hours / 8760) — the run stands for every day of its billing period.\n");
    std::fs::write(format!("{}/study.md", a.out), &md).unwrap();
    println!("{md}");
}

fn clock(h: f64) -> String {
    let min = (h * 60.0).round() as u32;
    format!("{:02}:{:02}", min / 60, min % 60)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}
