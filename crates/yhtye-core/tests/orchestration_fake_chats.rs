//! Chats and branches with fake agents (`core-design.md` §17): lazy start in the
//! worktree of the chat's branch, several orchestrators at once, the open-group
//! limit per branch, an orchestrator whose process ended, resuming past chats,
//! and which chats a restart restores.

mod common;

use std::path::{Path, PathBuf};

use common::chats::{
    Rx, TIMEOUT, branch_worktree, canonical, cwd_of, is_domain, new_chat, none, repo_config, said,
    say, settle, start, started, wait,
};
use common::fake_harness;
use common::orch::{
    assert_persisted, config, messages, prompts_to, shutdown_and_check, started_sessions, until,
};
use common::repo::{PROJECT, TempRepo};
use serde_json::{Value, json};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, GroupStatus, orchestrator_session};
use yhtye_core::git::worktree_root;
use yhtye_core::runtime::Orchestration;

#[tokio::test]
async fn an_orchestrator_starts_lazily_in_the_worktree_of_its_chats_branch() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let script = json!({"turns": [{"actions": [
        {"write_file": {"path": "where.txt", "text": "here\n"}},
        {"message": "ready"}
    ]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let main = new_chat(&orch, "main").await;
    let feature = new_chat(&orch, "feature").await;
    settle(&mut rx, &mut events).await;
    assert!(
        started_sessions(&events).is_empty(),
        "creating chats starts nothing"
    );
    assert!(r.extra_worktrees().is_empty(), "and creates no worktree");

    // `feature` is checked out nowhere: Yhtye makes a worktree for it, and the
    // orchestrator runs there. Nothing is switched in the main clone.
    say(&orch, &feature, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-2", "ready")).await;
    let wt = canonical(&branch_worktree(&r, "feature"));
    assert_eq!(cwd_of(&events, "orchestrator:C-2"), wt);
    assert!(
        wt.join("where.txt").exists(),
        "the agent works in that directory"
    );
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    assert_eq!(
        started_sessions(&events),
        ["orchestrator:C-2"],
        "only the used chat"
    );

    // `main` is checked out in the main clone: that is where its orchestrator runs.
    say(&orch, &main, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready")).await;
    assert_eq!(cwd_of(&events, "orchestrator:C-1"), canonical(&r.repo));
    assert_eq!(r.extra_worktrees().len(), 1, "no second worktree for main");
    assert_persisted(&orch).await;
    shutdown_and_check(orch, &events).await;
}

/// A shared orchestrator script for the group tests: each chat's message names
/// its work; the implementer writes that file (slowly, so the groups overlap).
fn planning_scripts() -> (Value, Value) {
    let plan = |needle: &str, instruction: &str| {
        json!({"match": needle, "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": needle}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": needle, "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": instruction}}},
            {"message": "planned"}
        ]})
    };
    let orch = json!({"turns": [
        plan("main-work", "write-main"),
        plan("feature-work", "write-feature"),
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
        ]}
    ]});
    let write = |needle: &str, path: &str| {
        json!({"match": needle, "actions": [
            {"sleep": 500},
            {"write_file": {"path": path, "text": format!("{path}\n")}},
            {"mcp_call": {"tool": "report_step_done", "args": {"result": format!("wrote {path}")}}}
        ]})
    };
    let implementer =
        json!({"turns": [write("write-main", "main.txt"), write("write-feature", "feature.txt")]});
    (orch, implementer)
}

#[tokio::test]
async fn two_chats_on_different_branches_run_groups_at_once_and_merge_into_their_own_branch() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = planning_scripts();
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let main = new_chat(&orch, "main").await;
    let feature = new_chat(&orch, "feature").await;
    say(&orch, &main, "main-work").await;
    say(&orch, &feature, "feature-work").await;
    let merged = |e: &ApiEvent| {
        is_domain(e, |d| {
            matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
        })
    };
    wait(&mut rx, &mut events, merged).await;
    let merges = |events: &[ApiEvent]| events.iter().filter(|e| merged(e)).count();
    while merges(&events) < 2 {
        until(&mut rx, &mut events, TIMEOUT, merged).await;
    }

    // Both groups were open at the same time.
    let position = |f: fn(&DomainEvent) -> bool| -> Vec<usize> {
        (0..events.len())
            .filter(|i| is_domain(&events[*i], f))
            .collect()
    };
    let created = position(|d| matches!(d, DomainEvent::GroupCreated { .. }));
    let finished = position(|d| matches!(d, DomainEvent::GroupMergeFinished { .. }));
    assert_eq!((created.len(), finished.len()), (2, 2));
    assert!(
        created[1] < finished[0],
        "the second group started before the first finished"
    );

    // Each landed on its own branch, in that branch's worktree.
    let snapshot = orch.snapshot().await.expect("snapshot");
    let by_chat = |chat: &str| {
        snapshot
            .state
            .groups
            .iter()
            .find(|g| g.chat == chat)
            .unwrap_or_else(|| panic!("a group of {chat}"))
    };
    assert_eq!(by_chat(&main).base_branch, "main");
    assert_eq!(by_chat(&feature).base_branch, "feature");
    assert!(
        snapshot
            .state
            .groups
            .iter()
            .all(|g| g.status == GroupStatus::Done)
    );
    assert_eq!(r.show("main", "main.txt"), "main.txt\n");
    assert_eq!(r.show("feature", "feature.txt"), "feature.txt\n");
    assert!(!r.git_status(&["cat-file", "-e", "main:feature.txt"]));
    assert!(!r.git_status(&["cat-file", "-e", "feature:main.txt"]));
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");

    // Each orchestrator heard about its own group only.
    for chat in [&main, &feature] {
        let group = &by_chat(chat).id;
        let other = &snapshot
            .state
            .groups
            .iter()
            .find(|g| &g.id != group)
            .expect("other")
            .id;
        let prompts = prompts_to(&events, &orchestrator_session(chat)).join("\n");
        assert!(
            prompts.contains(&format!("group_settled] group={group}")),
            "{prompts}"
        );
        assert!(!prompts.contains(&format!("group={other}")), "{prompts}");
    }
    assert_persisted(&orch).await;
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_second_open_group_on_the_same_branch_is_rejected_and_other_chats_are_out_of_reach() {
    let r = TempRepo::new();
    let orch_script = json!({"turns": [
        {"match": "first", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "one"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "t", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "hold"}}},
            {"message": "started"}
        ]},
        {"match": "second", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "two"}}},
            {"mcp_call": {"tool": "get_status", "args": {"group_id": "G-1"}}},
            {"mcp_call": {"tool": "cancel_group", "args": {"group_id": "G-1", "reason": "x"}}},
            {"message": "second-done"}
        ]}
    ]});
    let impl_script = json!({"turns": [{"actions": ["wait_cancel"]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let a = new_chat(&orch, "main").await;
    let b = new_chat(&orch, "main").await;
    say(&orch, &a, "first").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "started")).await;
    say(&orch, &b, "second").await;
    wait(
        &mut rx,
        &mut events,
        said("orchestrator:C-2", "second-done"),
    )
    .await;

    let replies = messages(&events, "orchestrator:C-2");
    let reply = |tool: &str| {
        replies
            .iter()
            .find(|m| m.starts_with(&format!("mcp:{tool}:error:")))
            .unwrap_or_else(|| panic!("{tool} error in {replies:#?}"))
            .clone()
    };
    let conflict = reply("create_group");
    assert!(
        conflict.contains("conflict") && conflict.contains("C-1"),
        "{conflict}"
    );
    assert!(reply("get_status").contains("forbidden"));
    assert!(reply("cancel_group").contains("forbidden"));
    let snapshot = orch.snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot.state.groups.len(),
        1,
        "the rejected group was not created"
    );
    assert_eq!(snapshot.state.groups[0].status, GroupStatus::Active);
    shutdown_and_check(orch, &events).await;
}

