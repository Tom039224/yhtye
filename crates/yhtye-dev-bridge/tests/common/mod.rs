//! Shared helpers of the bridge tests: fake agents, a temporary repository,
//! a running bridge and a WebSocket client.
#![allow(dead_code)]

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
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_dev_bridge::{BridgeConfig, serve};

pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const TOKEN: &str = "test-token";

pub type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fake_agent_bin() -> PathBuf {
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

pub fn fake_harness(bin: &Path, script: Value) -> HarnessConfig {
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

pub fn git(dir: &Path, args: &[&str]) {
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
pub fn temp_repo(root: &Path) -> (PathBuf, PathBuf) {
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

pub async fn connect(port: u16, token: &str, origin: Option<&str>) -> Result<Ws, WsError> {
    let mut req = format!("ws://127.0.0.1:{port}/ws?token={token}")
        .into_client_request()
        .expect("request");
    if let Some(o) = origin {
        req.headers_mut()
            .insert("origin", o.parse().expect("header"));
    }
    connect_async(req).await.map(|(ws, _)| ws)
}

pub async fn send(ws: &mut Ws, text: String) {
    ws.send(Message::Text(text.into())).await.expect("send");
}

pub async fn next_json(ws: &mut Ws) -> Value {
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
pub async fn reply(ws: &mut Ws, id: u64, events: &mut Vec<Value>) -> Value {
    loop {
        let msg = next_json(ws).await;
        if msg.get("event").is_some() {
            events.push(msg["event"].clone());
        } else if msg["id"] == json!(id) {
            return msg;
        }
    }
}

pub async fn until_event(ws: &mut Ws, events: &mut Vec<Value>, f: impl Fn(&Value) -> bool) {
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

/// A core whose every role runs the fake agent with these scripts.
pub async fn fake_core(data: &Path, orch: Value, implementer: Value) -> Core {
    let bin = fake_agent_bin();
    let mut cfg = CoreConfig::claude_code(data, "haiku");
    cfg.harnesses = vec![HarnessPreset::fixed(
        "fake",
        fake_harness(&bin, orch),
        fake_harness(&bin, implementer),
        fake_harness(&bin, json!({"turns": []})),
    )];
    cfg.default_agent = AgentChoice::new("fake", None);
    cfg.usage = None;
    Core::start(cfg).await.expect("core")
}

/// A bridge serving `core` on a free port until `stop` is sent.
pub struct Bridge {
    pub port: u16,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Bridge {
    pub async fn start(core: &Core) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve(
            listener,
            core.clone(),
            BridgeConfig::new(TOKEN),
            async move {
                let _ = stop_rx.await;
            },
        ));
        Self { port, stop, server }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(TIMEOUT, self.server)
            .await
            .expect("server stops")
            .expect("join")
            .expect("serve ok");
    }
}
