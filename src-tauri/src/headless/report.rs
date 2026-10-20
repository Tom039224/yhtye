//! What the headless instance and `yhtye remote …` print: the status, the
//! devices, and the pairing (URI, QR code for the terminal, host / port / code
//! to type in). Pure functions from the API's views to text.

use std::fmt::Write as _;

use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;
use yhtye_core::remote::{PairingCode, RemoteDevice, RemoteStatus};

/// The pairing, to show on a terminal.
#[must_use]
pub fn pairing(code: &PairingCode, status: &RemoteStatus, now_ms: u64) -> String {
    let mut out = String::new();
    match qr(&code.uri) {
        Some(qr) => {
            let _ = writeln!(
                out,
                "Scan this QR code with the Yhtye Android app:\n\n{qr}\n"
            );
            out.push_str("or enter these by hand:\n");
        }
        None => out.push_str("Enter these in the Yhtye Android app:\n"),
    }
    let host = status.dns_name.as_deref().or(status.ip.as_deref());
    if let Some(host) = host {
        let _ = writeln!(out, "  host  {host}");
    }
    let _ = writeln!(out, "  port  {}", status.port);
    let _ = writeln!(out, "  code  {}", code.code);
    let _ = writeln!(out, "  uri   {}", code.uri);
    let left = code.expires_at_ms.saturating_sub(now_ms).div_ceil(1000);
    let _ = writeln!(
        out,
        "The code works once and expires {} (a new one replaces it).",
        if left == 0 {
            "now".to_string()
        } else {
            format!("in {}:{:02}", left / 60, left % 60)
        }
    );
    out
}

/// The URI as a QR code of half-block characters, drawn for a terminal with a
/// light foreground on a dark background (the usual one; scanners also read
/// the inverted code). `None` when it does not fit in a QR code.
#[must_use]
pub fn qr(uri: &str) -> Option<String> {
    let code = QrCode::new(uri.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            // A dark module is the background: the code reads dark on light.
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
    )
}

/// The first lines of `remote status`: the headless instance that answers.
#[must_use]
pub fn instance(pid: Option<u32>, data_dir: &std::path::Path) -> String {
    let pid = pid.map_or_else(String::new, |pid| format!(", pid {pid}"));
    format!(
        "yhtye          running (headless{pid})\ndata dir       {}\n",
        data_dir.display()
    )
}

/// `remote status`.
#[must_use]
pub fn status(s: &RemoteStatus, now_ms: u64) -> String {
    let mut out = String::new();
    let state = if s.running {
        "listening"
    } else if s.enabled {
        "not running"
    } else {
        "off"
    };
    let _ = writeln!(out, "remote access  {state}");
    let _ = writeln!(
        out,
        "enabled        {}",
        if s.enabled { "yes" } else { "no" }
    );
    let _ = writeln!(out, "port           {}", s.port);
    if !s.addresses.is_empty() {
        let _ = writeln!(out, "addresses      {}", s.addresses.join(", "));
    }
    if let Some(dns) = &s.dns_name {
        let _ = writeln!(out, "host           {dns}");
    }
    if let Some(ip) = &s.ip {
        let _ = writeln!(out, "ip             {ip}");
    }
    if let Some(error) = &s.error {
        let _ = writeln!(out, "error          {error}");
    }
    if let Some(expires) = s.pairing_expires_at_ms.filter(|e| *e > now_ms) {
        let _ = writeln!(out, "pairing code   valid until {}", utc(expires));
    }
    out
}

/// `remote devices`.
#[must_use]
pub fn devices(devices: &[RemoteDevice]) -> String {
    if devices.is_empty() {
        return "No paired devices (pair one with `yhtye remote pair`).\n".into();
    }
    let rows: Vec<[String; 5]> = devices
        .iter()
        .map(|d| {
            [
                d.id.clone(),
                d.name.clone(),
                if d.connected { "connected" } else { "offline" }.to_string(),
                utc(d.created_ms),
                d.last_seen_ms.map_or_else(|| "never".to_string(), utc),
            ]
        })
        .collect();
    table(["ID", "NAME", "STATE", "PAIRED", "LAST SEEN"], &rows)
}

/// Columns padded to the widest cell (the last one is not padded).
fn table<const N: usize>(head: [&str; N], rows: &[[String; N]]) -> String {
    let mut widths = head.map(str::len);
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    let mut line = |cells: Vec<&str>| {
        let last = cells.len() - 1;
        for (i, cell) in cells.iter().enumerate() {
            if i == last {
                out.push_str(cell);
            } else {
                let _ = write!(out, "{cell:<width$}  ", width = widths[i]);
            }
        }
        out.push('\n');
    };
    line(head.to_vec());
    for row in rows {
        line(row.iter().map(String::as_str).collect());
    }
    out
}