/// `die` makes the orchestrator's process exit; `again` gets an answer.
fn mortal_orchestrator() -> Value {
    json!({"turns": [
        {"match": "die", "actions": [{"message": "dying"}, {"crash": 3}]},
        {"match": "hello", "actions": [{"message": "hi"}]},
        {"match": "again", "actions": [{"message": "back"}]}
    ]})
}

#[tokio::test]
async fn an_orchestrator_whose_process_ended_starts_again_on_the_next_send() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(
        dir.path(),
        fake_harness(mortal_orchestrator()),
        fake_harness(none()),
        fake_harness(none()),
    );
    let (orch, mut rx, mut events) = start(cfg).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "die").await;
    wait(&mut rx, &mut events, |e| {
        matches!(&e.body, ApiEventBody::SessionStopped { session, .. } if session == "orchestrator:C-1")
    })
    .await;
    // Ending is not a reason to start again: nothing is waiting for it.
    settle(&mut rx, &mut events).await;
    assert_eq!(started(&events, "orchestrator:C-1").len(), 1);

    say(&orch, &chat, "again").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "back")).await;
    let starts = started(&events, "orchestrator:C-1");
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert!(starts[1].1, "the stored session was loaded again");
    let prompts = prompts_to(&events, "orchestrator:C-1");
    let second = prompts.last().expect("a prompt");
    assert!(second.contains("again"), "{second}");
    assert!(
        second.contains("[yhtye:restarted]") && second.contains("cut off"),
        "the cut-off turn is reported: {second}"
    );
    assert_persisted(&orch).await;
    shutdown_and_check(orch, &events).await;
}

