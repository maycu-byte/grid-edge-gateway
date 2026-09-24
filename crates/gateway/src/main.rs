//! grid-edge-gateway: site controller between a DSO control centre
//! (IEC 60870-5-104) and the field devices of a prosumer site (Modbus TCP,
//! SunSpec). Usage: `gateway [path/to/gateway.toml]`.

mod api;
mod config;
mod dso;
mod field;
mod snapshot;
#[cfg(feature = "tls")]
mod tls;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use control::{Controller, DsoCommands, Fallback, Mode, Readings};
use devices::maps::evse;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::field::{FieldState, Slot};
use crate::snapshot::{now_ms, ChargerView, DsoView, HeatPumpView, InverterView, Snapshot};

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
    let period = Duration::from_millis(cfg.site.control_period_ms);
    let stale = Duration::from_millis(cfg.site.stale_after_ms);

    // DSO commands survive a restart: a dimming that was active before a
    // power cut must still be active afterwards.
    let state_file = path.with_file_name("dso-state.json");
    let initial = load_commands(&state_file);
    info!("DSO commands at start: §14a dimming {}, feed-in limit {:.0}%", initial.dim_14a, initial.feed_in_limit_pct);
    let commands = Arc::new(watch::channel(initial).0);

    let mut controller = Controller::new(cfg.site_config());
    info!("Pmin,14a of this site: {:.2} kW", controller.pmin_kw());

    let field: field::Shared = Arc::new(Mutex::new(FieldState {
        inverters: vec![Slot::default(); cfg.inverters.len()],
        meter_kw: Slot::default(),
        chargers: vec![Slot::default(); cfg.chargers.len()],
        heat_pumps: vec![Slot::default(); cfg.heat_pumps.len()],
    }));

    // Until the first cycle has run, devices get conservative setpoints.
    let initial_setpoints = control::Setpoints {
        pv_limit_pct: initial.feed_in_limit_pct,
        charger_current_a: cfg.chargers.iter().map(|c| c.failsafe_current_a).collect(),
        heat_pump_limit_kw: cfg.heat_pumps.iter().map(|h| 0.4 * h.rated_kw).collect(),
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

    // Persist DSO commands on every change.
    {
        let mut rx = commands.subscribe();
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                let c = *rx.borrow_and_update();
                save_commands(&state_file, &c);
            }
        });
    }

    // Control loop
    let t0 = Instant::now();
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_fallbacks = Vec::new();
    loop {
        tick.tick().await;
        let cmd = *commands.borrow();
        let (readings, views) = collect(&field.lock().unwrap(), stale);
        let (sp, st) = controller.step(t0.elapsed().as_secs_f64(), &cmd, &readings);

        // Devices get one staleness window to answer before we complain.
        if st.fallbacks != last_fallbacks && t0.elapsed() > stale {
            warn!("fallbacks: {:?}", st.fallbacks);
            last_fallbacks = st.fallbacks.clone();
        }
        let (inverters, mut chargers, mut heat_pumps) = views;
        for (c, &a) in chargers.iter_mut().zip(&sp.charger_current_a) {
            c.setpoint_a = a;
        }
        for (h, &kw) in heat_pumps.iter_mut().zip(&sp.heat_pump_limit_kw) {
            h.limit_kw = kw;
        }
        let snap = Snapshot {
            time_ms: now_ms(),
            dso: DsoView {
                dim_14a: cmd.dim_14a,
                feed_in_limit_pct: cmd.feed_in_limit_pct,
                connections: connections.load(Ordering::SeqCst),
            },
            mode: match st.mode {
                Mode::Normal => "normal",
                Mode::Dimmed => "dimmed",
                Mode::Releasing => "releasing",
            },
            grid_kw: readings.grid_kw,
            pv_kw: readings.pv_kw,
            base_load_kw: st.base_load_kw,
            pv_surplus_kw: st.pv_surplus_kw,
            steuve_kw: st.steuve_kw,
            steuve_grid_kw: st.steuve_grid_kw,
            steuve_budget_kw: st.steuve_budget_kw,
            pmin_kw: st.pmin_kw,
            allowed_export_kw: st.allowed_export_kw,
            pv_limit_pct: sp.pv_limit_pct,
            inverters,
            chargers,
            heat_pumps,
            fallbacks: st.fallbacks.iter().map(fallback_name).collect(),
        };
        setpoints_tx.send_replace(sp);
        snapshot_tx.send_replace(snap);
    }
}

