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
//! reply is just dropped).

use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::api::{ApiError, ApiErrorCode, ApiEvent, WsReply, WsRequest, WsServerMessage};
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

/// Serves one client until it leaves or `cancel` fires (then the socket is
/// closed at once, even while requests are still running).
///
/// A writer task owns the socket's sink; events and replies are queued to it.
pub async fn run_session(
    socket: WebSocket,
    core: Core,
    policy: SessionPolicy,
    cancel: CancellationToken,
) {
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    // Subscribe before reading requests so no event after a reply is missed.
    let events = core.subscribe();
    let writer_cancel = cancel.clone();
    let writer = tokio::spawn(async move {
        loop {
            let text = tokio::select! {
                text = out_rx.recv() => text,
                () = writer_cancel.cancelled() => {
                    // A reconnect tells the client why (a revoked device gets 401).
                    let frame = CloseFrame {
                        code: close_code::AWAY,
                        reason: "closed by the server".into(),
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
    let forwarder = tokio::spawn(forward_events(events, out_tx.clone(), policy));
    loop {
        let msg = tokio::select! {
            msg = stream.next() => msg,
            () = cancel.cancelled() => break,
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

async fn forward_events(
    mut events: broadcast::Receiver<ApiEvent>,
    out: mpsc::UnboundedSender<String>,
    policy: SessionPolicy,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                if !policy.local && event.body.is_local_only() {
                    continue;
                }
                if out
                    .send(to_json(&WsServerMessage::Event { event }))
                    .is_err()
                {
                    return;
                }
            }
            // The client sees a seq gap and catches up with ListEvents.
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("a WebSocket client fell behind; {n} events skipped");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn handle_request(
    core: &Core,
    text: &str,
    out: &mpsc::UnboundedSender<String>,
    policy: SessionPolicy,
) {
    let request = match parse_request(text) {
        Ok(r) => r,
        Err(Some((id, err))) => {
            let _ = out.send(to_json(&WsServerMessage::Reply(WsReply::Err { id, err })));
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
        let _ = out.send(to_json(&WsServerMessage::Reply(WsReply::Err { id, err })));
        return;
    }
    let core = core.clone();
    let out = out.clone();
    tokio::spawn(async move {
        let reply = match core.command(request.cmd).await {
            Ok(ok) => WsReply::Ok { id, ok },
            Err(err) => WsReply::Err { id, err },
        };
        let _ = out.send(to_json(&WsServerMessage::Reply(reply)));
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
        ];
        for json in anyone {
            let cmd: ApiCommand = serde_json::from_str(json).expect(json);
            assert!(!cmd.is_local_only(), "{json}");
        }
        assert_eq!(SessionPolicy::LOCAL, SessionPolicy { local: true });
    }
}