/// Starts on `dir`, runs `body`, and shuts down; returns every event.
async fn session_of(
    dir: &Path,
    orch_script: Value,
    body: impl AsyncFnOnce(&Orchestration, &mut Rx, &mut Vec<ApiEvent>),
) -> Vec<ApiEvent> {
    let cfg = config(
        dir,
        fake_harness(orch_script),
        fake_harness(none()),
        fake_harness(none()),
    );
    let (orch, mut rx, mut events) = start(cfg).await;
    body(&orch, &mut rx, &mut events).await;
    shutdown_and_check(orch, &events).await;
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

#[tokio::test]
async fn a_past_chat_is_resumed_by_sending_and_only_live_chats_come_back_at_startup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = session_of(dir.path(), mortal_orchestrator(), async |orch, rx, events| {
        let past = new_chat(orch, "main").await; // C-1: ends its process
        let live = new_chat(orch, "main").await; // C-2: idle and running at the quit
        let _unused = new_chat(orch, "main").await; // C-3
        say(orch, &past, "die").await;
        wait(rx, events, |e| {
            matches!(&e.body, ApiEventBody::SessionStopped { session, suspended: false } if session == "orchestrator:C-1")
        })
        .await;
        say(orch, &live, "hello").await;
        wait(rx, events, said("orchestrator:C-2", "hi")).await;
    })
    .await;
    let past_id = started(&first, "orchestrator:C-1")[0].0.clone();

    // A restart brings back C-2 (it was live) and nothing else.
    let second = session_of(
        dir.path(),
        mortal_orchestrator(),
        async |orch, rx, events| {
            wait(rx, events, |e| {
                !started_of(e, "orchestrator:C-2").is_empty()
            })
            .await;
            settle(rx, events).await;
            assert_eq!(
                started_sessions(events),
                ["orchestrator:C-2"],
                "only the live chat"
            );
            assert!(
                started(events, "orchestrator:C-2")[0].1,
                "restored with session/load"
            );

            // The past chat resumes when something is sent to it.
            say(orch, "C-1", "hello").await;
            wait(rx, events, said("orchestrator:C-1", "hi")).await;
        },
    )
    .await;
    let resumed = started(&second, "orchestrator:C-1");
    assert_eq!(resumed.len(), 1);
    assert_eq!(
        (resumed[0].0.as_str(), resumed[0].1),
        (past_id.as_str(), true)
    );
    assert!(
        started(&second, "orchestrator:C-3").is_empty(),
        "an unused chat never starts"
    );
}

fn started_of(e: &ApiEvent, session: &str) -> Vec<()> {
    match &e.body {
        ApiEventBody::SessionStarted { session: s, .. } if s == session => vec![()],
        _ => Vec::new(),
    }
}

