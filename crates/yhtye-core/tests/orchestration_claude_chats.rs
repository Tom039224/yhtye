//! Real Claude Code (always Haiku) with chats and branches (Stage 8c): two chats
//! on two branches (one not checked out in the main clone, so Yhtye creates a
//! worktree for it) each create a group that merges into its own branch; then
//! Yhtye "quits" and starts again on the same database, a past chat is resumed
//! with `session/load` (a codeword given in the first message comes back), and
//! an orchestrator whose process was killed restarts on the next message.
//! Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_chats -- --ignored --test-threads=1 --nocapture`.

mod common;

use std::path::{Path, PathBuf};

use common::orch::{
    assert_persisted, git_config, is_message, messages, shutdown_and_check, started_pids, until,
    until_healthy,
};
use common::real::{MODEL, REAL_TIMEOUT, assert_haiku};
use common::repo::{PROJECT, TempRepo};
use yhtye_core::acp::HarnessConfig;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, GroupStatus, orchestrator_session};
use yhtye_core::git::worktree_root;
use yhtye_core::runtime::Orchestration;

const WORD_MAIN: &str = "PELICAN";
const WORD_FEATURE: &str = "WALRUS";

fn request(word: &str, file: &str) -> String {
    format!(
        "The codeword for this chat is {word}; remember it. Create one group with exactly one code task \
         (implement step only) that creates the file {file} containing the line `{file}`. \
         When the group settles, call finish_group."
    )
}

fn config(r: &TempRepo) -> yhtye_core::runtime::OrchestrationConfig {
    git_config(
        r,
        HarnessConfig::claude_code(MODEL),
        HarnessConfig::claude_code(MODEL),
        HarnessConfig::claude_code(MODEL),
    )
}

fn merged_ok(e: &ApiEvent) -> bool {
    matches!(
        &e.body,
        ApiEventBody::Domain {
            event: DomainEvent::GroupMergeFinished { ok: true, .. }
        }
    )
}

/// `(acp session id, resumed, cwd, pid)` of every start of `session`.
fn started(events: &[ApiEvent], session: &str) -> Vec<(String, bool, Option<String>, Option<u32>)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session: s,
                acp_session_id,
                resumed,
                cwd,
                pid,
                ..
            } if s == session => Some((acp_session_id.clone(), *resumed, cwd.clone(), *pid)),
            _ => None,
        })
        .collect()
}

fn canonical(p: &str) -> PathBuf {
    std::fs::canonicalize(Path::new(p)).expect("canonical path")
}

fn reply(events: &[ApiEvent], session: &str) -> String {
    messages(events, session).concat()
}

