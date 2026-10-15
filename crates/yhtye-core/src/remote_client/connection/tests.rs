//! The connection against a scripted host (no sockets): ids, errors, events,
//! reconnecting with the backoff, refusals, timeouts.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::*;
use crate::api::ApiErrorCode;

/// What the client wrote to the fake socket.
#[derive(Debug)]
enum Sent {
    Text(String),
    Ping,
}

struct FakeLink {
    from_host: mpsc::UnboundedReceiver<Result<Incoming, String>>,
    to_host: mpsc::UnboundedSender<Sent>,
}

#[async_trait]
impl WsLink for FakeLink {
    async fn recv(&mut self) -> Result<Incoming, String> {
        self.from_host.recv().await.unwrap_or(Ok(Incoming::Closed))
    }

    async fn send(&mut self, text: String) -> Result<(), String> {
        self.to_host
            .send(Sent::Text(text))
            .map_err(|e| e.to_string())
    }

    async fn ping(&mut self) -> Result<(), String> {
        self.to_host.send(Sent::Ping).map_err(|e| e.to_string())
    }

    async fn close(&mut self) {}
}

/// The host's end of a fake socket.
struct Host {
    to_client: mpsc::UnboundedSender<Result<Incoming, String>>,
    from_client: mpsc::UnboundedReceiver<Sent>,
}

impl Host {
    /// The next request of the client (pings skipped), as JSON.
    async fn request(&mut self) -> Value {
        loop {
            match self.from_client.recv().await.expect("the client wrote") {
                Sent::Text(text) => return serde_json::from_str(&text).expect("json"),
                Sent::Ping => {}
            }
        }
    }

    fn say(&self, value: Value) {
        self.to_client
            .send(Ok(Incoming::Text(value.to_string())))
            .expect("client reads");
    }

    fn ok(&self, id: &Value, ok: Value) {
        self.say(json!({"id": id, "ok": ok}));
    }

    fn drop_connection(&self) {
        self.to_client
            .send(Ok(Incoming::Closed))
            .expect("client reads");
    }
}

fn link() -> (FakeLink, Host) {
    let (to_client, from_host) = mpsc::unbounded_channel();
    let (to_host, from_client) = mpsc::unbounded_channel();
    (
        FakeLink { from_host, to_host },
        Host {
            to_client,
            from_client,
        },
    )
}

/// Hands out the scripted connection attempts in order (then fails every one).
struct FakeTransport {
    script: Mutex<VecDeque<Result<FakeLink, TransportError>>>,
    attempts: AtomicUsize,
}

impl FakeTransport {
    fn new(script: Vec<Result<FakeLink, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into()),
            attempts: AtomicUsize::new(0),
        })
    }

    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WsTransport for FakeTransport {
    async fn connect(&self) -> Result<Box<dyn WsLink>, TransportError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let next = self.script.lock().unwrap().pop_front();
        match next {
            Some(Ok(link)) => Ok(Box::new(link)),
            Some(Err(e)) => Err(e),
            None => Err(TransportError::Failed("nobody there".into())),
        }
    }
}

fn failed(why: &str) -> Result<FakeLink, TransportError> {
    Err(TransportError::Failed(why.into()))
}

fn refused(refusal: Refusal, message: Option<&str>) -> Result<FakeLink, TransportError> {
    Err(TransportError::Refused {
        refusal,
        message: message.map(str::to_string),
    })
}

async fn wait_for(
    conn: &RemoteConnection,
    wanted: impl Fn(&ConnectionState) -> bool,
) -> ConnectionState {
    let mut state = conn.watch_state();
    let seen = state
        .wait_for(|s| wanted(s))
        .await
        .expect("the connection lives");
    seen.clone()
}

async fn connected(conn: &RemoteConnection) -> u64 {
    match wait_for(conn, |s| matches!(s, ConnectionState::Connected { .. })).await {
        ConnectionState::Connected { session } => session,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn replies_find_their_calls_whatever_the_order() {
    let (link, mut host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    conn.start().await;
    connected(&conn).await;

    let first = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    let a = host.request().await;
    let second = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::ListProjects).await }
    });
    let b = host.request().await;
    assert_eq!(a["cmd"]["type"], "ping");
    assert_eq!(b["cmd"]["type"], "list_projects");
    assert_ne!(a["id"], b["id"]);

    // The second is answered first.
    host.ok(&b["id"], json!({"type": "projects", "projects": []}));
    host.ok(
        &a["id"],
        json!({"type": "pong", "host": "pc", "version": "0.1.0"}),
    );
    assert_eq!(second.await.unwrap().unwrap()["type"], "projects");
    assert_eq!(first.await.unwrap().unwrap()["host"], "pc");
}

