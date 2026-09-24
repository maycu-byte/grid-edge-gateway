//! SunSpec Modbus register layout (information models 1, 103, 120, 123, 203).
//!
//! A SunSpec device exposes, from holding register 40000, the marker "SunS"
//! followed by a chain of models: each starts with its ID and length, then
//! the body. A client discovers what a device offers by walking that chain.
//! Offsets below are relative to the first register of a model's body.

pub const BASE: u16 = 40_000;
pub const MARKER: [u16; 2] = [0x5375, 0x6E53]; // "SunS"
pub const END_ID: u16 = 0xFFFF;

/// "Not implemented" values defined by the specification.
pub const NI_INT16: u16 = 0x8000;
pub const NI_UINT16: u16 = 0xFFFF;

pub mod common {
    pub const ID: u16 = 1;
    pub const LEN: usize = 66;
    pub const MANUFACTURER: usize = 0; // 16 registers
    pub const MODEL: usize = 16; // 16 registers
    pub const SERIAL: usize = 48; // 16 registers
    pub const DEVICE_ADDRESS: usize = 64;
}

/// Three-phase inverter, integer + scale factor representation.
pub mod inverter {
    pub const ID: u16 = 103;
    pub const LEN: usize = 50;
    pub const W: usize = 12;
    pub const W_SF: usize = 13;
    pub const HZ: usize = 14;
    pub const HZ_SF: usize = 15;
    pub const WH: usize = 22; // acc32, two registers
    pub const WH_SF: usize = 24;
    pub const ST: usize = 36;
    /// Operating state values (enum16 `St`).
    pub const ST_OFF: u16 = 1;
    pub const ST_MPPT: u16 = 4;
    pub const ST_THROTTLED: u16 = 5;
}

/// Nameplate ratings.
pub mod nameplate {
    pub const ID: u16 = 120;
    pub const LEN: usize = 26;
    pub const DER_TYPE: usize = 0;
    pub const W_RTG: usize = 1;
    pub const W_RTG_SF: usize = 2;
    pub const DER_TYPE_PV: u16 = 4;
}

/// Immediate inverter controls.
pub mod controls {
    pub const ID: u16 = 123;
    pub const LEN: usize = 24;
    pub const CONN: usize = 2;
    pub const W_MAX_LIM_PCT: usize = 3;
    pub const W_MAX_LIM_PCT_RVRT_TMS: usize = 5;
    pub const W_MAX_LIM_ENA: usize = 7;
    pub const W_MAX_LIM_PCT_SF: usize = 21;
}

/// Three-phase wye-connected meter.
pub mod meter {
    pub const ID: u16 = 203;
    pub const LEN: usize = 105;
    pub const HZ: usize = 14;
    pub const HZ_SF: usize = 15;
    pub const W: usize = 16;
    pub const W_SF: usize = 20;
}

/// Vendor model (SunSpec reserves IDs 64000–65535 for vendors): the power
/// the inverter could produce right now without any limit. Real inverters
/// expose this in vendor registers; it is what curtailment is measured from.
pub mod available {
    pub const ID: u16 = 64_900;
    pub const LEN: usize = 2;
    pub const W_AVAIL: usize = 0;
    pub const W_AVAIL_SF: usize = 1;
}

/// Applies a SunSpec scale factor: `value · 10^sf`.
pub fn scaled(value: i16, sf: i16) -> f64 {
    value as f64 * 10f64.powi(sf as i32)
}

/// The inverse of [`scaled`], saturating at the int16 range.
pub fn unscaled(value: f64, sf: i16) -> i16 {
    (value / 10f64.powi(sf as i32)).round().clamp(i16::MIN as f64 + 1.0, i16::MAX as f64) as i16
}

pub fn put_string(regs: &mut [u16], s: &str) {
    let bytes = s.as_bytes();
    for (i, r) in regs.iter_mut().enumerate() {
        let hi = *bytes.get(2 * i).unwrap_or(&0) as u16;
        let lo = *bytes.get(2 * i + 1).unwrap_or(&0) as u16;
        *r = hi << 8 | lo;
    }
}

pub fn get_string(regs: &[u16]) -> String {
    let bytes: Vec<u8> = regs.iter().flat_map(|r| r.to_be_bytes()).take_while(|&b| b != 0).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A model in a device's chain: its ID and the absolute address of its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelLocation {
    pub id: u16,
    pub body: u16,
    pub len: u16,
}

/// Builds the full register image starting at [`BASE`]: marker, models, end.
pub fn build_image(models: &[(u16, Vec<u16>)]) -> Vec<u16> {
    let mut image = MARKER.to_vec();
    for (id, body) in models {
        image.push(*id);
        image.push(body.len() as u16);
        image.extend_from_slice(body);
    }
    image.extend_from_slice(&[END_ID, 0]);
    image
}

/// Walks a model chain given the registers that follow the marker.
/// Returns `None` if the chain is malformed.
pub fn locate_models(after_marker: &[u16]) -> Option<Vec<ModelLocation>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    loop {
        let id = *after_marker.get(pos)?;
        let len = *after_marker.get(pos + 1)?;
        if id == END_ID {
            return Some(out);
        }
        out.push(ModelLocation { id, body: BASE + 2 + pos as u16 + 2, len });
        pos += 2 + len as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factors() {
        assert_eq!(scaled(5_432, 1), 54_320.0);
        assert_eq!(scaled(600, -1), 60.0);
        assert_eq!(unscaled(54_321.0, 1), 5_432);
        assert_eq!(unscaled(1e9, 0), i16::MAX);
    }

    #[test]
    fn strings_are_two_chars_per_register() {
        let mut r = [0u16; 4];
        put_string(&mut r, "SunS");
        assert_eq!(r[..2], MARKER);
        assert_eq!(get_string(&r), "SunS");
    }

    #[test]
    fn model_chain_is_discoverable() {
        let image = build_image(&[(1, vec![0; 66]), (103, vec![0; 50]), (123, vec![0; 24])]);
        let models = locate_models(&image[2..]).unwrap();
        let ids: Vec<u16> = models.iter().map(|m| m.id).collect();
        assert_eq!(ids, [1, 103, 123]);
        assert_eq!(models[0].body, 40_004);
        assert_eq!(models[1].body, 40_004 + 66 + 2);
        assert_eq!(models[2].len, 24);
    }
}
