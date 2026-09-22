//! Helpers for orchestration tests (fake and real agents).

use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::runtime::{OrchEvent, Orchestration};

use super::assert_group_gone;

/// Collects events until one matches `done` (inclusive).
pub async fn until(
    rx: &mut mpsc::UnboundedReceiver<OrchEvent>,
    seen: &mut Vec<OrchEvent>,
    timeout: Duration,
    done: impl Fn(&OrchEvent) -> bool,
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

pub fn message<'a>(ev: &'a OrchEvent, session: &str) -> Option<&'a str> {
    match ev {
        OrchEvent::Agent {
            session: s,
            event: AgentEvent::Output(o @ AgentOutput::MessageChunk(_)),
        } if s == session => o.chunk_text(),
        _ => None,
    }
}

pub fn is_message(ev: &OrchEvent, session: &str, needle: &str) -> bool {
    message(ev, session).is_some_and(|t| t.contains(needle))
}

pub fn summary(events: &[OrchEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchEvent::Agent {
                session,
                event: AgentEvent::Output(o @ AgentOutput::MessageChunk(_)),
            } => Some(format!("{session}: {}", o.chunk_text().unwrap_or_default())),
            OrchEvent::Agent { .. } => None,
            other => Some(format!("{other:?}")),
        })
        .collect()
}

pub fn tool_calls(events: &[OrchEvent]) -> Vec<(String, String, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchEvent::ToolCalled(r) => {
                Some((r.binding.session.clone(), r.tool.clone(), r.result.is_ok()))
            }
            _ => None,
        })
        .collect()
}

pub fn prompts_to<'a>(events: &'a [OrchEvent], session: &str) -> Vec<&'a str> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchEvent::Prompted { session: s, text } if s == session => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

pub fn started_pids(events: &[OrchEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchEvent::SessionStarted { pid, .. } => *pid,
            _ => None,
        })
        .collect()
}

pub async fn shutdown_and_check(orch: Orchestration, events: &[OrchEvent]) {
    orch.shutdown().await;
    for pid in started_pids(events) {
        assert_group_gone(pid, Duration::from_secs(5)).await;
    }
}
