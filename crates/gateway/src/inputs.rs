//! The other two ways a DSO's §14a signal reaches a German site, next to
//! IEC 104: the FNN control box's relay contact and EEBUS LPC.
//!
//! - **Relay.** The control box closes (or opens) a potential-free contact
//!   to demand a reduction to Pmin,14a. The contact is wired to a Modbus TCP
//!   I/O module and read as a discrete input (function 02).
//! - **EEBUS LPC.** The control box, as energy guard, writes a consumption
//!   limit in W. `eebus-bridge` (Go, on eebus-go) keeps the EEBUS side,
//!   including the heartbeat and the failsafe state, and passes the result
//!   on as one JSON line per change and at least every few seconds:
//!   `{"active":true,"limit_w":4200,"failsafe":false,"failsafe_limit_w":4200}`.
//!
//! Each input keeps its own state; [`merge`] combines them with the IEC 104
//! commands every control cycle.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use control::DsoCommands;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_modbus::Slave;
use tokio_modbus::client::Reader;
use tracing::{error, info, warn};

use crate::config::{Contact, ControlBoxRelay, Eebus, InputLossPolicy};

const RELAY_POLL: Duration = Duration::from_millis(200);
const IO_TIMEOUT: Duration = Duration::from_millis(800);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// Longest JSON line accepted from the bridge.
const MAX_LINE: usize = 1024;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct RelayView {
    /// The contact demands a reduction (after debouncing).
    pub active: bool,
    /// The I/O module answers.
    pub readable: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct EebusView {
    pub connected: bool,
    pub active: bool,
    pub limit_kw: Option<f64>,
    /// The failsafe limit applies: the energy guard's heartbeat, or the
    /// bridge itself, has been lost.
    pub failsafe: bool,
    /// Failsafe limit last announced by the bridge, kW.
    pub failsafe_limit_kw: Option<f64>,
}

/// State of the configured inputs (`None` = not configured).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct InputState {
    pub relay: Option<RelayView>,
    pub eebus: Option<EebusView>,
}

pub type Shared = Arc<Mutex<InputState>>;

/// The commands in force: IEC 104 and the inputs together. Any plain
/// reduction (IEC 104, relay) dims to Pmin; an EEBUS limit alone dims to
/// its own value, which the controller never lets fall below Pmin.
pub fn merge(dso: DsoCommands, inputs: &InputState) -> DsoCommands {
    let plain = dso.dim || inputs.relay.as_ref().is_some_and(|r| r.active);
    let lpc = inputs.eebus.as_ref().filter(|e| e.active);
    DsoCommands {
        dim: plain || lpc.is_some(),
        limit_kw: if plain { None } else { lpc.and_then(|e| e.limit_kw) },
        ..dso
    }
}

/// Which sources demand a reduction right now.
pub fn sources(dso: &DsoCommands, inputs: &InputState) -> Vec<&'static str> {
    let mut out = Vec::new();
    if dso.dim {
        out.push("iec104");
    }
    if inputs.relay.as_ref().is_some_and(|r| r.active) {
        out.push("relay");
    }
    if inputs.eebus.as_ref().is_some_and(|e| e.active) {
        out.push("eebus");
    }
    out
}

/// Input problems, in the same list as the field devices' fallbacks.
pub fn fallbacks(inputs: &InputState) -> Vec<String> {
    let mut out = Vec::new();
    if inputs.relay.as_ref().is_some_and(|r| !r.readable) {
        out.push("control box relay unreadable".into());
    }
    if let Some(e) = &inputs.eebus {
        if !e.connected {
            out.push("eebus bridge lost".into());
        } else if e.failsafe {
            out.push("eebus failsafe".into());
        }
    }
    out
}

// --- relay ---------------------------------------------------------------

/// A contact state counts once it has held for `hold`.
#[derive(Debug)]
pub struct Debounce {
    hold: Duration,
    stable: bool,
    candidate: Option<(bool, Instant)>,
}

impl Debounce {
    pub fn new(hold: Duration) -> Self {
        Debounce { hold, stable: false, candidate: None }
    }

    pub fn update(&mut self, raw: bool, now: Instant) -> bool {
        if raw == self.stable {
            self.candidate = None;
        } else {
            match self.candidate {
                Some((v, since)) if v == raw => {
                    if now.duration_since(since) >= self.hold {
                        self.stable = raw;
                        self.candidate = None;
                    }
                }
                _ => self.candidate = Some((raw, now)),
            }
        }
        self.stable
    }
}

pub fn spawn_relay(cfg: ControlBoxRelay, shared: Shared) {
    shared.lock().unwrap().relay = Some(RelayView::default());
    tokio::spawn(run_relay(cfg, shared));
}

