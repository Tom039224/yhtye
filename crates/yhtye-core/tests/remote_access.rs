//! Remote access (`core-design.md` §18) over real HTTP and WebSockets with a
//! fake Tailscale (this PC's "Tailscale address" is 127.0.0.1): pairing, the
//! device token, `whois` checks, local-only commands, revocation, persistence.
//! The same flow against the real Tailscale is `real_tailscale_round_trip`
//! (`#[ignore]`).

use std::net::IpAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use yhtye_core::api::{ApiCommand, ApiEventBody, ApiResponse};
use yhtye_core::remote::{
    ClientKind, Clients, MAX_FAILURES, PairingCode, RemoteDevice, RemoteStatus, Tailscale,
    TailscaleCli, TailscaleError, TailscaleSelf, token_hash,
};
use yhtye_core::runtime::{Core, CoreConfig, host_name};
use yhtye_core::secrets::MemoryBackend;
use yhtye_core::store::{Store, db_path};

const TIMEOUT: Duration = Duration::from_secs(10);
const ME: u64 = 1;
const SOMEONE_ELSE: u64 = 2;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
struct FakeTailscale {
    /// `Err`: Tailscale is "stopped".
    up: Mutex<bool>,
    /// The user every peer belongs to.
    peer_user: u64,
    /// `whois` lookups so far.
    whois_calls: AtomicUsize,
}

impl FakeTailscale {
    fn new() -> Arc<Self> {
        Self::with_peer_user(ME)
    }

    fn with_peer_user(peer_user: u64) -> Arc<Self> {
        Arc::new(Self {
            up: Mutex::new(true),
            peer_user,
            whois_calls: AtomicUsize::new(0),
        })
    }

    fn whois_calls(&self) -> usize {
        self.whois_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Tailscale for FakeTailscale {
    async fn status(&self) -> Result<TailscaleSelf, TailscaleError> {
        if !*self.up.lock().unwrap() {
            return Err(TailscaleError(
                "Tailscale is not running (state: Stopped)".into(),
            ));
        }
        Ok(TailscaleSelf {
            addresses: vec!["127.0.0.1".parse().unwrap()],
            dns_name: Some("pc.test.ts.net".into()),
            user_id: ME,
        })
    }

    async fn whois(&self, _ip: IpAddr) -> Result<u64, TailscaleError> {
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.peer_user)
    }
}

fn config(dir: &Path, tailscale: Arc<dyn Tailscale>) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(dir, "haiku");
    cfg.usage = None;
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    cfg.tailscale = tailscale;
    cfg
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}

async fn status(core: &Core, cmd: ApiCommand) -> RemoteStatus {
    match core.command(cmd).await.expect("status") {
        ApiResponse::RemoteStatus { status } => status,
        other => panic!("unexpected {other:?}"),
    }
}

async fn enable(core: &Core, port: u16) -> RemoteStatus {
    let cmd = ApiCommand::SetRemoteSettings {
        enabled: true,
        port: Some(port),
    };
    status(core, cmd).await
}

async fn new_code(core: &Core) -> PairingCode {
    match core
        .command(ApiCommand::CreatePairingCode)
        .await
        .expect("code")
    {
        ApiResponse::PairingCode { pairing } => pairing,
        other => panic!("unexpected {other:?}"),
    }
}

async fn devices(core: &Core, cmd: ApiCommand) -> Vec<RemoteDevice> {
    match core.command(cmd).await.expect("devices") {
        ApiResponse::RemoteDevices { devices } => devices,
        other => panic!("unexpected {other:?}"),
    }
}

/// `POST /pair`: the status and the JSON body.
async fn pair(base: &str, code: &str, name: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{base}/pair"))
        .json(&json!({"code": code, "device_name": name}))
        .send()
        .await
        .expect("pair request");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn connect(ws_url: &str, token: Option<&str>) -> Result<Ws, WsError> {
    let mut req = ws_url.into_client_request().expect("request");
    if let Some(t) = token {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {t}").parse().expect("header"),
        );
    }
    connect_async(req).await.map(|(ws, _)| ws)
}

fn http_status(e: WsError) -> u16 {
    match e {
        WsError::Http(resp) => resp.status().as_u16(),
        other => panic!("expected an HTTP refusal, got {other}"),
    }
}

