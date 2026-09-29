//! The development WebSocket bridge (`core-design.md` §10): the same
//! [`ApiCommand`] / [`ApiEvent`] API as the Tauri app, over a WebSocket, so the
//! React app can run in a plain browser against the real core (development and
//! browser E2E only; not shipped).
//!
//! Protocol (types in `yhtye_core::api::wire`): the client sends
//! `{"id": n, "cmd": ApiCommand}` and gets `{"id": n, "ok": ApiResponse}` or
//! `{"id": n, "err": ApiError}`; every core event is pushed as `{"event": ApiEvent}`.
//! Requests run concurrently (opening a project takes a while; a cancel must
//! not wait behind it), so replies may come out of order.
//!
//! Access: bind to 127.0.0.1 only, the `Origin` header (when present, i.e. from
//! a browser) must be one of the allowed origins, and `?token=` must match.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use yhtye_core::api::{ApiError, WsReply, WsRequest, WsServerMessage};
use yhtye_core::runtime::Core;

/// Default port. Vite serves on 1420 and uses 1421 for HMR when
/// `TAURI_DEV_HOST` is set, so the bridge takes the next one.
pub const DEFAULT_PORT: u16 = 1422;

/// Path of the WebSocket endpoint.
pub const WS_PATH: &str = "/ws";

/// Origins of the Vite dev server (the React app in a browser).
pub const DEFAULT_ORIGINS: [&str; 2] = ["http://localhost:1420", "http://127.0.0.1:1420"];

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Required as `?token=` on the WebSocket URL.
    pub token: String,
    /// Browser origins allowed to connect (exact match).
    pub allowed_origins: Vec<String>,
}

impl BridgeConfig {
    /// The default origins with `token`.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            allowed_origins: DEFAULT_ORIGINS.iter().map(ToString::to_string).collect(),
        }
    }
}

/// A random token (128 bits, hex).
pub fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

struct Shared {
    core: Core,
    cfg: BridgeConfig,
}

/// The bridge's routes (`GET /ws`), serving `core`.
pub fn router(core: Core, cfg: BridgeConfig) -> Router {
    let shared = Arc::new(Shared { core, cfg });
    Router::new()
        .route(WS_PATH, get(upgrade))
        .with_state(shared)
}

/// Serves [`router`] on `listener` until `shutdown` resolves. It does not stop
/// the core; call [`Core::shutdown`] afterwards.
pub async fn serve(
    listener: tokio::net::TcpListener,
    core: Core,
    cfg: BridgeConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        router(core, cfg).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

#[derive(serde::Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn upgrade(
    State(shared): State<Arc<Shared>>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err((status, why)) = check_access(&shared.cfg, &headers, q.token.as_deref()) {
        tracing::warn!("refused a bridge connection: {why}");
        return (status, why).into_response();
    }
    let core = shared.core.clone();
    ws.on_upgrade(move |socket| connection(socket, core))
}

/// Whether a connection may be accepted (`Err` = status and reason).
fn check_access(
    cfg: &BridgeConfig,
    headers: &HeaderMap,
    token: Option<&str>,
) -> Result<(), (StatusCode, &'static str)> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .is_ok_and(|o| cfg.allowed_origins.iter().any(|a| a == o));
        if !allowed {
            return Err((StatusCode::FORBIDDEN, "origin not allowed"));
        }
    }
    match token {
        Some(t) if constant_time_eq(t.as_bytes(), cfg.token.as_bytes()) => Ok(()),
        _ => Err((StatusCode::UNAUTHORIZED, "missing or wrong token")),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// One client: a writer task owns the socket's sink; events and replies are
/// queued to it. Requests are spawned and not aborted when the client leaves
/// (a half-run `OpenProject` must finish; its reply is just dropped).
async fn connection(socket: WebSocket, core: Core) {
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    // Subscribe before reading requests so no event after a reply is missed.
    let events = core.subscribe();
    let writer = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let forwarder = tokio::spawn(forward_events(events, out_tx.clone()));
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(text)) => handle_request(&core, text.as_str(), &out_tx),
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {} // ping / pong are answered by axum; binary is not used
        }
    }
    forwarder.abort();
    drop(out_tx);
    let _ = writer.await;
}

async fn forward_events(
    mut events: broadcast::Receiver<yhtye_core::api::ApiEvent>,
    out: mpsc::UnboundedSender<String>,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                if out
                    .send(to_json(&WsServerMessage::Event { event }))
                    .is_err()
                {
                    return;
                }
            }
            // The client sees a seq gap and catches up with ListEvents.
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("a bridge client fell behind; {n} events skipped");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn handle_request(core: &Core, text: &str, out: &mpsc::UnboundedSender<String>) {
    let request = match parse_request(text) {
        Ok(r) => r,
        Err(Some((id, err))) => {
            let _ = out.send(to_json(&WsServerMessage::Reply(WsReply::Err { id, err })));
            return;
        }
        Err(None) => {
            // Only the size: the message may be a command carrying a secret value.
            tracing::warn!(
                "ignored a bridge message without an id ({} bytes)",
                text.len()
            );
            return;
        }
    };
    let core = core.clone();
    let out = out.clone();
    tokio::spawn(async move {
        let id = request.id;
        let reply = match core.command(request.cmd).await {
            Ok(ok) => WsReply::Ok { id, ok },
            Err(err) => WsReply::Err { id, err },
        };
        let _ = out.send(to_json(&WsServerMessage::Reply(reply)));
    });
}

/// A request, or the error to reply with (`None`: no id to reply to).
fn parse_request(text: &str) -> Result<WsRequest, Option<(u64, ApiError)>> {
    let value: Value = serde_json::from_str(text).map_err(|_| None)?;
    let id = value.get("id").and_then(Value::as_u64).ok_or(None)?;
    serde_json::from_value(value)
        .map_err(|e| Some((id, ApiError::invalid_argument(format!("bad request: {e}")))))
}

fn to_json(msg: &WsServerMessage) -> String {
    serde_json::to_string(msg).unwrap_or_else(|e| {
        tracing::error!("could not serialize a bridge message: {e}");
        String::from("{}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn origin_and_token_are_checked() {
        let cfg = BridgeConfig::new("secret");
        let mut headers = HeaderMap::new();
        assert!(
            check_access(&cfg, &headers, Some("secret")).is_ok(),
            "no origin: a non-browser client"
        );
        assert_eq!(
            check_access(&cfg, &headers, None).unwrap_err().0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            check_access(&cfg, &headers, Some("secreT")).unwrap_err().0,
            StatusCode::UNAUTHORIZED
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:1420"),
        );
        assert!(check_access(&cfg, &headers, Some("secret")).is_ok());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert_eq!(
            check_access(&cfg, &headers, Some("secret")).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn requests_are_parsed_or_answered_with_an_error() {
        let ok = parse_request(r#"{"id": 3, "cmd": {"type": "list_projects"}}"#).expect("valid");
        assert_eq!(ok.id, 3);
        let Err(Some((id, err))) = parse_request(r#"{"id": 4, "cmd": {"type": "nope"}}"#) else {
            panic!("expected an error reply");
        };
        assert_eq!(id, 4);
        assert_eq!(err.code, yhtye_core::api::ApiErrorCode::InvalidArgument);
        assert!(matches!(parse_request("not json"), Err(None)));
        assert!(matches!(parse_request(r#"{"cmd": {}}"#), Err(None)));
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = random_token().expect("token");
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_token().expect("token"));
    }
}
