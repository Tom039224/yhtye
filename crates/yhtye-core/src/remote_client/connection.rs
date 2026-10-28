//! The connection to one host's `GET /ws`: requests `{id, cmd}` answered by
//! `{id, ok | err}`, pushed `{event}`s, and reconnecting when it breaks (the
//! Rust side of `mobile/…/client/YhtyeClient.kt` and `src/api/ws.ts`;
//! `core-design.md` §10, §18).
//!
//! - [`RemoteConnection::call`]: sends a command and waits for the reply with
//!   the same id. Replies come in any order (the host runs requests
//!   concurrently). A command sent while the connection is being made waits for
//!   it (within its timeout).
//! - [`RemoteConnection::subscribe_events`]: the events the host pushes, as JSON.
//!   They can be missed (while disconnected, or a slow reader falls behind): a
//!   reader takes a new snapshot when [`ConnectionState::Connected::session`]
//!   changes, or when it sees a gap in `seq`.
//! - [`RemoteConnection::watch_state`]: what the connection is doing. It
//!   reconnects with a growing wait ([`Backoff`]) until the host turns the
//!   device down: 401 is [`ConnectionState::Revoked`] (pair again), 403
//!   [`ConnectionState::Rejected`] (stops, [`RemoteConnection::start`] tries again).
//!
//! The replies and events are JSON values: the API types are written, not read,
//! by Rust, and the app hands them on to its UI as JSON anyway.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures::FutureExt;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{Notify, broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep, timeout, timeout_at};
use tokio_util::sync::{CancellationToken, DropGuard};

use super::transport::{Incoming, Refusal, TransportError, WsLink, WsTransport};
use crate::api::{ApiCommand, ApiError};

/// How long a command may take to be answered (opening a project starts an
/// agent, which can need a download first; `src/api/ws.ts` waits as long).
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(180);

/// An event of the host as the host sent it (`{seq, ts_ms, project, live, body}`).
pub type EventPayload = Arc<Value>;

/// The wait between attempts to connect: [`initial`](Self::initial), doubled
/// after each failure, up to [`max`](Self::max).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
        }
    }
}

impl Backoff {
    /// The wait after the `attempt`th failure in a row (counting from 1).
    #[must_use]
    pub fn delay(&self, attempt: u32) -> Duration {
        let doublings = attempt.saturating_sub(1).min(20);
        self.initial
            .max(Duration::from_millis(1))
            .saturating_mul(1 << doublings)
            .min(self.max)
    }
}

/// Timings of a [`RemoteConnection`].
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// How long [`RemoteConnection::call`] waits for the connection and the reply.
    pub call_timeout: Duration,
    /// How long one attempt to connect may take.
    pub connect_timeout: Duration,
    pub backoff: Backoff,
    /// Events kept for a subscriber that is slow; it misses the oldest beyond that.
    pub event_buffer: usize,
    /// How often a WebSocket ping is sent (the host closes a connection it
    /// hears nothing from for a minute).
    pub keepalive_interval: Duration,
    /// A connection the host has sent nothing on (the host pings every 20 s)
    /// for this long is taken for dead, and made again.
    pub idle_timeout: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            call_timeout: DEFAULT_CALL_TIMEOUT,
            connect_timeout: Duration::from_secs(25),
            backoff: Backoff::default(),
            event_buffer: 1024,
            keepalive_interval: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(60),
        }
    }
}

/// What a [`RemoteConnection`] is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    /// Not running ([`RemoteConnection::start`] not called yet, or [`RemoteConnection::stop`]ped).
    Idle,
    /// Making a connection; `attempt` failures in a row so far (0 for the first try).
    Connecting { attempt: u32 },
    /// Connected. `session` counts the connections: when it changes, events
    /// were missed in between and a reader gets its state again.
    Connected { session: u64 },
    /// Disconnected or could not connect; the next try is in `delay_ms`.
    /// `reason` is what went wrong (English, for a note).
    WaitingToReconnect {
        attempt: u32,
        delay_ms: u64,
        reason: Option<String>,
    },
    /// 401: the host does not know this device's token (the device was removed
    /// there). Does not reconnect; pair again.
    Revoked,
    /// 403: the host does not take this device (another Tailscale user's, or
    /// it could not be told). Stops; [`RemoteConnection::start`] tries again.
    Rejected { message: Option<String> },
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => f.write_str("not connected"),
            Self::Connecting { .. } => f.write_str("connecting"),
            Self::Connected { .. } => f.write_str("connected"),
            Self::WaitingToReconnect {
                reason: Some(r), ..
            } => write!(f, "waiting to reconnect ({r})"),
            Self::WaitingToReconnect { reason: None, .. } => f.write_str("waiting to reconnect"),
            Self::Revoked => f.write_str("this device was removed on the host; pair again"),
            Self::Rejected {
                message: Some(m), ..
            } => write!(f, "the host refused this device: {m}"),
            Self::Rejected { message: None } => f.write_str("the host refused this device"),
        }
    }
}