/// Sends a command and reads until its reply; events seen meanwhile are kept.
async fn call(ws: &mut Ws, id: u64, cmd: Value, events: &mut Vec<Value>) -> Value {
    let text = json!({"id": id, "cmd": cmd}).to_string();
    ws.send(Message::Text(text.into())).await.expect("send");
    loop {
        let msg = tokio::time::timeout(TIMEOUT, ws.next())
            .await
            .expect("a reply in time")
            .expect("open")
            .expect("ok");
        let Message::Text(t) = msg else { continue };
        let v: Value = serde_json::from_str(t.as_str()).expect("json");
        if let Some(ev) = v.get("event") {
            events.push(ev.clone());
        } else if v["id"] == json!(id) {
            return v;
        }
    }
}

/// Waits until the server closes the socket.
async fn closed(ws: &mut Ws) {
    tokio::time::timeout(TIMEOUT, async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await
    .expect("the server closes the connection");
}

#[tokio::test]
async fn pairing_connecting_local_only_commands_and_revocation() {
    let dir = tempfile::tempdir().expect("dir");
    let tailscale = FakeTailscale::new();
    let core = Core::start(config(dir.path(), tailscale.clone()))
        .await
        .expect("core");
    let mut local_events = core.subscribe();

    let off = status(&core, ApiCommand::GetRemoteStatus).await;
    assert!(!off.enabled && !off.running, "{off:?}");
    assert_eq!(off.port, 1423, "the default port");
    assert_eq!(off.server_id.len(), 32);
    assert!(
        core.command(ApiCommand::CreatePairingCode).await.is_err(),
        "no code while not running"
    );

    let port = free_port();
    let on = enable(&core, port).await;
    assert!(on.running, "{on:?}");
    assert_eq!(on.addresses, [format!("127.0.0.1:{port}")]);
    assert_eq!(on.ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(on.dns_name.as_deref(), Some("pc.test.ts.net"));
    let base = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws");

    // Pairing: a wrong code counts a try, the right one works once.
    let code = new_code(&core).await;
    assert_eq!(code.code.len(), 8);
    assert_eq!(
        code.uri,
        format!(
            "yhtye://pair?v=1&host=pc.test.ts.net&ip=127.0.0.1&port={port}&code={}&sid={}",
            code.code, on.server_id
        )
    );
    assert_eq!(pair(&base, "AAAAAAAA", "Pixel").await.0, 401);
    let (st, paired) = pair(&base, &code.code.to_lowercase(), " Pixel 9 ").await;
    assert_eq!(st, 200, "{paired}");
    assert_eq!(paired["server_id"], on.server_id.as_str());
    let token_a = paired["token"].as_str().expect("token").to_string();
    let device_a = paired["device_id"].as_str().expect("id").to_string();
    assert_eq!(token_a.len(), 64, "256 bits, hex");
    assert_eq!(
        pair(&base, &code.code, "Again").await.0,
        401,
        "a used code is gone"
    );
    let code_b = new_code(&core).await;
    let (st, paired_b) = pair(&base, &code_b.code, "Tablet").await;
    assert_eq!(st, 200, "{paired_b}");
    let device_b = paired_b["device_id"].as_str().expect("id").to_string();
    let token_b = paired_b["token"].as_str().expect("token").to_string();

    // Only the token's hash is stored.
    let store = Store::open(&db_path(dir.path())).await.expect("store");
    let rows = store.remote_devices().await.expect("devices");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "Pixel 9");
    assert_eq!(rows[0].token_hash, token_hash(&token_a));
    for entry in std::fs::read_dir(dir.path()).expect("data dir") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            let bytes = std::fs::read(&path).expect("read");
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(&token_a) && !text.contains(&token_b),
                "{} holds a token",
                path.display()
            );
        }
    }

    // The WebSocket needs a known token.
    assert_eq!(http_status(connect(&ws_url, None).await.unwrap_err()), 401);
    let wrong = "0".repeat(64);
    assert_eq!(
        http_status(connect(&ws_url, Some(&wrong)).await.unwrap_err()),
        401
    );
    let mut ws = connect(&ws_url, Some(&token_a)).await.expect("connects");
    let mut events = Vec::new();
    let pong = call(&mut ws, 1, json!({"type": "ping"}), &mut events).await;
    assert_eq!(pong["ok"]["type"], "pong", "{pong}");
    let listed = call(&mut ws, 2, json!({"type": "list_projects"}), &mut events).await;
    assert_eq!(listed["ok"]["type"], "projects", "{listed}");
    for (id, cmd) in [
        (3, json!({"type": "get_remote_status"})),
        (4, json!({"type": "set_remote_settings", "enabled": false})),
        (5, json!({"type": "create_pairing_code"})),
        (6, json!({"type": "list_remote_devices"})),
        (
            7,
            json!({"type": "revoke_remote_device", "device": device_a}),
        ),
    ] {
        let r = call(&mut ws, id, cmd, &mut events).await;
        assert_eq!(r["err"]["code"], "forbidden", "{r}");
    }

    let listed = devices(&core, ApiCommand::ListRemoteDevices).await;
    let a = listed.iter().find(|d| d.id == device_a).expect("device a");
    assert!(a.connected && a.last_seen_ms.is_some(), "{a:?}");

    // Revoking another device is reported locally, not to the remote client.
    let left = devices(
        &core,
        ApiCommand::RevokeRemoteDevice {
            device: device_b.clone(),
        },
    )
    .await;
    assert_eq!(left.len(), 1);
    let r = call(&mut ws, 8, json!({"type": "ping"}), &mut events).await;
    assert_eq!(r["ok"]["type"], "pong");
    assert!(
        events.iter().all(|e| !e["body"]["type"]
            .as_str()
            .unwrap_or("")
            .starts_with("remote_")),
        "remote events reach a remote client: {events:?}"
    );
    let mut saw_devices = false;
    while let Ok(ev) = local_events.try_recv() {
        if let ApiEventBody::RemoteDevicesChanged { devices } = &ev.body {
            assert!(ev.project.is_empty() && ev.live);
            saw_devices |= devices.len() == 1;
        }
    }
    assert!(saw_devices, "the local UI hears of the revocation");
    assert_eq!(
        http_status(connect(&ws_url, Some(&token_b)).await.unwrap_err()),
        401
    );

    // Revoking the connected device closes its socket at once; its token is dead.
    devices(
        &core,
        ApiCommand::RevokeRemoteDevice {
            device: device_a.clone(),
        },
    )
    .await;
    closed(&mut ws).await;
    assert_eq!(
        http_status(connect(&ws_url, Some(&token_a)).await.unwrap_err()),
        401
    );
    assert!(
        core.command(ApiCommand::RevokeRemoteDevice { device: device_a })
            .await
            .is_err(),
        "unknown device"
    );

    // Every request so far came from 127.0.0.1: one `whois` for all of them.
    assert_eq!(tailscale.whois_calls(), 1, "the answer is kept");

    let code = new_code(&core).await;
    // Five wrong tries drop the code; the status is reported only when it is dropped.
    drain(&mut local_events);
    for _ in 0..MAX_FAILURES - 1 {
        assert_eq!(pair(&base, "WRONGCDE", "Pixel").await.0, 401);
    }
    assert_eq!(
        status_events(&mut local_events),
        0,
        "the code is still valid"
    );
    assert_eq!(pair(&base, "WRONGCDE", "Pixel").await.0, 401);
    assert_eq!(status_events(&mut local_events), 1, "the code was dropped");
    let (st, body) = pair(&base, &code.code, "Pixel").await;
    assert_eq!(st, 401, "{body}");
    assert!(
        status(&core, ApiCommand::GetRemoteStatus)
            .await
            .pairing_expires_at_ms
            .is_none()
    );
    // A new code replaces the old one.
    let old = new_code(&core).await;
    let new = new_code(&core).await;
    if old.code != new.code {
        assert_eq!(pair(&base, &old.code, "Pixel").await.0, 401);
    }
    let (st, paired) = pair(&base, &new.code, "Pixel").await;
    assert_eq!(st, 200);

    // Turning it off stops listening; the device stays paired.
    let off = status(
        &core,
        ApiCommand::SetRemoteSettings {
            enabled: false,
            port: None,
        },
    )
    .await;
    assert!(!off.running && !off.enabled && off.addresses.is_empty());
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    enable(&core, port).await;
    core.shutdown().await;
    drop(core);

    // After a restart: same server id, settings and devices.
    let core = Core::start(config(dir.path(), tailscale.clone()))
        .await
        .expect("restart");
    let st = status(&core, ApiCommand::GetRemoteStatus).await;
    assert!(st.enabled && !st.running, "not started by `Core::start`");
    assert_eq!(st.server_id, on.server_id);
    core.start_remote().await;
    assert!(status(&core, ApiCommand::GetRemoteStatus).await.running);
    let token = paired["token"].as_str().expect("token");
    let mut ws = connect(&ws_url, Some(token)).await.expect("reconnects");
    let r = call(&mut ws, 1, json!({"type": "ping"}), &mut Vec::new()).await;
    assert_eq!(r["ok"]["type"], "pong");
    core.shutdown().await;
    closed(&mut ws).await;
}

