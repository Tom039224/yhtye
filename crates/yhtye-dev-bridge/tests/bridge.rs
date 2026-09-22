//! The bridge over a real WebSocket with the real core and fake agents:
//! access control, request/reply matching, errors, and pushed events.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use yhtye_core::acp::{HarnessConfig, ModelSelect};
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_dev_bridge::{BridgeConfig, serve};

const TIMEOUT: Duration = Duration::from_secs(30);
const TOKEN: &str = "test-token";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fake_agent_bin() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "-p", "yhtye-fake-agent"])
        .current_dir(workspace_root())
        .status()
        .expect("run cargo build");
    assert!(status.success(), "building yhtye-fake-agent failed");
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from)
        .join("debug/yhtye-fake-agent")
}

fn fake_harness(bin: &Path, script: Value) -> HarnessConfig {
    let mut h = HarnessConfig::plain(bin.display().to_string(), vec![]);
    h.env.insert("YHTYE_FAKE_SCRIPT".into(), script.to_string());
    h.mode_after_new = Some("bypassPermissions".into());
    h.model = Some(ModelSelect {
        config_id: "model".into(),
        value: "haiku".into(),
    });
    h.startup_timeout = Duration::from_secs(10);
    h
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .status()
        .expect("git")
        .success();
    assert!(ok, "git {args:?}");
}

/// A repository with one commit, and a data directory next to it.
fn temp_repo(root: &Path) -> (PathBuf, PathBuf) {
    let repo = root.join("repo");
    let data = root.join("data");
    std::fs::create_dir_all(&repo).expect("mkdir");
    std::fs::create_dir_all(&data).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    std::fs::write(repo.join("README.md"), "# test\n").expect("write");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "initial"]);
    (repo, data)
}

async fn connect(port: u16, token: &str, origin: Option<&str>) -> Result<Ws, WsError> {
    let mut req = format!("ws://127.0.0.1:{port}/ws?token={token}")
        .into_client_request()
        .expect("request");
    if let Some(o) = origin {
        req.headers_mut()
            .insert("origin", o.parse().expect("header"));
    }
    connect_async(req).await.map(|(ws, _)| ws)
}

fn http_status(e: WsError) -> u16 {
    match e {
        WsError::Http(resp) => resp.status().as_u16(),
        other => panic!("expected an HTTP refusal, got {other}"),
    }
}

async fn send(ws: &mut Ws, text: String) {
    ws.send(Message::Text(text.into())).await.expect("send");
}

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(TIMEOUT, ws.next())
            .await
            .expect("a message in time")
            .expect("open")
            .expect("ok");
        if let Message::Text(t) = msg {
            return serde_json::from_str(t.as_str()).expect("json");
        }
    }
}

/// Reads until the reply to `id`; events seen meanwhile are collected.
async fn reply(ws: &mut Ws, id: u64, events: &mut Vec<Value>) -> Value {
    loop {
        let msg = next_json(ws).await;
        if msg.get("event").is_some() {
            events.push(msg["event"].clone());
        } else if msg["id"] == json!(id) {
            return msg;
        }
    }
}

async fn until_event(ws: &mut Ws, events: &mut Vec<Value>, f: impl Fn(&Value) -> bool) {
    loop {
        let msg = next_json(ws).await;
        if let Some(ev) = msg.get("event") {
            events.push(ev.clone());
            if f(ev) {
                return;
            }
        }
    }
}

