//! Sans-IO link layer of a *controlled station* (the IEC 104 server side).
//!
//! [`Session`] owns the sequence numbers, the k/w flow-control windows and
//! the t1/t2/t3 timers of clause 5 of IEC 60870-5-104. It never touches a
//! socket or a clock: the caller feeds it received APDUs and the current
//! time, and collects the bytes to transmit. That keeps every timing rule
//! unit-testable without sleeping.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::apci::{seq_distance, Apdu, UFunction, SEQ_MODULO};
use crate::asdu::{Asdu, AsduError};

/// Protocol parameters, with the defaults recommended by the standard.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Max unacknowledged I-frames we may send before waiting (k).
    pub k: u16,
    /// Acknowledge at the latest after receiving w I-frames.
    pub w: u16,
    /// Time-out of send or test APDUs.
    pub t1: Duration,
    /// Time-out for acknowledging received I-frames when there is no data to send.
    pub t2: Duration,
    /// Time-out for sending test frames on an idle connection.
    pub t3: Duration,
    /// Frames queued while data transfer is stopped; older ones are dropped.
    pub max_queue: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            k: 12,
            w: 8,
            t1: Duration::from_secs(15),
            t2: Duration::from_secs(10),
            t3: Duration::from_secs(20),
            max_queue: 1_000,
        }
    }
}

/// Why the session asks for the connection to be closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Close {
    /// t1 expired waiting for an acknowledgement of our I-frames.
    AckTimeout,
    /// t1 expired waiting for TESTFR con.
    TestTimeout,
    /// The peer's N(S) is not the one we expected.
    SequenceError { expected: u16, got: u16 },
    /// The peer acknowledged frames we never sent.
    BadAcknowledge { nr: u16 },
}

/// What the caller must do after feeding the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Write these bytes to the stream.
    Transmit(Vec<u8>),
    /// An ASDU arrived from the controlling station.
    Received(Result<Asdu, AsduError>),
    /// Data transfer was started (STARTDT) — the application may send its
    /// end-of-initialisation and spontaneous data now.
    Started,
    Stopped,
}

pub struct Session {
    cfg: Config,
    started: bool,
    /// N(S) of the next I-frame we send.
    send_seq: u16,
    /// N(S) we expect in the next I-frame received; also our N(R).
    recv_seq: u16,
    /// Our N(R) the peer has last seen from us.
    last_acked_to_peer: u16,
    /// Send times of our unacknowledged I-frames, oldest first.
    unacked: VecDeque<Instant>,
    /// N(S) of the oldest unacknowledged I-frame.
    oldest_unacked_seq: u16,
    /// ASDUs waiting for STARTDT or for room in the k window.
    queue: VecDeque<Vec<u8>>,
    /// When the first still-unacknowledged received I-frame arrived (t2).
    unacked_rx_since: Option<Instant>,
    /// Last time any frame arrived (t3).
    last_rx: Instant,
    /// When we sent a TESTFR act still waiting for its confirmation.
    test_sent: Option<Instant>,
    out: Vec<Output>,
}