/// Throws away the events received so far.
fn drain(events: &mut tokio::sync::broadcast::Receiver<yhtye_core::api::ApiEvent>) {
    status_events(events);
}

/// `remote_status_changed` events received since the last look.
fn status_events(
    events: &mut tokio::sync::broadcast::Receiver<yhtye_core::api::ApiEvent>,
) -> usize {
    use tokio::sync::broadcast::error::TryRecvError;
    let mut n = 0;
    loop {
        match events.try_recv() {
            Ok(ev) => n += usize::from(matches!(ev.body, ApiEventBody::RemoteStatusChanged { .. })),
            Err(TryRecvError::Lagged(_)) => {}
            Err(_) => return n,
        }
    }
}

/// Another Tailscale user's device is refused before anything else (its tries
/// do not use up the code), every time, while `whois` runs once.
#[tokio::test]
async fn another_users_device_is_refused_first_and_looked_up_once() {
    let dir = tempfile::tempdir().expect("dir");
    let tailscale = FakeTailscale::with_peer_user(SOMEONE_ELSE);
    let core = Core::start(config(dir.path(), tailscale.clone()))
        .await
        .expect("core");
    let port = free_port();
    assert!(enable(&core, port).await.running);
    let base = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
    let code = new_code(&core).await;
    for _ in 0..=MAX_FAILURES {
        assert_eq!(pair(&base, "WRONGCDE", "Evil").await.0, 403);
    }
    assert_eq!(pair(&base, &code.code, "Evil").await.0, 403);
    let token = "0".repeat(64);
    assert_eq!(
        http_status(connect(&ws_url, Some(&token)).await.unwrap_err()),
        403
    );
    assert_eq!(
        status(&core, ApiCommand::GetRemoteStatus)
            .await
            .pairing_expires_at_ms,
        Some(code.expires_at_ms),
        "the code's tries were not used"
    );
    assert_eq!(tailscale.whois_calls(), 1, "the refusal is kept too");
    assert!(
        devices(&core, ApiCommand::ListRemoteDevices)
            .await
            .is_empty()
    );

    // Restarting the server forgets the answers.
    let restarted = status(
        &core,
        ApiCommand::SetRemoteSettings {
            enabled: false,
            port: None,
        },
    )
    .await;
    assert!(!restarted.running);
    enable(&core, port).await;
    assert_eq!(pair(&base, "WRONGCDE", "Evil").await.0, 403);
    assert_eq!(tailscale.whois_calls(), 2);
    core.shutdown().await;
}

