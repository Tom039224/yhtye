//! Tailscale, behind a trait so tests can use a fake: this PC's Tailscale
//! addresses, MagicDNS name and user (`tailscale status --json`), and the user
//! of a connecting peer (`tailscale whois --json <ip>`).

use std::net::IpAddr;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

/// How long one `tailscale` command may take.
const CLI_TIMEOUT: Duration = Duration::from_secs(5);

/// This PC on the tailnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleSelf {
    /// The addresses to listen on (IPv4 first). Never empty.
    pub addresses: Vec<IpAddr>,
    /// The MagicDNS name without the trailing dot (`pc.tailnet.ts.net`), if MagicDNS is on.
    pub dns_name: Option<String>,
    /// The Tailscale user who owns this PC.
    pub user_id: u64,
}

/// Why Tailscale could not answer. The message is shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TailscaleError(pub String);

#[async_trait]
pub trait Tailscale: Send + Sync + std::fmt::Debug {
    async fn status(&self) -> Result<TailscaleSelf, TailscaleError>;
    /// The Tailscale user of the node with address `ip`.
    async fn whois(&self, ip: IpAddr) -> Result<u64, TailscaleError>;
}

/// The `tailscale` command line.
#[derive(Debug, Clone, Default)]
pub struct TailscaleCli;

impl TailscaleCli {
    async fn run(args: &[&str]) -> Result<Vec<u8>, TailscaleError> {
        let mut last_missing = None;
        for program in candidates() {
            let mut cmd = tokio::process::Command::new(program);
            cmd.args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let output = match tokio::time::timeout(CLI_TIMEOUT, cmd.output()).await {
                Err(_) => {
                    return Err(TailscaleError(format!(
                        "`tailscale {}` did not answer within {}s",
                        args.join(" "),
                        CLI_TIMEOUT.as_secs()
                    )));
                }
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    last_missing = Some(e);
                    continue;
                }
                Ok(Err(e)) => {
                    return Err(TailscaleError(format!("could not run tailscale: {e}")));
                }
                Ok(Ok(output)) => output,
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(TailscaleError(format!(
                    "`tailscale {}` failed: {}",
                    args.first().copied().unwrap_or_default(),
                    stderr.trim()
                )));
            }
            return Ok(output.stdout);
        }
        tracing::debug!("tailscale not found: {last_missing:?}");
        Err(TailscaleError(
            "Tailscale is not installed (the `tailscale` command was not found)".into(),
        ))
    }
}