/// Why [`RemoteConnection::call`] did not return a reply.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CallError {
    /// The host answered with an error.
    #[error("{0}")]
    Api(ApiError),
    /// The command was sent but not answered in time (it may still run on the host).
    #[error("no reply to {command} within {timeout:?}")]
    Timeout { command: String, timeout: Duration },
    /// The connection broke before the answer (not sent again: the host may
    /// have run it).
    #[error("the connection was lost before the host answered")]
    ConnectionLost,
    /// There was no connection within the time, or it is stopped, revoked or refused.
    #[error("not connected: {0}")]
    NotConnected(ConnectionState),
    #[error("the command could not be encoded: {0}")]
    Encode(String),
}

type Reply = Result<Value, ApiError>;

#[derive(Default)]
struct Shared {
    /// Where messages for the host go; there while connected.
    out: Option<mpsc::UnboundedSender<String>>,
    pending: HashMap<u64, oneshot::Sender<Reply>>,
    session: u64,
}

struct Run {
    cancel: CancellationToken,
    join: JoinHandle<()>,
}

struct Inner {
    transport: Arc<dyn WsTransport>,
    config: ConnectionConfig,
    state: watch::Sender<ConnectionState>,
    events: broadcast::Sender<EventPayload>,
    shared: Mutex<Shared>,
    next_id: AtomicU64,
    /// "Try now" while waiting to reconnect.
    wake: Notify,
    /// The running loop; also serializes [`RemoteConnection::start`] and [`RemoteConnection::stop`].
    control: tokio::sync::Mutex<Option<Run>>,
    /// Cancelled when the last [`RemoteConnection`] handle is dropped.
    root: CancellationToken,
}

/// The connection to one host. Cheap to clone; the connection ends when
/// [`stop`](Self::stop) is called or the last clone is dropped.
#[derive(Clone)]
pub struct RemoteConnection {
    inner: Arc<Inner>,
    _alive: Arc<DropGuard>,
}

impl fmt::Debug for RemoteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteConnection")
            .field("state", &*self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct Request<'a> {
    id: u64,
    cmd: &'a Value,
}