/// `2026-10-14 09:30:00 UTC` (no time zone database is needed for it).
#[must_use]
pub fn utc(ms: u64) -> String {
    let secs = ms / 1000;
    let (year, month, day) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(i64::MAX));
    let rest = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yhtye_core::remote::ClientKind;

    const URI: &str = "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=100.64.0.7&port=1423&code=ABCD2345&sid=0123456789abcdef0123456789abcdef";

    fn remote_status() -> RemoteStatus {
        RemoteStatus {
            enabled: true,
            port: 1423,
            running: true,
            addresses: vec!["100.64.0.7:1423".into()],
            ip: Some("100.64.0.7".into()),
            dns_name: Some("pc.tail1234.ts.net".into()),
            server_id: "0123456789abcdef0123456789abcdef".into(),
            error: None,
            pairing_expires_at_ms: None,
        }
    }

    fn pairing_code() -> PairingCode {
        PairingCode {
            code: "ABCD2345".into(),
            expires_at_ms: 1_000_000 + 300_000,
            uri: URI.into(),
        }
    }

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_700_000_000_000), "2023-11-14 22:13:20 UTC");
        // A leap day, and the day after a century that is not a leap year.
        assert_eq!(utc(1_709_164_800_000), "2024-02-29 00:00:00 UTC");
        assert_eq!(utc(4_107_542_400_000), "2100-03-01 00:00:00 UTC");
        // Not a panic at the largest value.
        assert!(utc(u64::MAX).ends_with("UTC"));
    }

    #[test]
    fn the_qr_code_is_drawn_with_half_blocks_and_holds_the_uri() {
        let text = qr(URI).expect("a QR code");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() > 15, "{} lines", lines.len());
        let width = lines[0].chars().count();
        assert!(
            lines.iter().all(|l| l.chars().count() == width),
            "a rectangle"
        );
        assert!(text
            .chars()
            .all(|c| matches!(c, ' ' | '▀' | '▄' | '█' | '\n')));
        // The dark terminal's polarity: the quiet zone (light modules) is drawn.
        assert!(lines[0].chars().all(|c| c == '█'), "{:?}", lines[0]);
    }

    #[test]
    fn a_uri_too_long_for_a_qr_code_is_left_out() {
        assert!(qr(&"x".repeat(5000)).is_none());
    }

    #[test]
    fn the_pairing_shows_the_qr_code_and_what_to_type_in() {
        let text = pairing(&pairing_code(), &remote_status(), 1_000_000);
        assert!(text.contains('█'), "the QR code");
        assert!(text.contains("host  pc.tail1234.ts.net"), "{text}");
        assert!(text.contains("port  1423"));
        assert!(text.contains("code  ABCD2345"));
        assert!(text.contains(&format!("uri   {URI}")));
        assert!(text.contains("in 5:00"), "{text}");
    }

    #[test]
    fn the_pairing_falls_back_to_the_ip_without_magic_dns_and_says_when_it_expired() {
        let mut s = remote_status();
        s.dns_name = None;
        let text = pairing(&pairing_code(), &s, 2_000_000);
        assert!(text.contains("host  100.64.0.7"), "{text}");
        assert!(text.contains("expires now"), "{text}");
    }

    #[test]
    fn the_instance_lines_name_the_pid_when_it_is_known() {
        let dir = std::path::Path::new("/srv/yhtye");
        assert_eq!(
            instance(Some(4242), dir),
            "yhtye          running (headless, pid 4242)\ndata dir       /srv/yhtye\n"
        );
        assert!(instance(None, dir).starts_with("yhtye          running (headless)\n"));
    }

    #[test]
    fn the_status_says_where_remote_access_listens_or_why_it_does_not() {
        let text = status(&remote_status(), 0);
        assert!(text.starts_with("remote access  listening"), "{text}");
        assert!(text.contains("addresses      100.64.0.7:1423"));
        assert!(text.contains("host           pc.tail1234.ts.net"));

        let mut down = remote_status();
        down.running = false;
        down.addresses.clear();
        down.error = Some("tailscale is not running".into());
        down.pairing_expires_at_ms = Some(5_000);
        let text = status(&down, 0);
        assert!(text.starts_with("remote access  not running"), "{text}");
        assert!(text.contains("error          tailscale is not running"));
        assert!(text.contains("pairing code   valid until 1970-01-01 00:00:05 UTC"));
        // An expired code is not mentioned.
        assert!(!status(&down, 6_000).contains("pairing code"));

        down.enabled = false;
        assert!(status(&down, 0).starts_with("remote access  off"));
    }

    #[test]
    fn the_devices_are_listed_in_aligned_columns() {
        let list = [
            RemoteDevice {
                id: "d1".into(),
                name: "Pixel 8".into(),
                kind: ClientKind::Android,
                created_ms: 0,
                last_seen_ms: Some(1_700_000_000_000),
                connected: true,
            },
            RemoteDevice {
                id: "device-2".into(),
                name: "Tablet".into(),
                kind: ClientKind::Desktop,
                created_ms: 1_700_000_000_000,
                last_seen_ms: None,
                connected: false,
            },
        ];
        let text = devices(&list);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].starts_with("ID        NAME     STATE      PAIRED"),
            "{text}"
        );
        assert!(lines[1]
            .starts_with("d1        Pixel 8  connected  1970-01-01 00:00:00 UTC  2023-11-14"));
        assert!(lines[2].starts_with("device-2  Tablet   offline    2023-11-14"));
        assert!(lines[2].ends_with("never"));
        assert!(devices(&[]).contains("No paired devices"));
    }
}
