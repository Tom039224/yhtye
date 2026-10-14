//! Which hosts the client may talk to: Tailscale addresses only (the server
//! only listens there too). The traffic is plain HTTP / WebSocket (WireGuard
//! encrypts it), so this check is what keeps a pairing code or a device token
//! from being sent anywhere else (`mobile/…/pairing/TailscaleHosts.kt` is the
//! same rule).
//!
//! - IPv4 100.64.0.0/10 (Tailscale's CGNAT range)
//! - IPv6 fd7a:115c:a1e0::/48 (Tailscale's ULA range)
//! - MagicDNS names (`*.ts.net`)

use std::net::IpAddr;

use crate::remote::is_tailscale_address;

const MAGIC_DNS_SUFFIX: &str = ".ts.net";
/// `<machine>.<tailnet>.ts.net` at least.
const MAGIC_DNS_MIN_LABELS: usize = 4;
const MAX_DNS_LABEL: usize = 63;

/// Which hosts a pairing target may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostPolicy {
    /// Tailscale addresses and MagicDNS names only: what the app uses.
    #[default]
    TailscaleOnly,
    /// Any IP address or DNS name. For tests that run the server on loopback
    /// (a fake Tailscale); the app never uses it.
    AnyHost,
}

impl HostPolicy {
    /// Whether `host` (already [`normalize_host`]ed) may be talked to.
    #[must_use]
    pub fn allows(self, host: &str) -> bool {
        match self {
            Self::TailscaleOnly => is_tailscale_host(host),
            Self::AnyHost => host.parse::<IpAddr>().is_ok() || is_dns_name(host),
        }
    }
}

/// Fullwidth ASCII (`！`–`～`) and the ideographic space as their halfwidth
/// forms: what a Japanese IME types for `１００．８９…` or `ＡＢＣＤ－２３４５`.
#[must_use]
pub fn to_half_width(input: &str) -> String {
    const FIRST: char = '\u{FF01}';
    const LAST: char = '\u{FF5E}';
    const OFFSET: u32 = 0xFEE0;
    input
        .chars()
        .map(|c| match c {
            FIRST..=LAST => char::from_u32(u32::from(c) - OFFSET).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect()
}

/// A host as it is compared and stored: halfwidth, without surrounding spaces,
/// one pair of square brackets and a trailing `.`, in lower case.
#[must_use]
pub fn normalize_host(host: &str) -> String {
    let half = to_half_width(host);
    let host = half.trim();
    let host = host.strip_prefix('[').unwrap_or(host);
    let host = host.strip_suffix(']').unwrap_or(host);
    let host = host.strip_suffix('.').unwrap_or(host);
    host.to_lowercase()
}

/// `host` (as [`normalize_host`] leaves it) is a Tailscale IP address or a
/// MagicDNS name.
#[must_use]
pub fn is_tailscale_host(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(ip) => is_tailscale_address(&ip),
        Err(_) => host.ends_with(MAGIC_DNS_SUFFIX) && is_dns_name(host) && magic_dns_depth(host),
    }
}

fn magic_dns_depth(host: &str) -> bool {
    host.split('.').count() >= MAGIC_DNS_MIN_LABELS
}

/// Letters, digits and hyphens in dot-separated labels (what is safe to put in a URL).
fn is_dns_name(host: &str) -> bool {
    !host.is_empty() && host.split('.').all(is_dns_label)
}

fn is_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_DNS_LABEL
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The host in a URL (an IPv6 address in square brackets). `host` as
/// [`normalize_host`] leaves it.
#[must_use]
pub fn url_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailscale_ips_and_magic_dns_names_are_accepted() {
        for host in [
            "100.64.0.1",
            "100.89.117.15",
            "100.127.255.254",
            "fd7a:115c:a1e0::1",
            "fd7a:115c:a1e0:ab12:4843:cd96:6258:7f10",
            "pc.tail1234.ts.net",
            "my-pc.tail-1234.ts.net",
            "a.b.c.ts.net",
        ] {
            assert!(is_tailscale_host(host), "{host}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for host in [
            "127.0.0.1",
            "192.168.1.5",
            "100.63.255.255",
            "100.128.0.1",
            "10.0.0.1",
            "fd7b:115c:a1e0::1",
            "fe80::1",
            "::ffff:100.64.0.1",
            "example.com",
            "ts.net",
            "tail1234.ts.net",
            "evil.com/.tail1234.ts.net",
            "pc.tail1234.ts.net.evil.com",
            "pc .tail1234.ts.net",
            "-pc.tail1234.ts.net",
            "",
        ] {
            assert!(!is_tailscale_host(host), "{host}");
        }
    }

    #[test]
    fn hosts_are_normalized_before_they_are_compared() {
        assert_eq!(
            normalize_host("  [FD7A:115C:A1E0::1]  "),
            "fd7a:115c:a1e0::1"
        );
        assert_eq!(normalize_host("PC.Tail1234.ts.net."), "pc.tail1234.ts.net");
        assert_eq!(
            normalize_host("１００．８９．１１７．１５"),
            "100.89.117.15"
        );
        assert_eq!(normalize_host("\u{3000}100.64.0.1\u{3000}"), "100.64.0.1");
    }

    #[test]
    fn fullwidth_characters_become_halfwidth() {
        assert_eq!(to_half_width("ＡＢＣＤ－２３４５"), "ABCD-2345");
        assert_eq!(to_half_width("a\u{3000}b"), "a b");
        assert_eq!(to_half_width("日本語 abc"), "日本語 abc");
    }

    #[test]
    fn ipv6_hosts_are_bracketed_in_urls() {
        assert_eq!(url_host("fd7a:115c:a1e0::1"), "[fd7a:115c:a1e0::1]");
        assert_eq!(url_host("100.64.0.1"), "100.64.0.1");
        assert_eq!(url_host("pc.tail1234.ts.net"), "pc.tail1234.ts.net");
    }

    #[test]
    fn the_any_host_policy_still_keeps_url_unsafe_text_out() {
        let any = HostPolicy::AnyHost;
        assert!(any.allows("127.0.0.1"));
        assert!(any.allows("localhost"));
        assert!(any.allows("::1"));
        assert!(!any.allows("a/b"));
        assert!(!any.allows("user@host"));
        assert!(!any.allows(""));
        assert!(!HostPolicy::TailscaleOnly.allows("127.0.0.1"));
    }
}
