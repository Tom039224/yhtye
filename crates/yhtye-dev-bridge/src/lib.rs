//! The development WebSocket bridge (`core-design.md` §10): the same
//! [`ApiCommand`] / [`ApiEvent`] API as the Tauri app, over a WebSocket, so the
//! React app can run in a plain browser against the real core (development and
//! browser E2E only; not shipped).
//!
//! Protocol and session handling: [`yhtye_core::ws`] (shared with the remote
//! server). The bridge is a local client: every command is allowed, including
//! the local-only ones (remote access settings, pairing, devices).
//!
//! Access: bind to 127.0.0.1 only, the `Origin` header (when present, i.e. from
//! a browser) must be one of the allowed origins, and `?token=` must match.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio_util::sync::CancellationToken;
use yhtye_core::remote::constant_time_eq;
use yhtye_core::runtime::Core;
#[cfg(test)]
use yhtye_core::ws::parse_request;
use yhtye_core::ws::{SessionPolicy, run_session};

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
    ws.on_upgrade(move |socket| {
        run_session(socket, core, SessionPolicy::LOCAL, CancellationToken::new())
    })
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
