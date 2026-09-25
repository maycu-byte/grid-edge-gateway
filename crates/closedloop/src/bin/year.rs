//! Every evening of 2025 on its real prices and weather: how often a feeder
//! of depots would overload, and what a §14a reduction and its rebound do on
//! those days, with the depots on rules or on the price-aware planner.
//!
//! ```text
//! cargo run --release -p closedloop --bin year -- [--sites 20] [--days 0-364] [--out docs/study/year2025]
//! ```
//!
//! Each day runs four cases for every site — rules and planner, each with no
//! reduction and with a reduction from 17:30 to 19:30 ended by the German
//! 5-minute ramp — with different van timetables per site. The same weather
//! applies to all sites: they are one neighbourhood. Writes `year.json` (per
//! day: the feeder in 5-minute steps per site, minutes over several
//! transformer sizes) and `year.md` (the summary).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use closedloop::feeder::{FROM_H, SAMPLE_S};
use closedloop::{FeederCase, Strategy, run_site};
use devices::climate::{Climate, Season};
use serde::Serialize;

const REDUCTION: (f64, f64) = (17.5, 19.5);
const RAMP_S: f64 = 300.0;
/// Transformer capacity per site the summary evaluates, kW.
const CAPS_KW: [u32; 7] = [60, 70, 80, 90, 100, 110, 120];
/// Resolution of the curves in year.json, samples.
const STEP: usize = 5;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const CASES: [(&str, &str, bool); 4] = [
    ("rules_base", "rules", false),
    ("rules_ramp", "rules", true),
    ("mpc_base", "mpc", false),
    ("mpc_ramp", "mpc", true),
];

struct Args {
    sites: usize,
    days: (u16, u16),
    out: String,
}

fn usage() -> ! {
    eprintln!("usage: year [--sites N] [--days FROM-TO] [--out DIR]   (days 0 = 1 January 2025)");
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args { sites: 20, days: (0, 364), out: "docs/study/year2025".into() };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let v = argv.get(i + 1).cloned().unwrap_or_else(|| usage());
        match argv[i].as_str() {
            "--sites" => a.sites = v.parse().unwrap_or_else(|_| usage()),
            "--days" => {
                let (x, y) = v.split_once('-').unwrap_or_else(|| usage());
                a.days = (x.parse().unwrap_or_else(|_| usage()), y.parse().unwrap_or_else(|_| usage()));
            }
            "--out" => a.out = v,
            _ => usage(),
        }
        i += 2;
    }
    if a.sites == 0 || a.days.0 > a.days.1 || a.days.1 > 364 {
        usage();
    }
    a
}

#[derive(Serialize, Clone)]
struct CaseOut {
    /// Feeder load per site, kW, 5-minute means from 16:30.
    load: Vec<f64>,
    /// Energy the vans left without, kWh, whole feeder.
    ev_kwh: f64,
    /// Energy cost 16:30–22:00, €, whole feeder.
    cost_eur: f64,
    /// Highest one-minute feeder load per site after the release, kW.
    peak_after: f64,
}

#[derive(Serialize, Clone)]
struct DayOut {
    date: String,
    temp_mean_c: f64,
    /// Sunshine that day: kWh per kWp the rooftop PV could make.
    pv_kwh_per_kwp: f64,
    price_min: f64,
    price_max: f64,
    /// Mean day-ahead price 17:00–20:00, €/MWh.
    price_evening: f64,
    rules_base: CaseOut,
    rules_ramp: CaseOut,
    mpc_base: CaseOut,
    mpc_ramp: CaseOut,
    /// Per capacity in CAPS_KW: minutes over the transformer after the
    /// release (19:30–22:00) in [rules_base, rules_ramp, mpc_base, mpc_ramp],
    /// and over the whole evening 16:30–22:00 in the same four cases.
    over_after: Vec<[u32; 4]>,
    over_evening: Vec<[u32; 4]>,
}

fn day_weather(d: u16) -> (f64, f64, f64, f64, f64) {
    let c = Climate::of(Season::Day(d));
    let hours: Vec<f64> = (0..24).map(|h| h as f64 * 3600.0 + 1800.0).collect();
    let temp = hours.iter().map(|&t| c.outdoor_c(t)).sum::<f64>() / 24.0;
    let pv = hours.iter().map(|&t| c.solar_fraction(t, 1.0)).sum::<f64>();
    let prices: Vec<f64> = (0..24).map(|h| c.day_ahead_eur_mwh(h as f64 * 3600.0 + 1.0)).collect();
    let min = prices.iter().cloned().fold(f64::MAX, f64::min);
    let max = prices.iter().cloned().fold(f64::MIN, f64::max);
    (temp, pv, min, max, prices[17..20].iter().sum::<f64>() / 3.0)
}

