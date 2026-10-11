//! The API over a WebSocket (`core-design.md` §10, §18), shared by the
//! development bridge (`crates/yhtye-dev-bridge`) and the remote server
//! ([`crate::remote`]). Each transport does its own access check before the
//! upgrade and then hands the socket to [`run_session`].
//!
//! Protocol (types in [`crate::api::wire`]): the client sends
//! `{"id": n, "cmd": ApiCommand}` and gets `{"id": n, "ok": ApiResponse}` or
//! `{"id": n, "err": ApiError}`; every core event is pushed as `{"event": ApiEvent}`.
//! Requests run concurrently (opening a project takes a while; a cancel must
//! not wait behind it), so replies may come out of order. A request is not
//! aborted when the client leaves (a half-run `OpenProject` must finish; its
//! reply is just dropped). Requests of one connection are not run in the order
//! they were sent either: a client that needs one to follow another waits for
//! the first reply (`core-design.md` §10).

use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::api::{
    ApiError, ApiErrorCode, ApiEvent, ApiEventBody, WsReply, WsRequest, WsServerMessage,
};
use crate::runtime::Core;

/// What a session may do besides the commands every client may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    /// Commands that only the PC itself may run ([`crate::api::ApiCommand::is_local_only`])
    /// and the app-wide events about them ([`crate::api::ApiEventBody::is_local_only`]).
    pub local: bool,
}

impl SessionPolicy {
    /// A client on this PC (the app's own UI, the dev bridge): everything.
    pub const LOCAL: Self = Self { local: true };
    /// A paired remote device: no local-only commands or events.
    pub const REMOTE: Self = Self { local: false };
}

/// Messages queued for one client's socket. Bounded, so a client that stops
/// reading cannot make the core hold an ever-growing backlog: when it is full,
/// a project's live events are dropped and anything else closes the connection
/// (see [`forward_events`]); the client reconnects and catches up.
pub const OUT_QUEUE: usize = 1024;

/// Close code of a client dropped for falling behind (1013 "try again later").
const BEHIND: u16 = 1013;

/// Serves one client until it leaves or `cancel` fires (then the socket is
/// closed at once, even while requests are still running).
///
/// A writer task owns the socket's sink; events and replies are queued to it
/// ([`OUT_QUEUE`]).
pub async fn run_session(
    socket: WebSocket,
    core: Core,
    policy: SessionPolicy,
    cancel: CancellationToken,
) {
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<String>(OUT_QUEUE);
    // Subscribe before reading requests so no event after a reply is missed.
    let events = core.subscribe();
    // Fired by `cancel`, or when this client fell too far behind.
    let session = cancel.child_token();
    let behind = CancellationToken::new();
    let writer_session = session.clone();
    let writer_behind = behind.clone();
    let writer = tokio::spawn(async move {
        loop {
            let text = tokio::select! {
                text = out_rx.recv() => text,
                () = writer_session.cancelled() => {
                    // A reconnect tells the client why (a revoked device gets 401).
                    let frame = if writer_behind.is_cancelled() {
                        CloseFrame { code: BEHIND, reason: "fell behind; reconnect".into() }
                    } else {
                        CloseFrame { code: close_code::AWAY, reason: "closed by the server".into() }
                    };
                    let _ = sink.send(Message::Close(Some(frame))).await;
                    None
                }
            };
            let Some(text) = text else { break };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let forwarder = tokio::spawn({
        let out = out_tx.clone();
        let session = session.clone();
        async move {
            if forward_events(events, out, policy).await == Forwarded::Behind {
                tracing::warn!("a WebSocket client fell behind; closing its connection");
                behind.cancel();
                session.cancel();
            }
        }
    });
    loop {
        let msg = tokio::select! {
            msg = stream.next() => msg,
            () = session.cancelled() => break,
        };
        match msg {
            Some(Ok(Message::Text(text))) => handle_request(&core, text.as_str(), &out_tx, policy),
            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            Some(Ok(_)) => {} // ping / pong are answered by axum; binary is not used
        }
    }
    forwarder.abort();
    drop(out_tx);
    let _ = writer.await;
}

/// Why [`forward_events`] stopped.
#[derive(Debug, PartialEq, Eq)]
enum Forwarded {
    /// The client left or the core closed.
    Done,
    /// The queue was full for something that must not be dropped (a durable
    /// event, or an app-wide one such as `resync`): the connection is closed.
    Behind,
}

/// Queues the core's events for one client. A project's live events that do
/// not fit are dropped (the streamed text comes again as the block's
/// `agent_text`); a durable or app-wide one (a hint the client must not miss)
/// that does not fit ends the session ([`Forwarded::Behind`]: the client
/// reconnects, and catches up and reads everything again).
/// When the client's subscription lags behind the core's buffer, it is sent a
/// `resync` hint: what was skipped must be read again.
async fn forward_events(
    mut events: broadcast::Receiver<ApiEvent>,
    out: mpsc::Sender<String>,
    policy: SessionPolicy,
) -> Forwarded {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("a WebSocket client fell behind; {n} events skipped");
                ApiEvent::app_wide(ApiEventBody::Resync)
            }
            Err(broadcast::error::RecvError::Closed) => return Forwarded::Done,
        };
        if !policy.local && event.body.is_local_only() {
            continue;
        }
        let droppable = event.live && !event.body.is_app_wide();
        match out.try_send(to_json(&WsServerMessage::Event { event })) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) if droppable => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Forwarded::Behind,
            Err(mpsc::error::TrySendError::Closed(_)) => return Forwarded::Done,
        }
    }
}