/// Where the `tailscale` command may be (the `PATH` first).
fn candidates() -> Vec<&'static str> {
    let mut out = vec!["tailscale"];
    if cfg!(target_os = "macos") {
        out.push("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    }
    out
}

#[async_trait]
impl Tailscale for TailscaleCli {
    async fn status(&self) -> Result<TailscaleSelf, TailscaleError> {
        let out = Self::run(&["status", "--json"]).await?;
        parse_status(&out)
    }

    async fn whois(&self, ip: IpAddr) -> Result<u64, TailscaleError> {
        let out = Self::run(&["whois", "--json", &ip.to_string()]).await?;
        parse_whois(&out)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatusJson {
    backend_state: String,
    #[serde(rename = "Self")]
    me: Option<SelfJson>,
    current_tailnet: Option<TailnetJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SelfJson {
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "UserID")]
    user_id: u64,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<IpAddr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TailnetJson {
    #[serde(rename = "MagicDNSEnabled", default)]
    magic_dns_enabled: bool,
}

fn parse_status(out: &[u8]) -> Result<TailscaleSelf, TailscaleError> {
    let status: StatusJson = serde_json::from_slice(out)
        .map_err(|e| TailscaleError(format!("could not read `tailscale status --json`: {e}")))?;
    if status.backend_state != "Running" {
        return Err(TailscaleError(format!(
            "Tailscale is not running (state: {}); connect it with `tailscale up`",
            status.backend_state
        )));
    }
    let me = status
        .me
        .ok_or_else(|| TailscaleError("Tailscale reports no node for this PC".into()))?;
    let mut addresses: Vec<IpAddr> = me
        .tailscale_ips
        .into_iter()
        .filter(is_tailscale_address)
        .collect();
    addresses.sort_by_key(IpAddr::is_ipv6);
    if addresses.is_empty() {
        return Err(TailscaleError("this PC has no Tailscale address".into()));
    }
    let magic_dns = status.current_tailnet.is_none_or(|t| t.magic_dns_enabled);
    let name = me.dns_name.trim_end_matches('.');
    Ok(TailscaleSelf {
        addresses,
        dns_name: (magic_dns && !name.is_empty()).then(|| name.to_string()),
        user_id: me.user_id,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WhoisJson {
    user_profile: Option<UserJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserJson {
    #[serde(rename = "ID")]
    id: u64,
}

fn parse_whois(out: &[u8]) -> Result<u64, TailscaleError> {
    let whois: WhoisJson = serde_json::from_slice(out)
        .map_err(|e| TailscaleError(format!("could not read `tailscale whois --json`: {e}")))?;
    whois
        .user_profile
        .map(|u| u.id)
        .ok_or_else(|| TailscaleError("the peer has no Tailscale user".into()))
}

/// Tailscale's address ranges: 100.64.0.0/10 (CGNAT) and fd7a:115c:a1e0::/48.
#[must_use]
pub fn is_tailscale_address(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 100 && (b & 0xc0) == 64
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"{
        "BackendState": "Running",
        "Self": {
            "HostName": "pc",
            "DNSName": "pc.tail1234.ts.net.",
            "UserID": 7951023142460886,
            "TailscaleIPs": ["fd7a:115c:a1e0::fa32:750f", "100.89.117.15"]
        },
        "CurrentTailnet": {"Name": "x", "MagicDNSSuffix": "tail1234.ts.net", "MagicDNSEnabled": true}
    }"#;

    #[test]
    fn status_gives_the_addresses_ipv4_first_the_name_and_the_user() {
        let me = parse_status(STATUS.as_bytes()).expect("parsed");
        assert_eq!(
            me.addresses,
            [
                "100.89.117.15".parse::<IpAddr>().unwrap(),
                "fd7a:115c:a1e0::fa32:750f".parse().unwrap()
            ]
        );
        assert_eq!(me.dns_name.as_deref(), Some("pc.tail1234.ts.net"));
        assert_eq!(me.user_id, 7_951_023_142_460_886);
    }

    #[test]
    fn a_stopped_tailscale_is_an_error_that_says_so() {
        let err = parse_status(br#"{"BackendState": "Stopped", "Self": null}"#).unwrap_err();
        assert!(err.0.contains("not running (state: Stopped)"), "{err}");
        let err = parse_status(b"not json").unwrap_err();
        assert!(err.0.contains("could not read"), "{err}");
    }

    #[test]
    fn without_magic_dns_there_is_no_name() {
        let text = STATUS.replace(r#""MagicDNSEnabled": true"#, r#""MagicDNSEnabled": false"#);
        assert_eq!(parse_status(text.as_bytes()).unwrap().dns_name, None);
    }

    #[test]
    fn whois_gives_the_user_id() {
        let out = br#"{"Node": {"ID": 1}, "UserProfile": {"ID": 42, "LoginName": "a@b"}}"#;
        assert_eq!(parse_whois(out).unwrap(), 42);
        assert!(parse_whois(br#"{"Node": {}}"#).is_err());
    }

    #[test]
    fn only_tailscale_ranges_count() {
        let yes = ["100.64.0.1", "100.127.255.255", "fd7a:115c:a1e0::1"];
        let no = [
            "100.63.0.1",
            "100.128.0.1",
            "127.0.0.1",
            "0.0.0.0",
            "::1",
            "fd7a::1",
        ];
        for ip in yes {
            assert!(is_tailscale_address(&ip.parse().unwrap()), "{ip}");
        }
        for ip in no {
            assert!(!is_tailscale_address(&ip.parse().unwrap()), "{ip}");
        }
    }
}