/// Waits until a turn of `session` ended after `from` events.
async fn turn_ended(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ApiEvent>,
    events: &mut Vec<ApiEvent>,
    session: &str,
    from: usize,
) {
    let is_end = |e: &ApiEvent| matches!(&e.body, ApiEventBody::Agent { session: s, event: yhtye_core::acp::AgentEvent::TurnEnded(_) } if s == session);
    if !events[from..].iter().any(is_end) {
        until_healthy(rx, events, REAL_TIMEOUT, is_end).await;
    }
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_two_chats_on_two_branches_merge_into_their_own_branch_then_resume_after_a_restart() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let worktree = worktree_root(&r.data, PROJECT)
        .join("branches")
        .join("feature");

    // Phase 1: two chats at once.
    let (orch, mut rx) = Orchestration::start(config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    let main = orch.create_chat("main").await.expect("chat").id;
    let feature = orch.create_chat("feature").await.expect("chat").id;
    orch.send_user_message(&main, request(WORD_MAIN, "main.txt"))
        .await
        .expect("send");
    orch.send_user_message(&feature, request(WORD_FEATURE, "feature.txt"))
        .await
        .expect("send");
    let merges = |events: &[ApiEvent]| events.iter().filter(|e| merged_ok(e)).count();
    while merges(&events) < 2 {
        until_healthy(&mut rx, &mut events, REAL_TIMEOUT, merged_ok).await;
    }
    assert_haiku(&events);

    let snap = orch.snapshot().await.expect("snapshot");
    // Let each orchestrator finish the turn that merged its group, so the
    // "quit" below does not cut a turn off.
    for chat in [&main, &feature] {
        let group = snap
            .state
            .groups
            .iter()
            .find(|g| &g.chat == chat)
            .expect("group")
            .id
            .clone();
        let at = events
            .iter()
            .position(|e| matches!(&e.body, ApiEventBody::Domain { event: DomainEvent::GroupMergeFinished { group: g, .. } } if *g == group))
            .expect("merge event");
        turn_ended(&mut rx, &mut events, &orchestrator_session(chat), at).await;
    }
    for (chat, branch) in [(&main, "main"), (&feature, "feature")] {
        let g = snap
            .state
            .groups
            .iter()
            .find(|g| &g.chat == chat)
            .expect("a group of the chat");
        assert_eq!(
            (g.status, g.base_branch.as_str()),
            (GroupStatus::Done, branch),
            "{g:#?}"
        );
    }
    assert_eq!(r.show("main", "main.txt").trim(), "main.txt");
    assert_eq!(r.show("feature", "feature.txt").trim(), "feature.txt");
    assert!(!r.git_status(&["cat-file", "-e", "main:feature.txt"]));
    assert!(!r.git_status(&["cat-file", "-e", "feature:main.txt"]));
    assert_eq!(
        r.git(&["branch", "--show-current"]).trim(),
        "main",
        "the main clone is not switched"
    );
    // The orchestrators ran in their branch's worktree.
    let cwd = |chat: &String| {
        started(&events, &orchestrator_session(chat))[0]
            .2
            .clone()
            .expect("cwd")
    };
    assert_eq!(
        canonical(&cwd(&main)),
        canonical(r.repo.to_str().expect("utf8"))
    );
    assert_eq!(
        canonical(&cwd(&feature)),
        canonical(worktree.to_str().expect("utf8"))
    );
    assert_persisted(&orch).await;
    let acp_of = |chat: &String| started(&events, &orchestrator_session(chat))[0].0.clone();
    let (acp_main, acp_feature) = (acp_of(&main), acp_of(&feature));
    shutdown_and_check(orch, &events).await;

    // Phase 2: start again; the past chats resume with session/load.
    let (orch, mut rx) = Orchestration::start(config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    let ask = "What was the codeword for this chat? Answer with only the codeword and do not use any tool.";
    for (chat, word, acp) in [
        (&main, WORD_MAIN, &acp_main),
        (&feature, WORD_FEATURE, &acp_feature),
    ] {
        let session = orchestrator_session(chat);
        let from = events.len();
        orch.send_user_message(chat, ask).await.expect("send");
        turn_ended(&mut rx, &mut events, &session, from).await;
        let starts = started(&events, &session);
        eprintln!(
            "{session}: starts {starts:?}, reply {:?}",
            reply(&events, &session)
        );
        eprintln!(
            "{session}: prompts {:#?}",
            common::orch::prompts_to(&events, &session)
        );
        assert_eq!(starts.len(), 1, "one start");
        assert_eq!(
            (starts[0].0.as_str(), starts[0].1),
            (acp.as_str(), true),
            "session/load with the same id"
        );
        assert!(
            reply(&events, &session).contains(word),
            "{session} remembers {word}"
        );
    }

    // Phase 3: kill the main chat's orchestrator; the next message restarts it.
    let session = orchestrator_session(&main);
    let pid = started(&events, &session)[0].3.expect("pid");
    let killed = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .status()
        .expect("kill");
    assert!(killed.success());
    until(
        &mut rx,
        &mut events,
        REAL_TIMEOUT,
        |e| matches!(&e.body, ApiEventBody::SessionStopped { session: s, .. } if *s == session),
    )
    .await;
    let from = events.len();
    orch.send_user_message(
        &main,
        "Answer with only the word OK and do not use any tool.",
    )
    .await
    .expect("send");
    turn_ended(&mut rx, &mut events, &session, from).await;
    let starts = started(&events, &session);
    eprintln!(
        "after the kill: starts {starts:?}, reply {:?}",
        reply(&events[from..], &session)
    );
    assert_eq!(starts.len(), 2, "restarted by the next message");
    assert!(is_reply_ok(&events[from..], &session));
    assert_haiku(&events);
    assert!(started_pids(&events).len() >= 3);
    shutdown_and_check(orch, &events).await;
}

fn is_reply_ok(events: &[ApiEvent], session: &str) -> bool {
    events.iter().any(|e| is_message(e, session, "OK"))
}
