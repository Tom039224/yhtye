//! What [`super::RemoteConnection`] talks through: something that opens a
//! WebSocket to the host ([`WsTransport`]) and the open socket ([`WsLink`]).
//! [`TungsteniteTransport`] is the real one (`GET /ws` with the device token as
//! a bearer token); tests use a scripted fake.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};

use super::address::url_host;
use crate::remote::RemoteError;
use crate::ws::MAX_MESSAGE_BYTES;

/// Path of the host's WebSocket (`core-design.md` §18.4).
const WS_PATH: &str = "/ws";
/// How long one host may take to answer the handshake before the next is tried.
const HOST_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the host turned a connection down (the HTTP status of the handshake).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 401: the device token is not valid (the device was removed on the host).
    Unauthorized,
    /// 403: this device is not of the host's Tailscale user (or could not be told).
    Forbidden,
}

/// Why a connection could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The host answered and said no. Retrying does not help.
    Refused {
        refusal: Refusal,
        /// What the host said (English; for showing as a note).
        message: Option<String>,
    },
    /// Anything else: nobody there, a timeout, a bad answer. Retried.
    Failed(String),
}

/// What a read of the socket brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A text message (a reply or an event).
    Text(String),
    /// A ping or pong frame: the host is alive, nothing to handle.
    Heartbeat,
    /// The host closed the connection.
    Closed,
}

/// One open WebSocket (text messages only).
#[async_trait]
pub trait WsLink: Send {
    /// The next thing the host sent; `Err` when the connection broke.
    async fn recv(&mut self) -> Result<Incoming, String>;
    async fn send(&mut self, text: String) -> Result<(), String>;
    /// A WebSocket ping (keeps an idle connection warm and shows it is alive).
    async fn ping(&mut self) -> Result<(), String>;
    /// Closes it; fine to call on a broken one.
    async fn close(&mut self);
}

/// Opens connections to one host.
#[async_trait]
pub trait WsTransport: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn WsLink>, TransportError>;
}

/// The real transport: `ws://<host>:<port>/ws` with `Authorization: Bearer
/// <token>`, trying the hosts in turn (the one that worked last first).
pub struct TungsteniteTransport {
    hosts: Vec<String>,
    port: u16,
    token: String,
    preferred: Mutex<Option<String>>,
}

impl TungsteniteTransport {
    /// `hosts` are in the order to try, as stored (already checked at pairing).
    #[must_use]
    pub fn new(hosts: Vec<String>, port: u16, token: String) -> Self {
        Self {
            hosts,
            port,
            token,
            preferred: Mutex::new(None),
        }
    }

    fn hosts_to_try(&self) -> Vec<&str> {
        let preferred = self
            .preferred
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut hosts: Vec<&str> = self.hosts.iter().map(String::as_str).collect();
        // Stable: the others keep their order.
        hosts.sort_by_key(|h| Some(*h) != preferred.as_deref());
        hosts
    }

    async fn connect_to(&self, host: &str) -> Result<TungsteniteLink, TransportError> {
        let url = format!("ws://{}:{}{WS_PATH}", url_host(host), self.port);
        let mut request = url
            .into_client_request()
            .map_err(|e| TransportError::Failed(e.to_string()))?;
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", self.token))
            .map_err(|_| TransportError::Failed("the device token is not usable".into()))?;
        auth.set_sensitive(true);
        request.headers_mut().insert("authorization", auth);
        // The host lifts its own limits for attached files (see `MAX_MESSAGE_BYTES`),
        // so the reply to `get_attachment` can be far over the library's defaults.
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let connect = connect_async_with_config(request, Some(config), false);
        match tokio::time::timeout(HOST_TIMEOUT, connect).await {
            Err(_) => Err(TransportError::Failed(format!("{host}: timed out"))),
            Ok(Ok((ws, _))) => Ok(TungsteniteLink { ws }),
            Ok(Err(e)) => Err(handshake_error(host, e)),
        }
    }
}

impl std::fmt::Debug for TungsteniteTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TungsteniteTransport")
            .field("hosts", &self.hosts)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl WsTransport for TungsteniteTransport {
    async fn connect(&self) -> Result<Box<dyn WsLink>, TransportError> {
        let mut last = TransportError::Failed("no host to connect to".into());
        for host in self.hosts_to_try() {
            match self.connect_to(host).await {
                Ok(link) => {
                    *self
                        .preferred
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(host.to_string());
                    return Ok(Box::new(link));
                }
                // The same host answers for every address: no point in asking another.
                Err(refused @ TransportError::Refused { .. }) => return Err(refused),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

/// 401 and 403 of the handshake are a refusal; everything else is a failure to retry.
fn handshake_error(host: &str, e: WsError) -> TransportError {
    let WsError::Http(response) = &e else {
        return TransportError::Failed(format!("{host}: {e}"));
    };
    let message = response
        .body()
        .as_deref()
        .and_then(|body| serde_json::from_slice::<RemoteError>(body).ok())
        .map(|r| r.error);
    match response.status().as_u16() {
        401 => TransportError::Refused {
            refusal: Refusal::Unauthorized,
            message,
        },
        403 => TransportError::Refused {
            refusal: Refusal::Forbidden,
            message,
        },
        status => TransportError::Failed(match message {
            Some(m) => format!("{host}: HTTP {status}: {m}"),
            None => format!("{host}: HTTP {status}"),
        }),
    }
}

struct TungsteniteLink {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

#[async_trait]
impl WsLink for TungsteniteLink {
    async fn recv(&mut self) -> Result<Incoming, String> {
        match self.ws.next().await {
            None | Some(Ok(Message::Close(_))) => Ok(Incoming::Closed),
            Some(Ok(Message::Text(text))) => Ok(Incoming::Text(text.as_str().to_string())),
            // Binary frames are not used; they still show the host is alive.
            Some(Ok(_)) => Ok(Incoming::Heartbeat),
            Some(Err(WsError::ConnectionClosed | WsError::AlreadyClosed)) => Ok(Incoming::Closed),
            Some(Err(e)) => Err(e.to_string()),
        }
    }

    async fn send(&mut self, text: String) -> Result<(), String> {
        self.ws
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| e.to_string())
    }

    async fn ping(&mut self) -> Result<(), String> {
        self.ws
            .send(Message::Ping(Vec::new().into()))
            .await
            .map_err(|e| e.to_string())
    }

    async fn close(&mut self) {
        let _ = self.ws.close(None).await;
    }
}