fn run_day(d: u16, sites: usize) -> DayOut {
    let mut loads: Vec<Vec<f64>> = Vec::new();
    let mut outs = Vec::new();
    let release = ((REDUCTION.1 - FROM_H) * 3600.0 / SAMPLE_S).round() as usize;
    for &(_, strategy, reduce) in &CASES {
        let case = FeederCase {
            season: Season::Day(d),
            strategy: Strategy::parse(strategy).expect("strategy"),
            ramp_s: RAMP_S,
            delay_max_s: 0.0,
            dim: reduce.then_some(REDUCTION),
            groups: 1,
            mixed: true,
        };
        let (mut sum, mut ev, mut cost) = (Vec::new(), 0.0, 0.0);
        for i in 0..sites {
            let r = run_site(&case, 1001 + i as u64, i);
            if sum.is_empty() {
                sum = vec![0.0; r.load_kw.len()];
            }
            for (s, v) in sum.iter_mut().zip(&r.load_kw) {
                *s += v / sites as f64;
            }
            ev += r.ev_unmet_kwh;
            cost += r.energy_cost_eur;
        }
        let load5 = sum.chunks(STEP).map(|c| (c.iter().sum::<f64>() / c.len() as f64 * 10.0).round() / 10.0).collect();
        let peak_after = sum[release..].iter().cloned().fold(f64::MIN, f64::max);
        outs.push(CaseOut { load: load5, ev_kwh: round2(ev), cost_eur: round2(cost), peak_after: round2(peak_after) });
        loads.push(sum);
    }
    let over = |l: &[f64], cap: u32| l.iter().filter(|&&v| v > cap as f64).count() as u32;
    let over_after = CAPS_KW.iter().map(|&c| [0, 1, 2, 3].map(|k| over(&loads[k][release..], c))).collect();
    let over_evening = CAPS_KW.iter().map(|&c| [0, 1, 2, 3].map(|k| over(&loads[k], c))).collect();
    let (temp, pv, pmin, pmax, peve) = day_weather(d);
    let mut it = outs.into_iter();
    DayOut {
        date: Season::Day(d).label(),
        temp_mean_c: round2(temp),
        pv_kwh_per_kwp: round2(pv),
        price_min: round2(pmin),
        price_max: round2(pmax),
        price_evening: round2(peve),
        rules_base: it.next().expect("case"),
        rules_ramp: it.next().expect("case"),
        mpc_base: it.next().expect("case"),
        mpc_ramp: it.next().expect("case"),
        over_after,
        over_evening,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[derive(Serialize)]
struct YearOut<'a> {
    sites: usize,
    from_h: f64,
    step_min: usize,
    reduction_h: (f64, f64),
    ramp_s: f64,
    caps_kw: &'a [u32],
    days: &'a [DayOut],
}

fn main() {
    let args = parse_args();
    let days: Vec<u16> = (args.days.0..=args.days.1).collect();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let started = Instant::now();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&d) = days.get(k) else { break };
                    let out = run_day(d, args.sites);
                    let mut r = results.lock().expect("results");
                    r.push(out);
                    eprintln!(
                        "{} done ({}/{}, {:.0} s)",
                        Season::Day(d).label(),
                        r.len(),
                        days.len(),
                        started.elapsed().as_secs_f64()
                    );
                }
            });
        }
    });
    let mut days_out = results.into_inner().expect("results");
    days_out.sort_by(|a, b| a.date.cmp(&b.date));
    std::fs::create_dir_all(&args.out).expect("create output directory");
    let year = YearOut {
        sites: args.sites,
        from_h: FROM_H,
        step_min: STEP,
        reduction_h: REDUCTION,
        ramp_s: RAMP_S,
        caps_kw: &CAPS_KW,
        days: &days_out,
    };
    std::fs::write(format!("{}/year.json", args.out), serde_json::to_string(&year).expect("json"))
        .expect("write year.json");
    std::fs::write(format!("{}/year.md", args.out), summary(&args, &days_out)).expect("write year.md");
    eprintln!("wrote {}/year.json and year.md in {:.0} s", args.out, started.elapsed().as_secs_f64());
}

