//! APCI: the transport header of IEC 60870-5-104 (clause 5).
//!
//! Every APDU starts with `0x68`, a length octet and four control octets.
//! The control octets select one of three formats:
//!
//! * **I-format** (numbered information transfer) carries an ASDU plus the
//!   send and receive sequence numbers N(S) / N(R).
//! * **S-format** (numbered supervisory) only acknowledges received I-frames.
//! * **U-format** (unnumbered control) starts/stops data transfer and tests the link.

use crate::Error;

pub const START: u8 = 0x68;
/// Maximum APDU length after the length octet (253 = 4 control octets + 249 ASDU octets).
pub const MAX_LENGTH: usize = 253;
/// Sequence numbers are 15 bits wide and wrap at 32 768.
pub const SEQ_MODULO: u16 = 1 << 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UFunction {
    StartDtAct,
    StartDtCon,
    StopDtAct,
    StopDtCon,
    TestFrAct,
    TestFrCon,
}

impl UFunction {
    fn bits(self) -> u8 {
        match self {
            UFunction::StartDtAct => 0x07,
            UFunction::StartDtCon => 0x0B,
            UFunction::StopDtAct => 0x13,
            UFunction::StopDtCon => 0x23,
            UFunction::TestFrAct => 0x43,
            UFunction::TestFrCon => 0x83,
        }
    }

    fn from_bits(b: u8) -> Option<Self> {
        Some(match b {
            0x07 => UFunction::StartDtAct,
            0x0B => UFunction::StartDtCon,
            0x13 => UFunction::StopDtAct,
            0x23 => UFunction::StopDtCon,
            0x43 => UFunction::TestFrAct,
            0x83 => UFunction::TestFrCon,
            _ => return None,
        })
    }
}

/// One APDU as it travels on the TCP stream. The ASDU of an I-frame is kept
/// as raw octets here; [`crate::asdu::Asdu::decode`] interprets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Apdu {
    I { ns: u16, nr: u16, asdu: Vec<u8> },
    S { nr: u16 },
    U(UFunction),
}

impl Apdu {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6);
        out.push(START);
        out.push(0); // length, patched below
        match self {
            Apdu::I { ns, nr, asdu } => {
                out.extend_from_slice(&(ns << 1).to_le_bytes());
                out.extend_from_slice(&(nr << 1).to_le_bytes());
                out.extend_from_slice(asdu);
            }
            Apdu::S { nr } => {
                out.extend_from_slice(&[0x01, 0x00]);
                out.extend_from_slice(&(nr << 1).to_le_bytes());
            }
            Apdu::U(f) => out.extend_from_slice(&[f.bits(), 0, 0, 0]),
        }
        debug_assert!(out.len() - 2 <= MAX_LENGTH, "ASDU too long for one APDU");
        out[1] = (out.len() - 2) as u8;
        out
    }

    /// Tries to take one complete APDU from the front of `buf`.
    ///
    /// Returns `Ok(None)` while more bytes are needed, and the number of
    /// bytes consumed on success. A malformed header is a fatal error: the
    /// standard gives no way to resynchronise, so the connection must close.
    pub fn parse(buf: &[u8]) -> Result<Option<(Apdu, usize)>, Error> {
        if buf.len() < 2 {
            return Ok(None);
        }
        if buf[0] != START {
            return Err(Error::BadStart(buf[0]));
        }
        let len = buf[1] as usize;
        if !(4..=MAX_LENGTH).contains(&len) {
            return Err(Error::BadLength(len));
        }
        if buf.len() < 2 + len {
            return Ok(None);
        }
        let c = &buf[2..6];
        let apdu = if c[0] & 0x01 == 0 {
            Apdu::I {
                ns: u16::from_le_bytes([c[0], c[1]]) >> 1,
                nr: u16::from_le_bytes([c[2], c[3]]) >> 1,
                asdu: buf[6..2 + len].to_vec(),
            }
        } else if c[0] & 0x03 == 0x01 {
            if len != 4 {
                return Err(Error::BadLength(len));
            }
            Apdu::S { nr: u16::from_le_bytes([c[2], c[3]]) >> 1 }
        } else {
            if len != 4 {
                return Err(Error::BadLength(len));
            }
            Apdu::U(UFunction::from_bits(c[0]).ok_or(Error::BadControlField(c[0]))?)
        };
        Ok(Some((apdu, 2 + len)))
    }
}

/// Distance from `from` forward to `to` on the 15-bit sequence ring.
pub fn seq_distance(from: u16, to: u16) -> u16 {
    to.wrapping_sub(from) % SEQ_MODULO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u_frames_match_the_standard_octets() {
        assert_eq!(Apdu::U(UFunction::StartDtAct).encode(), [0x68, 0x04, 0x07, 0, 0, 0]);
        assert_eq!(Apdu::U(UFunction::TestFrCon).encode(), [0x68, 0x04, 0x83, 0, 0, 0]);
    }

    #[test]
    fn sequence_numbers_are_shifted_left_by_one() {
        // N(S) = 3, N(R) = 5 -> control octets 06 00 0A 00
        let f = Apdu::I { ns: 3, nr: 5, asdu: vec![0x64, 0x01, 0x06, 0x00, 0x01, 0x00, 0, 0, 0, 0x14] };
        let bytes = f.encode();
        assert_eq!(&bytes[..6], [0x68, 0x0E, 0x06, 0x00, 0x0A, 0x00]);
        assert_eq!(Apdu::parse(&bytes).unwrap(), Some((f, 16)));
        assert_eq!(Apdu::S { nr: 32_767 }.encode(), [0x68, 0x04, 0x01, 0x00, 0xFE, 0xFF]);
    }

    #[test]
    fn waits_for_a_complete_frame_and_rejects_garbage() {
        let bytes = Apdu::S { nr: 1 }.encode();
        assert_eq!(Apdu::parse(&bytes[..4]).unwrap(), None);
        assert!(matches!(Apdu::parse(&[0x00, 0x04]), Err(Error::BadStart(0))));
        assert!(matches!(Apdu::parse(&[0x68, 0x02]), Err(Error::BadLength(2))));
        assert!(matches!(Apdu::parse(&[0x68, 0x04, 0xFF, 0, 0, 0]), Err(Error::BadControlField(0xFF))));
    }

    #[test]
    fn sequence_distance_wraps() {
        assert_eq!(seq_distance(32_766, 1), 3);
        assert_eq!(seq_distance(5, 5), 0);
    }
}
