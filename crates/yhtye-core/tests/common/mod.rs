//! Shared helpers for ACP integration tests.
#![allow(dead_code)]

pub mod chats;
pub mod codex;
pub mod devin;
#[cfg(unix)]
pub mod gate;
pub mod grok_build;
pub mod minimax_code;
pub mod opencode;
pub mod orch;
pub mod real;
pub mod repo;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::schema::StopReason;
use yhtye_core::acp::{
    AgentError, AgentEvent, AgentHandle, AgentOutput, HarnessConfig, ModelSelect, SpawnOptions,
    spawn_agent,
};

/// Default wait for a single event in fake-agent tests.
pub const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Path of a binary of `yhtye-fake-agent`, building the crate once per test process.
pub fn fake_agent_crate_bin(name: &str) -> PathBuf {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--quiet", "-p", "yhtye-fake-agent"])
            .current_dir(workspace_root())
            .status()
            .expect("run cargo build for yhtye-fake-agent");
        assert!(status.success(), "building yhtye-fake-agent failed");
    });
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from);
    target.join("debug").join(name)
}

/// Path of the fake agent binary.
pub fn fake_agent_bin() -> PathBuf {
    fake_agent_crate_bin("yhtye-fake-agent")
}

/// A fake-agent harness configured like Claude Code (bypass mode, haiku model).
pub fn fake_harness(script: serde_json::Value) -> HarnessConfig {
    let mut h = HarnessConfig::plain(fake_agent_bin().display().to_string(), vec![]);
    h.env.insert("YHTYE_FAKE_SCRIPT".into(), script.to_string());
    h.mode_after_new = Some("bypassPermissions".into());
    h.model = Some(ModelSelect {
        config_id: "model".into(),
        value: "haiku".into(),
    });
    h.startup_timeout = Duration::from_secs(10);
    h
}

/// A running session plus its event stream.
pub struct Session {
    pub handle: AgentHandle,
    pub events: mpsc::UnboundedReceiver<AgentEvent>,
    pub timeout: Duration,
}

pub async fn start(harness: &HarnessConfig, cwd: &Path, options: SpawnOptions) -> Session {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = spawn_agent(harness, cwd, options, tx)
        .await
        .expect("agent starts");
    Session {
        handle,
        events: rx,
        timeout: EVENT_TIMEOUT,
    }
}

/// Starts and expects a startup failure; returns the error and the remaining events.
pub async fn start_err(
    harness: &HarnessConfig,
    cwd: &Path,
    options: SpawnOptions,
) -> (AgentError, mpsc::UnboundedReceiver<AgentEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    match spawn_agent(harness, cwd, options, tx).await {
        Ok(_) => panic!("startup unexpectedly succeeded"),
        Err(e) => (e, rx),
    }
}

impl Session {
    /// Next non-stderr event.
    pub async fn next(&mut self) -> AgentEvent {
        next_event(&mut self.events, self.timeout).await
    }

    /// Collects events up to and including `TurnEnded`.
    pub async fn turn(&mut self) -> (Vec<AgentEvent>, Result<StopReason, AgentError>) {
        let mut seen = Vec::new();
        loop {
            match self.next().await {
                AgentEvent::TurnEnded(r) => return (seen, r),
                ev => seen.push(ev),
            }
        }
    }

    /// Waits for the first message chunk containing `needle`.
    pub async fn until_message(&mut self, needle: &str) -> String {
        loop {
            if let AgentEvent::Output(out @ AgentOutput::MessageChunk(_)) = self.next().await {
                let text = out.chunk_text().unwrap_or_default().to_string();
                if text.contains(needle) {
                    return text;
                }
            }
        }
    }

    /// Waits for `Exited` and returns `(code, signal)`.
    pub async fn exited(&mut self) -> (Option<i32>, Option<i32>) {
        wait_exited(&mut self.events, self.timeout).await
    }
}

pub async fn next_event(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
    timeout: Duration,
) -> AgentEvent {
    loop {
        let ev = tokio::time::timeout(timeout, rx.recv())
            .await
            .expect("timed out waiting for an agent event")
            .expect("event channel closed");
        if !matches!(ev, AgentEvent::Stderr(_)) {
            return ev;
        }
    }
}

pub async fn wait_exited(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
    timeout: Duration,
) -> (Option<i32>, Option<i32>) {
    loop {
        if let AgentEvent::Exited { code, signal } = next_event(rx, timeout).await {
            return (code, signal);
        }
    }
}

/// Concatenated text of all message chunks in `events`.
pub fn message_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Output(o @ AgentOutput::MessageChunk(_)) => o.chunk_text(),
            _ => None,
        })
        .collect()
}

/// Whether process `pid` is gone (or a zombie awaiting its reaper).
pub fn process_gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
    }
}

/// Polls until `pid` is gone; panics after `timeout`.
pub async fn assert_process_gone(pid: u32, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while !process_gone(pid) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "process {pid} is still alive"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Live (non-zombie) processes whose process group is `pgid`.
pub fn group_members(pgid: u32) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            let Some((_, rest)) = stat.rsplit_once(')') else {
                return false;
            };
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // fields[0] = state, fields[2] = pgrp
            fields.first() != Some(&"Z") && fields.get(2) == Some(&pgid.to_string().as_str())
        })
        .collect()
}

/// Polls until no live process remains in group `pgid`; panics after `timeout`.
pub async fn assert_group_gone(pgid: u32, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let members = group_members(pgid);
        if members.is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "process group {pgid} still has {members:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
