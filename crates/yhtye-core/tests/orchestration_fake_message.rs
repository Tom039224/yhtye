//! `message_task` with fake agents: the orchestrator tells the agent of a
//! running step something, by steering where the harness supports it and by
//! interrupting the turn where it does not (`core-design.md` §8).

mod common;

use std::path::Path;
use std::time::Duration;

use common::fake_harness;
use common::orch::{
    ORCHESTRATOR_SESSION, config, domain_events, is_message, prompts_to, shutdown_and_check,
    started_sessions, summary, until,
};
use serde_json::{Value, json};
use yhtye_core::acp::schema::StopReason;
use yhtye_core::acp::{AgentError, AgentEvent};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, MessageOutcome, MessageVia, TaskStatus};
use yhtye_core::runtime::Orchestration;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Long enough to be cut short by the message, or never reached.
const LONG_WAIT_MS: u64 = 60_000;

/// An action that waits until `flag` exists (the agent of the step wrote it).
fn wait_for(flag: &Path) -> Value {
    let script = format!(
        "for i in $(seq 1 300); do [ -e '{}' ] && exit 0; sleep 0.1; done; exit 1",
        flag.display()
    );
    json!({"run": ["sh", "-c", script]})
}

fn write_flag(flag: &Path) -> Value {
    json!({"write_file": {"path": flag.display().to_string(), "text": "1"}})
}

/// The orchestrator: plans one task with `steps`, waits until the step's agent
/// has started (`flag`), sends `text` to it and reports what came back.
fn orchestrator(steps: Value, flag: &Path, text: &str) -> Value {
    json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "Add a line"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "append", "kind": "code",
                "steps": steps, "instruction": "append hello to README.md"}}},
            wait_for(flag),
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "${task_id}", "text": text}}},
            {"message": "messaged"}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
        ]}
    ]})
}

fn finished(e: &ApiEvent) -> bool {
    is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:")
}

/// The reply of the orchestrator's `message_task` call.
fn message_task_reply(events: &[ApiEvent]) -> Value {
    let reports = summary(events);
    let prefix = format!("{ORCHESTRATOR_SESSION}: mcp:message_task:ok:");
    let line = reports
        .iter()
        .find_map(|m| m.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no message_task reply in {reports:#?}"));
    serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))
}

/// `(outcome, via)` of every settled message.
fn resolutions(events: &[ApiEvent]) -> Vec<(MessageOutcome, Option<MessageVia>)> {
    domain_events(events)
        .into_iter()
        .filter_map(|e| match e {
            DomainEvent::TaskMessageResolved { outcome, via, .. } => Some((*outcome, *via)),
            _ => None,
        })
        .collect()
}

fn count(events: &[ApiEvent], f: impl Fn(&DomainEvent) -> bool) -> usize {
    domain_events(events).into_iter().filter(|e| f(e)).count()
}

fn nothing_went_wrong(events: &[ApiEvent]) {
    assert_eq!(
        count(events, |e| matches!(e, DomainEvent::NudgeSent { .. })),
        0,
        "no reminder: {:#?}",
        summary(events)
    );
    assert_eq!(
        count(events, |e| matches!(e, DomainEvent::HelpRaised { .. })),
        0,
        "no help: {:#?}",
        summary(events)
    );
}

fn task_done(events: &[ApiEvent]) -> bool {
    domain_events(events).into_iter().any(|e| {
        matches!(e, DomainEvent::TaskStatusChanged { task, status: TaskStatus::Done } if task == "T-1")
    })
}