#[tokio::test]
async fn the_api_round_trips_over_the_websocket() {
    let root = tempfile::tempdir().expect("tempdir");
    let (repo, data) = temp_repo(root.path());
    let bin = fake_agent_bin();
    let orch = json!({"turns": [
        {"match": "again", "actions": [{"message": "Again."}]},
        {"match": "hello", "actions": [{"message": "Hi "}, {"message": "there."}]}
    ]});
    let mut cfg = CoreConfig::claude_code(&data, "haiku");
    cfg.orchestrator = fake_harness(&bin, orch);
    cfg.implementer = fake_harness(&bin, json!({"turns": []}));
    cfg.reviewer = fake_harness(&bin, json!({"turns": []}));
    cfg.usage = None;
    let core = Core::start(cfg).await.expect("core");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(
        listener,
        core.clone(),
        BridgeConfig::new(TOKEN),
        async move {
            let _ = stop_rx.await;
        },
    ));

    // Access control.
    assert_eq!(
        http_status(connect(port, "wrong", None).await.unwrap_err()),
        401
    );
    let evil = Some("https://evil.example");
    assert_eq!(
        http_status(connect(port, TOKEN, evil).await.unwrap_err()),
        403
    );
    let mut ws = connect(port, TOKEN, Some("http://localhost:1420"))
        .await
        .expect("allowed origin connects");
    let mut events = Vec::new();

    // Errors: malformed command, unknown project.
    send(
        &mut ws,
        json!({"id": 1, "cmd": {"type": "nope"}}).to_string(),
    )
    .await;
    let r = reply(&mut ws, 1, &mut events).await;
    assert_eq!(r["err"]["code"], "invalid_argument", "{r}");
    let cmd = json!({"id": 2, "cmd": {"type": "get_snapshot", "project": "x"}});
    send(&mut ws, cmd.to_string()).await;
    assert_eq!(
        reply(&mut ws, 2, &mut events).await["err"]["code"],
        "not_found"
    );
    send(&mut ws, "garbage".into()).await; // ignored: nothing to reply to

    // Open, then two requests in flight at once, each answered under its id.
    let path = repo.display().to_string();
    let cmd = json!({"id": 3, "cmd": {"type": "open_project", "path": path}});
    send(&mut ws, cmd.to_string()).await;
    let opened = reply(&mut ws, 3, &mut events).await;
    assert_eq!(opened["ok"]["type"], "project", "{opened}");
    let project = opened["ok"]["project"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let list = json!({"id": 4, "cmd": {"type": "list_projects"}});
    let snap = json!({"id": 5, "cmd": {"type": "get_snapshot", "project": project}});
    send(&mut ws, list.to_string()).await;
    send(&mut ws, snap.to_string()).await;
    let mut replies = Vec::new();
    while replies.len() < 2 {
        let msg = next_json(&mut ws).await;
        if msg.get("id").is_some() {
            replies.push(msg);
        }
    }
    replies.sort_by_key(|m| m["id"].as_u64());
    assert_eq!(replies[0]["ok"]["projects"][0]["open"], true);
    assert_eq!(replies[1]["ok"]["type"], "snapshot");

    // A message: accepted, then the orchestrator's streamed and coalesced text arrives.
    let msg =
        json!({"id": 6, "cmd": {"type": "send_user_message", "project": project, "text": "hello"}});
    send(&mut ws, msg.to_string()).await;
    assert_eq!(
        reply(&mut ws, 6, &mut events).await["ok"]["type"],
        "accepted"
    );
    until_event(&mut ws, &mut events, |e| {
        e["body"]["type"] == "agent_text" && e["body"]["text"] == "Hi there."
    })
    .await;
    assert!(
        events.iter().any(|e| e["live"] == true),
        "chunks are pushed live"
    );
    let seqs: Vec<u64> = events
        .iter()
        .filter(|e| e["live"] == false)
        .filter_map(|e| e["seq"].as_u64())
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "durable seqs without gaps: {seqs:?}"
    );

    // A second client gets the same events.
    let mut other = connect(port, TOKEN, None).await.expect("second client");
    let msg = json!({"id": 1, "cmd": {"type": "send_user_message", "project": project, "text": "hello again"}});
    send(&mut ws, msg.to_string()).await;
    let mut seen = Vec::new();
    until_event(&mut other, &mut seen, |e| e["body"]["type"] == "agent_text").await;

    let _ = stop_tx.send(());
    drop(ws);
    drop(other);
    tokio::time::timeout(TIMEOUT, server)
        .await
        .expect("server stops")
        .expect("join")
        .expect("serve ok");
    core.shutdown().await;
}
