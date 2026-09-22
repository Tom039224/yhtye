//! Helpers for orchestration tests (fake and real agents).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainConfig, DomainEvent, TaskStatus};
use yhtye_core::git::NoopGit;
use yhtye_core::runtime::{Orchestration, OrchestrationConfig};

use super::assert_group_gone;

/// Config with [`NoopGit`] on `main`.
pub fn config(
    dir: &Path,
    orchestrator: HarnessConfig,
    implementer: HarnessConfig,
    reviewer: HarnessConfig,
) -> OrchestrationConfig {
    OrchestrationConfig {
        project: "P-1".into(),
        project_dir: dir.to_path_buf(),
        orchestrator,
        implementer,
        reviewer,
        mcp_bind: "127.0.0.1:0".parse().expect("addr"),
        domain: DomainConfig::default(),
        git: Arc::new(NoopGit::new(dir, Some("main".into()))),
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

pub fn message<'a>(ev: &'a ApiEvent, session: &str) -> Option<&'a str> {
    match &ev.body {
        ApiEventBody::Agent {
            session: s,
            event: AgentEvent::Output(o @ AgentOutput::MessageChunk(_)),
        } if s == session => o.chunk_text(),
        _ => None,
    }
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

/// Sequence numbers are gapless from 1.
pub fn assert_gapless(events: &[ApiEvent]) {
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "seq gap at {i}: {:?}", e.body);
    }
}

pub async fn shutdown_and_check(orch: Orchestration, events: &[ApiEvent]) {
    orch.shutdown().await;
    for pid in started_pids(events) {
        assert_group_gone(pid, Duration::from_secs(5)).await;
    }
}
