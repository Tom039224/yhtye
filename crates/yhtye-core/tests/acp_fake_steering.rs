//! Fake-agent tests: `_session/steering` (a message injected into the running turn).

mod common;

use common::{fake_harness, message_text, start};
use serde_json::json;
use yhtye_core::acp::schema::StopReason;
use yhtye_core::acp::{AgentError, AgentEvent, SpawnOptions, SteerOutcome};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// A steerable agent whose `work` turn runs until interrupted.
fn steerable() -> serde_json::Value {
    json!({
        "steering": true,
        "turns": [
            {"match": "work", "actions": [{"message": "working"}, "wait_cancel"]},
            {"match": "next", "actions": [{"message": "next-done"}]}
        ]
    })
}

#[tokio::test]
async fn the_steering_capability_is_read_from_initialize() {
    let dir = tmp();
    let mut s = start(
        &fake_harness(steerable()),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert!(info.supports_steering);
    assert!(s.handle.supports_steering());
    s.handle.shutdown().await;

    let mut s = start(
        &fake_harness(json!({})),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert!(!info.supports_steering);
    assert!(!s.handle.supports_steering());
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_steer_during_a_turn_is_injected_and_the_turn_ends_once() {
    let dir = tmp();
    let mut s = start(
        &fake_harness(steerable()),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    s.next().await; // Ready

    s.handle.prompt_text("work").expect("prompt");
    s.until_message("working").await;
    let outcome = s.handle.steer_text("change course").await;
    assert_eq!(outcome, Ok(SteerOutcome::Injected));

    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "steered:change course");
    assert!(!s.handle.is_turn_running());

    // No second response is pending: the next prompt runs normally.
    s.handle.prompt_text("next").expect("not Busy");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "next-done");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_steer_right_behind_the_prompt_reaches_the_agent_after_it() {
    let dir = tmp();
    let mut s = start(
        &fake_harness(steerable()),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    s.next().await; // Ready

    s.handle.prompt_text("work").expect("prompt");
    let outcome = s.handle.steer_text("at once").await;
    assert_eq!(outcome, Ok(SteerOutcome::Injected));
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert!(message_text(&events).ends_with("steered:at once"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_steer_while_idle_requires_a_prompt_and_changes_nothing() {
    let dir = tmp();
    let mut s = start(
        &fake_harness(steerable()),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    s.next().await; // Ready

    let outcome = s.handle.steer_text("anyone there?").await;
    assert_eq!(outcome, Ok(SteerOutcome::PromptRequired));
    assert!(!s.handle.is_turn_running());

    s.handle.prompt_text("next").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "next-done", "no steered turn ran");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_steer_to_an_agent_without_steering_fails_without_being_sent() {
    let dir = tmp();
    let mut script = steerable();
    script["steering"] = json!(false);
    let mut s = start(&fake_harness(script), dir.path(), SpawnOptions::default()).await;
    s.next().await; // Ready

    s.handle.prompt_text("work").expect("prompt");
    s.until_message("working").await;
    // Sent, the fake would answer "method not found" (a `Request` error).
    let outcome = s.handle.steer_text("hello?").await;
    assert_eq!(outcome, Err(AgentError::SteeringUnsupported));
    assert!(s.handle.is_turn_running(), "the turn is untouched");

    s.handle.cancel().expect("cancel");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::Cancelled));
    assert_eq!(message_text(&events), "");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_steer_and_a_cancel_in_flight_together_end_the_turn_once() {
    let dir = tmp();
    let mut s = start(
        &fake_harness(steerable()),
        dir.path(),
        SpawnOptions::default(),
    )
    .await;
    s.next().await; // Ready

    s.handle.prompt_text("work").expect("prompt");
    s.until_message("working").await;
    // Both go out before either is answered.
    let steer = s.handle.steer_text("and also");
    s.handle.cancel().expect("cancel");
    assert_eq!(steer.await, Ok(SteerOutcome::Injected));

    let (_, stop) = s.turn().await;
    assert!(
        matches!(stop, Ok(StopReason::Cancelled | StopReason::EndTurn)),
        "{stop:?}"
    );

    // Exactly one TurnEnded: the next turn's events come next.
    s.handle.prompt_text("next").expect("not Busy");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "next-done");
    s.handle.shutdown().await;
}