async fn run_relay(cfg: ControlBoxRelay, shared: Shared) {
    let mut debounce = Debounce::new(Duration::from_millis(cfg.debounce_ms));
    let mut last_ok = Instant::now();
    let mut ever_read = false;
    let lost_after = Duration::from_secs(cfg.input_loss_s);
    info!("control box relay on {} input {}", cfg.address, cfg.input);
    loop {
        let err = match timeout(IO_TIMEOUT * 2, tokio_modbus::client::tcp::connect_slave(cfg.address, Slave(cfg.unit)))
            .await
        {
            Err(_) => "connect: timeout".to_string(),
            Ok(Err(e)) => format!("connect: {e}"),
            Ok(Ok(mut ctx)) => loop {
                match timeout(IO_TIMEOUT, ctx.read_discrete_inputs(cfg.input, 1)).await {
                    Ok(Ok(Ok(bits))) if !bits.is_empty() => {
                        let active = bits[0] == (cfg.active_when == Contact::Closed);
                        let now = Instant::now();
                        let stable = debounce.update(active, now);
                        let view = RelayView { active: stable, readable: true };
                        let mut s = shared.lock().unwrap();
                        if s.relay.as_ref() != Some(&view) {
                            info!("control box relay: reduction {}", if stable { "ON" } else { "OFF" });
                        }
                        s.relay = Some(view);
                        last_ok = now;
                        ever_read = true;
                    }
                    Err(_) => break "read: timeout".to_string(),
                    Ok(Err(e)) => break format!("read: {e}"),
                    Ok(Ok(Err(x))) => break format!("read: exception {x:?}"),
                    Ok(Ok(Ok(_))) => break "read: empty answer".to_string(),
                }
                tokio::time::sleep(RELAY_POLL).await;
            },
        };
        if last_ok.elapsed() >= lost_after || !ever_read {
            let mut s = shared.lock().unwrap();
            let held = s.relay.as_ref().is_some_and(|r| r.active);
            let active = match cfg.on_input_loss {
                InputLossPolicy::Hold => held,
                InputLossPolicy::Dim => true,
            };
            if s.relay.as_ref().is_some_and(|r| r.readable) {
                warn!("control box relay lost ({err}): reduction {}", if active { "ON" } else { "OFF" });
            }
            s.relay = Some(RelayView { active, readable: false });
        }
        tokio::time::sleep(RECONNECT_DELAY.min(lost_after)).await;
    }
}

// --- EEBUS bridge ----------------------------------------------------------

/// One line from `eebus-bridge`.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BridgeMessage {
    pub active: bool,
    pub limit_w: Option<f64>,
    #[serde(default)]
    pub failsafe: bool,
    pub failsafe_limit_w: Option<f64>,
}

fn watts_to_kw(w: Option<f64>) -> Result<Option<f64>, String> {
    match w {
        None => Ok(None),
        Some(w) if w.is_finite() && w >= 0.0 => Ok(Some(w / 1000.0)),
        Some(w) => Err(format!("limit {w} W is not a power")),
    }
}

/// The state after a message from the bridge.
pub fn on_message(line: &str) -> Result<EebusView, String> {
    let m: BridgeMessage = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let limit_kw = watts_to_kw(m.limit_w)?;
    if m.active && limit_kw.is_none() {
        return Err("an active limit needs limit_w".into());
    }
    Ok(EebusView {
        connected: true,
        active: m.active,
        limit_kw,
        failsafe: m.failsafe,
        failsafe_limit_kw: watts_to_kw(m.failsafe_limit_w)?,
    })
}

/// The state once the bridge is gone: the failsafe limit it last announced,
/// as LPC prescribes when the energy guard falls silent (Pmin if none).
pub fn on_lost(last: Option<&EebusView>) -> EebusView {
    let failsafe_limit_kw = last.and_then(|e| e.failsafe_limit_kw);
    EebusView { connected: false, active: true, limit_kw: failsafe_limit_kw, failsafe: true, failsafe_limit_kw }
}

pub async fn spawn_eebus(cfg: Eebus, shared: Shared) {
    let listener = TcpListener::bind(cfg.bind).await.unwrap_or_else(|e| {
        error!("EEBUS bridge bind {}: {e}", cfg.bind);
        std::process::exit(1)
    });
    shared.lock().unwrap().eebus = Some(EebusView::default());
    tokio::spawn(run_eebus(listener, Duration::from_secs(cfg.timeout_s), shared));
}