#[tokio::test(start_paused = true)]
async fn an_error_reply_is_the_calls_api_error() {
    let (link, mut host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    conn.start().await;
    let call = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    let request = host.request().await;
    host.say(
        json!({"id": request["id"], "err": {"code": "not_found", "message": "no such project"}}),
    );
    assert_eq!(
        call.await.unwrap(),
        Err(CallError::Api(ApiError::new(
            ApiErrorCode::NotFound,
            "no such project"
        )))
    );
}

#[tokio::test(start_paused = true)]
async fn events_are_published_and_malformed_messages_are_dropped() {
    let (link, host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    let mut events = conn.subscribe_events();
    conn.start().await;
    connected(&conn).await;

    host.say(json!({"event": {"seq": 4, "ts_ms": 1, "project": "P-1", "live": false, "body": {"type": "x"}}}));
    host.to_client
        .send(Ok(Incoming::Text("not json".into())))
        .unwrap();
    host.say(json!({"event": {"no": "seq"}}));
    host.say(json!({"id": 99, "ok": {"type": "pong", "host": "x"}})); // nobody waits for it
    host.say(json!({"event": {"seq": 5, "ts_ms": 1, "project": "P-1", "live": true, "body": {"type": "y"}}}));

    let one = events.recv().await.unwrap();
    let two = events.recv().await.unwrap();
    assert_eq!(
        (one["seq"].clone(), two["seq"].clone()),
        (json!(4), json!(5))
    );
}

#[tokio::test(start_paused = true)]
async fn a_call_waits_for_the_connection() {
    let (link, mut host) = link();
    let transport = FakeTransport::new(vec![failed("down"), Ok(link)]);
    let conn = RemoteConnection::new(transport, ConnectionConfig::default());
    conn.start().await;
    // The first attempt fails; the call goes out once the second connects.
    let call = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    let request = host.request().await;
    host.ok(
        &request["id"],
        json!({"type": "pong", "host": "pc", "version": "0.1.0"}),
    );
    assert!(call.await.unwrap().is_ok());
}

#[tokio::test(start_paused = true)]
async fn a_call_without_start_or_after_stop_is_not_connected() {
    let (link, host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    assert_eq!(
        conn.call(&ApiCommand::Ping).await,
        Err(CallError::NotConnected(ConnectionState::Idle))
    );
    conn.start().await;
    connected(&conn).await;
    let waiting = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    // The request is out (the host never answers); stopping fails it.
    tokio::task::yield_now().await;
    conn.stop().await;
    assert_eq!(waiting.await.unwrap(), Err(CallError::ConnectionLost));
    assert_eq!(conn.state(), ConnectionState::Idle);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn a_call_gives_up_after_its_timeout_and_the_connection_goes_on() {
    let (link, mut host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    conn.start().await;
    connected(&conn).await;
    let started = Instant::now();
    let result = conn
        .call_with_timeout(&ApiCommand::Ping, Duration::from_secs(7))
        .await;
    assert_eq!(
        result,
        Err(CallError::Timeout {
            command: "ping".into(),
            timeout: Duration::from_secs(7)
        })
    );
    assert_eq!(started.elapsed(), Duration::from_secs(7));
    assert!(matches!(conn.state(), ConnectionState::Connected { .. }));
    // A late reply to the forgotten request is ignored; the next call works.
    let late = host.request().await;
    host.ok(&late["id"], json!({"type": "pong", "host": "late"}));
    let next = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    let request = host.request().await;
    host.ok(&request["id"], json!({"type": "pong", "host": "pc"}));
    assert_eq!(next.await.unwrap().unwrap()["host"], "pc");
}

#[tokio::test(start_paused = true)]
async fn the_connection_is_made_again_after_it_breaks() {
    let (first, mut host1) = link();
    let (second, host2) = link();
    let transport = FakeTransport::new(vec![Ok(first), Ok(second)]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    assert_eq!(connected(&conn).await, 1);

    let lost = tokio::spawn({
        let conn = conn.clone();
        async move { conn.call(&ApiCommand::Ping).await }
    });
    host1.request().await;
    host1.drop_connection();
    assert_eq!(lost.await.unwrap(), Err(CallError::ConnectionLost));

    let waiting = wait_for(&conn, |s| {
        matches!(s, ConnectionState::WaitingToReconnect { .. })
    })
    .await;
    assert_eq!(
        waiting,
        ConnectionState::WaitingToReconnect {
            attempt: 1,
            delay_ms: 1000,
            reason: Some("closed by the host".into())
        }
    );
    // The second connection is a new session: readers get their state again.
    assert_eq!(connected(&conn).await, 2);
    assert_eq!(transport.attempts(), 2);
    drop(host2);
}

#[tokio::test(start_paused = true)]
async fn failures_wait_longer_each_time_up_to_the_limit() {
    let backoff = Backoff::default();
    let delays: Vec<u64> = (1..=8).map(|n| backoff.delay(n).as_secs()).collect();
    assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(backoff.delay(0), Duration::from_secs(1));
    assert_eq!(backoff.delay(u32::MAX), Duration::from_secs(30));

    let transport = FakeTransport::new(vec![]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    for attempt in 1..=4u32 {
        let waiting = wait_for(&conn, |s| {
            matches!(s, ConnectionState::WaitingToReconnect { attempt: a, .. } if *a == attempt)
        })
        .await;
        let ConnectionState::WaitingToReconnect {
            delay_ms, reason, ..
        } = waiting
        else {
            unreachable!()
        };
        assert_eq!(delay_ms, 1000 * (1 << (attempt - 1)));
        assert_eq!(reason.as_deref(), Some("nobody there"));
    }
}

#[tokio::test(start_paused = true)]
async fn start_while_waiting_tries_at_once_and_counts_failures_afresh() {
    // Nobody answers: 1 s, then 2 s, then (asked to retry) straight away.
    let transport = FakeTransport::new(vec![]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    wait_for(&conn, |s| {
        matches!(s, ConnectionState::WaitingToReconnect { attempt: 2, .. })
    })
    .await;
    let asked = Instant::now();
    conn.start().await;
    // The counter starts over: the next wait is the shortest again, and it began at once.
    let waiting = wait_for(&conn, |s| {
        matches!(s, ConnectionState::WaitingToReconnect { attempt: 1, .. })
    })
    .await;
    assert!(matches!(
        waiting,
        ConnectionState::WaitingToReconnect { delay_ms: 1000, .. }
    ));
    assert_eq!(
        asked.elapsed(),
        Duration::ZERO,
        "no waiting out the backoff"
    );
    assert_eq!(transport.attempts(), 3);

    // The same while the first wait is on, ending in a connection.
    let (link, host) = link();
    let transport = FakeTransport::new(vec![failed("down"), Ok(link)]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    wait_for(&conn, |s| {
        matches!(s, ConnectionState::WaitingToReconnect { .. })
    })
    .await;
    let asked = Instant::now();
    conn.start().await;
    assert_eq!(connected(&conn).await, 1);
    assert_eq!(asked.elapsed(), Duration::ZERO);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn a_401_is_final_and_a_403_can_be_tried_again() {
    let transport = FakeTransport::new(vec![refused(Refusal::Unauthorized, Some("unknown token"))]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    wait_for(&conn, |s| *s == ConnectionState::Revoked).await;
    conn.start().await; // nothing to try with
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(conn.state(), ConnectionState::Revoked);
    assert_eq!(transport.attempts(), 1);
    assert_eq!(
        conn.call(&ApiCommand::Ping).await,
        Err(CallError::NotConnected(ConnectionState::Revoked))
    );
    conn.stop().await;
    assert_eq!(
        conn.state(),
        ConnectionState::Revoked,
        "stopping does not forgive a revocation"
    );

    let (link, host) = link();
    let transport = FakeTransport::new(vec![
        refused(Refusal::Forbidden, Some("another Tailscale user")),
        Ok(link),
    ]);
    let conn = RemoteConnection::new(transport.clone(), ConnectionConfig::default());
    conn.start().await;
    let rejected = wait_for(&conn, |s| matches!(s, ConnectionState::Rejected { .. })).await;
    assert_eq!(
        rejected,
        ConnectionState::Rejected {
            message: Some("another Tailscale user".into())
        }
    );
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(transport.attempts(), 1, "no retry of its own");
    conn.start().await;
    connected(&conn).await;
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn a_silent_connection_is_taken_for_dead() {
    let (quiet, host1) = link();
    let (alive, host2) = link();
    let transport = FakeTransport::new(vec![Ok(quiet), Ok(alive)]);
    let conn = RemoteConnection::new(transport, ConnectionConfig::default());
    conn.start().await;
    connected(&conn).await;
    // Pings now and then keep it; once they stop for a minute, it is let go.
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_secs(40)).await;
        host1.to_client.send(Ok(Incoming::Heartbeat)).unwrap();
    }
    assert!(matches!(
        conn.state(),
        ConnectionState::Connected { session: 1 }
    ));
    let waiting = wait_for(&conn, |s| {
        matches!(s, ConnectionState::WaitingToReconnect { .. })
    })
    .await;
    assert!(matches!(
        waiting,
        ConnectionState::WaitingToReconnect { reason: Some(r), .. } if r.contains("silent")
    ));
    assert_eq!(connected(&conn).await, 2);
    drop((host1, host2));
}

#[tokio::test(start_paused = true)]
async fn the_client_pings_to_keep_the_connection_warm() {
    let (link, mut host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    conn.start().await;
    connected(&conn).await;
    let sent = host.from_client.recv().await.expect("a ping");
    assert!(matches!(sent, Sent::Ping), "{sent:?}");
    host.to_client.send(Ok(Incoming::Heartbeat)).unwrap();
}

#[tokio::test(start_paused = true)]
async fn dropping_the_last_handle_ends_the_connection() {
    let (link, mut host) = link();
    let conn = RemoteConnection::new(
        FakeTransport::new(vec![Ok(link)]),
        ConnectionConfig::default(),
    );
    conn.start().await;
    connected(&conn).await;
    let other = conn.clone();
    drop(conn);
    // Another handle keeps it going.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(matches!(other.state(), ConnectionState::Connected { .. }));
    drop(other);
    // The loop stops and lets go of the socket, which the host sees as the end.
    assert!(host.from_client.recv().await.is_none());
}
