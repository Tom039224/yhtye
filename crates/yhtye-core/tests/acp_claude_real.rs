//! Real Claude Code tests (always Haiku). Ignored by default; run with
//! `cargo test -p yhtye-core --test acp_claude_real -- --ignored --test-threads=1`.
//! Uses the local Claude Code login. Prompts are kept tiny to save tokens.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{Session, assert_group_gone, message_text};
use tokio::sync::mpsc;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{
    AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, SteerOutcome, spawn_agent,
};

const REAL_TIMEOUT: Duration = Duration::from_secs(180);
const MODEL: &str = "haiku";

/// Starts Claude Code on Haiku in bypass mode and asserts both are in effect.
async fn start_claude(cwd: &Path, options: SpawnOptions) -> Session {
    let harness = HarnessConfig::claude_code(MODEL);
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = spawn_agent(&harness, cwd, options, tx)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let info = handle.info();
    assert_eq!(
        info.config_value("model"),
        Some(MODEL),
        "model must be haiku: {:?}",
        info.config_options
    );
    assert_eq!(info.current_mode(), Some("bypassPermissions"));
    eprintln!(
        "session {} (resumed={}) agent={:?}",
        info.acp_session_id.0, info.resumed, info.agent
    );
    Session {
        handle,
        events: rx,
        timeout: REAL_TIMEOUT,
    }
}

/// Shuts down and checks that no process of the agent's group is left (npx → node → claude).
async fn stop(s: Session) {
    let pgid = s.handle.pid().expect("pid");
    s.handle.shutdown().await;
    assert_group_gone(pgid, Duration::from_secs(10)).await;
}

fn summarize(events: &[AgentEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            AgentEvent::Ready(_) => "ready",
            AgentEvent::Output(o) => match o {
                AgentOutput::MessageChunk(_) => "message",
                AgentOutput::ThoughtChunk(_) => "thought",
                AgentOutput::ToolCall(_) => "tool_call",
                AgentOutput::ToolCallUpdate(_) => "tool_call_update",
                AgentOutput::Usage(_) => "usage",
                AgentOutput::Unknown(_) => "unknown",
                AgentOutput::AvailableCommands(_) => "commands",
                AgentOutput::ModeChanged(_) => "mode",
                AgentOutput::ConfigOptions(_) => "config",
                AgentOutput::SessionInfo(_) => "session_info",
                AgentOutput::Plan(_) => "plan",
                AgentOutput::UserMessageChunk(_) => "user_message",
            },
            AgentEvent::PermissionAutoAnswered { .. } => "permission",
            _ => "other",
        })
        .collect()
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_prompt_streams_on_haiku() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text("Reply with exactly one word: pong")
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    eprintln!("events: {:?}\ntext: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Output(AgentOutput::MessageChunk(_))))
    );
    assert!(text.to_lowercase().contains("pong"), "{text}");
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_cancel_mid_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text("Without using any tools, write a 1500-word story about a lighthouse keeper.")
        .expect("prompt");
    let first = s.until_message("").await;
    eprintln!("first chunk: {first:?}");
    let t0 = Instant::now();
    s.handle.cancel().expect("cancel");
    let (events, stop_reason) = s.turn().await;
    eprintln!(
        "cancelled after {:?}; {} more events",
        t0.elapsed(),
        events.len()
    );
    assert_eq!(stop_reason, Ok(StopReason::Cancelled));
    assert!(t0.elapsed() < Duration::from_secs(30));
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_steer_is_injected_into_the_running_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    assert!(
        s.handle.supports_steering(),
        "the adapter advertises steering"
    );
    assert_eq!(
        s.handle.steer_text("idle?").await,
        Ok(SteerOutcome::PromptRequired)
    );

    s.handle
        .prompt_text(
            "Use the Bash tool to run `sleep 8; echo STEP1`, then reply with the single word FIRSTDONE.",
        )
        .expect("prompt");
    loop {
        if let AgentEvent::Output(AgentOutput::ToolCall(_)) = s.next().await {
            break;
        }
    }
    let outcome = s
        .handle
        .steer_text("(steer) Forget the rest; just reply with the single word STEERED.")
        .await;
    assert_eq!(outcome, Ok(SteerOutcome::Injected));
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    eprintln!("events: {:?}; text: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.contains("STEERED"), "{text:?}");
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_bypass_mode_writes_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text(
            "Create a file named hello.txt in the current directory containing exactly: hi",
        )
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    eprintln!("events: {:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let body =
        std::fs::read_to_string(dir.path().join("hello.txt")).expect("hello.txt was created");
    assert_eq!(body.trim(), "hi");
    for e in &events {
        if let AgentEvent::PermissionAutoAnswered { chosen, .. } = e {
            eprintln!("permission asked; chosen = {chosen:?}");
            assert!(matches!(
                chosen,
                Some(PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways)
            ));
        }
    }
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_session_load_restores_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    let session = s.handle.info().acp_session_id.clone();
    s.handle
        .prompt_text("Remember the codeword PINEAPPLE-42. Reply only: ok")
        .expect("prompt");
    assert_eq!(s.turn().await.1, Ok(StopReason::EndTurn));
    stop(s).await;

    let opts = SpawnOptions {
        secret_env: Default::default(),
        resume: Some(session.clone()),
        ..SpawnOptions::default()
    };
    let mut s = start_claude(dir.path(), opts).await;
    assert!(s.handle.info().resumed);
    assert_eq!(s.handle.info().acp_session_id, session);
    s.handle
        .prompt_text("What was the codeword? Reply with only the codeword.")
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    eprintln!("answer: {text:?}");
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.contains("PINEAPPLE-42"), "{text}");
    stop(s).await;
}

