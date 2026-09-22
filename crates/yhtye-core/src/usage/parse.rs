//! Reads the Markdown that `claude-agent-acp` 0.81 renders for `/usage`
//! (`dist/usage-markdown.js`, `formatUsageResponse`):
//!
//! ```text
//! > Claude max subscription usage
//! **5-hour limit** — **77%** · Resets Sep 23, 10:50 PM UTC
//! **Weekly · all models** — **99%** · Resets Sep 24, 5:00 PM UTC
//! ```
//!
//! Labels and plan names are Markdown-escaped by the adapter (`\*`, `\_`, ...).
//! The reset time is `Intl.DateTimeFormat("en", {month: "short", day, hour,
//! minute, timeZoneName: "short"})` without a year; the probe runs the adapter
//! with `TZ=UTC` so the zone is `UTC` (a `GMT±h[:mm]` offset is read too).

use super::{UsageReport, UsageWindow, UsageWindowKind};

const PLAN_PREFIX: &str = "> Claude ";
const PLAN_SUFFIX: &str = " subscription usage";
const WINDOW_SEP: &str = "** — **";
const RESETS: &str = " · Resets ";
const DAY_MS: i64 = 86_400_000;

/// The usage in `/usage` Markdown, or `None` when the text is not that output
/// (e.g. Claude Code's own plain-text fallback, or a changed format). Windows
/// with an unreadable line are skipped; a reset time that cannot be read is `None`.
#[must_use]
pub fn parse_usage_markdown(text: &str, now_ms: u64) -> Option<UsageReport> {
    let mut plan = None;
    let mut windows = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix(PLAN_PREFIX)
            && let Some(name) = rest.strip_suffix(PLAN_SUFFIX)
        {
            plan = Some(unescape(name));
        } else if let Some(w) = parse_window(line, now_ms) {
            windows.push(w);
        }
    }
    if plan.is_none() && windows.is_empty() {
        return None;
    }
    Some(UsageReport {
        plan,
        windows,
        fetched_at_ms: now_ms,
    })
}

/// `**<label>** — **<percent>%**[ · Resets <when>]`
fn parse_window(line: &str, now_ms: u64) -> Option<UsageWindow> {
    let body = line.strip_prefix("**")?;
    let (label, rest) = body.split_once(WINDOW_SEP)?;
    let (percent, rest) = rest.split_once("%**")?;
    let percent: f64 = percent.trim().parse().ok()?;
    if !(0.0..=100.0).contains(&percent) {
        return None;
    }
    let resets_at_ms = rest
        .strip_prefix(RESETS)
        .and_then(|when| parse_reset(when.trim(), now_ms));
    let label = unescape(label);
    Some(UsageWindow {
        kind: window_kind(&label),
        label,
        percent,
        resets_at_ms,
    })
}

fn window_kind(label: &str) -> UsageWindowKind {
    match label {
        "5-hour limit" => UsageWindowKind::FiveHour,
        "Weekly · all models" => UsageWindowKind::Week,
        _ => UsageWindowKind::Other,
    }
}

/// Undoes the adapter's `escapeMarkdown` (a backslash before a punctuation char).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(next) = chars.next()
        {
            out.push(next);
        } else {
            out.push(c);
        }
    }
    out
}

/// `Sep 25, 2:00 AM UTC` → ms since the epoch. The year is not printed: the
/// candidate (last, this or next year) closest to `now` is taken — a reset is
/// always within days of now.
fn parse_reset(when: &str, now_ms: u64) -> Option<u64> {
    let (date, time) = when.split_once(", ")?;
    let (month, day) = date.split_once(' ')?;
    let month = month_number(month)?;
    let day: u32 = day.parse().ok().filter(|d| (1..=31).contains(d))?;
    let mut parts = time.split(' ');
    let clock = parts.next()?;
    let meridiem = parts.next()?;
    let offset_min = zone_offset_minutes(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let (hour, minute) = clock.split_once(':')?;
    let hour: u32 = hour.parse().ok().filter(|h| (1..=12).contains(h))?;
    let minute: u32 = minute.parse().ok().filter(|m| *m < 60)?;
    let hour = match meridiem {
        "AM" => hour % 12,
        "PM" => hour % 12 + 12,
        _ => return None,
    };
    let now = i64::try_from(now_ms).ok()?;
    let this_year = civil_year(now);
    let best = (this_year - 1..=this_year + 1)
        .map(|y| {
            let local = days_from_civil(y, month, day) * DAY_MS
                + i64::from(hour) * 3_600_000
                + i64::from(minute) * 60_000;
            local - offset_min * 60_000
        })
        .min_by_key(|t| (t - now).abs())?;
    u64::try_from(best).ok()
}

fn month_number(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|m| *m == name)
        .and_then(|i| u32::try_from(i + 1).ok())
}

