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
//! the first reply (`core-design.md` §10). At most [`MAX_IN_FLIGHT`] requests of
//! one connection are taken on at a time.
//!
//! The server pings every [`PING_INTERVAL`] and closes a connection it has
//! heard nothing from for [`IDLE_TIMEOUT`] (a phone that dropped off the
//! network without closing the socket).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use futures::{Sink, SinkExt, Stream, StreamExt};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc};
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};
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

/// Requests of one connection that are taken on at once: running, or done and
/// waiting for room in the reply queue. A request over the limit is not read
/// off the socket until one finishes, so a client that sends faster than it
/// reads is held back by TCP instead of piling up tasks and replies in memory;
/// one that stays stuck is closed ([`IDLE_TIMEOUT`]).
pub const MAX_IN_FLIGHT: usize = 64;

/// How often the server pings a client. A client answers with a pong on its
/// own (browsers, OkHttp and tungstenite all do); the Android client also pings
/// every 15 s itself.
pub const PING_INTERVAL: Duration = Duration::from_secs(20);

/// A connection is closed when nothing (not even a ping or a pong) was received
/// for this long, or when the client does not let a request finish for this
/// long while [`MAX_IN_FLIGHT`] are running. A few ping intervals, so one lost
/// pong does not cost the connection.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long closing waits for the writer to hand the close frame to a client
/// that is not taking data (a connection that is half-open).
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// Close code of a client dropped for falling behind (1013 "try again later").
const BEHIND: u16 = 1013;

/// Why the server closes a connection (the close frame it tries to send).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closing {
    /// `cancel` fired: a revoked device, or the server stopping.
    Server,
    /// The client fell behind (events or replies it does not take).
    Behind,
    /// It was silent for [`IDLE_TIMEOUT`].
    Idle,
}

impl Closing {
    fn frame(self) -> CloseFrame {
        // A reconnect tells the client why (a revoked device gets 401).
        let (code, reason) = match self {
            Self::Server => (close_code::AWAY, "closed by the server"),
            Self::Behind => (BEHIND, "fell behind; reconnect"),
            Self::Idle => (close_code::AWAY, "no sign of life; reconnect"),
        };
        CloseFrame {
            code,
            reason: reason.into(),
        }
    }
}

/// Serves one client until it leaves or `cancel` fires (then the socket is
/// closed at once, even while requests are still running).
///
/// A writer task owns the socket's sink; events and replies are queued to it
/// ([`OUT_QUEUE`]) and it pings the client ([`PING_INTERVAL`]). The reader
/// ([`read_requests`]) takes at most [`MAX_IN_FLIGHT`] requests on at once and
/// gives up on a client that is silent or stuck ([`IDLE_TIMEOUT`]).
pub async fn run_session(
    socket: WebSocket,
    core: Core,
    policy: SessionPolicy,
    cancel: CancellationToken,
) {
    let (sink, stream) = socket.split();
    let (out_tx, out_rx) = mpsc::channel::<String>(OUT_QUEUE);
    // Subscribe before reading requests so no event after a reply is missed.
    let events = core.subscribe();
    // Fired by `cancel`, or when this client fell too far behind or went silent.
    let session = cancel.child_token();
    let closing = Arc::new(OnceLock::new());
    let mut writer = tokio::spawn(write_loop(
        sink,
        out_rx,
        session.clone(),
        Arc::clone(&closing),
    ));
    let forwarder = tokio::spawn({
        let out = out_tx.clone();
        let session = session.clone();
        let closing = Arc::clone(&closing);
        async move {
            if forward_events(events, out, policy).await == Forwarded::Behind {
                tracing::warn!("a WebSocket client fell behind; closing its connection");
                let _ = closing.set(Closing::Behind);
                session.cancel();
            }
        }
    });
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let ended = read_requests(stream, &in_flight, &session, |text, slot| {
        handle_request(&core, text, &out_tx, policy, slot);
    })
    .await;
    match ended {
        ReadEnd::Silent => {
            tracing::warn!(
                "a WebSocket client sent nothing for {}s; closing its connection",
                IDLE_TIMEOUT.as_secs()
            );
            let _ = closing.set(Closing::Idle);
        }
        ReadEnd::Stalled => {
            tracing::warn!(
                "a WebSocket client let {MAX_IN_FLIGHT} requests wait for {}s; closing its connection",
                IDLE_TIMEOUT.as_secs()
            );
            let _ = closing.set(Closing::Behind);
        }
        ReadEnd::Left | ReadEnd::Cancelled => {}
    }
    forwarder.abort();
    drop(out_tx);
    // The client is gone: the writer stops now rather than when the last
    // request still running drops its sender (their replies are dropped).
    session.cancel();
    // A connection that is half-open does not take the close frame either: do
    // not wait for it, or the device would stay "connected".
    if timeout(CLOSE_GRACE, &mut writer).await.is_err() {
        tracing::warn!("a WebSocket client did not take the close frame; dropping it");
        writer.abort();
    }
}

