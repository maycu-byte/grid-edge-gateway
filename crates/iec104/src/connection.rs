//! Tokio driver: runs a [`Session`] over any byte stream (plain TCP or TLS).

use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::asdu::{Asdu, AsduError};
use crate::session::{Close, Config, Output, Session};
use crate::{Apdu, Error};

/// What the connection reports to the application.
#[derive(Debug)]
pub enum Event {
    Started,
    Stopped,
    Asdu(Result<Asdu, AsduError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Rx,
    Tx,
}

#[derive(Debug)]
pub enum Ended {
    PeerClosed,
    Io(std::io::Error),
    Framing(Error),
    Protocol(Close),
    /// The application dropped its end of the channels.
    ApplicationGone,
}

/// Drives one connection until it ends.
///
/// * `outbound` carries encoded ASDUs from the application (use
///   [`Asdu::encode`], or raw octets for mirrored unsupported types).
/// * `events` receives start/stop and every decoded ASDU.
/// * `tap` sees every APDU in both directions — for logs and the dashboard.
pub async fn run<S, F>(
    mut stream: S,
    cfg: Config,
    mut outbound: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<Event>,
    mut tap: F,
) -> Ended
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnMut(Direction, &[u8]),
{
    let mut session = Session::new(cfg, Instant::now());
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    let mut tick = tokio::time::interval(Duration::from_millis(100));

    loop {
        let step: Result<(), Ended> = tokio::select! {
            read = stream.read(&mut chunk) => match read {
                Ok(0) => Err(Ended::PeerClosed),
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    parse_all(&mut buf, &mut session, &mut tap)
                }
                Err(e) => Err(Ended::Io(e)),
            },
            asdu = outbound.recv() => match asdu {
                Some(asdu) => { session.send_raw(asdu, Instant::now()); Ok(()) }
                None => Err(Ended::ApplicationGone),
            },
            _ = tick.tick() => session.on_tick(Instant::now()).map_err(Ended::Protocol),
        };
        if let Err(end) = step {
            return end;
        }
        for out in session.drain() {
            let result = match out {
                Output::Transmit(bytes) => {
                    tap(Direction::Tx, &bytes);
                    stream.write_all(&bytes).await.map_err(Ended::Io)
                }
                Output::Received(asdu) => send_event(&events, Event::Asdu(asdu)).await,
                Output::Started => send_event(&events, Event::Started).await,
                Output::Stopped => send_event(&events, Event::Stopped).await,
            };
            if let Err(end) = result {
                return end;
            }
        }
    }
}

fn parse_all<F: FnMut(Direction, &[u8])>(buf: &mut Vec<u8>, session: &mut Session, tap: &mut F) -> Result<(), Ended> {
    loop {
        match Apdu::parse(buf) {
            Ok(Some((apdu, used))) => {
                tap(Direction::Rx, &buf[..used]);
                buf.drain(..used);
                session.on_apdu(apdu, Instant::now()).map_err(Ended::Protocol)?;
            }
            Ok(None) => return Ok(()),
            Err(e) => return Err(Ended::Framing(e)),
        }
    }
}

async fn send_event(events: &mpsc::Sender<Event>, e: Event) -> Result<(), Ended> {
    events.send(e).await.map_err(|_| Ended::ApplicationGone)
}
