//! ASDU: the application data unit (IEC 60870-5-101 clause 7, as profiled by -104).
//!
//! Only the type identifications a DSO telecontrol link to a prosumer site
//! needs are implemented; anything else decodes to [`AsduError::UnsupportedType`]
//! with the raw octets, so a station can mirror it back with cause 44.

use crate::time::Cp56Time2a;

/// Cause of transmission (six bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cause {
    Periodic = 1,
    Background = 2,
    Spontaneous = 3,
    Initialized = 4,
    Request = 5,
    Activation = 6,
    ActivationCon = 7,
    Deactivation = 8,
    DeactivationCon = 9,
    ActivationTermination = 10,
    Interrogated = 20,
    UnknownType = 44,
    UnknownCause = 45,
    UnknownCommonAddress = 46,
    UnknownObjectAddress = 47,
}

impl Cause {
    pub fn from_u8(v: u8) -> Option<Self> {
        use Cause::*;
        Some(match v {
            1 => Periodic,
            2 => Background,
            3 => Spontaneous,
            4 => Initialized,
            5 => Request,
            6 => Activation,
            7 => ActivationCon,
            8 => Deactivation,
            9 => DeactivationCon,
            10 => ActivationTermination,
            20 => Interrogated,
            44 => UnknownType,
            45 => UnknownCause,
            46 => UnknownCommonAddress,
            47 => UnknownObjectAddress,
            _ => return None,
        })
    }
}

/// Quality flags shared by SIQ and QDS. `overflow` only exists in QDS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quality {
    pub overflow: bool,
    pub blocked: bool,
    pub substituted: bool,
    pub not_topical: bool,
    pub invalid: bool,
}

impl Quality {
    pub const GOOD: Quality =
        Quality { overflow: false, blocked: false, substituted: false, not_topical: false, invalid: false };

    pub fn invalid() -> Self {
        Quality { invalid: true, ..Self::GOOD }
    }

    fn bits(self) -> u8 {
        (self.overflow as u8)
            | (self.blocked as u8) << 4
            | (self.substituted as u8) << 5
            | (self.not_topical as u8) << 6
            | (self.invalid as u8) << 7
    }

    fn from_bits(b: u8, has_overflow: bool) -> Self {
        Quality {
            overflow: has_overflow && b & 0x01 != 0,
            blocked: b & 0x10 != 0,
            substituted: b & 0x20 != 0,
            not_topical: b & 0x40 != 0,
            invalid: b & 0x80 != 0,
        }
    }
}

/// One information element, tagged by the ASDU type it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Element {
    /// M_SP_NA_1 (1): single-point information.
    SinglePoint { on: bool, quality: Quality },
    /// M_SP_TB_1 (30): single-point information with time tag.
    SinglePointTime { on: bool, quality: Quality, time: Cp56Time2a },
    /// M_ME_NC_1 (13): measured value, short floating point.
    Float { value: f32, quality: Quality },
    /// M_ME_TF_1 (36): measured value, short floating point, with time tag.
    FloatTime { value: f32, quality: Quality, time: Cp56Time2a },
    /// M_EI_NA_1 (70): end of initialisation, with cause of initialisation.
    EndOfInit { coi: u8 },
    /// C_SC_NA_1 (45): single command. `qualifier` is QU (0 = no pulse definition).
    SingleCommand { on: bool, select: bool, qualifier: u8 },
    /// C_SE_NC_1 (50): set-point command, short floating point.
    SetpointFloat { value: f32, select: bool, qualifier: u8 },
    /// C_IC_NA_1 (100): interrogation command. QOI 20 = station interrogation.
    Interrogation { qoi: u8 },
    /// C_CS_NA_1 (103): clock synchronisation command.
    ClockSync { time: Cp56Time2a },
}

impl Element {
    pub fn type_id(&self) -> u8 {
        match self {
            Element::SinglePoint { .. } => 1,
            Element::Float { .. } => 13,
            Element::SinglePointTime { .. } => 30,
            Element::FloatTime { .. } => 36,
            Element::SingleCommand { .. } => 45,
            Element::SetpointFloat { .. } => 50,
            Element::EndOfInit { .. } => 70,
            Element::Interrogation { .. } => 100,
            Element::ClockSync { .. } => 103,
        }
    }