#[tokio::test]
async fn a_stopped_tailscale_is_reported_and_retried() {
    let dir = tempfile::tempdir().expect("dir");
    let tailscale = FakeTailscale::new();
    *tailscale.up.lock().unwrap() = false;
    let core = Core::start(config(dir.path(), tailscale.clone()))
        .await
        .expect("core");
    let port = free_port();
    let st = enable(&core, port).await;
    assert!(st.enabled && !st.running, "{st:?}");
    assert!(
        st.error
            .as_deref()
            .is_some_and(|e| e.contains("not running")),
        "{st:?}"
    );
    *tailscale.up.lock().unwrap() = true;
    let st = enable(&core, port).await;
    assert!(st.running && st.error.is_none(), "{st:?}");
    // The port must be free: a second core on the same port fails clearly.
    let dir2 = tempfile::tempdir().expect("dir");
    let other = Core::start(config(dir2.path(), tailscale.clone()))
        .await
        .expect("core");
    let st = enable(&other, port).await;
    assert!(
        !st.running
            && st
                .error
                .as_deref()
                .is_some_and(|e| e.contains("cannot listen")),
        "{st:?}"
    );
    assert!(
        core.command(ApiCommand::SetRemoteSettings {
            enabled: true,
            port: Some(0)
        })
        .await
        .is_err()
    );
    other.shutdown().await;
    core.shutdown().await;
}

