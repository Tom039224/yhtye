//! The user's "compact" button with fake agents: `/compact` reaches the chat's
//! orchestrator as the whole prompt (not as an inbox batch), only while it is
//! running and idle, and the inbox is delivered once that turn ends.

mod common;

use common::chats::{
    Rx, TIMEOUT, is_domain, new_chat, repo_config, said, say, settle, start, wait,
};
use common::orch::{prompts_to, shutdown_and_check, until};
use common::repo::TempRepo;
use serde_json::json;
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{COMPACT_PROMPT, DomainEvent, ErrorCode};
use yhtye_core::runtime::{Orchestration, UserActionError};

const SESSION: &str = "orchestrator:C-1";

fn turn_ended(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(_) } if session == SESSION)
}

async fn compact_error(orch: &Orchestration, chat: &str) -> ErrorCode {
    match orch.compact_chat(chat).await {
        Ok(()) => panic!("compacting {chat} was accepted"),
        Err(UserActionError::Rejected(e)) => e.code,
        Err(e) => panic!("unexpected {e:?}"),
    }
}

async fn ended_turns(rx: &mut Rx, events: &mut Vec<ApiEvent>, n: usize) {
    while events.iter().filter(|e| turn_ended(e)).count() < n {
        until(rx, events, TIMEOUT, turn_ended).await;
    }
}

#[tokio::test]
async fn compact_sends_the_bare_command_and_the_inbox_follows_after_its_turn() {
    let r = TempRepo::new();
    let script = json!({"turns": [
        {"match": "hello", "actions": [{"message": "hi"}]},
        {"match": COMPACT_PROMPT, "actions": [
            {"sleep": 500},
            {"update": {"sessionUpdate": "usage_update", "used": 21000, "size": 200000}},
            "report_state"
        ]},
        {"match": "later", "actions": [{"message": "got-later"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, json!({"turns": []}))).await;
    let chat = new_chat(&orch, "main").await;

    // Nothing to compact before the orchestrator runs; an unknown chat is not found.
    assert_eq!(compact_error(&orch, &chat).await, ErrorCode::InvalidState);
    assert_eq!(compact_error(&orch, "C-9").await, ErrorCode::NotFound);

    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said(SESSION, "hi")).await;
    ended_turns(&mut rx, &mut events, 1).await;

    orch.compact_chat(&chat).await.expect("compact accepted");
    // A second press while the compaction runs is refused; a message waits.
    assert_eq!(compact_error(&orch, &chat).await, ErrorCode::InvalidState);
    say(&orch, &chat, "later").await;
    wait(&mut rx, &mut events, said(SESSION, "got-later")).await;
    settle(&mut rx, &mut events).await;

    // The agent got exactly `/compact`, then the message in a batch of its own.
    let prompts = prompts_to(&events, SESSION);
    assert_eq!(prompts.len(), 3, "{prompts:?}");
    assert_eq!(prompts[1], COMPACT_PROMPT);
    assert!(
        prompts[2].starts_with("[yhtye:user_message]"),
        "{prompts:?}"
    );
    assert!(prompts[2].contains("later"), "{prompts:?}");
    let state = events
        .iter()
        .find_map(|e| match &e.body {
            ApiEventBody::AgentText { session, text, .. }
                if session == SESSION && text.starts_with("state:") =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .expect("the compact turn reported its prompt");
    assert!(
        state.ends_with(&format!(";prompt={COMPACT_PROMPT}")),
        "{state}"
    );

    // The message waited for the end of the compact turn: it was queued while
    // the turn ran and sent after the turn's last output.
    let at = |f: &dyn Fn(&ApiEvent) -> bool| events.iter().position(f).expect("event");
    let queued = at(&|e| {
        is_domain(
            e,
            |d| matches!(d, DomainEvent::InboxQueued { entry } if entry.item.body == "later"),
        )
    });
    let last_output =
        at(&|e| common::orch::message(e, SESSION).is_some_and(|m| m.starts_with("state:")));
    let delivered =
        at(&|e| matches!(&e.body, ApiEventBody::Prompted { text, .. } if text.contains("later")));
    assert!(queued < last_output && last_output < delivered);

    // Usage updates are streamed live, never stored.
    let usage = events
        .iter()
        .find(|e| {
            matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::Output(AgentOutput::Usage(u)) }
                if session == SESSION && u.used == 21000 && u.size == 200000)
        })
        .expect("a usage update");
    assert!(usage.live);

    // `/compact` never titles the chat.
    let titles: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Domain {
                event: DomainEvent::ChatTitled { title, .. },
            } => Some(title.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["hello"]);
    assert!(!events.iter().any(|e| is_domain(e, |d| matches!(d, DomainEvent::InboxQueued { entry } if entry.item.body.contains(COMPACT_PROMPT)))));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn compact_is_refused_while_the_orchestrator_is_in_a_turn() {
    let r = TempRepo::new();
    let script = json!({"turns": [{"match": "work", "actions": ["wait_cancel"]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, json!({"turns": []}))).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "work").await;
    wait(
        &mut rx,
        &mut events,
        |e| matches!(&e.body, ApiEventBody::Prompted { session, .. } if session == SESSION),
    )
    .await;

    assert_eq!(compact_error(&orch, &chat).await, ErrorCode::InvalidState);

    orch.cancel_orchestrator_turn(&chat).await.expect("cancel");
    wait(&mut rx, &mut events, turn_ended).await;
    settle(&mut rx, &mut events).await;
    assert_eq!(prompts_to(&events, SESSION).len(), 1, "nothing was sent");
    shutdown_and_check(orch, &events).await;
}