type Views = (Vec<InverterView>, Vec<ChargerView>, Vec<HeatPumpView>);

/// Turns the field state into controller readings, marking stale devices offline.
fn collect(f: &FieldState, stale: Duration) -> (Readings, Views) {
    let inverters: Vec<Option<field::InverterReading>> = f.inverters.iter().map(|s| s.fresh(stale)).collect();
    // The PV total is only known if every inverter answers.
    let pv_kw = inverters.iter().map(|r| r.as_ref().map(|r| r.kw)).sum::<Option<f64>>();
    let chargers: Vec<Option<field::ChargerReading>> = f.chargers.iter().map(|s| s.fresh(stale)).collect();
    let heat_pumps: Vec<Option<field::HeatPumpReading>> = f.heat_pumps.iter().map(|s| s.fresh(stale)).collect();

    let readings = Readings {
        grid_kw: f.meter_kw.fresh(stale),
        pv_kw,
        chargers: chargers
            .iter()
            .map(|c| match c {
                Some(c) => control::ChargerReading {
                    online: true,
                    car_waiting: matches!(c.status, evse::STATUS_CONNECTED | evse::STATUS_CHARGING | evse::STATUS_FAILSAFE),
                    current_a: c.current_a,
                    power_kw: c.kw,
                    session_kwh: c.session_kwh,
                },
                None => control::ChargerReading::default(),
            })
            .collect(),
        heat_pumps: heat_pumps
            .iter()
            .map(|h| match h {
                Some(h) => control::HeatPumpReading { online: true, power_kw: h.kw, demand_kw: h.demand_kw },
                None => control::HeatPumpReading::default(),
            })
            .collect(),
    };

    let views = (
        inverters
            .iter()
            .map(|r| InverterView {
                online: r.is_some(),
                kw: r.as_ref().map(|r| r.kw),
                rated_kw: r.as_ref().map(|r| r.rated_kw),
            })
            .collect(),
        chargers
            .iter()
            .map(|c| ChargerView {
                online: c.is_some(),
                status: match c.as_ref().map(|c| c.status) {
                    None => "offline",
                    Some(evse::STATUS_AVAILABLE) => "available",
                    Some(evse::STATUS_CONNECTED) => "waiting",
                    Some(evse::STATUS_CHARGING) => "charging",
                    Some(evse::STATUS_FAILSAFE) => "failsafe",
                    Some(evse::STATUS_FINISHED) => "finished",
                    Some(_) => "unknown",
                },
                current_a: c.as_ref().map(|c| c.current_a),
                setpoint_a: 0.0,
                kw: c.as_ref().map(|c| c.kw),
                session_kwh: c.as_ref().map(|c| c.session_kwh),
            })
            .collect(),
        heat_pumps
            .iter()
            .map(|h| HeatPumpView {
                online: h.is_some(),
                kw: h.as_ref().map(|h| h.kw),
                demand_kw: h.as_ref().map(|h| h.demand_kw),
                limit_kw: 0.0,
            })
            .collect(),
    );
    (readings, views)
}

fn fallback_name(f: &Fallback) -> String {
    match f {
        Fallback::MeterOffline => "meter offline".into(),
        Fallback::PvOffline => "inverter offline".into(),
        Fallback::ChargerOffline(i) => format!("charger{i} offline"),
        Fallback::HeatPumpOffline(i) => format!("heatpump{i} offline"),
    }
}

fn load_commands(path: &Path) -> DsoCommands {
    let parsed = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    match parsed {
        Some(v) => DsoCommands {
            dim_14a: v["dim_14a"].as_bool().unwrap_or(false),
            feed_in_limit_pct: v["feed_in_limit_pct"].as_f64().unwrap_or(100.0).clamp(0.0, 100.0),
        },
        None => DsoCommands::default(),
    }
}

fn save_commands(path: &Path, c: &DsoCommands) {
    let text = serde_json::json!({ "dim_14a": c.dim_14a, "feed_in_limit_pct": c.feed_in_limit_pct }).to_string();
    // Write-then-rename so a power cut never leaves a half-written file.
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("could not persist DSO commands: {e}");
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