    /// Standard mnemonic, e.g. `M_ME_TF_1`, for logs and the dashboard.
    pub fn type_name(type_id: u8) -> &'static str {
        match type_id {
            1 => "M_SP_NA_1",
            13 => "M_ME_NC_1",
            30 => "M_SP_TB_1",
            36 => "M_ME_TF_1",
            45 => "C_SC_NA_1",
            50 => "C_SE_NC_1",
            70 => "M_EI_NA_1",
            100 => "C_IC_NA_1",
            103 => "C_CS_NA_1",
            _ => "unsupported",
        }
    }

    fn size(type_id: u8) -> Option<usize> {
        Some(match type_id {
            1 | 45 | 70 | 100 => 1,
            13 | 50 => 5,
            30 => 8,
            36 => 12,
            103 => 7,
            _ => return None,
        })
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match *self {
            Element::SinglePoint { on, quality } => out.push(quality.bits() & 0xF0 | on as u8),
            Element::SinglePointTime { on, quality, time } => {
                out.push(quality.bits() & 0xF0 | on as u8);
                time.encode(out);
            }
            Element::Float { value, quality } => {
                out.extend_from_slice(&value.to_le_bytes());
                out.push(quality.bits());
            }
            Element::FloatTime { value, quality, time } => {
                out.extend_from_slice(&value.to_le_bytes());
                out.push(quality.bits());
                time.encode(out);
            }
            Element::EndOfInit { coi } => out.push(coi),
            Element::SingleCommand { on, select, qualifier } => {
                out.push(on as u8 | (qualifier & 0x1F) << 2 | (select as u8) << 7)
            }
            Element::SetpointFloat { value, select, qualifier } => {
                out.extend_from_slice(&value.to_le_bytes());
                out.push(qualifier & 0x7F | (select as u8) << 7);
            }
            Element::Interrogation { qoi } => out.push(qoi),
            Element::ClockSync { time } => time.encode(out),
        }
    }

    fn decode(type_id: u8, b: &[u8]) -> Result<Self, AsduError> {
        let f32_at = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let time_at = |i: usize| Cp56Time2a::decode(&b[i..]).map_err(|_| AsduError::Truncated);
        Ok(match type_id {
            1 => Element::SinglePoint { on: b[0] & 1 != 0, quality: Quality::from_bits(b[0], false) },
            30 => Element::SinglePointTime {
                on: b[0] & 1 != 0,
                quality: Quality::from_bits(b[0], false),
                time: time_at(1)?,
            },
            13 => Element::Float { value: f32_at(0), quality: Quality::from_bits(b[4], true) },
            36 => Element::FloatTime { value: f32_at(0), quality: Quality::from_bits(b[4], true), time: time_at(5)? },
            70 => Element::EndOfInit { coi: b[0] },
            45 => Element::SingleCommand { on: b[0] & 1 != 0, select: b[0] & 0x80 != 0, qualifier: (b[0] >> 2) & 0x1F },
            50 => Element::SetpointFloat { value: f32_at(0), select: b[4] & 0x80 != 0, qualifier: b[4] & 0x7F },
            100 => Element::Interrogation { qoi: b[0] },
            103 => Element::ClockSync { time: time_at(0)? },
            _ => unreachable!("size() filtered unsupported types"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InformationObject {
    /// Information object address (three octets on 104).
    pub ioa: u32,
    pub element: Element,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asdu {
    pub type_id: u8,
    pub cause: Cause,
    pub negative: bool,
    pub test: bool,
    pub originator: u8,
    pub common_address: u16,
    pub objects: Vec<InformationObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsduError {
    Truncated,
    /// The type is valid on the wire but not implemented here. `raw` holds the
    /// full ASDU so the station can mirror it with cause 44 (unknown type).
    UnsupportedType {
        type_id: u8,
        raw: Vec<u8>,
    },
    /// The cause octet holds a value this implementation does not know.
    UnsupportedCause {
        raw: Vec<u8>,
    },
}

impl Asdu {
    /// One ASDU carrying a single information object.
    pub fn single(cause: Cause, common_address: u16, ioa: u32, element: Element) -> Self {
        Asdu {
            type_id: element.type_id(),
            cause,
            negative: false,
            test: false,
            originator: 0,
            common_address,
            objects: vec![InformationObject { ioa, element }],
        }
    }

    /// Encodes with SQ = 0 (every object carries its own address). Objects
    /// must all share `type_id`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(10 + self.objects.len() * 8);
        out.push(self.type_id);
        out.push(self.objects.len() as u8 & 0x7F);
        out.push(self.cause as u8 | (self.negative as u8) << 6 | (self.test as u8) << 7);
        out.push(self.originator);
        out.extend_from_slice(&self.common_address.to_le_bytes());
        for obj in &self.objects {
            debug_assert_eq!(obj.element.type_id(), self.type_id);
            out.extend_from_slice(&obj.ioa.to_le_bytes()[..3]);
            obj.element.encode(&mut out);
        }
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, AsduError> {
        if b.len() < 6 {
            return Err(AsduError::Truncated);
        }
        let type_id = b[0];
        let sequence = b[1] & 0x80 != 0;
        let count = (b[1] & 0x7F) as usize;
        let size = Element::size(type_id).ok_or_else(|| AsduError::UnsupportedType { type_id, raw: b.to_vec() })?;
        let cause = Cause::from_u8(b[2] & 0x3F).ok_or_else(|| AsduError::UnsupportedCause { raw: b.to_vec() })?;
        let expected = if sequence { 6 + 3 + count * size } else { 6 + count * (3 + size) };
        if b.len() < expected {
            return Err(AsduError::Truncated);
        }
        let ioa_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], 0]);
        let mut objects = Vec::with_capacity(count);
        let mut pos = 6;
        let first_ioa = ioa_at(pos);
        if sequence {
            pos += 3;
        }
        for k in 0..count {
            let ioa = if sequence {
                first_ioa + k as u32
            } else {
                let a = ioa_at(pos);
                pos += 3;
                a
            };
            objects.push(InformationObject { ioa, element: Element::decode(type_id, &b[pos..pos + size])? });
            pos += size;
        }
        Ok(Asdu {
            type_id,
            cause,
            negative: b[2] & 0x40 != 0,
            test: b[2] & 0x80 != 0,
            originator: b[3],
            common_address: u16::from_le_bytes([b[4], b[5]]),
            objects,
        })
    }

    /// The reply a station sends to a command: same type, objects and
    /// addresses, with a new cause and the P/N bit.
    pub fn mirror(&self, cause: Cause, negative: bool) -> Self {
        Asdu { cause, negative, ..self.clone() }
    }
}

/// Mirrors raw ASDU octets with a new cause and P/N = negative. Used for
/// types or causes this implementation cannot decode.
pub fn mirror_raw(raw: &[u8], cause: Cause) -> Vec<u8> {
    let mut out = raw.to_vec();
    if out.len() > 2 {
        out[2] = (out[2] & 0x80) | 0x40 | cause as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrogation_command_matches_reference_octets() {
        // Station interrogation, CA 1, as sent by most 104 masters:
        // 64 01 06 00 01 00 00 00 00 14
        let a = Asdu::single(Cause::Activation, 1, 0, Element::Interrogation { qoi: 20 });
        let bytes = a.encode();
        assert_eq!(bytes, [0x64, 0x01, 0x06, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x14]);
        assert_eq!(Asdu::decode(&bytes).unwrap(), a);
    }

    #[test]
    fn float_measurement_with_time_round_trips() {
        let time = Cp56Time2a::from_unix_ms(1_790_257_512_345);
        let a = Asdu::single(
            Cause::Spontaneous,
            7,
            0x01_02_03,
            Element::FloatTime { value: -42.5, quality: Quality { overflow: true, ..Quality::GOOD }, time },
        );
        let bytes = a.encode();
        assert_eq!(bytes.len(), 6 + 3 + 12);
        assert_eq!(&bytes[6..9], [0x03, 0x02, 0x01]);
        assert_eq!(Asdu::decode(&bytes).unwrap(), a);
    }

    #[test]
    fn setpoint_select_bit_and_negative_mirror() {
        let cmd = Asdu::single(
            Cause::Activation,
            1,
            5001,
            Element::SetpointFloat { value: 60.0, select: true, qualifier: 0 },
        );
        let bytes = cmd.encode();
        assert_eq!(*bytes.last().unwrap(), 0x80);
        let reply = Asdu::decode(&bytes).unwrap().mirror(Cause::UnknownObjectAddress, true);
        let enc = reply.encode();
        assert_eq!(enc[2], 0x40 | 47);
    }

    #[test]
    fn decodes_sequence_of_elements() {
        // M_SP_NA_1, SQ=1, 3 objects starting at IOA 100: on, off, on+invalid
        let bytes = [0x01, 0x83, 0x14, 0x00, 0x01, 0x00, 100, 0, 0, 0x01, 0x00, 0x81];
        let a = Asdu::decode(&bytes).unwrap();
        assert_eq!(a.cause, Cause::Interrogated);
        let ioas: Vec<u32> = a.objects.iter().map(|o| o.ioa).collect();
        assert_eq!(ioas, [100, 101, 102]);
        assert_eq!(a.objects[2].element, Element::SinglePoint { on: true, quality: Quality::invalid() });
    }

    #[test]
    fn unsupported_type_keeps_raw_bytes_for_mirroring() {
        let raw = [0x2E, 0x01, 0x06, 0x00, 0x01, 0x00, 1, 0, 0, 0x01]; // C_DC_NA_1
        let err = Asdu::decode(&raw).unwrap_err();
        assert_eq!(err, AsduError::UnsupportedType { type_id: 46, raw: raw.to_vec() });
        assert_eq!(mirror_raw(&raw, Cause::UnknownType)[2], 0x40 | 44);
    }

    #[test]
    fn rejects_truncated_objects() {
        let bytes =
            Asdu::single(Cause::Spontaneous, 1, 1, Element::Float { value: 1.0, quality: Quality::GOOD }).encode();
        assert_eq!(Asdu::decode(&bytes[..bytes.len() - 1]), Err(AsduError::Truncated));
    }
}