/// Forgets a request that is no longer waited for (answered, timed out, or the call dropped).
struct PendingGuard<'a> {
    inner: &'a Inner,
    id: u64,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.inner.lock().pending.remove(&self.id);
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_state(&self, state: ConnectionState) {
        self.state.send_replace(state);
    }

    /// Connects, and after every disconnection waits and connects again, until
    /// the host refuses this device or `cancel` fires.
    async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let mut failures: u32 = 0;
        loop {
            self.set_state(ConnectionState::Connecting { attempt: failures });
            let attempt = tokio::select! {
                () = cancel.cancelled() => return,
                r = timeout(self.config.connect_timeout, self.transport.connect()) => r,
            };
            let reason = match attempt {
                Err(_) => {
                    failures += 1;
                    "timed out".to_string()
                }
                Ok(Err(TransportError::Refused { refusal, message })) => {
                    self.set_state(match refusal {
                        Refusal::Unauthorized => ConnectionState::Revoked,
                        Refusal::Forbidden => ConnectionState::Rejected { message },
                    });
                    return;
                }
                Ok(Err(TransportError::Failed(message))) => {
                    failures += 1;
                    message
                }
                Ok(Ok(link)) => {
                    let Some(reason) = self.serve(link, &cancel).await else {
                        return;
                    };
                    failures = 1;
                    reason
                }
            };
            let delay = self.config.backoff.delay(failures);
            self.set_state(ConnectionState::WaitingToReconnect {
                attempt: failures,
                delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                reason: Some(reason),
            });
            tokio::select! {
                () = cancel.cancelled() => return,
                () = sleep(delay) => {}
                () = self.wake.notified() => failures = 0,
            }
        }
    }

    /// Reads the host's messages and writes the queued ones until the
    /// connection breaks (returns why) or `cancel` fires (`None`).
    async fn serve(&self, mut link: Box<dyn WsLink>, cancel: &CancellationToken) -> Option<String> {
        // A "try now" from before this connection is done with.
        let _ = self.wake.notified().now_or_never();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        {
            let mut shared = self.lock();
            shared.session += 1;
            shared.out = Some(out_tx);
            self.set_state(ConnectionState::Connected {
                session: shared.session,
            });
        }
        let silence = self.config.idle_timeout;
        let mut keepalive = interval_at(
            Instant::now() + self.config.keepalive_interval,
            self.config.keepalive_interval,
        );
        keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let idle = sleep(silence);
        tokio::pin!(idle);
        let ended = loop {
            tokio::select! {
                () = cancel.cancelled() => break None,
                incoming = link.recv() => match incoming {
                    Ok(Incoming::Text(text)) => {
                        idle.as_mut().reset(Instant::now() + silence);
                        self.handle(&text);
                    }
                    Ok(Incoming::Heartbeat) => idle.as_mut().reset(Instant::now() + silence),
                    Ok(Incoming::Closed) => break Some("closed by the host".to_string()),
                    Err(e) => break Some(e),
                },
                Some(text) = out_rx.recv() => {
                    match timeout(silence, link.send(text)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => break Some(e),
                        Err(_) => break Some("the host does not take data".to_string()),
                    }
                }
                _ = keepalive.tick() => {
                    if let Err(e) = link.ping().await {
                        break Some(e);
                    }
                }
                () = &mut idle => break Some("the host has been silent too long".to_string()),
            }
        };
        {
            let mut shared = self.lock();
            shared.out = None;
            // Dropping the senders tells every waiting call the connection is lost.
            shared.pending.clear();
        }
        // A broken or half-open connection may not take the close frame.
        let _ = timeout(Duration::from_secs(2), link.close()).await;
        ended
    }

    /// One text message of the host: a reply, an event, or something to ignore.
    fn handle(&self, text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match classify(message) {
            Some(Message::Event(event)) => {
                // No subscriber is fine.
                let _ = self.events.send(Arc::new(event));
            }
            Some(Message::Reply { id, reply }) => {
                let waiting = self.lock().pending.remove(&id);
                if let Some(tx) = waiting {
                    let _ = tx.send(reply);
                }
            }
            None => {}
        }
    }
}

enum Message {
    Event(Value),
    Reply { id: u64, reply: Reply },
}

/// Sorts a host message like `parseIncoming` of `src/api/ws.ts`; `None` for anything malformed.
fn classify(message: Value) -> Option<Message> {
    let Value::Object(mut obj) = message else {
        return None;
    };
    if let Some(event) = obj.remove("event") {
        let has_seq = event.get("seq").is_some_and(Value::is_number);
        return (event.is_object() && has_seq).then_some(Message::Event(event));
    }
    let id = obj.get("id")?.as_u64()?;
    if let Some(ok) = obj.remove("ok") {
        let typed = ok.get("type").is_some_and(Value::is_string);
        return typed.then_some(Message::Reply { id, reply: Ok(ok) });
    }
    let err = obj.remove("err")?;
    let error = serde_json::from_value::<ApiError>(err)
        .unwrap_or_else(|_| ApiError::internal("the host sent an error this client cannot read"));
    Some(Message::Reply {
        id,
        reply: Err(error),
    })
}

impl RemoteConnection {
    /// A connection that is not running yet ([`start`](Self::start) runs it).
    #[must_use]
    pub fn new(transport: Arc<dyn WsTransport>, config: ConnectionConfig) -> Self {
        let (state, _) = watch::channel(ConnectionState::Idle);
        let (events, _) = broadcast::channel(config.event_buffer.max(1));
        let root = CancellationToken::new();
        let alive = Arc::new(root.clone().drop_guard());
        Self {
            inner: Arc::new(Inner {
                transport,
                config,
                state,
                events,
                shared: Mutex::new(Shared::default()),
                next_id: AtomicU64::new(1),
                wake: Notify::new(),
                control: tokio::sync::Mutex::new(None),
                root,
            }),
            _alive: alive,
        }
    }