/// `UTC` / `GMT` → 0, `GMT+9` → 540, `GMT-5:30` → -330. Other names → `None`.
fn zone_offset_minutes(zone: &str) -> Option<i64> {
    if zone == "UTC" || zone == "GMT" {
        return Some(0);
    }
    let rest = zone.strip_prefix("GMT")?;
    let (sign, rest) = match rest.as_bytes().first()? {
        b'+' => (1, &rest[1..]),
        b'-' => (-1, &rest[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
    let h: i64 = h.parse().ok().filter(|h| *h <= 14)?;
    let m: i64 = m.parse().ok().filter(|m| *m < 60)?;
    Some(sign * (h * 60 + m))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The UTC year of `ms` since the epoch.
fn civil_year(ms: i64) -> i64 {
    let days = ms.div_euclid(DAY_MS);
    // Inverse of days_from_civil, year only.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-23T00:00:00Z
    const NOW: u64 = 1_790_121_600_000;

    /// Real output of `/usage` (claude-agent-acp 0.81, Claude Code 2.1.280),
    /// with the reset times as the probe gets them (`TZ=UTC`).
    const SAMPLE: &str = "## Usage

> Claude max subscription usage

### Limits

**5-hour limit** — **77%** · Resets Sep 22, 10:50 PM UTC

`███████████████░░░░░`

**Weekly · all models** — **99%** · Resets Sep 24, 5:00 PM UTC

`████████████████████`

**Weekly · Fable** — **7%** · Resets Sep 24, 5:00 PM UTC

`█░░░░░░░░░░░░░░░░░░░`

---

### This session

| Cost | API time | Active |
|:--|:--|:--|
| $0.05 | 2s | 7s |
";

    #[test]
    fn epoch_helpers_match_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 23) * DAY_MS, NOW as i64);
        assert_eq!(civil_year(NOW as i64), 2026);
        assert_eq!(civil_year(days_from_civil(2027, 1, 1) * DAY_MS - 1), 2026);
        assert_eq!(civil_year(days_from_civil(2024, 2, 29) * DAY_MS), 2024);
    }

    #[test]
    fn reads_the_plan_and_every_window() {
        let r = parse_usage_markdown(SAMPLE, NOW).expect("usage");
        assert_eq!(r.plan.as_deref(), Some("max"));
        assert_eq!(r.fetched_at_ms, NOW);
        let labels: Vec<_> = r.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(
            labels,
            ["5-hour limit", "Weekly · all models", "Weekly · Fable"]
        );
        let five = r.window(UsageWindowKind::FiveHour).expect("5h");
        assert!((five.percent - 77.0).abs() < f64::EPSILON);
        // Sep 22 22:50 UTC = NOW - 1h10m
        assert_eq!(five.resets_at_ms, Some(NOW - 70 * 60_000));
        let week = r.window(UsageWindowKind::Week).expect("week");
        assert!((week.percent - 99.0).abs() < f64::EPSILON);
        assert_eq!(week.resets_at_ms, Some(NOW + (24 + 17) * 3_600_000));
        assert_eq!(r.windows[2].kind, UsageWindowKind::Other);
    }

    #[test]
    fn reads_gmt_offsets_fractions_and_escapes() {
        let text = "> Claude team\\_premium subscription usage\n\
                    **5-hour limit** — **12.5%** · Resets Sep 23, 7:50 AM GMT+9\n\
                    **Weekly \\* odd** — **3%**";
        let r = parse_usage_markdown(text, NOW).expect("usage");
        assert_eq!(r.plan.as_deref(), Some("team_premium"));
        let five = &r.windows[0];
        assert!((five.percent - 12.5).abs() < f64::EPSILON);
        // 07:50 GMT+9 = 22:50 UTC the day before
        assert_eq!(five.resets_at_ms, Some(NOW - 70 * 60_000));
        assert_eq!(r.windows[1].label, "Weekly * odd");
        assert_eq!(r.windows[1].resets_at_ms, None);
    }

    #[test]
    fn a_year_boundary_picks_the_nearest_year() {
        let dec31 = (days_from_civil(2026, 12, 31) * DAY_MS) as u64;
        let text = "**5-hour limit** — **1%** · Resets Jan 1, 2:00 AM UTC";
        let r = parse_usage_markdown(text, dec31).expect("usage");
        assert_eq!(
            r.windows[0].resets_at_ms,
            Some((days_from_civil(2027, 1, 1) * DAY_MS + 2 * 3_600_000) as u64)
        );
    }

    #[test]
    fn unreadable_parts_are_left_out_not_guessed() {
        // Claude Code's own plain-text output (the adapter's fallback) and prose.
        assert_eq!(parse_usage_markdown("Current session: 12% used", NOW), None);
        assert_eq!(parse_usage_markdown("", NOW), None);
        let text = "**5-hour limit** — **150%** · Resets Sep 23, 1:00 AM UTC\n\
                    **Weekly · all models** — **40%** · Resets Sep 25, 1:00 AM PDT\n\
                    **Weekly · Opus** — **x%**";
        let r = parse_usage_markdown(text, NOW).expect("usage");
        assert_eq!(r.windows.len(), 1, "{r:?}");
        assert_eq!(r.windows[0].kind, UsageWindowKind::Week);
        assert_eq!(r.windows[0].resets_at_ms, None, "unknown zone name");
        assert_eq!(r.plan, None);
    }
}
