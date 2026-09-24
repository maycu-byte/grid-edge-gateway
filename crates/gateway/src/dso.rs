//! The DSO link: an IEC 60870-5-104 controlled station.
//!
//! Point list (common address from the config, default 1). Commands:
//!
//! | IOA  | Type      | Meaning                                                     |
//! |------|-----------|-------------------------------------------------------------|
//! | 5001 | C_SC_NA_1 | reduce consumption ON / OFF (§14a in DE, contract in AT/CH) |
//! | 5002 | C_SE_NC_1 | feed-in limit, % of installed PV (0–100)                    |
//! | 5003 | C_SC_NA_1 | emergency: immediate serious threat ON / OFF                |
//!
//! Monitoring (site → DSO), M_ME_TF_1 unless noted:
//!
//! | IOA  | Meaning                                                           |
//! |------|-------------------------------------------------------------------|
//! | 1001 | active power at the grid connection, kW (+ import)                |
//! | 1002 | PV active power, kW                                               |
//! | 1003 | controllable devices (steuVE) power, kW                           |
//! | 1004 | steuVE power drawn from the grid, kW                              |
//! | 1005 | consumption floor while dimmed, kW (Pmin,14a in DE)               |
//! | 1006 | feed-in limit commanded, % (feedback of 5002)                     |
//! | 1007 | PV limit sent to the inverters, %                                 |
//! | 1008 | feed-in limit in force after country rules, %                     |
//! | 1009 | battery power, kW (+ charging)                                    |
//! | 1010 | battery state of charge, %                                        |
//! | 1011 | consumption dimmed today, minutes                                 |
//! | 1012 | PV energy curtailed this year, kWh                                |
//! | 1013 | free curtailment budget used, % (CH)                              |
//! | 2001 | M_SP_TB_1: consumption dimming active (feedback of 5001)          |
//! | 2002 | M_SP_TB_1: gradual release in progress                            |
//! | 2003 | M_SP_TB_1: meter fallback (grid measurement lost)                 |
//! | 2004 | M_SP_TB_1: field device fault                                     |
//! | 2005 | M_SP_TB_1: emergency active (feedback of 5003)                    |
//! | 2006 | M_SP_TB_1: contract day limit reached, dimming refused            |
//! | 2007 | M_SP_TB_1: curtailment budget used up                             |
//!
//! Measurements go out spontaneously when they move by more than the
//! deadband, and all points in a general interrogation; always with a
//! CP56Time2a time tag, so every point has exactly one type.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use iec104::asdu::{AsduError, mirror_raw};
use iec104::connection::{self, Direction, Event};
use iec104::describe::describe;
use iec104::{Asdu, Cause, Cp56Time2a, Element, InformationObject, Quality};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{info, warn};

use crate::snapshot::{FrameLog, Snapshot, now_ms};

pub const IOA_DIM: u32 = 5001;
pub const IOA_FEED_IN_LIMIT: u32 = 5002;
pub const IOA_EMERGENCY: u32 = 5003;

#[derive(Clone)]
pub struct Station {
    pub common_address: u16,
    pub deadband_kw: f64,
    pub commands: Arc<watch::Sender<control::DsoCommands>>,
    pub snapshot: watch::Receiver<Snapshot>,
    pub frames: broadcast::Sender<FrameLog>,
    pub connections: Arc<AtomicUsize>,
}

/// Value of every monitored point, `None` = not available (sent as invalid).
fn measurements(s: &Snapshot) -> Vec<(u32, Option<f64>)> {
    let battery_kw = (!s.batteries.is_empty()).then(|| s.batteries.iter().map(|b| b.kw).sum::<Option<f64>>()).flatten();
    let soc = s.batteries.first().and_then(|b| b.soc_pct);
    vec![
        (1001, s.grid_kw),
        (1002, s.pv_kw),
        (1003, Some(s.steuve_kw)),
        (1004, s.steuve_grid_kw),
        (1005, Some(s.floor_kw)),
        (1006, Some(s.dso.feed_in_limit_pct)),
        (1007, Some(s.pv_limit_pct)),
        (1008, Some(s.feed_in_limit_in_force_pct)),
        (1009, battery_kw),
        (1010, soc),
        (1011, Some(s.totals.dimmed_min_today)),
        (1012, Some(s.totals.curtailed_kwh_year)),
        (1013, s.totals.curtailment_budget_used_pct),
    ]
}

fn single_points(s: &Snapshot) -> Vec<(u32, bool)> {
    vec![
        (2001, s.mode == "dimmed"),
        (2002, s.mode == "releasing"),
        (2003, s.fallbacks.iter().any(|f| f.starts_with("meter"))),
        (2004, !s.fallbacks.is_empty()),
        (2005, s.dso.emergency),
        (2006, s.refusals.contains(&"contract day limit reached")),
        (2007, s.refusals.contains(&"curtailment budget used up")),
    ]
}

