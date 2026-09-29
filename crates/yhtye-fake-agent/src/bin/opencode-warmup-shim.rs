//! Test-only stdio shim in front of `opencode acp` (never used by the app).
//!
//! Works around the upstream bug anomalyco/opencode#50236 (OpenCode 2.0.18): the
//! model catalog of the FIRST `session/new` of an `opencode acp` process is
//! snapshotted before the providers are loaded, so `set_config_option model=…`
//! fails with `model not found`. Later sessions (another cwd) are fine.
//!
//! The shim proxies newline-delimited JSON-RPC both ways. Once the client's
//! `initialize` response has been forwarded it sends a throwaway `session/new`
//! (fresh temp dir, no MCP servers), holds the client's messages until that
//! response arrives, then drops the response and every notification of the
//! throwaway session. Everything else passes through unchanged. EOF on stdin
//! closes OpenCode's stdin (so `opencode serve --stdio` exits with it); the
//! shim exits with OpenCode.
//!
//! Usage: `opencode-warmup-shim [args of opencode acp]` (default `acp`); the
//! `opencode` binary is `$YHTYE_OPENCODE_BIN` or `opencode` from the `PATH`.

use std::path::PathBuf;
use std::process::{ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

const WARMUP_ID: &str = "yhtye-test-warmup";

enum Phase {
    /// Waiting for the `initialize` response (its request id, once seen).
    BeforeInit(Option<Value>),
    /// Warm-up sent; client messages are held.
    Warming(Vec<String>),
    /// Warm-up finished; plain proxy.
    Live,
}

struct Shared {
    phase: Phase,
    /// Sender to OpenCode's stdin; `None` after the client's EOF.
    to_agent: Option<mpsc::UnboundedSender<String>>,
    warm_session: Option<String>,
}

type State = Arc<Mutex<Shared>>;

fn lock(state: &State) -> std::sync::MutexGuard<'_, Shared> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn send(state: &Shared, line: String) {
    if let Some(tx) = &state.to_agent {
        let _ = tx.send(line);
    }
}

fn warmup_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("yhtye-warmup-{}-{nanos}", std::process::id()))
}

/// Client → agent: holds messages while the warm-up runs.
async fn client_to_agent(state: State) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut s = lock(&state);
        if let Phase::BeforeInit(id) = &mut s.phase
            && let Ok(v) = serde_json::from_str::<Value>(&line)
            && v.get("method").and_then(Value::as_str) == Some("initialize")
        {
            *id = v.get("id").cloned();
        }
        match &mut s.phase {
            Phase::Warming(queue) => queue.push(line),
            _ => send(&s, line),
        }
    }
    // Client EOF: close OpenCode's stdin.
    lock(&state).to_agent = None;
}

/// What the agent's line means for the warm-up.
enum Action {
    Forward,
    Drop,
}

fn on_agent_line(state: &State, line: &str, dir: &std::path::Path) -> Action {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Action::Forward;
    };
    let mut s = lock(state);
    let is_response = v.get("method").is_none() && v.get("id").is_some();
    match &s.phase {
        Phase::BeforeInit(Some(id)) if is_response && v.get("id") == Some(id) => {
            let req = json!({"jsonrpc": "2.0", "id": WARMUP_ID, "method": "session/new",
                "params": {"cwd": dir, "mcpServers": []}});
            s.phase = Phase::Warming(Vec::new());
            send(&s, req.to_string());
            Action::Forward
        }
        Phase::Warming(_) if is_response && v.get("id") == Some(&json!(WARMUP_ID)) => {
            s.warm_session = v
                .pointer("/result/sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            if v.get("error").is_some() {
                eprintln!("opencode-warmup-shim: warm-up session/new failed: {line}");
            }
            let Phase::Warming(queue) = std::mem::replace(&mut s.phase, Phase::Live) else {
                unreachable!()
            };
            for held in queue {
                send(&s, held);
            }
            Action::Drop
        }
        // While warming, no client session exists yet, so every session-scoped
        // message is the warm-up's (its id is not known before the response).
        Phase::Warming(_) if !is_response && v.pointer("/params/sessionId").is_some() => {
            Action::Drop
        }
        _ => {
            let session = v.pointer("/params/sessionId").and_then(Value::as_str);
            if is_response || session.is_none() || session != s.warm_session.as_deref() {
                Action::Forward
            } else {
                Action::Drop
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args.push("acp".into());
    }
    let bin = std::env::var("YHTYE_OPENCODE_BIN").unwrap_or_else(|_| "opencode".into());
    let mut child = match Command::new(&bin)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("opencode-warmup-shim: cannot run {bin}: {e}");
            return ExitCode::from(127);
        }
    };
    let dir = warmup_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("opencode-warmup-shim: cannot create {}: {e}", dir.display());
        return ExitCode::from(70);
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let state: State = Arc::new(Mutex::new(Shared {
        phase: Phase::BeforeInit(None),
        to_agent: Some(tx),
        warm_session: None,
    }));

    let mut stdin = child.stdin.take().expect("piped stdin");
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if stdin
                .write_all(format!("{line}\n").as_bytes())
                .await
                .is_err()
                || stdin.flush().await.is_err()
            {
                break;
            }
        }
        // `stdin` drops here: OpenCode sees EOF.
    });
    tokio::spawn(client_to_agent(state.clone()));

    let stdout = child.stdout.take().expect("piped stdout");
    let forward_state = state.clone();
    let forward_dir = dir.clone();
    let forwarder = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut out = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            if matches!(
                on_agent_line(&forward_state, &line, &forward_dir),
                Action::Drop
            ) {
                continue;
            }
            if out.write_all(format!("{line}\n").as_bytes()).await.is_err()
                || out.flush().await.is_err()
            {
                break;
            }
        }
    });

    let status = child.wait().await;
    // Let the last output through (a grandchild could hold the pipe open).
    let _ = tokio::time::timeout(Duration::from_secs(2), forwarder).await;
    let _ = std::fs::remove_dir_all(&dir);
    // The stdin reader may be blocked in a read; do not wait for it.
    let code = match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 1,
    };
    std::process::exit(code)
}