/// The whole flow against this PC's real Tailscale: listens only on the
/// Tailscale addresses, pair → WebSocket (Ping, ListProjects) → revoke → 401.
/// Needs a running Tailscale: `cargo test -p yhtye-core --test remote_access -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "needs a running Tailscale"]
async fn real_tailscale_round_trip() {
    let dir = tempfile::tempdir().expect("dir");
    let core = Core::start(config(dir.path(), Arc::new(TailscaleCli)))
        .await
        .expect("core");
    let me = TailscaleCli.status().await.expect("tailscale status");
    println!("tailscale: {me:?}");
    let port = free_port();
    let st = enable(&core, port).await;
    assert!(st.running, "{st:?}");
    println!("listening on {:?}", st.addresses);
    let ip = st.ip.clone().expect("an IPv4 address");
    assert!(ip.starts_with("100."), "{ip}");
    // Not on loopback or the wildcard.
    assert!(
        TcpStream::connect(("127.0.0.1", port)).await.is_err(),
        "127.0.0.1 is not served"
    );
    let base = format!("http://{ip}:{port}");
    let ws_url = format!("ws://{ip}:{port}/ws");
    let code = new_code(&core).await;
    println!("pairing URI: {}", code.uri);
    let (status_code, paired) = pair(&base, &code.code, "script client").await;
    assert_eq!(status_code, 200, "{paired}");
    println!(
        "paired: device {} (server {})",
        paired["device_id"], paired["server_id"]
    );
    let token = paired["token"].as_str().expect("token").to_string();
    let mut ws = connect(&ws_url, Some(&token)).await.expect("connects");
    let mut events = Vec::new();
    let pong = call(&mut ws, 1, json!({"type": "ping"}), &mut events).await;
    println!("ping → {pong}");
    assert_eq!(pong["ok"]["type"], "pong");
    let listed = call(&mut ws, 2, json!({"type": "list_projects"}), &mut events).await;
    println!("list_projects → {listed}");
    assert_eq!(listed["ok"]["type"], "projects");
    let r = call(
        &mut ws,
        3,
        json!({"type": "create_pairing_code"}),
        &mut events,
    )
    .await;
    println!("create_pairing_code (remote) → {r}");
    assert_eq!(r["err"]["code"], "forbidden");
    assert_eq!(http_status(connect(&ws_url, None).await.unwrap_err()), 401);
    let device = paired["device_id"].as_str().expect("id").to_string();
    devices(&core, ApiCommand::RevokeRemoteDevice { device }).await;
    closed(&mut ws).await;
    println!("revoked: the connection was closed");
    let refused = http_status(connect(&ws_url, Some(&token)).await.unwrap_err());
    println!("reconnect after revoke → HTTP {refused}");
    assert_eq!(refused, 401);
    core.shutdown().await;
}

async fn clients(core: &Core) -> Clients {
    match core
        .command(ApiCommand::ListClients)
        .await
        .expect("clients")
    {
        ApiResponse::Clients { clients } => clients,
        other => panic!("unexpected {other:?}"),
    }
}

/// The `clients` of `clients_changed` events received since the last look.
fn client_events(
    events: &mut tokio::sync::broadcast::Receiver<yhtye_core::api::ApiEvent>,
) -> Vec<Clients> {
    use tokio::sync::broadcast::error::TryRecvError;
    let mut out = Vec::new();
    loop {
        match events.try_recv() {
            Ok(ev) => {
                if let ApiEventBody::ClientsChanged { clients } = ev.body {
                    assert!(ev.project.is_empty() && ev.live && ev.seq == 0);
                    out.push(clients);
                }
            }
            Err(TryRecvError::Lagged(_)) => {}
            Err(_) => return out,
        }
    }
}

/// Reads events until a `clients_changed` that `pred` accepts.
async fn client_event(ws: &mut Ws, pred: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let Some(Ok(Message::Text(t))) = ws.next().await else {
                panic!("closed before the event");
            };
            let v: Value = serde_json::from_str(t.as_str()).expect("json");
            let clients = &v["event"]["body"]["clients"];
            if v["event"]["body"]["type"] == "clients_changed" && pred(clients) {
                return clients.clone();
            }
        }
    })
    .await
    .expect("clients_changed in time")
}

