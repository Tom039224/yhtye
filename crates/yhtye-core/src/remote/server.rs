//! The remote server's routes: `POST /pair` and `GET /ws` (`core-design.md` §18.4).
//!
//! Every request is first checked with `tailscale whois` (answers kept for a
//! minute per address, a few lookups at once: [`super::whois`]): the peer must
//! belong to the same Tailscale user as this PC and not be a tagged node (else
//! 403), so other users' devices cannot even use up a pairing code's tries. A lookup that fails
//! (Tailscale briefly unreachable, a timeout) is 503, so clients retry instead
//! of taking it for a refusal. `/ws` then needs
//! `Authorization: Bearer <device token>` (else 401).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{WebSocketUpgrade, rejection::WebSocketUpgradeRejection};
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::pairing::PairRefused;
use super::{ClientKind, Shared, device_name, now_ms};
use crate::runtime::Core;
use crate::ws::{SessionPolicy, run_session};

pub const PAIR_PATH: &str = "/pair";
pub const WS_PATH: &str = "/ws";
/// Largest request body (a pairing request is tiny).
const BODY_LIMIT: usize = 4096;

#[derive(Clone)]
pub(super) struct ServerCtx {
    pub(super) shared: Arc<Shared>,
    pub(super) core: Core,
    /// The Tailscale user of this PC.
    pub(super) self_user: u64,
    /// Cancelled when the server stops: stops serving and closes every connection.
    pub(super) cancel: CancellationToken,
    /// Which server this is ([`Shared::attach`]).
    pub(super) generation: u64,
}

/// Serves on `listener` until the server's token is cancelled.
pub(super) async fn serve(listener: tokio::net::TcpListener, ctx: ServerCtx) {
    let cancel = ctx.cancel.clone();
    let app = Router::new()
        .route(PAIR_PATH, post(pair))
        .route(WS_PATH, get(ws))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(ctx)
        .into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async move { cancel.cancelled().await })
        .await;
    if let Err(e) = served {
        tracing::error!("remote access server error: {e}");
    }
}

/// Body of `POST /pair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    /// The pairing code shown on the PC.
    pub code: String,
    /// The name the device is listed under (trimmed, at most
    /// [`super::MAX_DEVICE_NAME_CHARS`] characters).
    pub device_name: String,
    /// What the device is; left out by the Android client (which is the
    /// default), `desktop` for another Yhtye desktop app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ClientKind>,
}

/// Answer of a successful `POST /pair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PairResponse {
    pub device_id: String,
    /// The device token: `Authorization: Bearer <token>` on `/ws`.
    pub token: String,
    /// Identifies this Yhtye (the QR code's `sid`).
    pub server_id: String,
}

/// Body of every refused HTTP request of the remote server (`/pair`, `/ws`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RemoteError {
    /// Why, meant to be shown to the user.
    pub error: String,
}

fn refuse(status: StatusCode, why: &str) -> Response {
    let body = RemoteError {
        error: why.to_string(),
    };
    (status, Json(body)).into_response()
}

/// `Err`: the refusal of a peer that is not this PC's Tailscale user (403),
/// or 503 when it could not be looked up (a passing failure: retried).
async fn check_peer(ctx: &ServerCtx, peer: SocketAddr) -> Result<(), (StatusCode, &'static str)> {
    let ip = peer.ip().to_canonical();
    match ctx.shared.whois.peer_of(&*ctx.shared.tailscale, ip).await {
        // Tagged nodes share one user; whoever they are, they are not this PC's user's.
        Ok(peer) if peer.tagged => {
            tracing::warn!("remote access: refused {ip} (a tagged Tailscale node)");
            Err((
                StatusCode::FORBIDDEN,
                "tagged Tailscale devices cannot connect",
            ))
        }
        Ok(peer) if peer.user_id == ctx.self_user => Ok(()),
        Ok(peer) => {
            tracing::warn!(
                "remote access: refused {ip} (Tailscale user {})",
                peer.user_id
            );
            Err((
                StatusCode::FORBIDDEN,
                "this device belongs to another Tailscale user",
            ))
        }
        Err(e) => {
            tracing::warn!("remote access: could not identify {ip}: {e}");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "this device could not be identified on Tailscale; try again",
            ))
        }
    }
}

async fn pair(
    State(ctx): State<ServerCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<PairRequest>, JsonRejection>,
) -> Response {
    if let Err((status, why)) = check_peer(&ctx, peer).await {
        return refuse(status, why);
    }
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &e.body_text()),
    };
    let name = match device_name(&req.device_name) {
        Ok(n) => n,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, &e),
    };
    // The code is used up here, before the device is stored: a second request
    // with it, now, is refused. A failed write gives it back below.
    let redeemed = ctx.shared.lock_pairing().redeem(&req.code, now_ms());
    let redeemed = match redeemed {
        Ok(redeemed) => redeemed,
        Err(why) => {
            tracing::warn!("remote access: pairing refused for {peer} ({why:?})");
            let msg = match why {
                PairRefused::NoCode => "no pairing code is valid; issue a new one on the PC",
                PairRefused::Wrong => "wrong pairing code",
            };
            ctx.shared.emit_pairing_change();
            return refuse(StatusCode::UNAUTHORIZED, msg);
        }
    };
    let kind = req.kind.unwrap_or(ClientKind::Android);
    let (device_id, token) = match ctx.shared.add_device(name.clone(), kind).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("remote access: could not store a paired device: {e}");
            ctx.shared.lock_pairing().restore(redeemed, now_ms());
            ctx.shared.emit_pairing_change();
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not store the device",
            );
        }
    };
    tracing::info!("remote access: paired device {device_id} ({name}, {kind:?}) from {peer}");
    ctx.shared.emit_pairing_change();
    ctx.shared.emit_devices().await;
    Json(PairResponse {
        device_id,
        token,
        server_id: ctx.shared.server_id.clone(),
    })
    .into_response()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

async fn ws(
    State(ctx): State<ServerCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    if let Err((status, why)) = check_peer(&ctx, peer).await {
        return refuse(status, why);
    }
    let device = match bearer(&headers) {
        Some(token) => match ctx.shared.device_by_token(token).await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("remote access: could not read the devices: {e}");
                return refuse(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "could not read the devices",
                );
            }
        },
        None => None,
    };
    let Some(device) = device else {
        tracing::warn!("remote access: refused {peer}: missing or unknown token");
        return refuse(StatusCode::UNAUTHORIZED, "missing or unknown device token");
    };
    let upgrade = match upgrade {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    upgrade.on_upgrade(move |socket| async move {
        let shared = Arc::clone(&ctx.shared);
        // Revoked between the check and now: close at once.
        let Some(cancel) = shared.attach(&device.id, &ctx.cancel, ctx.generation).await else {
            return;
        };
        if let Err(e) = shared.store.touch_remote_device(&device.id, now_ms()).await {
            tracing::warn!("could not record the connection of {}: {e}", device.id);
        }
        tracing::info!("remote access: device {} connected from {peer}", device.id);
        shared.emit_devices().await;
        run_session(socket, ctx.core.clone(), SessionPolicy::REMOTE, cancel).await;
        shared.detach(&device.id, ctx.generation);
        tracing::info!("remote access: device {} disconnected", device.id);
        shared.emit_devices().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn the_bearer_token_is_read_from_the_authorization_header() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(bearer(&h), Some("abc"));
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer  abc "),
        );
        assert_eq!(bearer(&h), Some("abc"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer(&h), None);
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(bearer(&h), None);
    }
}