    /// Connects, and keeps the connection up (reconnecting) until
    /// [`stop`](Self::stop). Already running: tries at once if it is waiting to
    /// reconnect (the app came back to the front; a "retry" button). Does
    /// nothing once [`ConnectionState::Revoked`].
    pub async fn start(&self) {
        let mut control = self.inner.control.lock().await;
        let state = self.state();
        if state == ConnectionState::Revoked {
            return;
        }
        // A refused connection ends its loop (a moment after it says so).
        let running = control.as_ref().is_some_and(|r| !r.join.is_finished())
            && !matches!(state, ConnectionState::Rejected { .. });
        if running {
            self.inner.wake.notify_one();
            return;
        }
        self.inner
            .set_state(ConnectionState::Connecting { attempt: 0 });
        let cancel = self.inner.root.child_token();
        let join = tokio::spawn(Arc::clone(&self.inner).run(cancel.clone()));
        *control = Some(Run { cancel, join });
    }

    /// Disconnects and goes [`ConnectionState::Idle`] (a revoked connection stays
    /// revoked). Commands waiting for an answer fail with [`CallError::ConnectionLost`].
    pub async fn stop(&self) {
        let mut control = self.inner.control.lock().await;
        if let Some(run) = control.take() {
            run.cancel.cancel();
            let _ = run.join.await;
        }
        {
            let mut shared = self.inner.lock();
            shared.out = None;
            shared.pending.clear();
        }
        if self.state() != ConnectionState::Revoked {
            self.inner.set_state(ConnectionState::Idle);
        }
    }

    #[must_use]
    pub fn state(&self) -> ConnectionState {
        self.inner.state.borrow().clone()
    }

    /// A receiver of the state, that wakes on every change.
    #[must_use]
    pub fn watch_state(&self) -> watch::Receiver<ConnectionState> {
        self.inner.state.subscribe()
    }

    /// The events the host pushes from now on (see the module docs for what can be missed).
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<EventPayload> {
        self.inner.events.subscribe()
    }

    /// Runs `command` on the host and returns the reply's `ok` value (a JSON
    /// object with a `type`), waiting up to [`ConnectionConfig::call_timeout`].
    pub async fn call(&self, command: &ApiCommand) -> Result<Value, CallError> {
        self.call_with_timeout(command, self.inner.config.call_timeout)
            .await
    }

    /// [`call`](Self::call) with its own timeout. The time covers waiting for
    /// the connection and for the reply.
    pub async fn call_with_timeout(
        &self,
        command: &ApiCommand,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let deadline = Instant::now() + timeout;
        let cmd = serde_json::to_value(command).map_err(|e| CallError::Encode(e.to_string()))?;
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let text = serde_json::to_string(&Request { id, cmd: &cmd })
            .map_err(|e| CallError::Encode(e.to_string()))?;
        let registered = timeout_at(deadline, self.register(id, &text)).await;
        let reply = match registered {
            Ok(registered) => registered?,
            Err(_) => return Err(CallError::NotConnected(self.state())),
        };
        let _forget = PendingGuard {
            inner: &self.inner,
            id,
        };
        match timeout_at(deadline, reply).await {
            Err(_) => Err(CallError::Timeout {
                command: command_name(&cmd),
                timeout,
            }),
            Ok(Err(_)) => Err(CallError::ConnectionLost),
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(CallError::Api(error)),
        }
    }

    /// Waits for the connection, then queues the request for it and registers
    /// the reply's receiver.
    async fn register(&self, id: u64, text: &str) -> Result<oneshot::Receiver<Reply>, CallError> {
        let mut state = self.inner.state.subscribe();
        loop {
            let current = state.borrow_and_update().clone();
            match current {
                ConnectionState::Idle
                | ConnectionState::Revoked
                | ConnectionState::Rejected { .. } => return Err(CallError::NotConnected(current)),
                ConnectionState::Connected { .. } => {
                    if let Some(reply) = self.try_queue(id, text) {
                        return Ok(reply);
                    }
                    // Just disconnected (the state follows): wait for the next one.
                }
                ConnectionState::Connecting { .. } | ConnectionState::WaitingToReconnect { .. } => {
                }
            }
            if state.changed().await.is_err() {
                return Err(CallError::NotConnected(self.state()));
            }
        }
    }

    fn try_queue(&self, id: u64, text: &str) -> Option<oneshot::Receiver<Reply>> {
        let mut shared = self.inner.lock();
        let out = shared.out.clone()?;
        out.send(text.to_string()).ok()?;
        let (tx, rx) = oneshot::channel();
        shared.pending.insert(id, tx);
        Some(rx)
    }
}

/// The `type` of a command (for messages).
fn command_name(cmd: &Value) -> String {
    cmd.get("type")
        .and_then(Value::as_str)
        .unwrap_or("command")
        .to_string()
}

#[cfg(test)]
mod tests;