/// Points whose deadband is 1 unit of their own (%, minutes, kWh) rather than kW.
fn deadband_in_units(ioa: u32) -> bool {
    matches!(ioa, 1006..=1008 | 1010..=1013)
}

fn float_element(v: Option<f64>, time: Cp56Time2a) -> Element {
    match v {
        Some(v) => Element::FloatTime { value: v as f32, quality: Quality::GOOD, time },
        None => Element::FloatTime { value: 0.0, quality: Quality::invalid(), time },
    }
}

impl Station {
    /// Serves one control-centre connection until it closes.
    pub async fn serve<S>(self, stream: S, peer: String)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let n = self.connections.fetch_add(1, Ordering::SeqCst) + 1;
        info!("DSO {peer} connected ({n} connection(s))");
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(512);
        let (ev_tx, mut ev_rx) = mpsc::channel::<Event>(512);
        let frames = self.frames.clone();
        let tap_peer = peer.clone();
        let link =
            tokio::spawn(connection::run(stream, iec104::Config::default(), out_rx, ev_tx, move |dir, bytes| {
                let _ = frames.send(FrameLog {
                    time_ms: now_ms(),
                    peer: tap_peer.clone(),
                    dir: if dir == Direction::Rx { "rx" } else { "tx" },
                    hex: bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
                    text: describe(bytes),
                });
            }));

        let mut snapshot = self.snapshot.clone();
        let mut started = false;
        let mut reported_f: HashMap<u32, Option<f64>> = HashMap::new();
        let mut reported_b: HashMap<u32, bool> = HashMap::new();
        let mut selected: HashMap<u32, std::time::Instant> = HashMap::new();