impl Session {
    pub fn new(cfg: Config, now: Instant) -> Self {
        Session {
            cfg,
            started: false,
            send_seq: 0,
            recv_seq: 0,
            last_acked_to_peer: 0,
            unacked: VecDeque::new(),
            oldest_unacked_seq: 0,
            queue: VecDeque::new(),
            unacked_rx_since: None,
            last_rx: now,
            test_sent: None,
            out: Vec::new(),
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Number of our I-frames the peer has not acknowledged yet.
    pub fn in_flight(&self) -> usize {
        self.unacked.len()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Takes everything produced since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    /// Queues an ASDU for sending. It goes out once data transfer is started
    /// and the k window has room.
    pub fn send(&mut self, asdu: &Asdu, now: Instant) {
        self.send_raw(asdu.encode(), now);
    }

    pub fn send_raw(&mut self, asdu: Vec<u8>, now: Instant) {
        if self.queue.len() >= self.cfg.max_queue {
            self.queue.pop_front();
        }
        self.queue.push_back(asdu);
        self.flush(now);
    }

    pub fn on_apdu(&mut self, apdu: Apdu, now: Instant) -> Result<(), Close> {
        self.last_rx = now;
        match apdu {
            Apdu::I { ns, nr, asdu } => {
                if ns != self.recv_seq {
                    return Err(Close::SequenceError { expected: self.recv_seq, got: ns });
                }
                self.recv_seq = (self.recv_seq + 1) % SEQ_MODULO;
                self.acknowledge(nr)?;
                self.unacked_rx_since.get_or_insert(now);
                self.out.push(Output::Received(Asdu::decode(&asdu)));
                if seq_distance(self.last_acked_to_peer, self.recv_seq) >= self.cfg.w {
                    self.transmit_s();
                }
            }
            Apdu::S { nr } => self.acknowledge(nr)?,
            Apdu::U(f) => match f {
                UFunction::StartDtAct => {
                    self.transmit(Apdu::U(UFunction::StartDtCon));
                    if !self.started {
                        self.started = true;
                        self.out.push(Output::Started);
                    }
                }
                UFunction::StopDtAct => {
                    // Acknowledge whatever we received before confirming the stop.
                    if self.recv_seq != self.last_acked_to_peer {
                        self.transmit_s();
                    }
                    self.transmit(Apdu::U(UFunction::StopDtCon));
                    if self.started {
                        self.started = false;
                        self.out.push(Output::Stopped);
                    }
                }
                UFunction::TestFrAct => self.transmit(Apdu::U(UFunction::TestFrCon)),
                UFunction::TestFrCon => self.test_sent = None,
                // A controlled station never sends act frames for STARTDT/STOPDT,
                // so their confirmations are ignored.
                UFunction::StartDtCon | UFunction::StopDtCon => {}
            },
        }
        self.flush(now);
        Ok(())
    }

    /// Runs the timers. Call it regularly (every 100 ms is plenty).
    pub fn on_tick(&mut self, now: Instant) -> Result<(), Close> {
        if let Some(&oldest) = self.unacked.front()
            && now.duration_since(oldest) >= self.cfg.t1 {
                return Err(Close::AckTimeout);
            }
        if let Some(sent) = self.test_sent
            && now.duration_since(sent) >= self.cfg.t1 {
                return Err(Close::TestTimeout);
            }
        if let Some(since) = self.unacked_rx_since
            && now.duration_since(since) >= self.cfg.t2 {
                self.transmit_s();
            }
        if self.test_sent.is_none() && now.duration_since(self.last_rx) >= self.cfg.t3 {
            self.transmit(Apdu::U(UFunction::TestFrAct));
            self.test_sent = Some(now);
        }
        Ok(())
    }

    fn acknowledge(&mut self, nr: u16) -> Result<(), Close> {
        let count = seq_distance(self.oldest_unacked_seq, nr) as usize;
        if count > self.unacked.len() {
            return Err(Close::BadAcknowledge { nr });
        }
        self.unacked.drain(..count);
        self.oldest_unacked_seq = nr;
        Ok(())
    }

    fn flush(&mut self, now: Instant) {
        while self.started && self.unacked.len() < self.cfg.k as usize {
            let Some(asdu) = self.queue.pop_front() else { break };
            let ns = self.send_seq;
            self.send_seq = (self.send_seq + 1) % SEQ_MODULO;
            self.unacked.push_back(now);
            // Every I-frame carries our N(R), so it acknowledges the peer too.
            self.transmit(Apdu::I { ns, nr: self.recv_seq, asdu });
            self.mark_acked_to_peer();
        }
    }

    fn transmit_s(&mut self) {
        self.transmit(Apdu::S { nr: self.recv_seq });
        self.mark_acked_to_peer();
    }

    fn mark_acked_to_peer(&mut self) {
        self.last_acked_to_peer = self.recv_seq;
        self.unacked_rx_since = None;
    }

    fn transmit(&mut self, apdu: Apdu) {
        self.out.push(Output::Transmit(apdu.encode()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asdu::{Cause, Element};

    fn transmitted(out: &[Output]) -> Vec<Apdu> {
        out.iter()
            .filter_map(|o| match o {
                Output::Transmit(b) => Some(Apdu::parse(b).unwrap().unwrap().0),
                _ => None,
            })
            .collect()
    }

    fn measurement(v: f32) -> Asdu {
        Asdu::single(Cause::Spontaneous, 1, 1000, Element::Float { value: v, quality: Default::default() })
    }

    fn started(cfg: Config, t0: Instant) -> Session {
        let mut s = Session::new(cfg, t0);
        s.on_apdu(Apdu::U(UFunction::StartDtAct), t0).unwrap();
        let out = s.drain();
        assert_eq!(transmitted(&out), [Apdu::U(UFunction::StartDtCon)]);
        assert!(out.contains(&Output::Started));
        s
    }

    #[test]
    fn holds_data_until_startdt() {
        let t0 = Instant::now();
        let mut s = Session::new(Config::default(), t0);
        s.send(&measurement(1.0), t0);
        assert!(transmitted(&s.drain()).is_empty());
        s.on_apdu(Apdu::U(UFunction::StartDtAct), t0).unwrap();
        let sent = transmitted(&s.drain());
        assert_eq!(sent.len(), 2);
        assert!(matches!(sent[1], Apdu::I { ns: 0, nr: 0, .. }));
    }

    #[test]
    fn respects_the_k_window() {
        let t0 = Instant::now();
        let mut s = started(Config { k: 3, ..Config::default() }, t0);
        for i in 0..5 {
            s.send(&measurement(i as f32), t0);
        }
        assert_eq!(transmitted(&s.drain()).len(), 3);
        assert_eq!((s.in_flight(), s.queued()), (3, 2));
        // Peer acknowledges two frames: two more may go out.
        s.on_apdu(Apdu::S { nr: 2 }, t0).unwrap();
        let sent = transmitted(&s.drain());
        assert!(matches!(sent[..], [Apdu::I { ns: 3, .. }, Apdu::I { ns: 4, .. }]));
    }

    #[test]
    fn acknowledges_after_w_frames() {
        let t0 = Instant::now();
        let mut s = started(Config { w: 2, ..Config::default() }, t0);
        let ic = Asdu::single(Cause::Activation, 1, 0, Element::Interrogation { qoi: 20 }).encode();
        s.on_apdu(Apdu::I { ns: 0, nr: 0, asdu: ic.clone() }, t0).unwrap();
        assert!(transmitted(&s.drain()).is_empty());
        s.on_apdu(Apdu::I { ns: 1, nr: 0, asdu: ic }, t0).unwrap();
        let out = s.drain();
        assert_eq!(transmitted(&out), [Apdu::S { nr: 2 }]);
        assert!(matches!(out[0], Output::Received(Ok(_))));
    }

    #[test]
    fn acknowledges_after_t2_when_idle() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        let ic = Asdu::single(Cause::Activation, 1, 0, Element::Interrogation { qoi: 20 }).encode();
        s.on_apdu(Apdu::I { ns: 0, nr: 0, asdu: ic }, t0).unwrap();
        s.drain();
        s.on_tick(t0 + Duration::from_secs(9)).unwrap();
        assert!(transmitted(&s.drain()).is_empty());
        s.on_tick(t0 + Duration::from_secs(10)).unwrap();
        assert_eq!(transmitted(&s.drain()), [Apdu::S { nr: 1 }]);
    }

    #[test]
    fn closes_when_our_frames_are_not_acknowledged_within_t1() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        s.send(&measurement(1.0), t0);
        assert_eq!(s.on_tick(t0 + Duration::from_secs(14)), Ok(()));
        assert_eq!(s.on_tick(t0 + Duration::from_secs(15)), Err(Close::AckTimeout));
    }

    #[test]
    fn tests_an_idle_link_and_closes_if_it_is_dead() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        s.on_tick(t0 + Duration::from_secs(20)).unwrap();
        assert_eq!(transmitted(&s.drain()), [Apdu::U(UFunction::TestFrAct)]);
        assert_eq!(s.on_tick(t0 + Duration::from_secs(35)), Err(Close::TestTimeout));
    }

    #[test]
    fn answered_test_keeps_the_link_up() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        s.on_tick(t0 + Duration::from_secs(20)).unwrap();
        s.on_apdu(Apdu::U(UFunction::TestFrCon), t0 + Duration::from_secs(21)).unwrap();
        assert_eq!(s.on_tick(t0 + Duration::from_secs(36)), Ok(()));
    }

    #[test]
    fn rejects_out_of_order_and_impossible_acknowledgements() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        let err = s.on_apdu(Apdu::I { ns: 4, nr: 0, asdu: vec![] }, t0).unwrap_err();
        assert_eq!(err, Close::SequenceError { expected: 0, got: 4 });
        let mut s = started(Config::default(), t0);
        assert_eq!(s.on_apdu(Apdu::S { nr: 1 }, t0), Err(Close::BadAcknowledge { nr: 1 }));
    }

    #[test]
    fn sequence_numbers_wrap_at_32768() {
        let t0 = Instant::now();
        let mut s = started(Config { k: 1, ..Config::default() }, t0);
        s.send_seq = SEQ_MODULO - 1;
        s.oldest_unacked_seq = SEQ_MODULO - 1;
        s.send(&measurement(1.0), t0);
        s.send(&measurement(2.0), t0);
        assert!(matches!(transmitted(&s.drain())[..], [Apdu::I { ns: 32_767, .. }]));
        s.on_apdu(Apdu::S { nr: 0 }, t0).unwrap();
        assert!(matches!(transmitted(&s.drain())[..], [Apdu::I { ns: 0, .. }]));
    }

    #[test]
    fn stopdt_confirms_and_halts_data() {
        let t0 = Instant::now();
        let mut s = started(Config::default(), t0);
        s.on_apdu(Apdu::U(UFunction::StopDtAct), t0).unwrap();
        let out = s.drain();
        assert_eq!(transmitted(&out), [Apdu::U(UFunction::StopDtCon)]);
        assert!(out.contains(&Output::Stopped));
        s.send(&measurement(1.0), t0);
        assert!(transmitted(&s.drain()).is_empty());
    }
}
