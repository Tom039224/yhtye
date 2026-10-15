//! Pairing: `POST /pair` with the one-time code (`core-design.md` §18.3; the
//! Rust side of `mobile/…/pairing/PairingClient.kt`).

use std::time::Duration;

use reqwest::StatusCode;

use super::address::url_host;
use super::target::{PairingTarget, normalize_device_name};
use crate::remote::{ClientKind, MAX_DEVICE_NAME_CHARS, PairRequest, PairResponse, RemoteError};

/// How long one host may take to answer before the next one is tried.
pub const PAIR_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Answers of the host are tiny; a longer one is not read.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// What pairing gives: the host's identity and what to connect with.
#[derive(Clone, PartialEq, Eq)]
pub struct PairedHost {
    /// Identifies the host's Yhtye (`sid` of its QR code).
    pub server_id: String,
    /// The hosts to connect to, the one that answered first.
    pub hosts: Vec<String>,
    pub port: u16,
    /// This device's id on the host.
    pub device_id: String,
    /// The name this device is listed under on the host.
    pub device_name: String,
    /// The device token (`Authorization: Bearer`). Not for screens or logs.
    pub token: String,
}

impl std::fmt::Debug for PairedHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairedHost")
            .field("server_id", &self.server_id)
            .field("hosts", &self.hosts)
            .field("port", &self.port)
            .field("device_id", &self.device_id)
            .field("device_name", &self.device_name)
            .finish_non_exhaustive()
    }
}

/// Why pairing failed (the UI picks its words; `message` is the host's English
/// explanation, a note to show with them).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingFailure {
    /// 401: wrong code, or no valid one (expired, used, five wrong tries).
    #[error("the pairing code was refused{}", note(.0))]
    CodeRejected(Option<String>),
    /// 403: this device is not of the host's Tailscale user.
    #[error("the host does not take devices of other Tailscale users{}", note(.0))]
    OtherUser(Option<String>),
    /// 400: the device name or the request was not accepted.
    #[error("the host did not accept the request{}", note(.0))]
    BadRequest(Option<String>),
    /// No host answered (Tailscale is off, remote access is off on the host,
    /// a wrong address, ...).
    #[error("could not reach {}:{port}{}", hosts.join(", "), note(.cause))]
    Unreachable {
        hosts: Vec<String>,
        port: u16,
        cause: Option<String>,
    },
    /// Another Yhtye answered than the QR code's (a wrong address). The
    /// token is thrown away. The host has registered this device anyway: have
    /// it removed in that Yhtye's device list.
    #[error("another Yhtye answered (expected {expected}, got {actual})")]
    WrongServer { expected: String, actual: String },
    /// An answer that makes no sense (5xx, JSON of another shape).
    #[error("the host answered with an error ({status}){}", note(.message))]
    ServerError {
        status: u16,
        message: Option<String>,
    },
}

fn note(message: &Option<String>) -> String {
    message
        .as_deref()
        .map(|m| format!(": {m}"))
        .unwrap_or_default()
}

/// Pairs this device (as a desktop client) with the host `target` names:
/// tries its hosts in turn, 10 seconds each, and decides on the first one that
/// answers over HTTP (another host would be the same Yhtye, and would only use
/// up the code's tries).
pub async fn pair(target: &PairingTarget, device_name: &str) -> Result<PairedHost, PairingFailure> {
    pair_with_timeout(target, device_name, PAIR_ATTEMPT_TIMEOUT).await
}

/// [`pair`] with another time per host.
pub async fn pair_with_timeout(
    target: &PairingTarget,
    device_name: &str,
    attempt_timeout: Duration,
) -> Result<PairedHost, PairingFailure> {
    let name = normalize_device_name(device_name).ok_or_else(|| {
        PairingFailure::BadRequest(Some(format!(
            "the device name must be 1 to {MAX_DEVICE_NAME_CHARS} characters"
        )))
    })?;
    let request = PairRequest {
        code: target.code().to_string(),
        device_name: name.clone(),
        kind: Some(ClientKind::Desktop),
    };
    // The code and token go to the host named, nowhere else: no proxy, no redirect.
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| unreachable_all(target, format!("no HTTP client: {e}")))?;
    let mut last_cause = None;
    for host in target.hosts() {
        let url = format!("http://{}:{}/pair", url_host(host), target.port());
        match tokio::time::timeout(attempt_timeout, post(&http, &url, &request)).await {
            Err(_) => last_cause = Some("timed out".to_string()),
            Ok(Err(cause)) => last_cause = Some(cause),
            Ok(Ok((status, body))) => return interpret(status, &body, host, target, name),
        }
    }
    Err(PairingFailure::Unreachable {
        hosts: target.hosts().to_vec(),
        port: target.port(),
        cause: last_cause,
    })
}

fn unreachable_all(target: &PairingTarget, cause: String) -> PairingFailure {
    PairingFailure::Unreachable {
        hosts: target.hosts().to_vec(),
        port: target.port(),
        cause: Some(cause),
    }
}

/// Sends the request and reads (a bounded part of) the answer. `Err`: no HTTP answer.
async fn post(
    http: &reqwest::Client,
    url: &str,
    request: &PairRequest,
) -> Result<(StatusCode, Vec<u8>), String> {
    let mut response = http
        .post(url)
        .json(request)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| e.without_url().to_string())?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY_BYTES {
            break;
        }
    }
    Ok((status, body))
}

fn interpret(
    status: StatusCode,
    body: &[u8],
    host: &str,
    target: &PairingTarget,
    name: String,
) -> Result<PairedHost, PairingFailure> {
    let message = || {
        serde_json::from_slice::<RemoteError>(body)
            .ok()
            .map(|e| e.error)
    };
    match status.as_u16() {
        200 => paired(body, host, target, name),
        400 => Err(PairingFailure::BadRequest(message())),
        401 => Err(PairingFailure::CodeRejected(message())),
        403 => Err(PairingFailure::OtherUser(message())),
        other => Err(PairingFailure::ServerError {
            status: other,
            message: message(),
        }),
    }
}

fn paired(
    body: &[u8],
    host: &str,
    target: &PairingTarget,
    name: String,
) -> Result<PairedHost, PairingFailure> {
    let response: PairResponse =
        serde_json::from_slice(body).map_err(|e| PairingFailure::ServerError {
            status: 200,
            message: Some(format!("unexpected answer: {e}")),
        })?;
    if let Some(expected) = target.server_id()
        && expected != response.server_id
    {
        return Err(PairingFailure::WrongServer {
            expected: expected.to_string(),
            actual: response.server_id,
        });
    }
    // The host that answered first, then the others.
    let hosts = std::iter::once(host.to_string())
        .chain(target.hosts().iter().filter(|h| *h != host).cloned())
        .collect();
    Ok(PairedHost {
        server_id: response.server_id,
        hosts,
        port: target.port(),
        device_id: response.device_id,
        device_name: name,
        token: response.token,
    })
}