#[tokio::test]
async fn a_chat_whose_stored_session_cannot_be_loaded_gets_a_new_one_and_a_summary() {
    let dir = tempfile::tempdir().expect("tempdir");
    session_of(dir.path(), mortal_orchestrator(), async |orch, rx, events| {
        let chat = new_chat(orch, "main").await;
        say(orch, &chat, "die").await;
        wait(rx, events, |e| {
            matches!(&e.body, ApiEventBody::SessionStopped { session, suspended: false } if session == "orchestrator:C-1")
        })
        .await;
    })
    .await;
    let mut script = mortal_orchestrator();
    script["fail_at"] = json!("session/load");
    let events = session_of(dir.path(), script, async |orch, rx, events| {
        say(orch, "C-1", "hello").await;
        wait(rx, events, said("orchestrator:C-1", "hi")).await;
    })
    .await;
    let starts = started(&events, "orchestrator:C-1");
    assert_eq!(starts.len(), 1);
    assert!(!starts[0].1, "a new session");
    assert!(events.iter().any(|e| matches!(&e.body,
        ApiEventBody::SessionFailed { session, error }
            if session == "orchestrator:C-1" && error.contains("could not restore"))));
    let prompts = prompts_to(&events, "orchestrator:C-1");
    assert!(prompts[0].contains("could not be restored"), "{prompts:?}");
    assert!(
        prompts[0].contains("hello"),
        "the message is still delivered: {prompts:?}"
    );
}

#[tokio::test]
async fn a_stored_session_from_another_directory_is_not_loaded() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let script = json!({"turns": [{"actions": [{"message": "ready"}]}]});
    let cfg = || repo_config(&r, script.clone(), none());
    let (orch, mut rx, mut events) = start(cfg()).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "one").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready")).await;
    shutdown_and_check(orch, &events).await;

    // The user moves the branch to a worktree of their own.
    let managed = branch_worktree(&r, "feature");
    r.git(&["worktree", "remove", "--force", &managed.to_string_lossy()]);
    let own = r.data.join("my-feature");
    r.git(&["worktree", "add", "-q", &own.to_string_lossy(), "feature"]);

    let (orch, mut rx, mut events) = start(cfg()).await;
    say(&orch, "C-1", "two").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready")).await;
    let starts = started(&events, "orchestrator:C-1");
    let start = starts.last().expect("started");
    assert!(!start.1, "not loaded: OpenCode needs the same directory");
    assert_eq!(cwd_of(&events, "orchestrator:C-1"), canonical(&own));
    let prompts = prompts_to(&events, "orchestrator:C-1");
    assert!(prompts[0].contains("could not be restored"), "{prompts:?}");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_restart_also_brings_back_a_chat_with_an_open_group_whose_orchestrator_had_stopped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        {"match": "work", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "g"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "t", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "hold"}}},
            {"crash": 3}
        ]},
        {"match": "hello", "actions": [{"message": "hi"}]}
    ]});
    let impl_script = json!({"turns": [{"actions": ["wait_cancel"]}]});
    let run = |dir: PathBuf, first: bool| {
        let (o, i) = (orch_script.clone(), impl_script.clone());
        async move {
            let cfg = config(&dir, fake_harness(o), fake_harness(i), fake_harness(none()));
            let (orch, mut rx, mut events) = start(cfg).await;
            if first {
                let busy = new_chat(&orch, "main").await;
                let _idle = new_chat(&orch, "main").await;
                say(&orch, &busy, "work").await;
                wait(&mut rx, &mut events, |e| {
                    matches!(&e.body, ApiEventBody::SessionStopped { session, suspended: false } if session == "orchestrator:C-1")
                })
                .await;
            } else {
                wait(&mut rx, &mut events, |e| {
                    !started_of(e, "orchestrator:C-1").is_empty()
                })
                .await;
                settle(&mut rx, &mut events).await;
            }
            shutdown_and_check(orch, &events).await;
            while let Ok(e) = rx.try_recv() {
                events.push(e);
            }
            events
        }
    };
    run(dir.path().to_path_buf(), true).await;
    let events = run(dir.path().to_path_buf(), false).await;
    let sessions = started_sessions(&events);
    assert!(sessions.contains(&"orchestrator:C-1"), "{sessions:?}");
    assert!(
        !sessions.contains(&"orchestrator:C-2"),
        "the idle chat stays down: {sessions:?}"
    );
}