fn device<'a>(clients: &'a Value, id: &str) -> Option<&'a Value> {
    clients["devices"]
        .as_array()
        .expect("devices")
        .iter()
        .find(|d| d["id"] == id)
}

/// `list_clients` and `clients_changed` are for every client, remote devices
/// too, and carry no token: pairing, connecting, disconnecting and revoking
/// are reported to the PC and to a connected device.
#[tokio::test]
async fn every_client_sees_the_clients_and_their_changes() {
    let dir = tempfile::tempdir().expect("dir");
    let core = Core::start(config(dir.path(), FakeTailscale::new()))
        .await
        .expect("core");
    let mut local_events = core.subscribe();

    let empty = clients(&core).await;
    assert_eq!(empty.server.host, host_name());
    assert!(empty.devices.is_empty(), "no device paired yet");

    let port = enable(&core, free_port()).await.addresses[0].clone();
    let base = format!("http://{port}");
    let ws_url = format!("ws://{port}/ws");
    let (_, a) = pair(&base, &new_code(&core).await.code, "Pixel 9").await;
    let (_, b) = pair(&base, &new_code(&core).await.code, "Tablet").await;
    let (id_a, token_a) = (
        a["device_id"].as_str().unwrap(),
        a["token"].as_str().unwrap(),
    );
    let (id_b, token_b) = (
        b["device_id"].as_str().unwrap(),
        b["token"].as_str().unwrap(),
    );
    let paired = client_events(&mut local_events);
    assert_eq!(
        paired.iter().map(|c| c.devices.len()).collect::<Vec<_>>(),
        [1, 2],
        "one event per pairing"
    );
    let listed = clients(&core).await;
    assert_eq!(listed.devices.len(), 2);
    assert_eq!(listed.devices[0].name, "Pixel 9", "oldest first");
    assert!(
        listed
            .devices
            .iter()
            .all(|d| d.kind == ClientKind::Android && !d.connected)
    );

    // A remote device may read the list; it holds no token or hash.
    let mut ws_a = connect(&ws_url, Some(token_a)).await.expect("a connects");
    let r = call(
        &mut ws_a,
        1,
        json!({"type": "list_clients"}),
        &mut Vec::new(),
    )
    .await;
    assert_eq!(r["ok"]["type"], "clients", "{r}");
    let seen = &r["ok"]["clients"];
    assert_eq!(seen["server"]["host"], host_name());
    assert_eq!(device(seen, id_a).expect("a")["connected"], true);
    assert_eq!(device(seen, id_b).expect("b")["connected"], false);
    assert_eq!(device(seen, id_a).expect("a")["kind"], "android");
    let text = r.to_string();
    for secret in [token_a, token_b, &token_hash(token_a), &token_hash(token_b)] {
        assert!(!text.contains(secret), "a token leaks: {text}");
    }
    assert!(!text.contains("token"), "{text}");

    // B connects, then leaves: A hears both, and so does the PC.
    drain(&mut local_events);
    let ws_b = connect(&ws_url, Some(token_b)).await.expect("b connects");
    client_event(&mut ws_a, |c| {
        device(c, id_b).is_some_and(|d| d["connected"] == true)
    })
    .await;
    drop(ws_b);
    let left = client_event(&mut ws_a, |c| {
        device(c, id_b).is_some_and(|d| d["connected"] == false)
    })
    .await;
    assert!(device(&left, id_b).expect("b")["last_seen_ms"].is_u64());
    let local = client_events(&mut local_events);
    let b_states: Vec<bool> = local
        .iter()
        .filter_map(|c| c.devices.iter().find(|d| d.id == id_b).map(|d| d.connected))
        .collect();
    assert_eq!(b_states, [true, false], "connect and disconnect");

    // Revoking B removes it from the view everywhere.
    devices(
        &core,
        ApiCommand::RevokeRemoteDevice {
            device: id_b.into(),
        },
    )
    .await;
    client_event(&mut ws_a, |c| device(c, id_b).is_none()).await;
    let after = client_events(&mut local_events);
    assert_eq!(after.len(), 1, "one event for the revocation");
    assert_eq!(after[0].devices.len(), 1);
    core.shutdown().await;
}
