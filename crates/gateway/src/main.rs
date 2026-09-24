//! grid-edge-gateway: site controller between a DSO control centre
//! (IEC 60870-5-104) and the field devices of a prosumer site (Modbus TCP,
//! SunSpec), under German, Austrian or Swiss rules.
//! Usage: `gateway [path/to/gateway.toml]`.

mod api;
mod config;
mod dso;
mod field;
mod persist;
mod readings;
mod snapshot;
#[cfg(feature = "tls")]
mod tls;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use control::{Clock, Controller, Mode};
use iec104::Cp56Time2a;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

use crate::config::{Config, LinkLossPolicy};
use crate::field::{FieldState, Slot};
use crate::snapshot::{DsoView, Snapshot, TotalsView, now_ms};

/// How often the running totals are written to disk.
const PERSIST_EVERY: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let path = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "gateway.toml".into()));
    let cfg = match Config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            std::process::exit(2);
        }
    };
    // Config::load validated all of this.
    let site = cfg.site_config().expect("validated");
    let jurisdiction = cfg.jurisdiction().expect("validated").code();
    let period = Duration::from_millis(cfg.site.control_period_ms);
    let stale = Duration::from_millis(cfg.site.stale_after_ms);

    let state_file = path.with_file_name("dso-state.json");
    let saved = persist::load(&state_file);
    let initial = saved.commands;
    info!(
        "rules: {jurisdiction}; DSO commands at start: dim {}, feed-in limit {:.0}%, emergency {}",
        initial.dim, initial.feed_in_limit_pct, initial.emergency
    );
    let commands = Arc::new(watch::channel(initial).0);

    let mut controller = Controller::new(site.clone());
    if let Some(t) = saved.totals {
        controller = controller.with_totals(t);
    }
    info!("consumption floor while dimmed: {:.2} kW", controller.floor_kw());

    let field: field::Shared = Arc::new(Mutex::new(FieldState {
        inverters: vec![Slot::default(); cfg.inverters.len()],
        meter_kw: Slot::default(),
        chargers: vec![Slot::default(); cfg.chargers.len()],
        heat_pumps: vec![Slot::default(); cfg.heat_pumps.len()],
        batteries: vec![Slot::default(); cfg.batteries.len()],
    }));

    // Until the first cycle has run, devices get conservative setpoints.
    let initial_setpoints = control::Setpoints {
        pv_limit_pct: initial.feed_in_limit_pct.min(site.policy.static_feed_in_cap_pct.unwrap_or(100.0)),
        charger_current_a: cfg.chargers.iter().map(|c| c.failsafe_current_a).collect(),
        heat_pump_limit_kw: cfg.heat_pumps.iter().map(|h| 0.4 * h.rated_kw).collect(),
        battery_kw: vec![0.0; cfg.batteries.len()],
        heat_pump_ext_kw: vec![None; cfg.heat_pumps.len()],
    };
    let (setpoints_tx, setpoints_rx) = watch::channel(initial_setpoints);
    let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot::default());
    let (frames_tx, _) = broadcast::channel(512);
    let connections = Arc::new(AtomicUsize::new(0));

    field::spawn_all(&cfg, field.clone(), setpoints_rx, period);

    // DSO link
    let station = dso::Station {
        common_address: cfg.iec104.common_address,
        deadband_kw: cfg.iec104.deadband_kw,
        commands: commands.clone(),
        snapshot: snapshot_rx.clone(),
        frames: frames_tx.clone(),
        connections: connections.clone(),
    };
    let listener = TcpListener::bind(cfg.iec104.bind).await.unwrap_or_else(|e| {
        error!("IEC 104 bind {}: {e}", cfg.iec104.bind);
        std::process::exit(1)
    });
    spawn_iec104(listener, station, cfg.iec104.tls.clone());

    // Dashboard API
    let app = api::router(snapshot_rx, frames_tx, cfg.api.web_root.clone());
    let api_listener = TcpListener::bind(cfg.api.bind).await.unwrap_or_else(|e| {
        error!("API bind {}: {e}", cfg.api.bind);
        std::process::exit(1)
    });
    info!("dashboard API on http://{}", cfg.api.bind);
    tokio::spawn(async move { axum::serve(api_listener, app).await });

    // Control loop
    let t0 = Instant::now();
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_fallbacks = Vec::new();
    let mut last_refusals = Vec::new();
    let mut last_persist = Instant::now();
    let mut last_commands = initial;
    let mut link_seen = Instant::now();
    let mut overruns = 0u64;
    loop {
        tick.tick().await;
        let cycle_start = Instant::now();

        // Link-loss policy: without any control centre for too long, lift
        // the commands (an emergency stays until the DSO clears it).
        if connections.load(Ordering::SeqCst) > 0 {
            link_seen = Instant::now();
        } else if cfg.iec104.on_link_loss == LinkLossPolicy::Release
            && link_seen.elapsed() > Duration::from_secs(cfg.iec104.link_loss_release_s)
        {
            let c = *commands.borrow();
            if c.dim || c.feed_in_limit_pct < 100.0 {
                warn!("no DSO connection for {} s: lifting dim and feed-in limit", cfg.iec104.link_loss_release_s);
                commands.send_modify(|c| {
                    c.dim = false;
                    c.feed_in_limit_pct = 100.0;
                });
            }
        }

        let cmd = *commands.borrow();
        let (readings, views) = readings::collect(&field.lock().unwrap(), stale);
        let now = now_ms();
        let clock = Clock {
            t_s: t0.elapsed().as_secs_f64(),
            day: now.div_euclid(86_400_000),
            year: 2000 + Cp56Time2a::from_unix_ms(now).year as i32,
        };
        let (sp, st) = controller.step(&clock, &cmd, &readings);

        // Devices get one staleness window to answer before we complain.
        if st.fallbacks != last_fallbacks && t0.elapsed() > stale {
            warn!("fallbacks: {:?}", st.fallbacks);
            last_fallbacks = st.fallbacks.clone();
        }
        if st.refusals != last_refusals {
            if !st.refusals.is_empty() {
                warn!("DSO command not applied: {:?}", st.refusals);
            }
            last_refusals = st.refusals.clone();
        }
        if cmd != last_commands || last_persist.elapsed() > PERSIST_EVERY {
            persist::save(&state_file, &cmd, &st.totals);
            last_commands = cmd;
            last_persist = Instant::now();
        }

        let mut v = views;
        for (c, &a) in v.chargers.iter_mut().zip(&sp.charger_current_a) {
            c.setpoint_a = a;
        }
        for (h, &kw) in v.heat_pumps.iter_mut().zip(&sp.heat_pump_limit_kw) {
            h.limit_kw = kw;
        }
        for (b, &kw) in v.batteries.iter_mut().zip(&sp.battery_kw) {
            b.setpoint_kw = kw;
        }
        let cycle_ms = cycle_start.elapsed().as_secs_f64() * 1000.0;
        if cycle_ms > period.as_secs_f64() * 1000.0 {
            overruns += 1;
            if overruns.is_power_of_two() {
                warn!("control cycle took {cycle_ms:.0} ms, longer than the {period:?} period ({overruns} overruns)");
            }
        }
        let snap = Snapshot {
            time_ms: now,
            jurisdiction,
            dso: DsoView {
                dim: cmd.dim,
                feed_in_limit_pct: cmd.feed_in_limit_pct,
                emergency: cmd.emergency,
                connections: connections.load(Ordering::SeqCst),
            },
            mode: match st.mode {
                Mode::Normal => "normal",
                Mode::Dimmed => "dimmed",
                Mode::Releasing => "releasing",
            },
            grid_kw: readings.grid_kw,
            pv_kw: readings.pv_kw,
            pv_available_kw: readings.pv_available_kw,
            base_load_kw: st.base_load_kw,
            pv_surplus_kw: st.pv_surplus_kw,
            steuve_kw: st.steuve_kw,
            steuve_grid_kw: st.steuve_grid_kw,
            steuve_budget_kw: st.steuve_budget_kw,
            floor_kw: st.floor_kw,
            feed_in_limit_in_force_pct: st.feed_in_limit_pct,
            allowed_export_kw: st.allowed_export_kw,
            pv_limit_pct: sp.pv_limit_pct,
            inverters: v.inverters,
            chargers: v.chargers,
            heat_pumps: v.heat_pumps,
            batteries: v.batteries,
            totals: TotalsView {
                dimmed_min_today: st.totals.dimmed_s_today / 60.0,
                produced_kwh_year: st.totals.produced_kwh_year,
                curtailed_kwh_year: st.totals.curtailed_kwh_year,
                curtailment_budget_used_pct: st.curtailment_budget_used_pct,
            },
            refusals: st.refusals.iter().map(readings::refusal_name).collect(),
            fallbacks: st.fallbacks.iter().map(readings::fallback_name).collect(),
            cycle_ms,
        };
        setpoints_tx.send_replace(sp);
        snapshot_tx.send_replace(snap);
    }
}

fn spawn_iec104(listener: TcpListener, station: dso::Station, tls: Option<config::Tls>) {
    #[cfg(feature = "tls")]
    let acceptor = tls.map(|t| {
        tls::acceptor(&t).unwrap_or_else(|e| {
            error!("TLS: {e}");
            std::process::exit(1)
        })
    });
    #[cfg(feature = "tls")]
    let mode = if acceptor.is_some() { "TLS with client certificates" } else { "plain TCP" };
    #[cfg(not(feature = "tls"))]
    let mode = {
        if tls.is_some() {
            error!("this build has no TLS support (cargo feature \"tls\")");
            std::process::exit(1);
        }
        "plain TCP"
    };
    let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_default();
    info!("IEC 104 station on {addr} ({mode})");

    tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    warn!("IEC 104 accept: {e}");
                    continue;
                }
            };
            let _ = stream.set_nodelay(true);
            let station = station.clone();
            #[cfg(feature = "tls")]
            if let Some(acceptor) = acceptor.clone() {
                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(s) => station.serve(s, peer.to_string()).await,
                        Err(e) => warn!("TLS handshake with {peer} failed: {e}"),
                    }
                });
                continue;
            }
            tokio::spawn(station.serve(stream, peer.to_string()));
        }
    });
}