/// Writes the queued messages to the client and pings it every
/// [`PING_INTERVAL`]; on `session` being cancelled, sends the close frame (see
/// [`Closing`]) and ends.
async fn write_loop<Si>(
    mut sink: Si,
    mut out_rx: mpsc::Receiver<String>,
    session: CancellationToken,
    closing: Arc<OnceLock<Closing>>,
) where
    Si: Sink<Message> + Unpin,
{
    let mut pings = interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    pings.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        let message = tokio::select! {
            text = out_rx.recv() => match text {
                Some(text) => Message::Text(text.into()),
                None => break,
            },
            _ = pings.tick() => Message::Ping(Bytes::new()),
            () = session.cancelled() => {
                let why = closing.get().copied().unwrap_or(Closing::Server);
                let _ = sink.send(Message::Close(Some(why.frame()))).await;
                break;
            }
        };
        if sink.send(message).await.is_err() {
            break;
        }
    }
    let _ = sink.close().await;
}

/// How [`read_requests`] ended.
#[derive(Debug, PartialEq, Eq)]
enum ReadEnd {
    /// The client left: a close frame, an error, or the end of the stream.
    Left,
    /// The session was cancelled (device revoked, server stopping, or the
    /// client fell behind).
    Cancelled,
    /// Nothing was received for [`IDLE_TIMEOUT`].
    Silent,
    /// No request slot became free for [`IDLE_TIMEOUT`]: the client sends but
    /// does not take its replies.
    Stalled,
}

