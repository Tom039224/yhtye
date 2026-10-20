//! Whom to pair with: the pairing URI of a QR code / deep link and what a
//! person types (host, port, code). A port of
//! `mobile/…/pairing/PairingTarget.kt`, so both clients read the same input the
//! same way.

use std::fmt;

use super::address::{HostPolicy, normalize_host, to_half_width};
use crate::remote::{CODE_ALPHABET, CODE_LEN, DEFAULT_PORT, MAX_DEVICE_NAME_CHARS};

/// The pairing URI's scheme and host (`yhtye://pair?...`).
const URI_PREFIX: &str = "yhtye://pair";
/// The URI format this client reads (`v=`).
const URI_VERSION: &str = "1";
/// Longest port number as text.
const MAX_PORT_DIGITS: usize = 5;

/// Whom to pair with.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingTarget {
    hosts: Vec<String>,
    port: u16,
    code: String,
    server_id: Option<String>,
}

impl PairingTarget {
    /// The hosts to try, in order (MagicDNS name, then IP). Never empty; each
    /// one is allowed by the [`HostPolicy`] it was read with.
    #[must_use]
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The code in its normal form (8 upper-case characters, no separators).
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The server's id when the target came from a QR code (`sid`); the
    /// pairing refuses an answer from another server. Unknown for manual entry.
    #[must_use]
    pub fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }
}

// The code is not for screens or logs.
impl fmt::Debug for PairingTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingTarget")
            .field("hosts", &self.hosts)
            .field("port", &self.port)
            .field("server_id", &self.server_id)
            .finish_non_exhaustive()
    }
}

/// Why an input cannot be used (the UI words it).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPairingInput {
    /// Not `yhtye://pair?...` (another QR code).
    #[error("this is not a Yhtye pairing code")]
    NotPairingUri,
    /// A QR code of a newer format: this app needs an update.
    #[error("this pairing code is of a newer format (v={0}); update Yhtye")]
    UnsupportedVersion(String),
    #[error("no host was given")]
    MissingHost,
    /// Not a Tailscale address (100.64.0.0/10, fd7a:115c:a1e0::/48, `*.ts.net`).
    #[error("{0} is not a Tailscale address")]
    NotTailscaleHost(String),
    #[error("the port must be a number from 1 to 65535")]
    InvalidPort,
    #[error("no pairing code was given")]
    MissingCode,
    /// Not 8 characters, or characters the codes do not use (0/O, 1/I/L, ...).
    #[error("the pairing code must be {CODE_LEN} letters and digits (no 0, O, 1, I or L)")]
    InvalidCode,
}

/// A code as a person types it, in the form the server expects: spaces and `-`
/// removed, upper case (`abcd-2345` → `ABCD2345`); `None` if it is not 8
/// characters of the code alphabet.
#[must_use]
pub fn normalize_pairing_code(input: &str) -> Option<String> {
    let code: String = to_half_width(input)
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect();
    (code.chars().count() == CODE_LEN
        && code
            .chars()
            .all(|c| u8::try_from(c).is_ok_and(|b| CODE_ALPHABET.contains(&b))))
    .then_some(code)
}

/// A device name the way the server keeps it: control characters and
/// surrounding spaces removed, 1 to [`MAX_DEVICE_NAME_CHARS`] characters;
/// `None` if that leaves nothing or too much.
#[must_use]
pub fn normalize_device_name(input: &str) -> Option<String> {
    let name: String = input.chars().filter(|c| !c.is_control()).collect();
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_DEVICE_NAME_CHARS).then(|| name.to_string())
}

/// Whether the text looks like a pairing URI (to let other QR codes pass while scanning).
#[must_use]
pub fn looks_like_pairing_uri(text: &str) -> bool {
    text.trim()
        .get(..URI_PREFIX.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(URI_PREFIX))
}

/// Reads `yhtye://pair?v=1&host=<MagicDNS name>&ip=<100.x>&port=<N>&code=<code>&sid=<server id>`
/// (what the PC's QR code holds). `host` may be missing; values are
/// percent-encoded. Only Tailscale hosts are accepted.
pub fn parse_pairing_uri(uri: &str) -> Result<PairingTarget, InvalidPairingInput> {
    parse_pairing_uri_with(uri, HostPolicy::TailscaleOnly)
}