#[tokio::test]
async fn a_steering_harness_takes_the_message_into_the_running_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("agent-running");
    let impl_script = json!({"steering": true, "steer_continues": true, "turns": [
        {"match": "[yhtye:step]", "actions": [
            write_flag(&flag),
            {"sleep": LONG_WAIT_MS},
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "used tabs"}}}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}]),
            &flag,
            "use tabs",
        )),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "add a line to README").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["task_id"], "T-1");
    assert_eq!(reply["step"], 0);
    assert_eq!(reply["outcome"], "delivered", "{reply}");
    assert_eq!(reply["via"], "steering", "{reply}");
    let body = "[yhtye:orchestrator_message]\nuse tabs\n\n(This message is from the orchestrator";
    // The agent's turn got the message, and its conversation shows it.
    let mut steered = String::from("T-1/implementer");
    steered.push_str(": steered:");
    steered.push_str(body);
    assert!(
        summary(&events).iter().any(|m| m.starts_with(&steered)),
        "{:#?}",
        summary(&events)
    );
    let prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(
        prompts.len(),
        2,
        "step prompt and the message: {prompts:#?}"
    );
    assert!(prompts[1].starts_with(body), "{}", prompts[1]);
    // One turn, no reminder: the agent went on and reported.
    nothing_went_wrong(&events);
    assert_eq!(
        resolutions(&events),
        vec![(MessageOutcome::Delivered, Some(MessageVia::Steering))]
    );
    assert_eq!(
        count(
            &events,
            |e| matches!(e, DomainEvent::TaskMessageSent { text, .. } if text == "use tabs")
        ),
        1
    );
    assert!(task_done(&events));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_harness_without_steering_is_interrupted_and_gets_the_message_as_a_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("agent-running");
    let impl_script = json!({"turns": [
        {"match": "[yhtye:orchestrator_message]", "actions": [
            {"message": "took the message"},
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "used tabs"}}}
        ]},
        {"match": "[yhtye:step]", "actions": [
            write_flag(&flag),
            "wait_cancel",
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "never reached"}}}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}]),
            &flag,
            "use tabs",
        )),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "add a line to README").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["outcome"], "delivered", "{reply}");
    assert_eq!(reply["via"], "interrupt", "{reply}");
    let prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(prompts.len(), 2, "{prompts:#?}");
    assert!(
        prompts[1].starts_with(
            "[yhtye:orchestrator_message]\n(Your previous turn was interrupted by Yhtye to \
             deliver this message; your work so far is kept.)\nuse tabs\n"
        ),
        "{}",
        prompts[1]
    );
    // The cancelled turn did not count as a silent one.
    let cancelled = events.iter().any(|e| {
        matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(Ok(StopReason::Cancelled)) }
            if session == "T-1/implementer")
    });
    assert!(cancelled, "the turn was cancelled");
    let errored = events.iter().any(|e| {
        matches!(
            &e.body,
            ApiEventBody::Agent {
                event: AgentEvent::TurnEnded(Err(AgentError::Closed)),
                ..
            }
        )
    });
    assert!(!errored);
    nothing_went_wrong(&events);
    assert!(
        summary(&events)
            .iter()
            .any(|m| m.contains("took the message"))
    );
    assert_eq!(
        resolutions(&events),
        vec![(MessageOutcome::Delivered, Some(MessageVia::Interrupt))]
    );
    assert!(task_done(&events));
    assert_eq!(
        started_sessions(&events)
            .iter()
            .filter(|s| s.ends_with("implementer"))
            .count(),
        1,
        "the same session goes on"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_message_to_a_step_that_already_reported_is_handed_back_and_the_task_goes_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let reported = dir.path().join("agent-reported");
    // The agent reports, then takes its time to end the turn.
    let impl_script = json!({"turns": [
        {"match": "[yhtye:step]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "added hello"}}},
            write_flag(&reported),
            {"sleep": 3000}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}]),
            &reported,
            "one more thing",
        )),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "add a line to README").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["outcome"], "returned", "{reply}");
    assert_eq!(reply["via"], Value::Null);
    let detail = reply["detail"].as_str().expect("detail");
    assert!(detail.contains("already reported step 1 done"), "{detail}");
    assert!(detail.contains("modify_steps"), "{detail}");
    assert_eq!(
        prompts_to(&events, "T-1/implementer").len(),
        1,
        "nothing was sent"
    );
    nothing_went_wrong(&events);
    assert_eq!(resolutions(&events), vec![(MessageOutcome::Returned, None)]);
    // The task went on as if nothing had been sent.
    assert!(task_done(&events));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn the_agent_of_a_review_step_can_be_messaged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("reviewer-running");
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "implemented"}}}]}]});
    let review_script = json!({"steering": true, "steer_continues": true, "turns": [
        {"match": "[yhtye:step]", "actions": [
            write_flag(&flag),
            {"sleep": LONG_WAIT_MS},
            {"mcp_call": {"tool": "report_step_done", "args": {
                "result": "lgtm, docs checked", "verdict": "approve"}}}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}, {"kind": "review"}]),
            &flag,
            "also check the docs",
        )),
        fake_harness(impl_script),
        fake_harness(review_script),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "add a line to README").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["step"], 1, "{reply}");
    assert_eq!(reply["outcome"], "delivered", "{reply}");
    assert_eq!(reply["via"], "steering", "{reply}");
    let prompts = prompts_to(&events, "T-1/review-1");
    assert_eq!(prompts.len(), 2, "{prompts:#?}");
    assert!(prompts[1].starts_with("[yhtye:orchestrator_message]\nalso check the docs\n"));
    assert_eq!(prompts_to(&events, "T-1/implementer").len(), 1);
    nothing_went_wrong(&events);
    assert!(task_done(&events));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn refusals_reach_the_orchestrator_as_tool_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("agent-running");
    let orch_script = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "g"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "t", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "do it"}}},
            wait_for(&flag),
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "${task_id}", "text": " "}}},
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "T-99", "text": "hi"}}},
            {"mcp_call": {"tool": "cancel_task", "args": {"task_id": "${task_id}", "reason": "r"}}},
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "T-1", "text": "hi"}}}
        ]}
    ]});
    let impl_script = json!({"turns": [
        {"match": "[yhtye:step]", "actions": [
            write_flag(&flag),
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "T-1", "text": "me?"}}},
            {"sleep": LONG_WAIT_MS}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "go").await;
    let last = |e: &ApiEvent| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:message_task:error:")
            && is_message(e, ORCHESTRATOR_SESSION, "\"invalid_state\"")
    };
    until(&mut rx, &mut events, TIMEOUT, last).await;

    let reports: Vec<String> = summary(&events)
        .into_iter()
        .filter_map(|m| {
            m.strip_prefix(&format!("{ORCHESTRATOR_SESSION}: "))
                .map(str::to_string)
        })
        .filter(|m| m.starts_with("mcp:message_task:"))
        .collect();
    assert_eq!(reports.len(), 3, "{reports:#?}");
    assert!(
        reports[0].contains("\"invalid_argument\""),
        "{}",
        reports[0]
    );
    assert!(reports[1].contains("\"not_found\""), "{}", reports[1]);
    assert!(
        reports[2].contains("\"invalid_state\"") && reports[2].contains("is cancelled"),
        "{}",
        reports[2]
    );
    // A task agent cannot message tasks.
    let own = summary(&events).into_iter().any(|m| {
        m.starts_with("T-1/implementer: mcp:message_task:error:") && m.contains("\"forbidden\"")
    });
    assert!(own, "{:#?}", summary(&events));
    // Only the accepted ones are in the log: none was.
    assert_eq!(
        count(&events, |e| matches!(
            e,
            DomainEvent::TaskMessageSent { .. }
        )),
        0
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_message_while_the_agent_is_starting_is_handed_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "g"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "t", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "do it"}}},
            {"mcp_call": {"tool": "message_task", "args": {"task_id": "${task_id}", "text": "hi"}}}
        ]}
    ]});
    // The agent never gets past `session/new`: it is starting for as long as the test lasts.
    let mut stuck = fake_harness(json!({"hang_at": "session/new"}));
    stuck.startup_timeout = Duration::from_secs(2);
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        stuck,
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "go").await;
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:message_task:")
    })
    .await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["outcome"], "returned", "{reply}");
    assert!(
        reply["detail"]
            .as_str()
            .expect("detail")
            .contains("still starting"),
        "{reply}"
    );
    assert_eq!(resolutions(&events), vec![(MessageOutcome::Returned, None)]);
    assert!(prompts_to(&events, "T-1/implementer").is_empty());
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn cancelling_the_task_hands_back_a_message_waiting_for_a_stuck_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("agent-running");
    // No steering, and the agent does not react to the cancel for a while.
    let impl_script = json!({"turns": [
        {"match": "[yhtye:step]", "actions": [write_flag(&flag), {"busy": 2500}]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}]),
            &flag,
            "use tabs",
        )),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "go").await;
    // The message is waiting for the turn to end ...
    for _ in 0..300 {
        if flag.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    // ... and the loop is free meanwhile: the user cancels the task.
    orch.cancel_task("T-1", "no longer needed")
        .await
        .expect("cancelled");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:message_task:")
    })
    .await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["outcome"], "returned", "{reply}");
    let detail = reply["detail"].as_str().expect("detail");
    assert!(detail.contains("is now cancelled"), "{detail}");
    assert_eq!(resolutions(&events), vec![(MessageOutcome::Returned, None)]);
    // The stuck turn ends cancelled; nothing is sent to the stopped agent.
    assert_eq!(prompts_to(&events, "T-1/implementer").len(), 1);
    shutdown_and_check(orch, &events).await;
}