/// Reads the client's messages and hands each request (a text message) to
/// `on_request` with a slot of `in_flight`, which the request holds until it is
/// done. When all slots are taken, the next request waits (and so does
/// everything behind it in the socket) for one to free up.
async fn read_requests<S, E>(
    mut stream: S,
    in_flight: &Arc<Semaphore>,
    session: &CancellationToken,
    mut on_request: impl FnMut(&str, OwnedSemaphorePermit),
) -> ReadEnd
where
    S: Stream<Item = Result<Message, E>> + Unpin,
{
    loop {
        let msg = tokio::select! {
            msg = timeout(IDLE_TIMEOUT, stream.next()) => match msg {
                Ok(msg) => msg,
                Err(_) => return ReadEnd::Silent,
            },
            () = session.cancelled() => return ReadEnd::Cancelled,
        };
        match msg {
            Some(Ok(Message::Text(text))) => {
                let slot = tokio::select! {
                    slot = timeout(IDLE_TIMEOUT, Arc::clone(in_flight).acquire_owned()) => slot,
                    () = session.cancelled() => return ReadEnd::Cancelled,
                };
                match slot {
                    Ok(Ok(slot)) => on_request(text.as_str(), slot),
                    // The semaphore is never closed.
                    Ok(Err(_)) => return ReadEnd::Cancelled,
                    Err(_) => return ReadEnd::Stalled,
                }
            }
            Some(Ok(Message::Close(_)) | Err(_)) | None => return ReadEnd::Left,
            // Pings are answered by axum; a ping or a pong is a sign of life
            // (the timer above restarts); binary is not used.
            Some(Ok(_)) => {}
        }
    }
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

/// Runs one request in the background. `slot` is held until its reply is in
/// the queue (or dropped because the client left), which is what bounds the
/// tasks and the replies waiting for room.
fn handle_request(
    core: &Core,
    text: &str,
    out: &mpsc::Sender<String>,
    policy: SessionPolicy,
    slot: OwnedSemaphorePermit,
) {
    let request = match parse_request(text) {
        Ok(r) => r,
        Err(Some((id, err))) => {
            send_reply(out, WsReply::Err { id, err }, slot);
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
        send_reply(out, WsReply::Err { id, err }, slot);
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
        drop(slot);
    });
}

/// Queues an error reply in the background (the reader must not wait on a
/// client that does not read, and replies are never dropped).
fn send_reply(out: &mpsc::Sender<String>, reply: WsReply, slot: OwnedSemaphorePermit) {
    let out = out.clone();
    tokio::spawn(async move {
        let _ = out.send(to_json(&WsServerMessage::Reply(reply))).await;
        drop(slot);
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
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            r#"{"type": "sleep_chat", "project": "p", "chat": "c"}"#,
            r#"{"type": "sleep_project", "project": "p"}"#,
            r#"{"type": "reorder_projects", "ids": ["p"]}"#,
            r#"{"type": "get_branch_order", "project": "p"}"#,
            r#"{"type": "reorder_branches", "project": "p", "keys": ["/w"]}"#,
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

    type Incoming = futures::channel::mpsc::UnboundedSender<Result<Message, ()>>;

    /// A client's side of the socket: what it sends, and the server's reader.
    fn socket() -> (
        Incoming,
        futures::channel::mpsc::UnboundedReceiver<Result<Message, ()>>,
    ) {
        futures::channel::mpsc::unbounded()
    }

    fn ping_request(id: u64) -> Result<Message, ()> {
        Ok(Message::Text(
            format!(r#"{{"id": {id}, "cmd": {{"type": "ping"}}}}"#).into(),
        ))
    }

    /// Lets the tasks that can run, run (time does not move).
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    async fn test_core(dir: &std::path::Path) -> Core {
        let mut cfg = crate::runtime::CoreConfig::claude_code(dir, "haiku");
        cfg.usage = None;
        cfg.secret_backend = Arc::new(crate::secrets::MemoryBackend::default());
        Core::start(cfg).await.expect("core")
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_client_is_given_up_on_but_any_sign_of_life_restarts_the_timer() {
        let (tx, rx) = socket();
        let in_flight = Arc::new(Semaphore::new(1));
        let session = CancellationToken::new();
        let reader =
            tokio::spawn(async move { read_requests(rx, &in_flight, &session, |_, _| {}).await });
        tokio::time::advance(IDLE_TIMEOUT - Duration::from_secs(1)).await;
        settle().await;
        assert!(!reader.is_finished(), "still within the timeout");
        // A pong, a ping or any message counts.
        tx.unbounded_send(Ok(Message::Pong(Bytes::new())))
            .expect("send");
        tokio::time::advance(IDLE_TIMEOUT - Duration::from_secs(1)).await;
        settle().await;
        assert!(!reader.is_finished(), "the pong restarted the timer");
        tx.unbounded_send(Ok(Message::Ping(Bytes::new())))
            .expect("send");
        tokio::time::advance(IDLE_TIMEOUT - Duration::from_secs(1)).await;
        settle().await;
        assert!(!reader.is_finished(), "so did the ping");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(reader.await.expect("joined"), ReadEnd::Silent);
    }

    #[tokio::test]
    async fn a_client_that_leaves_or_a_cancelled_session_ends_the_reader() {
        let in_flight = Arc::new(Semaphore::new(1));
        let session = CancellationToken::new();
        let (tx, rx) = socket();
        tx.unbounded_send(Ok(Message::Close(None))).expect("send");
        assert_eq!(
            read_requests(rx, &in_flight, &session, |_, _| {}).await,
            ReadEnd::Left
        );
        let (tx, rx) = socket();
        tx.unbounded_send(Err(())).expect("send");
        assert_eq!(
            read_requests(rx, &in_flight, &session, |_, _| {}).await,
            ReadEnd::Left
        );
        let (_tx, rx) = socket();
        session.cancel();
        assert_eq!(
            read_requests(rx, &in_flight, &session, |_, _| {}).await,
            ReadEnd::Cancelled
        );
    }

    #[tokio::test(start_paused = true)]
    async fn requests_over_the_limit_wait_for_a_free_slot_and_then_stall() {
        let (tx, rx) = socket();
        let in_flight = Arc::new(Semaphore::new(2));
        let session = CancellationToken::new();
        for id in 0..3 {
            tx.unbounded_send(ping_request(id)).expect("send");
        }
        let taken = Arc::new(Mutex::new(Vec::new()));
        let reader = tokio::spawn({
            let (in_flight, taken) = (Arc::clone(&in_flight), Arc::clone(&taken));
            async move {
                read_requests(rx, &in_flight, &session, |text, slot| {
                    taken.lock().unwrap().push((text.to_string(), slot));
                })
                .await
            }
        });
        settle().await;
        assert_eq!(taken.lock().unwrap().len(), 2, "the third waits for a slot");
        assert!(!reader.is_finished());
        // One finishes: the waiting one is taken on.
        drop(taken.lock().unwrap().remove(0));
        settle().await;
        assert_eq!(taken.lock().unwrap().len(), 2);
        assert!(taken.lock().unwrap()[1].0.contains(r#""id": 2"#));
        // Nothing frees up for the timeout: the client is stuck.
        tx.unbounded_send(ping_request(3)).expect("send");
        tokio::time::advance(IDLE_TIMEOUT).await;
        assert_eq!(reader.await.expect("joined"), ReadEnd::Stalled);
        assert_eq!(in_flight.available_permits(), 0);
    }

    /// The reply queue of a client that never reads holds at most its capacity,
    /// and no more than the slots' worth of requests wait to put a reply in it,
    /// however many requests the client sends (A of the PR #8 review).
    #[tokio::test]
    async fn a_client_that_does_not_read_cannot_pile_up_requests_or_replies() {
        const QUEUE: usize = 2;
        const SLOTS: usize = 3;
        let dir = tempfile::tempdir().expect("dir");
        let core = test_core(dir.path()).await;
        let (out_tx, out_rx) = mpsc::channel::<String>(QUEUE);
        let (tx, rx) = socket();
        for id in 0..1000 {
            tx.unbounded_send(ping_request(id)).expect("send");
        }
        let in_flight = Arc::new(Semaphore::new(SLOTS));
        let session = CancellationToken::new();
        let handled = AtomicUsize::new(0);
        tokio::time::pause();
        let ended = read_requests(rx, &in_flight, &session, |text, slot| {
            handled.fetch_add(1, Ordering::SeqCst);
            handle_request(&core, text, &out_tx, SessionPolicy::LOCAL, slot);
        })
        .await;
        tokio::time::resume();
        assert_eq!(ended, ReadEnd::Stalled);
        // The queue is full and the slots all wait to put a reply in it; the
        // rest of the 1000 requests were never taken off the socket.
        assert_eq!(handled.load(Ordering::SeqCst), QUEUE + SLOTS);
        assert_eq!(in_flight.available_permits(), 0);
        assert_eq!(out_rx.len(), QUEUE);
        // The client goes away: the waiting ones give up and free their slots.
        drop(out_rx);
        for _ in 0..100 {
            if in_flight.available_permits() == SLOTS {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(in_flight.available_permits(), SLOTS);
        core.shutdown().await;
    }

    #[tokio::test]
    async fn error_replies_hold_a_slot_too() {
        let dir = tempfile::tempdir().expect("dir");
        let core = test_core(dir.path()).await;
        let (out_tx, mut out_rx) = mpsc::channel::<String>(1);
        let in_flight = Arc::new(Semaphore::new(2));
        let handle = |text: &str, remote: bool| {
            let slot = Arc::clone(&in_flight).try_acquire_owned().expect("a slot");
            let policy = if remote {
                SessionPolicy::REMOTE
            } else {
                SessionPolicy::LOCAL
            };
            handle_request(&core, text, &out_tx, policy, slot);
        };
        // A bad request and a forbidden one are answered, in the background.
        handle(r#"{"id": 1, "cmd": {"type": "nope"}}"#, false);
        handle(r#"{"id": 2, "cmd": {"type": "create_pairing_code"}}"#, true);
        settle().await;
        assert_eq!(
            in_flight.available_permits(),
            1,
            "the second one waits for room"
        );
        let first: Value = serde_json::from_str(&out_rx.recv().await.expect("one")).expect("json");
        assert_eq!(first["err"]["code"], "invalid_argument");
        settle().await;
        let second: Value = serde_json::from_str(&out_rx.recv().await.expect("two")).expect("json");
        assert_eq!(second["err"]["code"], "forbidden");
        settle().await;
        assert_eq!(in_flight.available_permits(), 2);
        // Without an id there is nothing to answer, and no slot is kept.
        handle("[]", false);
        assert_eq!(in_flight.available_permits(), 2);
        core.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_writer_pings_every_interval_and_sends_the_close_frame() {
        let (sink, mut sent) = futures::channel::mpsc::unbounded::<Message>();
        let (out_tx, out_rx) = mpsc::channel(4);
        let session = CancellationToken::new();
        let closing = Arc::new(OnceLock::new());
        let writer = tokio::spawn(write_loop(
            sink,
            out_rx,
            session.clone(),
            Arc::clone(&closing),
        ));
        out_tx.send("hello".into()).await.expect("queued");
        assert!(matches!(sent.next().await, Some(Message::Text(t)) if t.as_str() == "hello"));
        for _ in 0..3 {
            tokio::time::advance(PING_INTERVAL).await;
            assert!(matches!(sent.next().await, Some(Message::Ping(_))));
        }
        let _ = closing.set(Closing::Idle);
        session.cancel();
        let Some(Message::Close(Some(frame))) = sent.next().await else {
            panic!("a close frame");
        };
        assert_eq!(frame.code, close_code::AWAY);
        assert!(frame.reason.as_str().contains("no sign of life"));
        writer.await.expect("joined");
    }

    #[tokio::test(start_paused = true)]
    async fn the_writer_closes_with_the_reason_of_the_session() {
        for (why, code) in [
            (None, close_code::AWAY),
            (Some(Closing::Server), close_code::AWAY),
            (Some(Closing::Behind), BEHIND),
        ] {
            let (sink, mut sent) = futures::channel::mpsc::unbounded::<Message>();
            let (_out_tx, out_rx) = mpsc::channel(4);
            let session = CancellationToken::new();
            let closing = Arc::new(OnceLock::new());
            if let Some(why) = why {
                let _ = closing.set(why);
            }
            let writer = tokio::spawn(write_loop(sink, out_rx, session.clone(), closing));
            session.cancel();
            let Some(Message::Close(Some(frame))) = sent.next().await else {
                panic!("a close frame");
            };
            assert_eq!(frame.code, code, "{why:?}");
            writer.await.expect("joined");
        }
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
