//! Helpers for orchestration tests (fake and real agents).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig};
use yhtye_core::agents::AgentCatalog;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainConfig, DomainEvent, HelpKind, TaskStatus};
use yhtye_core::git::NoopGit;
use yhtye_core::runtime::{Orchestration, OrchestrationConfig};

use super::assert_group_gone;
use super::repo::{PROJECT, TempRepo};

/// The chat the tests talk to (created on `main` by [`send`]).
pub const CHAT: &str = "C-1";

/// Session key of the orchestrator of [`CHAT`].
pub const ORCHESTRATOR_SESSION: &str = "orchestrator:C-1";

/// Sends `text` to the orchestrator of chat [`CHAT`], creating that chat on
/// `main` first if the project has none.
pub async fn send(orch: &Orchestration, text: impl Into<String>) {
    let snapshot = orch.snapshot().await.expect("snapshot");
    if snapshot.state.chat(CHAT).is_none() {
        orch.create_chat("main").await.expect("create the chat");
    }
    orch.send_user_message(CHAT, text).await.expect("send");
}

/// Where tests without git keep the database of a project directory
/// (`dir/.yhtye/`; `dir` is not a repository). Tests on a git repository use
/// [`git_config`], which keeps it outside the repository.
pub fn test_db(dir: &Path) -> std::path::PathBuf {
    yhtye_core::store::db_path(&dir.join(".yhtye"))
}

/// Config with [`NoopGit`] on `main` and the database in [`test_db`].
pub fn config(
    dir: &Path,
    orchestrator: HarnessConfig,
    implementer: HarnessConfig,
    reviewer: HarnessConfig,
) -> OrchestrationConfig {
    OrchestrationConfig {
        project: "P-1".into(),
        project_dir: dir.to_path_buf(),
        agents: Arc::new(AgentCatalog::fixed(orchestrator, implementer, reviewer)),
        mcp_bind: "127.0.0.1:0".parse().expect("addr"),
        domain: DomainConfig::default(),
        git: Arc::new(NoopGit::new(dir, Some("main".into()))),
        db_path: test_db(dir),
    }
}

/// Config on a temporary git repository with the real [`GitCli`]; the database
/// and worktrees are in the repository's separate data directory.
pub fn git_config(
    repo: &TempRepo,
    orchestrator: HarnessConfig,
    implementer: HarnessConfig,
    reviewer: HarnessConfig,
) -> OrchestrationConfig {
    OrchestrationConfig {
        project: PROJECT.into(),
        project_dir: repo.repo.clone(),
        agents: Arc::new(AgentCatalog::fixed(orchestrator, implementer, reviewer)),
        mcp_bind: "127.0.0.1:0".parse().expect("addr"),
        domain: DomainConfig::default(),
        git: Arc::new(repo.git_cli()),
        db_path: repo.db(),
    }
}

/// Collects events until one matches `done` (inclusive).
pub async fn until(
    rx: &mut mpsc::UnboundedReceiver<ApiEvent>,
    seen: &mut Vec<ApiEvent>,
    timeout: Duration,
    done: impl Fn(&ApiEvent) -> bool,
) {
    loop {
        let ev = tokio::time::timeout(timeout, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out; events so far: {:#?}", summary(seen)))
            .expect("event channel open");
        let stop = done(&ev);
        seen.push(ev);
        if stop {
            return;
        }
    }
}

/// Like [`until`] for real-agent runs, which never expect a failure: a session
/// that fails to start (`SessionFailed`) or an agent that crashed
/// (`AgentCrashed` help) fails the test at once with the error instead of after
/// the whole timeout.
pub async fn until_healthy(
    rx: &mut mpsc::UnboundedReceiver<ApiEvent>,
    seen: &mut Vec<ApiEvent>,
    timeout: Duration,
    done: impl Fn(&ApiEvent) -> bool,
) {
    until(rx, seen, timeout, |ev| {
        match &ev.body {
            ApiEventBody::SessionFailed { session, error } => {
                panic!("{session} failed to start: {error}")
            }
            ApiEventBody::Domain {
                event: DomainEvent::HelpRaised { help },
            } if help.kind == HelpKind::AgentCrashed => {
                panic!("agent crashed (task {}): {}", help.task, help.message)
            }
            _ => {}
        }
        done(ev)
    })
    .await;
}

pub fn message<'a>(ev: &'a ApiEvent, session: &str) -> Option<&'a str> {
    match &ev.body {
        ApiEventBody::Agent {
            session: s,
            event: AgentEvent::Output(o @ AgentOutput::MessageChunk(_)),
        } if s == session => o.chunk_text(),
        _ => None,
    }
}