        loop {
            tokio::select! {
                ev = ev_rx.recv() => match ev {
                    None => break,
                    Some(Event::Started) => {
                        started = true;
                        let eoi = Asdu::single(Cause::Initialized, self.common_address, 0, Element::EndOfInit { coi: 0 });
                        let _ = out_tx.send(eoi.encode()).await;
                    }
                    Some(Event::Stopped) => started = false,
                    Some(Event::Asdu(Ok(asdu))) => {
                        for reply in self.handle(&asdu, &mut selected, &mut reported_f, &mut reported_b) {
                            let _ = out_tx.send(reply).await;
                        }
                    }
                    Some(Event::Asdu(Err(AsduError::UnsupportedType { type_id, raw }))) => {
                        warn!("DSO {peer}: unsupported type {type_id}");
                        let _ = out_tx.send(mirror_raw(&raw, Cause::UnknownType)).await;
                    }
                    Some(Event::Asdu(Err(AsduError::UnsupportedCause { raw }))) => {
                        let _ = out_tx.send(mirror_raw(&raw, Cause::UnknownCause)).await;
                    }
                    Some(Event::Asdu(Err(AsduError::Truncated))) => warn!("DSO {peer}: truncated ASDU ignored"),
                },
                changed = snapshot.changed() => {
                    if changed.is_err() { break; }
                    if started {
                        let s = snapshot.borrow_and_update().clone();
                        for asdu in self.spontaneous(&s, &mut reported_f, &mut reported_b) {
                            let _ = out_tx.send(asdu).await;
                        }
                    }
                }
            }
        }
        drop(out_tx);
        let ended = link.await;
        let n = self.connections.fetch_sub(1, Ordering::SeqCst) - 1;
        info!("DSO {peer} disconnected: {:?} ({n} connection(s) left)", ended.ok());
    }

    fn handle(
        &self,
        a: &Asdu,
        selected: &mut HashMap<u32, std::time::Instant>,
        reported_f: &mut HashMap<u32, Option<f64>>,
        reported_b: &mut HashMap<u32, bool>,
    ) -> Vec<Vec<u8>> {
        let broadcast_ok = matches!(a.type_id, 100 | 103);
        if a.common_address != self.common_address && !(broadcast_ok && a.common_address == 0xFFFF) {
            return vec![a.mirror(Cause::UnknownCommonAddress, true).encode()];
        }
        let Some(obj) = a.objects.first().copied() else { return vec![] };
        match obj.element {
            Element::Interrogation { .. } => {
                if a.cause != Cause::Activation {
                    return vec![a.mirror(Cause::UnknownCause, true).encode()];
                }
                let s = self.snapshot.borrow().clone();
                let mut out = vec![a.mirror(Cause::ActivationCon, false).encode()];
                out.extend(self.interrogation_response(&s));
                for (ioa, v) in measurements(&s) {
                    reported_f.insert(ioa, v);
                }
                for (ioa, v) in single_points(&s) {
                    reported_b.insert(ioa, v);
                }
                out.push(a.mirror(Cause::ActivationTermination, false).encode());
                out
            }
            Element::ClockSync { time } => {
                // The gateway keeps its own NTP-disciplined clock; the offset is only logged.
                let offset = now_ms() - time.to_unix_ms();
                info!("clock sync from DSO, local offset {offset} ms");
                let reply = Asdu::single(
                    Cause::ActivationCon,
                    self.common_address,
                    obj.ioa,
                    Element::ClockSync { time: Cp56Time2a::from_unix_ms(now_ms()) },
                );
                vec![reply.encode()]
            }
            Element::SingleCommand { on, select, .. } if obj.ioa == IOA_DIM => {
                self.command(a, obj, select, selected, |c| c.dim = on)
            }
            Element::SingleCommand { on, select, .. } if obj.ioa == IOA_EMERGENCY => {
                self.command(a, obj, select, selected, |c| c.emergency = on)
            }
            Element::SetpointFloat { value, select, .. } if obj.ioa == IOA_FEED_IN_LIMIT => {
                if !(0.0..=100.0).contains(&value) || value.is_nan() {
                    return vec![a.mirror(Cause::ActivationCon, true).encode()];
                }
                self.command(a, obj, select, selected, |c| c.feed_in_limit_pct = value as f64)
            }
            Element::SingleCommand { .. } | Element::SetpointFloat { .. } => {
                vec![a.mirror(Cause::UnknownObjectAddress, true).encode()]
            }
            // Monitoring types have no business in the control direction.
            _ => vec![a.mirror(Cause::UnknownType, true).encode()],
        }
    }

    /// Direct execute, or select-before-operate when the DSO uses it: a
    /// select is confirmed and must be followed by the execute within 10 s.
    fn command(
        &self,
        a: &Asdu,
        obj: InformationObject,
        select: bool,
        selected: &mut HashMap<u32, std::time::Instant>,
        apply: impl FnOnce(&mut control::DsoCommands),
    ) -> Vec<Vec<u8>> {
        match a.cause {
            Cause::Activation if select => {
                selected.insert(obj.ioa, std::time::Instant::now());
                vec![a.mirror(Cause::ActivationCon, false).encode()]
            }
            Cause::Activation => {
                let was_selected = selected.remove(&obj.ioa);
                if was_selected.is_some_and(|t| t.elapsed().as_secs() > 10) {
                    return vec![a.mirror(Cause::ActivationCon, true).encode()];
                }
                self.commands.send_modify(apply);
                let c = *self.commands.borrow();
                info!(
                    "DSO command: dim {}, feed-in limit {:.0}%, emergency {}",
                    c.dim, c.feed_in_limit_pct, c.emergency
                );
                vec![
                    a.mirror(Cause::ActivationCon, false).encode(),
                    a.mirror(Cause::ActivationTermination, false).encode(),
                ]
            }
            Cause::Deactivation => {
                selected.remove(&obj.ioa);
                vec![a.mirror(Cause::DeactivationCon, false).encode()]
            }
            _ => vec![a.mirror(Cause::UnknownCause, true).encode()],
        }
    }

    /// General interrogation answer. Points keep one type each (the
    /// time-tagged one), as most control-centre point databases expect.
    fn interrogation_response(&self, s: &Snapshot) -> Vec<Vec<u8>> {
        let time = Cp56Time2a::from_unix_ms(s.time_ms);
        let floats = Asdu {
            type_id: 36,
            cause: Cause::Interrogated,
            negative: false,
            test: false,
            originator: 0,
            common_address: self.common_address,
            objects: measurements(s)
                .into_iter()
                .map(|(ioa, v)| InformationObject { ioa, element: float_element(v, time) })
                .collect(),
        };
        let points = Asdu {
            type_id: 30,
            objects: single_points(s)
                .into_iter()
                .map(|(ioa, on)| InformationObject {
                    ioa,
                    element: Element::SinglePointTime { on, quality: Quality::GOOD, time },
                })
                .collect(),
            ..floats.clone()
        };
        vec![floats.encode(), points.encode()]
    }

    fn spontaneous(
        &self,
        s: &Snapshot,
        reported_f: &mut HashMap<u32, Option<f64>>,
        reported_b: &mut HashMap<u32, bool>,
    ) -> Vec<Vec<u8>> {
        let time = Cp56Time2a::from_unix_ms(s.time_ms);
        let mut out = Vec::new();
        for (ioa, v) in measurements(s) {
            let deadband = if deadband_in_units(ioa) { 1.0 } else { self.deadband_kw };
            let moved = match (reported_f.get(&ioa).copied().flatten(), v) {
                (Some(old), Some(new)) => (new - old).abs() >= deadband,
                (None, None) => false,
                _ => true,
            };
            if moved || !reported_f.contains_key(&ioa) {
                reported_f.insert(ioa, v);
                out.push(Asdu::single(Cause::Spontaneous, self.common_address, ioa, float_element(v, time)).encode());
            }
        }
        for (ioa, on) in single_points(s) {
            if reported_b.get(&ioa) != Some(&on) {
                reported_b.insert(ioa, on);
                let e = Element::SinglePointTime { on, quality: Quality::GOOD, time };
                out.push(Asdu::single(Cause::Spontaneous, self.common_address, ioa, e).encode());
            }
        }
        out
    }
}
