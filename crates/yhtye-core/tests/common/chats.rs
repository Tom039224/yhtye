//! Helpers for the fake-agent chat tests (`orchestration_fake_chats`,
//! `orchestration_fake_worktree`): a repository-backed orchestration, sending
//! to chats and waiting for events.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::DomainEvent;
use yhtye_core::git::worktree_root;
use yhtye_core::runtime::{Orchestration, OrchestrationConfig};

use super::fake_harness;
use super::orch::{git_config, is_message, until};
use super::repo::{PROJECT, TempRepo};

pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Long enough for a wrongly started process or an unwanted retry to show up.
pub const QUIET: Duration = Duration::from_millis(400);

pub type Rx = UnboundedReceiver<ApiEvent>;

pub fn none() -> Value {
    json!({"turns": []})
}

/// A repository-backed config; every orchestrator runs `orch_script`, every
/// implementer `impl_script`.
pub fn repo_config(r: &TempRepo, orch_script: Value, impl_script: Value) -> OrchestrationConfig {
    git_config(
        r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(none()),
    )
}

pub async fn start(cfg: OrchestrationConfig) -> (Orchestration, Rx, Vec<ApiEvent>) {
    let (orch, rx) = Orchestration::start(cfg).await.expect("starts");
    (orch, rx, Vec::new())
}

pub async fn new_chat(orch: &Orchestration, branch: &str) -> String {
    orch.create_chat(branch).await.expect("chat created").id
}

pub async fn say(orch: &Orchestration, chat: &str, text: &str) {
    orch.send_user_message(chat, text).await.expect("send");
}

/// Waits for an event matching `done`, looking at what was already received first.
pub async fn wait(rx: &mut Rx, seen: &mut Vec<ApiEvent>, done: impl Fn(&ApiEvent) -> bool + Copy) {
    if !seen.iter().any(done) {
        until(rx, seen, TIMEOUT, done).await;
    }
}

/// Lets `QUIET` pass and takes in whatever arrived.
pub async fn settle(rx: &mut Rx, seen: &mut Vec<ApiEvent>) {
    tokio::time::sleep(QUIET).await;
    while let Ok(e) = rx.try_recv() {
        seen.push(e);
    }
}

pub fn started(events: &[ApiEvent], session: &str) -> Vec<(String, bool, Option<String>)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session: s,
                acp_session_id,
                resumed,
                cwd,
                ..
            } if s == session => Some((acp_session_id.clone(), *resumed, cwd.clone())),
            _ => None,
        })
        .collect()
}

pub fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).expect("canonical path")
}

pub fn cwd_of(events: &[ApiEvent], session: &str) -> PathBuf {
    let cwd = started(events, session)
        .pop()
        .and_then(|s| s.2)
        .unwrap_or_else(|| panic!("{session} has not started"));
    canonical(Path::new(&cwd))
}

pub fn branch_worktree(r: &TempRepo, name: &str) -> PathBuf {
    worktree_root(&r.data, PROJECT).join("branches").join(name)
}

pub fn said(session: &'static str, needle: &'static str) -> impl Fn(&ApiEvent) -> bool + Copy {
    move |e| is_message(e, session, needle)
}

pub fn is_domain(e: &ApiEvent, f: impl Fn(&DomainEvent) -> bool) -> bool {
    matches!(&e.body, ApiEventBody::Domain { event } if f(event))
}

/// Whether `e` says the session `key` stopped (not by a shutdown).
pub fn is_stopped(e: &ApiEvent, key: &str) -> bool {
    matches!(&e.body, ApiEventBody::SessionStopped { session, suspended: false } if session == key)
}