/// [`parse_pairing_uri`] with another [`HostPolicy`] (tests only).
pub fn parse_pairing_uri_with(
    uri: &str,
    policy: HostPolicy,
) -> Result<PairingTarget, InvalidPairingInput> {
    let text = uri.trim();
    if !looks_like_pairing_uri(text) {
        return Err(InvalidPairingInput::NotPairingUri);
    }
    let rest = &text[URI_PREFIX.len()..];
    if !(rest.is_empty() || rest.starts_with('?') || rest == "/" || rest.starts_with("/?")) {
        return Err(InvalidPairingInput::NotPairingUri);
    }
    let query = rest.split_once('?').map_or("", |(_, q)| q);
    let query = query.split_once('#').map_or(query, |(q, _)| q);
    let params = parse_query(query);
    let version = params.get("v").ok_or(InvalidPairingInput::NotPairingUri)?;
    if version != URI_VERSION {
        return Err(InvalidPairingInput::UnsupportedVersion(version.clone()));
    }
    let hosts: Vec<&str> = ["host", "ip"]
        .iter()
        .filter_map(|k| params.get(*k))
        .map(String::as_str)
        .filter(|h| !h.trim().is_empty())
        .collect();
    let server_id = params.get("sid").filter(|s| !s.trim().is_empty()).cloned();
    build(
        &hosts,
        params.get("port").map(String::as_str),
        params.get("code").map(String::as_str),
        server_id,
        policy,
    )
}

/// Reads what a person typed: a host name or IP, the port (empty: 1423) and the code.
pub fn parse_manual_entry(
    host: &str,
    port: &str,
    code: &str,
) -> Result<PairingTarget, InvalidPairingInput> {
    parse_manual_entry_with(host, port, code, HostPolicy::TailscaleOnly)
}

/// [`parse_manual_entry`] with another [`HostPolicy`] (tests only).
pub fn parse_manual_entry_with(
    host: &str,
    port: &str,
    code: &str,
    policy: HostPolicy,
) -> Result<PairingTarget, InvalidPairingInput> {
    let hosts: Vec<&str> = [host]
        .into_iter()
        .filter(|h| !h.trim().is_empty())
        .collect();
    let port = port.trim();
    let default_port = DEFAULT_PORT.to_string();
    let port = if port.is_empty() { &default_port } else { port };
    build(&hosts, Some(port), Some(code), None, policy)
}

fn build(
    hosts: &[&str],
    port: Option<&str>,
    code: Option<&str>,
    server_id: Option<String>,
    policy: HostPolicy,
) -> Result<PairingTarget, InvalidPairingInput> {
    if hosts.is_empty() {
        return Err(InvalidPairingInput::MissingHost);
    }
    let mut normalized: Vec<String> = Vec::with_capacity(hosts.len());
    for host in hosts.iter().map(|h| normalize_host(h)) {
        if !normalized.contains(&host) {
            normalized.push(host);
        }
    }
    if let Some(bad) = normalized.iter().find(|h| !policy.allows(h)) {
        return Err(InvalidPairingInput::NotTailscaleHost(bad.clone()));
    }
    let port = port
        .and_then(parse_port)
        .ok_or(InvalidPairingInput::InvalidPort)?;
    let code = match code {
        Some(c) if !c.trim().is_empty() => c,
        _ => return Err(InvalidPairingInput::MissingCode),
    };
    let code = normalize_pairing_code(code).ok_or(InvalidPairingInput::InvalidCode)?;
    Ok(PairingTarget {
        hosts: normalized,
        port,
        code,
        server_id,
    })
}

fn parse_port(text: &str) -> Option<u16> {
    let text = to_half_width(text);
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_PORT_DIGITS || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse::<u16>().ok().filter(|p| *p != 0)
}

/// `a=1&b=x%20y` → the first value of each key.
fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(percent_decode(key))
            .or_insert_with(|| percent_decode(value));
    }
    out
}