fn summary(args: &Args, days: &[DayOut]) -> String {
    let mut md = String::new();
    md.push_str("# 2025, evening by evening\n\n");
    md.push_str(&format!(
        "{} depots on one feeder, each with its own van timetable, simulated on every evening from {} to {} with that day's real day-ahead prices (SMARD, DE-LU) and measured weather (Open-Meteo ERA5, Stuttgart). \
         Four cases per day: rules and planner, each without a reduction and with a §14a reduction from 17:30 to 19:30 ended by the 5-minute ramp. \
         A grid operator is assumed to reduce only on days the feeder would otherwise go over its transformer between 16:30 and 22:00. \
         *Rebound overload* = minutes over the transformer after 19:30 with the reduction, minus the same minutes without it.\n\n",
        args.sites,
        days.first().map_or("", |d| &d.date),
        days.last().map_or("", |d| &d.date),
    ));
    md.push_str("## How often, by transformer size\n\n");
    md.push_str("| Transformer per depot | Days the feeder would overload (rules) | … of which the reduction's rebound overloads it again (rules) | … (planner) | Rebound overload, minutes in the year (rules) | … (planner) | Days the planner alone keeps within the transformer |\n");
    md.push_str("|---|---|---|---|---|---|---|\n");
    for (k, cap) in CAPS_KW.iter().enumerate() {
        let need: Vec<&DayOut> = days.iter().filter(|d| d.over_evening[k][0] > 0).collect();
        let extra = |d: &DayOut, with: usize, base: usize| d.over_after[k][with].saturating_sub(d.over_after[k][base]);
        let rules_days = need.iter().filter(|d| extra(d, 1, 0) > 0).count();
        let mpc_days = need.iter().filter(|d| extra(d, 3, 2) > 0).count();
        let rules_min: u32 = need.iter().map(|d| extra(d, 1, 0)).sum();
        let mpc_min: u32 = need.iter().map(|d| extra(d, 3, 2)).sum();
        let planner_ok = need.iter().filter(|d| d.over_evening[k][2] == 0).count();
        md.push_str(&format!(
            "| {cap} kW | {} | {rules_days} | {mpc_days} | {rules_min} | {mpc_min} | {planner_ok} of {} |\n",
            need.len(),
            need.len()
        ));
    }
    md.push_str("\n## By month, 90 kW per depot\n\n");
    md.push_str("| Month | Mean temperature, °C | PV, kWh/kWp | Evening price, €/MWh | Days over (rules) | Rebound overload days (rules) | (planner) | Mean peak after release, kW/depot (rules → planner) |\n|---|---|---|---|---|---|---|---|\n");
    let k90 = CAPS_KW.iter().position(|&c| c == 90).expect("90 kW");
    for (m, name) in MONTHS.iter().enumerate() {
        let md_days: Vec<&DayOut> = days.iter().filter(|d| d.date[5..7].parse::<usize>().ok() == Some(m + 1)).collect();
        if md_days.is_empty() {
            continue;
        }
        let n = md_days.len() as f64;
        let mean = |f: &dyn Fn(&DayOut) -> f64| md_days.iter().map(|d| f(d)).sum::<f64>() / n;
        let over = md_days.iter().filter(|d| d.over_evening[k90][0] > 0).count();
        let rb = md_days
            .iter()
            .filter(|d| d.over_evening[k90][0] > 0 && d.over_after[k90][1] > d.over_after[k90][0])
            .count();
        let rbm = md_days
            .iter()
            .filter(|d| d.over_evening[k90][0] > 0 && d.over_after[k90][3] > d.over_after[k90][2])
            .count();
        md.push_str(&format!(
            "| {name} | {:.1} | {:.1} | {:.0} | {over} | {rb} | {rbm} | {:.0} → {:.0} |\n",
            mean(&|d| d.temp_mean_c),
            mean(&|d| d.pv_kwh_per_kwp),
            mean(&|d| d.price_evening),
            mean(&|d| d.rules_ramp.peak_after),
            mean(&|d| d.mpc_ramp.peak_after),
        ));
    }
    let ev = |f: &dyn Fn(&DayOut) -> f64| days.iter().map(f).sum::<f64>();
    md.push_str(&format!(
        "\nEnergy the vans left without over the year, whole feeder: rules {:.0} kWh without and {:.0} kWh with the daily reduction; planner {:.0} and {:.0} kWh.\n",
        ev(&|d| d.rules_base.ev_kwh),
        ev(&|d| d.rules_ramp.ev_kwh),
        ev(&|d| d.mpc_base.ev_kwh),
        ev(&|d| d.mpc_ramp.ev_kwh),
    ));
    md
}
