//! CP56Time2a: the seven-octet time tag of IEC 60870-5-101/104 (clause 7.2.6.18 of -101).

use crate::Error;

/// Calendar time with millisecond resolution, as carried on the wire.
///
/// The year is stored as an offset from 2000 (0..=99), as the standard only
/// transmits two digits. `invalid` is the IV bit, `summer_time` the SU bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cp56Time2a {
    pub millisecond: u16, // 0..=59_999, seconds included
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub weekday: u8, // 1 = Monday .. 7 = Sunday, 0 = not used
    pub month: u8,
    pub year: u8, // years since 2000
    pub invalid: bool,
    pub summer_time: bool,
}

impl Cp56Time2a {
    pub const LEN: usize = 7;

    /// Builds a time tag from milliseconds since the Unix epoch (UTC).
    pub fn from_unix_ms(ms: i64) -> Self {
        let days = ms.div_euclid(86_400_000);
        let ms_of_day = ms.rem_euclid(86_400_000);
        let (year, month, day) = civil_from_days(days);
        // 1970-01-01 was a Thursday (ISO weekday 4).
        let weekday = ((days + 3).rem_euclid(7) + 1) as u8;
        Self {
            millisecond: (ms_of_day % 60_000) as u16,
            minute: ((ms_of_day / 60_000) % 60) as u8,
            hour: (ms_of_day / 3_600_000) as u8,
            day: day as u8,
            weekday,
            month: month as u8,
            year: (year - 2000).clamp(0, 99) as u8,
            invalid: false,
            summer_time: false,
        }
    }

    /// Milliseconds since the Unix epoch, interpreting the tag as UTC.
    pub fn to_unix_ms(&self) -> i64 {
        let days = days_from_civil(2000 + self.year as i64, self.month as i64, self.day as i64);
        days * 86_400_000 + self.hour as i64 * 3_600_000 + self.minute as i64 * 60_000 + self.millisecond as i64
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.millisecond.to_le_bytes());
        out.push((self.minute & 0x3F) | if self.invalid { 0x80 } else { 0 });
        out.push((self.hour & 0x1F) | if self.summer_time { 0x80 } else { 0 });
        out.push((self.day & 0x1F) | ((self.weekday & 0x07) << 5));
        out.push(self.month & 0x0F);
        out.push(self.year & 0x7F);
    }

    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() < Self::LEN {
            return Err(Error::Truncated);
        }
        Ok(Self {
            millisecond: u16::from_le_bytes([b[0], b[1]]),
            minute: b[2] & 0x3F,
            invalid: b[2] & 0x80 != 0,
            hour: b[3] & 0x1F,
            summer_time: b[3] & 0x80 != 0,
            day: b[4] & 0x1F,
            weekday: b[4] >> 5,
            month: b[5] & 0x0F,
            year: b[6] & 0x7F,
        })
    }
}

// Howard Hinnant's civil calendar algorithms (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_known_instant() {
        // 2026-09-24 13:45:12.345 UTC, a Thursday.
        let ms = 1_790_257_512_345;
        let t = Cp56Time2a::from_unix_ms(ms);
        assert_eq!((t.year, t.month, t.day, t.weekday), (26, 9, 24, 4));
        assert_eq!((t.hour, t.minute, t.millisecond), (13, 45, 12_345));
        let mut buf = Vec::new();
        t.encode(&mut buf);
        assert_eq!(buf, [0x39, 0x30, 45, 13, 24 | (4 << 5), 9, 26]);
        let back = Cp56Time2a::decode(&buf).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.to_unix_ms(), ms);
    }

    #[test]
    fn flags_survive_encoding() {
        let mut t = Cp56Time2a::from_unix_ms(946_684_800_000); // 2000-01-01
        t.invalid = true;
        t.summer_time = true;
        let mut buf = Vec::new();
        t.encode(&mut buf);
        assert_eq!(Cp56Time2a::decode(&buf).unwrap(), t);
    }
}