fn handle_request(core: &Core, text: &str, out: &mpsc::Sender<String>, policy: SessionPolicy) {
    let request = match parse_request(text) {
        Ok(r) => r,
        Err(Some((id, err))) => {
            send_reply(out, WsReply::Err { id, err });
            return;
        }
        Err(None) => {
            // Only the size: the message may be a command carrying a secret value.
            tracing::warn!(
                "ignored a WebSocket message without an id ({} bytes)",
                text.len()
            );
            return;
        }
    };
    let id = request.id;
    if !policy.local && request.cmd.is_local_only() {
        let err = ApiError::new(
            ApiErrorCode::Forbidden,
            "this command can only be run on the PC itself",
        );
        send_reply(out, WsReply::Err { id, err });
        return;
    }
    let core = core.clone();
    let out = out.clone();
    tokio::spawn(async move {
        let reply = match core.command(request.cmd).await {
            Ok(ok) => WsReply::Ok { id, ok },
            Err(err) => WsReply::Err { id, err },
        };
        // Waits for room: a reply is never dropped (the client may be reading
        // slowly; if it left, the queue is closed and the send fails).
        let _ = out.send(to_json(&WsServerMessage::Reply(reply))).await;
    });
}

/// Queues an error reply in the background (the reader must not wait on a
/// client that does not read, and replies are never dropped).
fn send_reply(out: &mpsc::Sender<String>, reply: WsReply) {
    let out = out.clone();
    tokio::spawn(async move {
        let _ = out.send(to_json(&WsServerMessage::Reply(reply))).await;
    });
}

/// A request, or the error to reply with (`None`: no id to reply to).
pub fn parse_request(text: &str) -> Result<WsRequest, Option<(u64, ApiError)>> {
    let value: Value = serde_json::from_str(text).map_err(|_| None)?;
    let id = value.get("id").and_then(Value::as_u64).ok_or(None)?;
    serde_json::from_value(value)
        .map_err(|e| Some((id, ApiError::invalid_argument(format!("bad request: {e}")))))
}