/// RFC 3986 percent-decoding (UTF-8); `+` stays a plus, a broken escape stays as it is.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| hex_byte(bytes[i + 1], bytes[i + 2]))
            .flatten();
        match escaped {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(hi: u8, lo: u8) -> Option<u8> {
    let digit = |b: u8| char::from(b).to_digit(16);
    u8::try_from(digit(hi)? * 16 + digit(lo)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=100.89.117.15&port=1423&code=ABCD2345&sid=sid1";

    #[test]
    fn a_full_uri_gives_both_hosts_the_port_code_and_server() {
        let t = parse_pairing_uri(FULL).expect("parsed");
        assert_eq!(t.hosts(), ["pc.tail1234.ts.net", "100.89.117.15"]);
        assert_eq!(t.port(), 1423);
        assert_eq!(t.code(), "ABCD2345");
        assert_eq!(t.server_id(), Some("sid1"));
    }

    #[test]
    fn the_uri_may_lack_the_dns_name_and_use_ipv6_escaped() {
        let t = parse_pairing_uri(
            "yhtye://pair?v=1&ip=fd7a%3A115c%3Aa1e0%3A%3A1&port=1423&code=abcd-2345",
        )
        .expect("parsed");
        assert_eq!(t.hosts(), ["fd7a:115c:a1e0::1"]);
        assert_eq!(t.code(), "ABCD2345");
        assert_eq!(t.server_id(), None);
    }

    #[test]
    fn the_scheme_is_matched_without_regard_to_case_and_the_slash_form_works() {
        assert!(parse_pairing_uri("YHTYE://PAIR/?v=1&ip=100.64.0.1&port=1&code=ABCD2345").is_ok());
        assert!(
            parse_pairing_uri("  yhtye://pair?v=1&ip=100.64.0.1&port=1&code=ABCD2345 ").is_ok()
        );
        assert!(looks_like_pairing_uri(" yhtye://pair?x"));
        assert!(!looks_like_pairing_uri("https://example.com"));
        assert!(!looks_like_pairing_uri("日本語"));
    }

    #[test]
    fn other_uris_and_formats_are_told_apart() {
        for bad in [
            "https://example.com",
            "yhtye://pairing?v=1",
            "yhtye://pair/x?v=1&ip=100.64.0.1&port=1&code=ABCD2345",
            "yhtye://pair?ip=100.64.0.1&port=1&code=ABCD2345",
            "",
        ] {
            assert_eq!(
                parse_pairing_uri(bad),
                Err(InvalidPairingInput::NotPairingUri),
                "{bad}"
            );
        }
        assert_eq!(
            parse_pairing_uri("yhtye://pair?v=2&ip=100.64.0.1&port=1&code=ABCD2345"),
            Err(InvalidPairingInput::UnsupportedVersion("2".into()))
        );
    }

    #[test]
    fn the_first_value_of_a_repeated_key_wins() {
        let t = parse_pairing_uri(
            "yhtye://pair?v=1&ip=100.64.0.1&ip=100.64.0.2&port=1&port=2&code=ABCD2345&code=ZZZZ2222",
        )
        .expect("parsed");
        assert_eq!(t.hosts(), ["100.64.0.1"]);
        assert_eq!((t.port(), t.code()), (1, "ABCD2345"));
    }

    #[test]
    fn only_tailscale_hosts_are_accepted() {
        assert_eq!(
            parse_pairing_uri("yhtye://pair?v=1&ip=192.168.1.5&port=1423&code=ABCD2345"),
            Err(InvalidPairingInput::NotTailscaleHost("192.168.1.5".into()))
        );
        // One good host does not excuse a bad one.
        assert_eq!(
            parse_pairing_uri(
                "yhtye://pair?v=1&host=evil.example.com&ip=100.64.0.1&port=1&code=ABCD2345"
            ),
            Err(InvalidPairingInput::NotTailscaleHost(
                "evil.example.com".into()
            ))
        );
        assert_eq!(
            parse_manual_entry("localhost", "1423", "ABCD2345"),
            Err(InvalidPairingInput::NotTailscaleHost("localhost".into()))
        );
        assert!(
            parse_manual_entry_with("localhost", "1423", "ABCD2345", HostPolicy::AnyHost).is_ok()
        );
    }

    #[test]
    fn a_missing_or_bad_part_names_what_is_wrong() {
        let uri = |query: &str| format!("yhtye://pair?v=1{query}");
        let cases = [
            ("&port=1&code=ABCD2345", InvalidPairingInput::MissingHost),
            (
                "&ip=%20&port=1&code=ABCD2345",
                InvalidPairingInput::MissingHost,
            ),
            (
                "&ip=100.64.0.1&code=ABCD2345",
                InvalidPairingInput::InvalidPort,
            ),
            (
                "&ip=100.64.0.1&port=0&code=ABCD2345",
                InvalidPairingInput::InvalidPort,
            ),
            (
                "&ip=100.64.0.1&port=65536&code=ABCD2345",
                InvalidPairingInput::InvalidPort,
            ),
            (
                "&ip=100.64.0.1&port=-1&code=ABCD2345",
                InvalidPairingInput::InvalidPort,
            ),
            ("&ip=100.64.0.1&port=1", InvalidPairingInput::MissingCode),
            (
                "&ip=100.64.0.1&port=1&code=%20",
                InvalidPairingInput::MissingCode,
            ),
            (
                "&ip=100.64.0.1&port=1&code=ABCD234",
                InvalidPairingInput::InvalidCode,
            ),
            (
                "&ip=100.64.0.1&port=1&code=ABCD2340",
                InvalidPairingInput::InvalidCode,
            ),
        ];
        for (query, want) in cases {
            assert_eq!(parse_pairing_uri(&uri(query)), Err(want), "{query}");
        }
    }

    #[test]
    fn manual_entry_defaults_the_port_and_reads_fullwidth_input() {
        let t = parse_manual_entry("100.89.117.15", "", "abcd 2345").expect("parsed");
        assert_eq!(t.port(), DEFAULT_PORT);
        assert_eq!(t.code(), "ABCD2345");
        let t = parse_manual_entry(
            " １００．８９．１１７．１５ ",
            "１４２３",
            "ＡＢＣＤ－２３４５",
        )
        .expect("parsed");
        assert_eq!(t.hosts(), ["100.89.117.15"]);
        assert_eq!(t.port(), 1423);
        assert_eq!(t.code(), "ABCD2345");
        assert_eq!(
            parse_manual_entry(" ", "1423", "ABCD2345"),
            Err(InvalidPairingInput::MissingHost)
        );
    }

    #[test]
    fn codes_are_normalized_like_the_server_does() {
        assert_eq!(
            normalize_pairing_code("abcd-2345").as_deref(),
            Some("ABCD2345")
        );
        assert_eq!(
            normalize_pairing_code(" a b c d 2 3 4 5 ").as_deref(),
            Some("ABCD2345")
        );
        assert_eq!(
            normalize_pairing_code("ＡＢＣＤ－２３４５").as_deref(),
            Some("ABCD2345")
        );
        // 0, O, 1, I and L are not in the alphabet; the length must be 8.
        for bad in [
            "ABCD234O",
            "ABCD2341",
            "ABCD234I",
            "ABCD234L",
            "ABCD234",
            "ABCD23456",
            "",
        ] {
            assert_eq!(normalize_pairing_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn device_names_follow_the_servers_rule() {
        assert_eq!(normalize_device_name("  My PC\n").as_deref(), Some("My PC"));
        assert_eq!(normalize_device_name("a\u{0}b").as_deref(), Some("ab"));
        assert_eq!(normalize_device_name(" \t"), None);
        assert!(normalize_device_name(&"x".repeat(MAX_DEVICE_NAME_CHARS)).is_some());
        assert_eq!(
            normalize_device_name(&"x".repeat(MAX_DEVICE_NAME_CHARS + 1)),
            None
        );
        // Characters, not bytes.
        assert!(normalize_device_name(&"あ".repeat(MAX_DEVICE_NAME_CHARS)).is_some());
    }

    #[test]
    fn percent_decoding_keeps_broken_escapes_and_plus() {
        assert_eq!(percent_decode("a%20b+c"), "a b+c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%E3%81%82"), "あ");
        // A trailing escape counts.
        assert_eq!(percent_decode("x%41"), "xA");
    }

    #[test]
    fn debug_output_leaves_the_code_out() {
        let t = parse_pairing_uri(FULL).expect("parsed");
        let shown = format!("{t:?}");
        assert!(!shown.contains("ABCD2345"), "{shown}");
        assert!(shown.contains("sid1"));
    }
}
