//! Calendar arithmetic for day bucketing (no date library: only UTC
//! timestamps in, civil dates out) and the RFC 3339 timestamps of the logs.

/// Milliseconds in a day.
pub(super) const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// Days since 1970-01-01 of the civil date `y-m-d` (proleptic Gregorian;
/// Howard Hinnant's `days_from_civil`).
pub(super) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of a day number (inverse of
/// [`days_from_civil`]).
pub(super) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `YYYY-MM-DD` of a day number.
pub(super) fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The local day number of `ts_ms` for a client `tz_offset_min` minutes ahead
/// of UTC (day 0 is 1970-01-01).
pub(super) fn local_day(ts_ms: i64, tz_offset_min: i32) -> i64 {
    (ts_ms + i64::from(tz_offset_min) * MINUTE_MS).div_euclid(DAY_MS)
}

/// UTC milliseconds of the start of local day `day`.
pub(super) fn local_day_start_ms(day: i64, tz_offset_min: i32) -> i64 {
    day * DAY_MS - i64::from(tz_offset_min) * MINUTE_MS
}

/// Milliseconds since the Unix epoch of an RFC 3339 timestamp
/// (`2026-09-21T06:41:56.760Z`, `…+09:00`), `None` for anything else or a
/// time before 1970.
pub(super) fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let digits = b.get(from..to)?;
        digits
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut at = 19;
    let mut millis = 0;
    if b.get(at) == Some(&b'.') {
        let start = at + 1;
        at = start;
        while b.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return None;
        }
        // The first three digits are the milliseconds.
        let frac = &s[start..at.min(start + 3)];
        millis = frac.parse::<i64>().ok()? * 10_i64.pow(u32::try_from(3 - frac.len()).ok()?);
    }
    let offset_min = match b.get(at)? {
        b'Z' | b'z' if at + 1 == b.len() => 0,
        sign @ (b'+' | b'-') => {
            let (oh, om) = (num(at + 1, at + 3)?, num(at + 4, at + 6)?);
            if b.get(at + 3) != Some(&b':') || at + 6 != b.len() {
                return None;
            }
            let minutes = oh * 60 + om;
            if *sign == b'-' { -minutes } else { minutes }
        }
        _ => return None,
    };
    let days = days_from_civil(y, u32::try_from(mo).ok()?, u32::try_from(d).ok()?);
    let secs = days * 86_400 + h * 3600 + mi * 60 + sec - offset_min * 60;
    u64::try_from(secs * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_round_trip_through_day_numbers() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for day in [-800_000, -1, 0, 59, 60, 11_016, 20_000, 2_932_896] {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day, "{y}-{m}-{d}");
        }
        assert_eq!(format_date(days_from_civil(2024, 2, 29)), "2024-02-29");
        assert_eq!(format_date(0), "1970-01-01");
    }

    #[test]
    fn timestamps_are_read_with_fractions_and_offsets() {
        let z = parse_rfc3339_ms("2026-09-21T06:41:56.760Z").unwrap();
        assert_eq!(z % 1000, 760);
        assert_eq!(parse_rfc3339_ms("2026-09-21T06:41:56Z").unwrap(), z - 760);
        // 15:41:56.760 in +09:00 is the same instant.
        assert_eq!(parse_rfc3339_ms("2026-09-21T15:41:56.76+09:00"), Some(z));
        assert_eq!(parse_rfc3339_ms("2026-09-21T06:41:56.7609999Z"), Some(z));
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        for bad in [
            "",
            "2026-09-21",
            "2026-13-01T00:00:00Z",
            "2026-09-21T06:41:56",
            "x",
        ] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn days_follow_the_clients_offset() {
        let t = parse_rfc3339_ms("2026-09-21T20:00:00Z").unwrap() as i64;
        let utc = local_day(t, 0);
        assert_eq!(format_date(utc), "2026-09-21");
        // Tokyo is already on the 22nd; Los Angeles (UTC-7) still on the 21st.
        assert_eq!(format_date(local_day(t, 540)), "2026-09-22");
        assert_eq!(format_date(local_day(t, -420)), "2026-09-21");
        assert_eq!(
            local_day_start_ms(local_day(t, 540), 540),
            parse_rfc3339_ms("2026-09-21T15:00:00Z").unwrap() as i64
        );
    }
}