fn to_json(msg: &WsServerMessage) -> String {
    serde_json::to_string(msg).unwrap_or_else(|e| {
        tracing::error!("could not serialize a WebSocket message: {e}");
        String::from("{}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiCommand;

    #[test]
    fn requests_are_parsed_or_answered_with_an_error() {
        let ok = parse_request(r#"{"id": 3, "cmd": {"type": "ping"}}"#).expect("valid");
        assert_eq!((ok.id, ok.cmd), (3, ApiCommand::Ping));
        let Err(Some((id, err))) = parse_request(r#"{"id": 4, "cmd": {"type": "nope"}}"#) else {
            panic!("expected an error reply");
        };
        assert_eq!((id, err.code), (4, ApiErrorCode::InvalidArgument));
        assert!(matches!(parse_request("[]"), Err(None)));
    }

    #[test]
    fn exactly_the_remote_access_commands_are_local_only() {
        let local_only = [
            r#"{"type": "get_remote_status"}"#,
            r#"{"type": "set_remote_settings", "enabled": true}"#,
            r#"{"type": "create_pairing_code"}"#,
            r#"{"type": "list_remote_devices"}"#,
            r#"{"type": "revoke_remote_device", "device": "d"}"#,
        ];
        for json in local_only {
            let cmd: ApiCommand = serde_json::from_str(json).expect(json);
            assert!(cmd.is_local_only(), "{json}");
        }
        let anyone = [
            r#"{"type": "ping"}"#,
            r#"{"type": "list_projects"}"#,
            r#"{"type": "set_secret_env", "name": "A", "value": "v"}"#,
            r#"{"type": "send_user_message", "project": "p", "chat": "c", "text": "t"}"#,
            r#"{"type": "list_clients"}"#,
        ];
        for json in anyone {
            let cmd: ApiCommand = serde_json::from_str(json).expect(json);
            assert!(!cmd.is_local_only(), "{json}");
        }
        assert_eq!(SessionPolicy::LOCAL, SessionPolicy { local: true });
    }

    fn event(seq: u64, live: bool) -> ApiEvent {
        ApiEvent {
            seq,
            ts_ms: 0,
            project: "p".into(),
            live,
            body: ApiEventBody::Prompted {
                session: "s".into(),
                text: "t".into(),
            },
        }
    }

    fn body_type(text: &str) -> String {
        let v: Value = serde_json::from_str(text).expect("json");
        v["event"]["body"]["type"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[tokio::test]
    async fn a_full_queue_drops_live_events_and_closes_on_a_durable_one() {
        let (tx, rx) = broadcast::channel(16);
        let (out, mut queued) = mpsc::channel(2);
        tx.send(event(1, false)).expect("send");
        tx.send(event(1, true)).expect("send");
        tx.send(event(1, true)).expect("send"); // full: dropped
        tx.send(event(2, false)).expect("send"); // full: the client is behind
        let forwarded = forward_events(rx, out, SessionPolicy::LOCAL).await;
        assert_eq!(forwarded, Forwarded::Behind);
        let first: Value = serde_json::from_str(&queued.recv().await.expect("one")).expect("json");
        let second: Value = serde_json::from_str(&queued.recv().await.expect("two")).expect("json");
        assert_eq!(
            (
                first["event"]["live"].clone(),
                second["event"]["live"].clone()
            ),
            (Value::Bool(false), Value::Bool(true))
        );
        assert!(queued.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_full_queue_closes_on_an_app_wide_hint_too() {
        let (tx, rx) = broadcast::channel(16);
        let (out, _queued) = mpsc::channel(1);
        tx.send(event(1, true)).expect("send");
        tx.send(ApiEvent::app_wide(ApiEventBody::SettingsChanged {
            kind: crate::api::SettingsKind::Secrets,
            project: None,
            revision: None,
        }))
        .expect("send");
        let forwarded = forward_events(rx, out, SessionPolicy::LOCAL).await;
        assert_eq!(forwarded, Forwarded::Behind);
    }

    #[tokio::test]
    async fn a_lagging_subscription_gets_a_resync_hint() {
        let (tx, rx) = broadcast::channel(2);
        for seq in 1..=5 {
            tx.send(event(seq, false)).expect("send");
        }
        drop(tx);
        let (out, mut queued) = mpsc::channel(16);
        let forwarded = forward_events(rx, out, SessionPolicy::LOCAL).await;
        assert_eq!(forwarded, Forwarded::Done);
        let mut types = Vec::new();
        while let Ok(text) = queued.try_recv() {
            types.push(body_type(&text));
        }
        // The skipped events are not sent; a resync comes first, then what is left.
        assert_eq!(types, ["resync", "prompted", "prompted"]);
    }

    #[tokio::test]
    async fn remote_clients_get_app_wide_events_except_remote_access_ones() {
        let (tx, rx) = broadcast::channel(16);
        tx.send(ApiEvent::app_wide(ApiEventBody::RemoteDevicesChanged {
            devices: Vec::new(),
        }))
        .expect("send");
        tx.send(ApiEvent::app_wide(ApiEventBody::ProjectsChanged {
            project: "p".into(),
        }))
        .expect("send");
        tx.send(ApiEvent::app_wide(ApiEventBody::ClientsChanged {
            clients: crate::remote::Clients::new("pc".into(), &[]),
        }))
        .expect("send");
        drop(tx);
        let (out, mut queued) = mpsc::channel(16);
        forward_events(rx, out, SessionPolicy::REMOTE).await;
        assert_eq!(
            body_type(&queued.try_recv().expect("one")),
            "projects_changed"
        );
        assert_eq!(
            body_type(&queued.try_recv().expect("two")),
            "clients_changed"
        );
        assert!(queued.try_recv().is_err());
    }
}