#[tokio::test]
async fn a_group_completes_when_the_main_clone_is_detached() {
    let r = TempRepo::new();
    r.git(&["checkout", "-q", "--detach"]);
    let (orch_script, impl_script) = planning_scripts();
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "main-work").await;
    wait(&mut rx, &mut events, |e| {
        is_domain(e, |d| {
            matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
        })
    })
    .await;
    // `main` is checked out nowhere, so Yhtye's worktree of it took the merge.
    assert_eq!(r.show("main", "main.txt"), "main.txt\n");
    assert_eq!(
        cwd_of(&events, "orchestrator:C-1"),
        canonical(&branch_worktree(&r, "main"))
    );
    assert_eq!(
        r.git(&["branch", "--show-current"]).trim(),
        "",
        "still detached"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_chat_whose_branch_disappeared_refuses_messages() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let script = json!({"turns": [{"match": "hello", "actions": [{"message": "hi"}]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "topic").await;
    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "hi")).await;
    // Deleted behind Yhtye's back (worktree first, git will not delete a checked-out branch).
    let wt = branch_worktree(&r, "topic");
    r.git(&["worktree", "remove", "--force", &wt.to_string_lossy()]);
    r.git(&["branch", "-D", "topic"]);

    // Sending is refused (`create_group` checks the same way).
    let err = orch
        .send_user_message(&chat, "make")
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("topic"), "{err}");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn group_numbers_skip_leftovers_of_a_wiped_database() {
    let r = TempRepo::new();
    // What a database that was wiped left in git: group and task branches, and
    // a worktree directory of a group.
    r.git(&["branch", "yhtye/G-3"]);
    r.git(&["branch", "yhtye/G-3-T-1"]);
    std::fs::create_dir_all(worktree_root(&r.data, PROJECT).join("G-5/_group")).expect("mkdir");
    let script = json!({"turns": [{"match": "hello", "actions": [
        {"mcp_call": {"tool": "create_group", "args": {"title": "work"}}},
        {"message": "planned"}
    ]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "planned")).await;
    let created: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Domain {
                event: DomainEvent::GroupCreated { group },
            } => Some(group.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(created, ["G-6"], "past G-3 and the G-5 directory");
    assert!(r.branch_exists("yhtye/G-6"));
    shutdown_and_check(orch, &events).await;
}

/// Git that fails to look at branches once a branch was created, so the chat
/// of a new branch cannot be made.
struct FailsAfterCreate {
    inner: yhtye_core::git::GitCli,
    created: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl yhtye_core::git::GitService for FailsAfterCreate {
    async fn current_branch(&self) -> Result<Option<String>, String> {
        self.inner.current_branch().await
    }
    async fn branch_exists(&self, branch: &str) -> Result<bool, String> {
        if self.created.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("git went away".into());
        }
        self.inner.branch_exists(branch).await
    }
    async fn resolve_branch_worktree(
        &self,
        branch: &str,
    ) -> Result<PathBuf, yhtye_core::git::BranchWorktreeError> {
        self.inner.resolve_branch_worktree(branch).await
    }
    async fn create_branch(
        &self,
        name: &str,
        from: Option<&str>,
    ) -> Result<PathBuf, yhtye_core::git::CreateBranchError> {
        let path = self.inner.create_branch(name, from).await?;
        self.created
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(path)
    }
    async fn discard_branch(&self, name: &str) -> Result<(), String> {
        self.created
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.inner.discard_branch(name).await
    }
    async fn run(&self, op: &yhtye_core::domain::GitOp) -> yhtye_core::domain::GitResult {
        self.inner.run(op).await
    }
}

#[tokio::test]
async fn a_branch_whose_chat_could_not_be_made_is_rolled_back_so_a_retry_works() {
    let r = TempRepo::new();
    let mut cfg = repo_config(&r, none(), none());
    cfg.git = std::sync::Arc::new(FailsAfterCreate {
        inner: r.git_cli(),
        created: false.into(),
    });
    let (orch, _rx, events) = start(cfg).await;
    orch.create_branch("topic", None)
        .await
        .expect_err("no chat without git");
    assert!(!r.branch_exists("topic"), "the branch was removed again");
    assert!(r.extra_worktrees().is_empty(), "and its worktree");
    // Retrying is not refused as "already exists".
    let again = orch.create_branch("topic", None).await;
    let text = format!("{again:?}");
    assert!(!text.contains("already exists"), "{text}");
    shutdown_and_check(orch, &events).await;
}