/// Every message chunk of `session`, in order.
pub fn messages(events: &[ApiEvent], session: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| message(e, session))
        .map(str::to_string)
        .collect()
}

pub fn is_message(ev: &ApiEvent, session: &str, needle: &str) -> bool {
    message(ev, session).is_some_and(|t| t.contains(needle))
}

/// Whether `ev` changes task `task` to `status`.
pub fn is_task_status(ev: &ApiEvent, task: &str, status: TaskStatus) -> bool {
    match &ev.body {
        ApiEventBody::Domain {
            event: DomainEvent::TaskStatusChanged { task: t, status: s },
        } => t == task && *s == status,
        ApiEventBody::Domain {
            event: DomainEvent::TaskCancelled { task: t, .. },
        } => t == task && status == TaskStatus::Cancelled,
        _ => false,
    }
}

pub fn summary(events: &[ApiEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Agent {
                session,
                event: AgentEvent::Output(o @ AgentOutput::MessageChunk(_)),
            } => Some(format!("{session}: {}", o.chunk_text().unwrap_or_default())),
            ApiEventBody::Agent { .. } => None,
            other => Some(format!("{other:?}")),
        })
        .collect()
}

pub fn tool_calls(events: &[ApiEvent]) -> Vec<(String, String, bool)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::ToolCalled { record: r } => {
                Some((r.binding.session.clone(), r.tool.clone(), r.result.is_ok()))
            }
            _ => None,
        })
        .collect()
}

pub fn prompts_to<'a>(events: &'a [ApiEvent], session: &str) -> Vec<&'a str> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Prompted { session: s, text } if s == session => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

pub fn domain_events(events: &[ApiEvent]) -> Vec<&DomainEvent> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Domain { event } => Some(event),
            _ => None,
        })
        .collect()
}

pub fn started_sessions(events: &[ApiEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted { session, .. } => Some(session.as_str()),
            _ => None,
        })
        .collect()
}

pub fn started_pids(events: &[ApiEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted { pid, .. } => *pid,
            _ => None,
        })
        .collect()
}

/// Durable events are numbered without gaps from `first`; live events carry the
/// `seq` of the last durable event.
pub fn assert_gapless_from(events: &[ApiEvent], first: u64) {
    let mut last = first - 1;
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.live, e.body.is_live(), "live flag at {i}: {:?}", e.body);
        if e.live {
            assert_eq!(e.seq, last, "live event seq at {i}: {:?}", e.body);
        } else {
            last += 1;
            assert_eq!(e.seq, last, "seq gap at {i}: {:?}", e.body);
        }
    }
}

/// Durable sequence numbers are gapless from 1.
pub fn assert_gapless(events: &[ApiEvent]) {
    assert_gapless_from(events, 1);
}

/// The live state equals the state read back from the current-state tables and
/// the state rebuilt by replaying the stored domain events, and the log holds
/// every durable event published so far. Agents may still be winding down, so a
/// mismatch is retried until the state stops changing.
pub async fn assert_persisted(orch: &Orchestration) {
    let mut last = None;
    for _ in 0..40 {
        let check = persisted_check(orch).await;
        if check.is_ok() {
            return;
        }
        last = Some(check);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if let Some(Err(e)) = last {
        panic!("{e}");
    }
}

async fn persisted_check(orch: &Orchestration) -> Result<(), String> {
    let snap = orch.snapshot().await.expect("snapshot");
    let store = orch.store();
    let project = snap.state.project.clone();
    let loaded = store
        .load_state(&project)
        .await
        .expect("load")
        .expect("project stored");
    let replayed = store
        .replay_state(&project, snap.state.config)
        .await
        .expect("replay");
    let stored_seq = store.last_seq(&project).await.expect("seq");
    if loaded != snap.state {
        return Err(format!(
            "current-state tables differ from live state:\n{loaded:#?}\nvs\n{:#?}",
            snap.state
        ));
    }
    if replayed != snap.state {
        return Err(format!(
            "event replay differs from live state:\n{replayed:#?}\nvs\n{:#?}",
            snap.state
        ));
    }
    if stored_seq != snap.seq {
        return Err(format!(
            "stored seq {stored_seq} != published seq {}",
            snap.seq
        ));
    }
    Ok(())
}

pub async fn shutdown_and_check(orch: Orchestration, events: &[ApiEvent]) {
    assert_persisted(&orch).await;
    orch.shutdown().await;
    for pid in started_pids(events) {
        assert_group_gone(pid, Duration::from_secs(5)).await;
    }
}