/// Takes the whole interrupt timeout (60 s): `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "waits for the 60 s interrupt timeout"]
async fn a_stuck_agent_hands_the_message_back_and_the_step_goes_on_after_a_reminder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let flag = dir.path().join("agent-running");
    // The cancel is ignored past the timeout; the turn then ends cancelled
    // with the step unreported, which must not leave the step hanging.
    let impl_script = json!({"turns": [
        {"match": "[yhtye:reminder]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "done after all"}}}
        ]},
        {"match": "[yhtye:step]", "actions": [write_flag(&flag), {"busy": 63_000}]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator(
            json!([{"kind": "implement"}]),
            &flag,
            "use tabs",
        )),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "go").await;
    until(&mut rx, &mut events, Duration::from_secs(120), finished).await;

    let reply = message_task_reply(&events);
    assert_eq!(reply["outcome"], "returned", "{reply}");
    let detail = reply["detail"].as_str().expect("detail");
    assert!(detail.contains("did not end within 60 s"), "{detail}");
    assert_eq!(resolutions(&events), vec![(MessageOutcome::Returned, None)]);
    // The message was not sent; the step went on after a reminder.
    assert!(
        !prompts_to(&events, "T-1/implementer")
            .iter()
            .any(|p| p.contains("orchestrator_message"))
    );
    assert_eq!(
        count(&events, |e| matches!(e, DomainEvent::NudgeSent { .. })),
        1
    );
    assert_eq!(
        count(&events, |e| matches!(e, DomainEvent::HelpRaised { .. })),
        0
    );
    assert!(task_done(&events));
    shutdown_and_check(orch, &events).await;
}