async fn run_eebus(listener: TcpListener, silence: Duration, shared: Shared) {
    info!("waiting for eebus-bridge on {}", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
    loop {
        // Until a bridge connects (and between connections) the silence
        // rule applies as well.
        let accepted = match timeout(silence, listener.accept()).await {
            Ok(Ok((stream, peer))) => Some((stream, peer)),
            Ok(Err(e)) => {
                warn!("EEBUS bridge accept: {e}");
                None
            }
            Err(_) => None,
        };
        let Some((stream, peer)) = accepted else {
            lost(&shared, "no bridge connected");
            continue;
        };
        serve_bridge(stream, peer, silence, &shared).await;
        lost(&shared, "bridge disconnected");
    }
}

async fn serve_bridge(stream: tokio::net::TcpStream, peer: SocketAddr, silence: Duration, shared: &Shared) {
    info!("eebus-bridge connected from {peer}");
    let mut lines = BufReader::new(stream).lines();
    loop {
        let line = match timeout(silence, lines.next_line()).await {
            Err(_) => {
                warn!("eebus-bridge silent for {silence:?}");
                return;
            }
            Ok(Err(e)) => {
                warn!("eebus-bridge: {e}");
                return;
            }
            Ok(Ok(None)) => return,
            Ok(Ok(Some(l))) => l,
        };
        if line.len() > MAX_LINE {
            warn!("eebus-bridge: line of {} bytes dropped", line.len());
            return;
        }
        match on_message(&line) {
            Ok(view) => {
                let mut s = shared.lock().unwrap();
                if s.eebus.as_ref().is_none_or(|old| old.active != view.active || old.limit_kw != view.limit_kw) {
                    info!(
                        "EEBUS LPC: limit {} {:?} kW{}",
                        if view.active { "ON" } else { "OFF" },
                        view.limit_kw,
                        if view.failsafe { " (failsafe)" } else { "" }
                    );
                }
                s.eebus = Some(view);
            }
            Err(e) => warn!("eebus-bridge: {e}: {line}"),
        }
    }
}

fn lost(shared: &Shared, why: &str) {
    let mut s = shared.lock().unwrap();
    let next = on_lost(s.eebus.as_ref());
    if s.eebus.as_ref().is_none_or(|e| e.connected || !e.failsafe) {
        warn!("EEBUS: {why}, failsafe limit {:?} kW (Pmin if none)", next.limit_kw);
    }
    s.eebus = Some(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    fn dso(dim: bool) -> DsoCommands {
        DsoCommands { dim, ..Default::default() }
    }

    fn lpc(active: bool, kw: f64) -> Option<EebusView> {
        Some(EebusView { connected: true, active, limit_kw: Some(kw), ..Default::default() })
    }

    #[test]
    fn any_source_dims_and_a_plain_reduction_wins_over_the_lpc_limit() {
        let none = InputState::default();
        assert_eq!(merge(dso(false), &none), dso(false));

        let relay = InputState { relay: Some(RelayView { active: true, readable: true }), eebus: None };
        assert!(merge(dso(false), &relay).dim);
        assert_eq!(sources(&dso(false), &relay), ["relay"]);

        let only_lpc = InputState { relay: None, eebus: lpc(true, 11.0) };
        let m = merge(dso(false), &only_lpc);
        assert!(m.dim);
        assert_eq!(m.limit_kw, Some(11.0));

        // IEC 104 dims to Pmin at the same time: Pmin is the tighter one.
        let both = merge(dso(true), &only_lpc);
        assert_eq!(both.limit_kw, None);
        assert_eq!(sources(&dso(true), &only_lpc), ["iec104", "eebus"]);

        // An inactive LPC limit changes nothing; the feed-in limit is kept.
        let off = InputState { relay: None, eebus: lpc(false, 11.0) };
        let cmd = DsoCommands { feed_in_limit_pct: 60.0, ..Default::default() };
        assert_eq!(merge(cmd, &off), cmd);
    }

    #[test]
    fn a_contact_counts_only_once_it_has_held() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut d = Debounce::new(Duration::from_millis(500));
        assert!(!d.update(true, ms(0)));
        assert!(!d.update(false, ms(100)), "a 100 ms bounce is ignored");
        assert!(!d.update(true, ms(200)));
        assert!(!d.update(true, ms(600)));
        assert!(d.update(true, ms(700)), "held for 500 ms");
        assert!(d.update(false, ms(800)));
        assert!(!d.update(false, ms(1300)));
    }

    #[test]
    fn bridge_messages_are_checked() {
        let v = on_message(r#"{"active":true,"limit_w":4200,"failsafe":false,"failsafe_limit_w":6000}"#).unwrap();
        assert_eq!((v.active, v.limit_kw, v.failsafe_limit_kw), (true, Some(4.2), Some(6.0)));
        assert!(on_message(r#"{"active":true}"#).is_err(), "an active limit needs a value");
        assert!(on_message(r#"{"active":true,"limit_w":-5}"#).is_err());
        assert!(on_message(r#"{"active":false,"limit_w":null,"command":"open"}"#).is_err(), "unknown fields");
        assert!(on_message("not json").is_err());
    }

    #[test]
    fn a_lost_bridge_applies_the_last_failsafe_limit() {
        let last = on_message(r#"{"active":false,"limit_w":null,"failsafe_limit_w":6000}"#).unwrap();
        let v = on_lost(Some(&last));
        assert!(v.active && v.failsafe && !v.connected);
        assert_eq!(v.limit_kw, Some(6.0));
        assert_eq!(on_lost(None).limit_kw, None, "no failsafe value known: Pmin");
        let s = InputState { relay: None, eebus: Some(v) };
        assert_eq!(fallbacks(&s), ["eebus bridge lost"]);
    }

    async fn wait_for(shared: &Shared, want: impl Fn(&InputState) -> bool) -> InputState {
        for _ in 0..100 {
            let s = shared.lock().unwrap().clone();
            if want(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("state not reached: {:?}", shared.lock().unwrap());
    }

    #[tokio::test]
    async fn the_bridge_socket_sets_and_loses_the_limit() {
        let shared: Shared = Arc::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        shared.lock().unwrap().eebus = Some(EebusView::default());
        tokio::spawn(run_eebus(listener, Duration::from_secs(1), shared.clone()));

        let mut bridge = TcpStream::connect(addr).await.unwrap();
        bridge.write_all(b"{\"active\":true,\"limit_w\":11000,\"failsafe_limit_w\":4200}\n").await.unwrap();
        let s = wait_for(&shared, |s| s.eebus.as_ref().is_some_and(|e| e.active && e.connected)).await;
        assert_eq!(s.eebus.unwrap().limit_kw, Some(11.0));

        // The bridge goes silent: failsafe with its last failsafe value.
        let s = wait_for(&shared, |s| s.eebus.as_ref().is_some_and(|e| !e.connected)).await;
        let e = s.eebus.unwrap();
        assert!(e.failsafe && e.active);
        assert_eq!(e.limit_kw, Some(4.2));
        drop(bridge);
    }

    /// A Modbus TCP I/O module with one discrete input, answering function 02.
    async fn io_module(contact: Arc<Mutex<bool>>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let contact = contact.clone();
                tokio::spawn(async move {
                    let mut req = [0u8; 12];
                    while s.read_exact(&mut req).await.is_ok() {
                        assert_eq!(req[7], 0x02, "function 02");
                        let bit = u8::from(*contact.lock().unwrap());
                        // MBAP: same transaction id, protocol 0, length 4, unit; then fc, byte count, bits.
                        let resp = [req[0], req[1], 0, 0, 0, 4, req[6], 0x02, 1, bit];
                        if s.write_all(&resp).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn the_relay_is_read_debounced_and_held_when_the_module_is_lost() {
        let contact = Arc::new(Mutex::new(false));
        let addr = io_module(contact.clone()).await;
        let shared: Shared = Arc::default();
        let cfg = ControlBoxRelay {
            address: addr,
            unit: 1,
            input: 0,
            active_when: Contact::Closed,
            debounce_ms: 300,
            on_input_loss: InputLossPolicy::Hold,
            input_loss_s: 1,
        };
        spawn_relay(cfg, shared.clone());
        wait_for(&shared, |s| s.relay.as_ref().is_some_and(|r| r.readable && !r.active)).await;

        *contact.lock().unwrap() = true;
        let s = wait_for(&shared, |s| s.relay.as_ref().is_some_and(|r| r.active)).await;
        assert!(merge(DsoCommands::default(), &s).dim);

        *contact.lock().unwrap() = false;
        wait_for(&shared, |s| s.relay.as_ref().is_some_and(|r| !r.active)).await;
    }

    #[tokio::test]
    async fn an_unreachable_module_dims_when_configured_to() {
        // Nothing listens on this port.
        let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        let shared: Shared = Arc::default();
        let cfg = ControlBoxRelay {
            address: port,
            unit: 1,
            input: 0,
            active_when: Contact::Closed,
            debounce_ms: 0,
            on_input_loss: InputLossPolicy::Dim,
            input_loss_s: 1,
        };
        spawn_relay(cfg, shared.clone());
        let s = wait_for(&shared, |s| s.relay.as_ref().is_some_and(|r| r.active && !r.readable)).await;
        assert_eq!(fallbacks(&s), ["control box relay unreadable"]);
    }
}
