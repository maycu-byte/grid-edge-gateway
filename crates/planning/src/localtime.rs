//! Central European time without a time-zone database: Germany, Austria
//! and Switzerland all use CET (UTC+1) and CEST (UTC+2) from the last
//! Sunday of March to the last Sunday of October, switching at 01:00 UTC.

/// A calendar date and the day of the week (0 = Monday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
}

/// Date of a day number (days since 1970-01-01), proleptic Gregorian
/// (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> Civil {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Civil { year, month, day, weekday: (days + 3).rem_euclid(7) as u32 }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix time of a UTC date and time.
pub fn unix_from_utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    days_from_civil(year, month, day) * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
}

/// The UTC date at `unix_s`.
pub fn civil_from_unix(unix_s: f64) -> Civil {
    civil_from_days((unix_s / 86_400.0).floor() as i64)
}

/// Unix time of 01:00 UTC on the last Sunday of `month` (31-day months only).
fn last_sunday_0100(year: i32, month: u32) -> i64 {
    let d31 = days_from_civil(year, month, 31);
    let back = (civil_from_days(d31).weekday + 1) % 7; // Monday = 0 → Sunday = 6
    (d31 - i64::from(back)) * 86_400 + 3600
}

/// Offset of Central European time from UTC at `unix_s`, seconds.
pub fn cet_offset_s(unix_s: f64) -> f64 {
    let t = unix_s.floor() as i64;
    let year = civil_from_unix(unix_s).year;
    let summer = (last_sunday_0100(year, 3)..last_sunday_0100(year, 10)).contains(&t);
    if summer { 7200.0 } else { 3600.0 }
}

/// Seconds since local midnight in Central European time.
pub fn local_seconds_of_day(unix_s: f64) -> f64 {
    (unix_s + cet_offset_s(unix_s)).rem_euclid(86_400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix(y: i32, mo: u32, d: u32, h: i64, mi: i64) -> f64 {
        (days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60) as f64
    }

    #[test]
    fn dates_round_trip() {
        for days in [-1, 0, 59, 365, 11_016, 20_000, 20_720] {
            let c = civil_from_days(days);
            assert_eq!(days_from_civil(c.year, c.month, c.day), days);
        }
        assert_eq!(civil_from_unix(0.0), Civil { year: 1970, month: 1, day: 1, weekday: 3 });
        assert_eq!(civil_from_unix(unix(2025, 1, 20, 12, 0)).weekday, 0, "20 Jan 2025 was a Monday");
        assert_eq!(civil_from_unix(unix(2024, 2, 29, 0, 0)).day, 29);
    }

    #[test]
    fn summer_time_follows_the_eu_rule() {
        // 2025: 30 March and 26 October, at 01:00 UTC.
        assert_eq!(cet_offset_s(unix(2025, 3, 30, 0, 59)), 3600.0);
        assert_eq!(cet_offset_s(unix(2025, 3, 30, 1, 0)), 7200.0);
        assert_eq!(cet_offset_s(unix(2025, 10, 26, 0, 59)), 7200.0);
        assert_eq!(cet_offset_s(unix(2025, 10, 26, 1, 0)), 3600.0);
        // 2026: 29 March and 25 October.
        assert_eq!(cet_offset_s(unix(2026, 3, 29, 1, 30)), 7200.0);
        assert_eq!(cet_offset_s(unix(2026, 10, 25, 1, 30)), 3600.0);
        assert_eq!(cet_offset_s(unix(2026, 1, 15, 12, 0)), 3600.0);
    }

    #[test]
    fn local_time_of_day() {
        // 16:00 UTC is 17:00 in January and 18:00 in July.
        assert_eq!(local_seconds_of_day(unix(2025, 1, 20, 16, 0)), 17.0 * 3600.0);
        assert_eq!(local_seconds_of_day(unix(2025, 7, 1, 16, 0)), 18.0 * 3600.0);
        assert_eq!(local_seconds_of_day(unix(2025, 7, 1, 22, 30)), 30.0 * 60.0, "past local midnight");
    }
}