/// `/usage` (a local command, no model call) through the probe harness is read
/// into a usage report (Stage 6b): plan, 5-hour and weekly windows, reset times.
#[tokio::test]
#[ignore = "real Claude Code (subscription usage)"]
async fn real_usage_command_reports_the_subscription() {
    use yhtye_core::usage::{UsageWindowKind, probe_usage};
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = HarnessConfig::claude_code_usage_probe();
    let report = probe_usage(&harness, dir.path())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    eprintln!("usage: {report:?}");
    assert!(report.plan.is_some(), "{report:?}");
    for kind in [UsageWindowKind::FiveHour, UsageWindowKind::Week] {
        let w = report
            .window(kind)
            .unwrap_or_else(|| panic!("{kind:?}: {report:?}"));
        assert!((0.0..=100.0).contains(&w.percent));
        assert!(w.resets_at_ms.is_some(), "{w:?}");
    }
    // Nothing was written to the probe's directory.
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
}

/// Stage 7b: the model list is read from the adapter's `session/new`
/// `configOptions` (no prompt, so no model call) and the process group is gone.
#[tokio::test]
#[ignore = "real Claude Code"]
async fn real_model_list_comes_from_the_adapter() {
    use yhtye_core::agents::{HarnessPreset, probe_models};
    let dir = tempfile::tempdir().expect("tempdir");
    let started = Instant::now();
    let preset = HarnessPreset::claude_code(MODEL);
    let listed = probe_models(
        &yhtye_core::secrets::Secrets::none(),
        &preset,
        dir.path(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("models");
    eprintln!(
        "models in {:?}: current={:?} {:?}",
        started.elapsed(),
        listed.current,
        listed
            .models
            .iter()
            .map(|m| format!("{} ({})", m.value, m.name))
            .collect::<Vec<_>>()
    );
    for m in &listed.models {
        eprintln!(
            "  efforts of {}: {:?}",
            m.value,
            m.efforts
                .as_ref()
                .map(|e| e.iter().map(|o| o.value.as_str()).collect::<Vec<_>>())
        );
    }
    assert!(
        listed.models.iter().all(|m| m.efforts.is_some()),
        "a short list has every model's efforts read: {listed:?}"
    );
    assert_eq!(listed.harness, "claude-code");
    assert!(listed.models.iter().any(|m| m.value == MODEL), "{listed:?}");
    assert_eq!(
        listed.current.as_deref(),
        Some(MODEL),
        "ANTHROPIC_MODEL of the probe"
    );
}

/// Stage 7d: the effort of a model is set with `set_config_option` after the
/// model and the adapter reports it back. No prompt is sent (no model call):
/// Haiku has no effort option, so `sonnet`'s session start is exercised, never
/// a turn.
#[tokio::test]
#[ignore = "real Claude Code (no prompt)"]
async fn real_effort_is_applied_after_the_model() {
    use yhtye_core::acp::{ModelSelect, SpawnOptions, spawn_agent};
    use yhtye_core::agents::{AgentRole, HarnessPreset, probe_models};
    let dir = tempfile::tempdir().expect("tempdir");
    let preset = HarnessPreset::claude_code(MODEL);
    let listed = probe_models(
        &yhtye_core::secrets::Secrets::none(),
        &preset,
        dir.path(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("models");
    let with_efforts =
        |m: &&yhtye_core::agents::ModelOption| m.efforts.as_ref().is_some_and(|e| !e.is_empty());
    let chosen = listed
        .models
        .iter()
        .filter(with_efforts)
        .find(|m| m.value == "sonnet")
        .expect("sonnet has efforts");
    let efforts: Vec<&str> = chosen
        .efforts
        .iter()
        .flatten()
        .map(|e| e.value.as_str())
        .collect();
    eprintln!("using {} with efforts {efforts:?}", chosen.value);
    assert!(
        !efforts.contains(&"default"),
        "the `default` row is filtered"
    );
    // Not the adapter's own default, so a passing check cannot be a coincidence.
    let effort = efforts
        .iter()
        .find(|e| **e != "medium")
        .copied()
        .expect("an effort");

    let harness = preset.config(AgentRole::Implementer, Some(&chosen.value), Some(effort));
    assert_eq!(
        harness.effort,
        Some(ModelSelect {
            config_id: "effort".into(),
            value: effort.into()
        })
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = spawn_agent(&harness, dir.path(), SpawnOptions::default(), tx)
        .await
        .expect("session with the effort");
    assert_eq!(
        handle.info().config_value("model"),
        Some(chosen.value.as_str())
    );
    assert_eq!(handle.info().config_value("effort"), Some(effort));
    eprintln!(
        "session reports model={:?} effort={:?}",
        handle.info().config_value("model"),
        handle.info().config_value("effort")
    );

    // An effort the model does not offer fails the start with a clear error.
    let bad = preset.config(
        AgentRole::Implementer,
        Some(&chosen.value),
        Some("no-such-effort"),
    );
    let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
    let err = spawn_agent(&bad, dir.path(), SpawnOptions::default(), tx2)
        .await
        .err()
        .expect("unknown effort is refused");
    eprintln!("refused: {err}");
    assert!(err.to_string().contains("set_config_option"), "{err}");

    let pid = handle.pid().expect("pid");
    handle.shutdown().await;
    drop(rx);
    assert_group_gone(pid, Duration::from_secs(5)).await;
}

// ---------------------------------------------------------------------------
// Claude Code billed to the subscription or to an API key
// (`claude-code` / `claude-code-api`). The API tests read the key the user
// registered as `YHTYE_ANTHROPIC_API_KEY` from the OS credential store and skip
// when there is none. A key value is never printed.
// ---------------------------------------------------------------------------

/// The name the API key is registered under, with the user's real credential
/// store behind it; `None` (the test is skipped) when no key is registered or
/// the store cannot be read.
fn registered_api_key() -> Option<yhtye_core::secrets::Secrets> {
    use std::sync::Arc;
    use yhtye_core::acp::CLAUDE_API_KEY_SECRET;
    use yhtye_core::secrets::{KeyringBackend, SecretBackend, Secrets};
    let backend = Arc::new(KeyringBackend::new());
    match backend.get(CLAUDE_API_KEY_SECRET) {
        Ok(Some(_)) => Some(Secrets::new(backend, [CLAUDE_API_KEY_SECRET.to_string()])),
        Ok(None) => {
            eprintln!("skipped: {CLAUDE_API_KEY_SECRET} is not registered");
            None
        }
        Err(e) => {
            eprintln!("skipped: the credential store cannot be read: {e}");
            None
        }
    }
}

/// Secrets in memory (never the user's): the names and values given.
async fn memory_secrets(vars: &[(&str, &str)]) -> yhtye_core::secrets::Secrets {
    use std::sync::Arc;
    use yhtye_core::secrets::{MemoryBackend, SecretValue, Secrets};
    let secrets = Secrets::new(Arc::new(MemoryBackend::default()), []);
    for (name, value) in vars {
        secrets
            .set(name, SecretValue::new(*value))
            .await
            .expect("set");
    }
    secrets
}

/// What one prompt to Claude Code on Haiku came to.
#[derive(Debug)]
enum Answer {
    /// The agent did not start (`spawn_agent_with_secrets` failed): the error.
    NotStarted(String),
    /// The turn ended: the message text and how it ended.
    Turn(String, Result<StopReason, String>),
}

impl Answer {
    /// The message of a start that failed (never an answer).
    fn not_started(&self) -> Option<&str> {
        match self {
            Self::NotStarted(e) => Some(e),
            Self::Turn(..) => None,
        }
    }
}

/// Starts `harness` with `secrets`, sends `prompt` and reports what came back
/// (a start that fails is an answer too). The agent is stopped afterwards.
async fn ask(
    harness: &HarnessConfig,
    secrets: &yhtye_core::secrets::Secrets,
    prompt: &str,
) -> Answer {
    let dir = tempfile::tempdir().expect("tempdir");
    let (tx, rx) = mpsc::unbounded_channel();
    let started = yhtye_core::secrets::spawn_agent_with_secrets(
        secrets,
        harness,
        dir.path(),
        SpawnOptions::default(),
        tx,
    )
    .await;
    let handle = match started {
        Ok(h) => h,
        Err(e) => return Answer::NotStarted(e.to_string()),
    };
    let mut s = Session {
        handle,
        events: rx,
        timeout: REAL_TIMEOUT,
    };
    s.handle.prompt_text(prompt).expect("prompt");
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    stop(s).await;
    Answer::Turn(text, stop_reason.map_err(|e| e.to_string()))
}

/// The subscription Claude Code answers a turn, and an `ANTHROPIC_API_KEY`
/// among the user's secrets (here an invalid one: it would fail the turn if it
/// reached the agent) is not what it uses.
#[tokio::test]
#[ignore = "real Claude Code (Haiku, subscription)"]
async fn real_subscription_claude_code_answers_and_ignores_an_api_key_secret() {
    let secrets = memory_secrets(&[
        ("ANTHROPIC_API_KEY", "sk-ant-api03-invalid-key-for-the-test"),
        (
            "YHTYE_ANTHROPIC_API_KEY",
            "sk-ant-api03-invalid-key-for-the-test",
        ),
    ])
    .await;
    let answer = ask(
        &HarnessConfig::claude_code(MODEL),
        &secrets,
        "Reply with exactly one word: pong",
    )
    .await;
    eprintln!("answer: {answer:?}");
    let Answer::Turn(text, stop) = answer else {
        panic!(
            "the subscription Claude Code did not start: {:?}",
            answer.not_started()
        );
    };
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert!(text.to_lowercase().contains("pong"), "{text}");
}

/// `claude-code-api` answers a turn on Haiku with the registered key, which
/// only it gets (as `ANTHROPIC_API_KEY`).
#[tokio::test]
#[ignore = "real Claude Code (Haiku, API key from the credential store)"]
async fn real_api_claude_code_answers_with_the_registered_key() {
    let Some(secrets) = registered_api_key() else {
        return;
    };
    let answer = ask(
        &HarnessConfig::claude_code_api(MODEL),
        &secrets,
        "Reply with exactly one word: pong",
    )
    .await;
    eprintln!("answer: {answer:?}");
    let Answer::Turn(text, stop) = answer else {
        panic!("claude-code-api did not start: {:?}", answer.not_started());
    };
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert!(text.to_lowercase().contains("pong"), "{text}");
}

/// That `claude-code-api` really runs on the key: with an invalid one it cannot
/// answer, whereas the machine's subscription login (which `claude-code`
/// uses) would have.
#[tokio::test]
#[ignore = "real Claude Code (invalid API key)"]
async fn real_api_claude_code_with_an_invalid_key_cannot_answer() {
    let secrets = memory_secrets(&[(
        "YHTYE_ANTHROPIC_API_KEY",
        "sk-ant-api03-invalid-key-for-the-test",
    )])
    .await;
    let answer = ask(
        &HarnessConfig::claude_code_api(MODEL),
        &secrets,
        "Reply with exactly one word: pong",
    )
    .await;
    eprintln!("answer: {answer:?}");
    let Answer::Turn(text, stop) = answer else {
        panic!(
            "it should start and fail the turn: {:?}",
            answer.not_started()
        );
    };
    assert!(text.contains("Invalid API key"), "{text}");
    assert!(!text.to_lowercase().contains("pong"), "{text}");
    let error = stop.expect_err("the turn fails on authentication");
    assert!(error.contains("Authentication required"), "{error}");
}

/// The models and efforts of `claude-code-api` are read with the key too.
#[tokio::test]
#[ignore = "real Claude Code (API key from the credential store, no prompt)"]
async fn real_api_model_list_is_read_with_the_registered_key() {
    use yhtye_core::agents::{HarnessPreset, probe_models};
    let Some(secrets) = registered_api_key() else {
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let preset = HarnessPreset::claude_code_api(MODEL);
    let listed = probe_models(
        &secrets,
        &preset,
        dir.path(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("models");
    eprintln!(
        "models: current={:?} {:?}",
        listed.current,
        listed
            .models
            .iter()
            .map(|m| m.value.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(listed.harness, "claude-code-api");
    assert!(listed.models.iter().any(|m| m.value == MODEL), "{listed:?}");
}

/// The label of the subscription Claude Code names the plan of the login that
/// is really on this machine (no model call, nothing is written or printed
/// of the login but the plan name).
#[test]
#[ignore = "reads the real ~/.claude/.credentials.json"]
fn real_subscription_label_names_the_plan_of_the_login() {
    let label = yhtye_core::agents::claude_code_label(&|k: &str| std::env::var_os(k));
    eprintln!("label: {label}");
    assert!(
        label.starts_with("Claude Code (") && label.ends_with(')'),
        "{label}"
    );
    assert!(!label.contains("sk-ant"), "{label}");
}
