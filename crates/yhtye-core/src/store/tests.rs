//! Store tests: migrations on an empty database, state round trips through the
//! current-state tables, atomicity of a transition, and the session table.

use std::path::{Path, PathBuf};

use super::*;
use crate::acp::AgentEvent;
use crate::acp::schema::StopReason;
use crate::agents::AgentChoice;
use crate::api::{ApiEventBody, TextKind};
use crate::domain::{
    Chat, DomainEvent, Group, GroupStatus, Help, HelpKind, HelpSource, HelpState, InboxEntry,
    InboxItem, InboxKind, Role, Step, StepKind, StepStatus, Task, TaskKind, TaskStatus, Verdict,
};

fn db(dir: &Path) -> PathBuf {
    db_path(&dir.join("data"))
}

fn event(seq: u64, body: ApiEventBody) -> ApiEvent {
    ApiEvent {
        seq,
        ts_ms: 1_000 + seq,
        project: "P-1".into(),
        live: body.is_live(),
        body,
    }
}

fn step(kind: StepKind, status: StepStatus) -> Step {
    Step {
        kind,
        instruction: Some(format!("do {kind:?}")),
        status,
        result: (status == StepStatus::Done).then(|| "ok".to_string()),
        verdict: (kind == StepKind::Review && status == StepStatus::Done)
            .then_some(Verdict::NeedsChanges),
        note: Some("a note".into()),
        nudges: 1,
    }
}

/// A state with every kind of row and most optional fields set.
fn rich_state() -> State {
    let mut s = State::new("P-1", DomainConfig::default());
    s.chats.push(Chat {
        id: "C-1".into(),
        worktree: "/repo".into(),
        title: Some("first chat".into()),
    });
    s.chats.push(Chat {
        id: "C-2".into(),
        worktree: "/data/worktrees/P-1/branches/feature-x".into(),
        title: None,
    });
    s.groups.push(Group {
        id: "G-1".into(),
        chat: "C-1".into(),
        title: "Group".into(),
        summary: Some("sum".into()),
        base_branch: "main".into(),
        group_branch: "yhtye/G-1".into(),
        status: GroupStatus::Active,
        finish_summary: None,
        finish_nudges: 2,
        detail: Some("d".into()),
    });
    let t1 = Task {
        id: "T-1".into(),
        group: "G-1".into(),
        title: "first".into(),
        kind: TaskKind::Code,
        depends_on: vec![],
        instruction: Some("go".into()),
        steps: vec![
            step(StepKind::Implement, StepStatus::Done),
            step(StepKind::Review, StepStatus::Done),
            step(StepKind::Implement, StepStatus::Running),
            step(StepKind::Done, StepStatus::Pending),
        ],
        current: 2,
        status: TaskStatus::Handling,
        review_rounds: 1,
        workdir: Some("/wt/T-1".into()),
        cancel_reason: None,
        agent: Some(AgentChoice::new("claude-code", Some("sonnet"))),
        review_agent: None,
    };
    let t2 = Task {
        id: "T-2".into(),
        group: "G-1".into(),
        title: "second".into(),
        kind: TaskKind::Investigate,
        depends_on: vec!["T-1".into()],
        instruction: None,
        steps: vec![step(StepKind::Done, StepStatus::Pending)],
        current: 0,
        status: TaskStatus::Pending,
        review_rounds: 0,
        workdir: None,
        cancel_reason: Some("r".into()),
        agent: None,
        review_agent: Some(AgentChoice::new("other", None)),
    };
    s.tasks = vec![t1, t2];
    s.helps.push(Help {
        id: "H-1".into(),
        task: "T-1".into(),
        step: 2,
        kind: HelpKind::Blocked,
        message: "stuck".into(),
        source: HelpSource::Agent {
            role: Role::Implementer,
        },
        state: HelpState::Open,
        agent_lost: true,
        reply: None,
    });
    s.inbox.push(InboxEntry {
        id: 3,
        chat: "C-2".into(),
        item: InboxItem::new(
            InboxKind::HelpRaised,
            &[("help_id", "H-1"), ("task", "T-1")],
            "stuck",
        ),
    });
    s.counters.chats = 2;
    s.counters.groups = 1;
    s.counters.tasks = 2;
    s.counters.helps = 1;
    s.counters.inbox = 3;
    s
}

async fn stored_rich(dir: &Path) -> (Store, State) {
    let store = Store::open(&db(dir)).await.expect("open");
    let empty = State::new("P-1", DomainConfig::default());
    store.create_project(&empty, dir).await.expect("create");
    let rich = rich_state();
    let events = [event(
        1,
        ApiEventBody::Domain {
            event: DomainEvent::InboxDelivered {
                chat: "C-2".into(),
                up_to: 0,
            },
        },
    )];
    store.commit(&events, &empty, &rich).await.expect("commit");
    (store, rich)
}

#[tokio::test]
async fn migrations_create_the_schema_on_an_empty_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db(dir.path())).await.expect("open");
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&store.pool)
        .await
        .expect("migrations table");
    assert_eq!(applied, 7);
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
    )
    .fetch_all(&store.pool)
    .await
    .expect("tables");
    assert_eq!(
        tables,
        [
            "agent_sessions",
            "agent_settings",
            "chats",
            "events",
            "helps",
            "inbox",
            "projects",
            "secret_env_names",
            "steps",
            "task_deps",
            "task_groups",
            "tasks"
        ]
    );
    assert_eq!(store.load_state("P-1").await.expect("load"), None);
    assert_eq!(store.last_seq("P-1").await.expect("seq"), 0);
    drop(store);
    // Opening again does not re-apply migrations.
    let again = Store::open(&db(dir.path())).await.expect("reopen");
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&again.pool)
        .await
        .expect("migrations table");
    assert_eq!(applied, 7);
}

#[tokio::test]
async fn state_round_trips_through_the_tables() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, rich) = stored_rich(dir.path()).await;
    assert_eq!(
        store.load_state("P-1").await.expect("load"),
        Some(rich.clone())
    );

    // A later transition rewrites only what changed; the result still matches.
    let mut next = rich.clone();
    next.tasks[0].status = TaskStatus::Running;
    next.tasks[0].steps.truncate(3);
    next.helps[0].state = HelpState::Answered;
    next.helps[0].reply = Some("try again".into());
    next.inbox.clear();
    next.inbox.push(InboxEntry {
        id: 4,
        chat: "C-1".into(),
        item: InboxItem::user_message("hi"),
    });
    next.counters.inbox = 4;
    next.groups[0].status = GroupStatus::MergeBlocked;
    let events = [event(
        2,
        ApiEventBody::Domain {
            event: DomainEvent::InboxDelivered {
                chat: "C-2".into(),
                up_to: 3,
            },
        },
    )];
    store.commit(&events, &rich, &next).await.expect("commit");
    assert_eq!(store.load_state("P-1").await.expect("load"), Some(next));
    assert_eq!(store.last_seq("P-1").await.expect("seq"), 2);
}

#[tokio::test]
async fn a_failed_transition_stores_neither_events_nor_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, rich) = stored_rich(dir.path()).await;
    // Fail in the middle of the transaction: after the events and the task row,
    // while writing its steps.
    sqlx::query(
        "CREATE TRIGGER crash BEFORE INSERT ON steps BEGIN SELECT RAISE(ABORT, 'simulated crash'); END",
    )
    .execute(&store.pool)
    .await
    .expect("trigger");
    let mut next = rich.clone();
    next.tasks[1].status = TaskStatus::Running;
    next.tasks[1].steps[0].status = StepStatus::Running;
    let events = [
        event(
            2,
            ApiEventBody::Domain {
                event: DomainEvent::TaskStatusChanged {
                    task: "T-2".into(),
                    status: TaskStatus::Running,
                },
            },
        ),
        event(
            3,
            ApiEventBody::Domain {
                event: DomainEvent::InboxDelivered {
                    chat: "C-2".into(),
                    up_to: 3,
                },
            },
        ),
    ];
    let err = store.commit(&events, &rich, &next).await;
    assert!(
        matches!(&err, Err(StoreError::Db(e)) if e.to_string().contains("simulated crash")),
        "{err:?}"
    );
    assert_eq!(store.last_seq("P-1").await.expect("seq"), 1);
    assert_eq!(
        store.load_state("P-1").await.expect("load"),
        Some(rich.clone())
    );

    sqlx::query("DROP TRIGGER crash")
        .execute(&store.pool)
        .await
        .expect("drop trigger");
    store.commit(&events, &rich, &next).await.expect("commit");
    assert_eq!(store.last_seq("P-1").await.expect("seq"), 3);
    assert_eq!(store.load_state("P-1").await.expect("load"), Some(next));
}

#[tokio::test]
async fn replay_rebuilds_the_state_from_domain_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db(dir.path())).await.expect("open");
    let empty = State::new("P-1", DomainConfig::default());
    store
        .create_project(&empty, dir.path())
        .await
        .expect("create");
    let entry = InboxEntry {
        id: 1,
        chat: "C-1".into(),
        item: InboxItem::user_message("hello"),
    };
    let mut after = empty.clone();
    let domain = DomainEvent::InboxQueued { entry };
    after.apply(&domain);
    let events = [
        event(
            1,
            ApiEventBody::Prompted {
                session: "orchestrator:C-1".into(),
                text: "x".into(),
            },
        ),
        event(2, ApiEventBody::Domain { event: domain }),
    ];
    store.commit(&events, &empty, &after).await.expect("commit");
    let replayed = store
        .replay_state("P-1", DomainConfig::default())
        .await
        .expect("replay");
    assert_eq!(replayed, after);
    let stored = store.events_after("P-1", 0, 10).await.expect("events");
    let kinds: Vec<&str> = stored.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["prompted", "domain.inbox_queued"]);
    assert_eq!(stored[0].session.as_deref(), Some("orchestrator:C-1"));
    assert_eq!(
        store
            .events_after("P-1", 1, 10)
            .await
            .expect("events")
            .len(),
        1
    );
}

fn session_started(seq: u64, key: &str, acp: &str) -> ApiEvent {
    event(
        seq,
        ApiEventBody::SessionStarted {
            session: key.into(),
            role: Role::Implementer,
            task: Some("T-1".into()),
            pid: Some(1),
            acp_session_id: acp.into(),
            resumed: false,
            agent: Some(AgentChoice::new("claude-code", Some(acp)).with_effort("high")),
            replaced: None,
            cwd: Some("/work/tree".into()),
        },
    )
}

#[tokio::test]
async fn session_events_keep_the_agent_sessions_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, _) = stored_rich(dir.path()).await;
    let key = "T-1/implementer";
    let events = [
        session_started(2, key, "acp-1"),
        event(
            3,
            ApiEventBody::Prompted {
                session: key.into(),
                text: "go".into(),
            },
        ),
        event(
            4,
            ApiEventBody::AgentText {
                session: key.into(),
                kind: TextKind::Message,
                text: "working".into(),
            },
        ),
        event(
            5,
            ApiEventBody::SessionStopped {
                session: key.into(),
                suspended: true,
            },
        ),
        session_started(6, "T-1/review-1", "acp-2"),
        event(
            7,
            ApiEventBody::Agent {
                session: "T-1/review-1".into(),
                event: AgentEvent::TurnEnded(Ok(StopReason::EndTurn)),
            },
        ),
        event(
            8,
            ApiEventBody::SessionStopped {
                session: "T-1/review-1".into(),
                suspended: false,
            },
        ),
    ];
    store.append(&events).await.expect("append");
    let sessions = store.sessions("P-1").await.expect("sessions");
    let summary: Vec<(&str, &str, SessionStatus, bool)> = sessions
        .iter()
        .map(|s| {
            (
                s.session_key.as_str(),
                s.acp_session_id.as_str(),
                s.status,
                s.turn_running,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("T-1/implementer", "acp-1", SessionStatus::Suspended, true),
            ("T-1/review-1", "acp-2", SessionStatus::Stopped, false),
        ]
    );
    assert!(sessions[0].status.is_resumable() && !sessions[1].status.is_resumable());
    let history = store.session_events("P-1", key).await.expect("history");
    let kinds: Vec<&str> = history.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "session_started",
            "prompted",
            "agent_text",
            "session_stopped"
        ]
    );

    // Restarting the same key replaces the stored ACP session.
    store
        .append(&[session_started(9, key, "acp-3")])
        .await
        .expect("append");
    let sessions = store.sessions("P-1").await.expect("sessions");
    assert_eq!(sessions[0].acp_session_id, "acp-3");
    assert_eq!(sessions[0].status, SessionStatus::Live);
}

#[tokio::test]
async fn sessions_record_the_agent_they_ran() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, _) = stored_rich(dir.path()).await;
    store
        .append(&[session_started(2, "T-1/implementer", "acp-1")])
        .await
        .expect("append");
    let sessions = store.sessions("P-1").await.expect("sessions");
    assert_eq!(
        sessions[0].agent,
        Some(AgentChoice::new("claude-code", Some("acp-1")).with_effort("high")),
        "the effort is stored with the session (migration 0004)"
    );
}

#[tokio::test]
async fn agent_settings_are_stored_per_scope_and_role() {
    use crate::agents::{AgentRole, Candidate, RoleSettings};
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db(dir.path())).await.expect("open");
    let a = RoleSettings::only(AgentChoice::new("claude-code", Some("haiku")));
    let b = RoleSettings {
        candidates: vec![
            AgentChoice::new("claude-code", Some("haiku")).into(),
            Candidate::from(AgentChoice::new("claude-code", Some("sonnet")).with_effort("high"))
                .with_note("hard bugs"),
            AgentChoice::new("other", None).into(),
        ],
        default: AgentChoice::new("other", None),
    };
    store
        .set_agent_settings(None, AgentRole::Reviewer, Some(&a), 1)
        .await
        .expect("global");
    store
        .set_agent_settings(Some("P-1"), AgentRole::Reviewer, Some(&b), 2)
        .await
        .expect("project");
    store
        .set_agent_settings(Some("P-1"), AgentRole::Implementer, Some(&a), 3)
        .await
        .expect("project");
    store
        .set_agent_settings(Some("P-1"), AgentRole::Reviewer, Some(&a), 4)
        .await
        .expect("replace");
    store
        .set_agent_settings(Some("P-1"), AgentRole::Implementer, None, 5)
        .await
        .expect("remove");
    let rows = store.agent_settings().await.expect("list");
    let summary: Vec<(Option<&str>, AgentRole, &RoleSettings)> = rows
        .iter()
        .map(|r| (r.project.as_deref(), r.role, &r.settings))
        .collect();
    assert_eq!(
        summary,
        [
            (None, AgentRole::Reviewer, &a),
            (Some("P-1"), AgentRole::Reviewer, &a),
        ]
    );
    let _ = b;
}

#[tokio::test]
async fn secret_names_are_stored_sorted_without_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db(dir.path())).await.expect("open");
    store.add_secret_name("B_KEY", 1).await.expect("add");
    store.add_secret_name("A_KEY", 2).await.expect("add");
    store.add_secret_name("A_KEY", 3).await.expect("again");
    assert_eq!(
        store.secret_names().await.expect("list"),
        ["A_KEY", "B_KEY"]
    );
    store.remove_secret_name("A_KEY").await.expect("remove");
    store
        .remove_secret_name("A_KEY")
        .await
        .expect("remove again");
    assert_eq!(store.secret_names().await.expect("list"), ["B_KEY"]);
}

#[tokio::test]
async fn chats_keep_their_times_from_the_event_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(&db(dir.path())).await.expect("open");
    let empty = State::new("P-1", DomainConfig::default());
    store
        .create_project(&empty, dir.path())
        .await
        .expect("create");
    let created = |seq, id: &str| {
        let chat = Chat {
            id: id.into(),
            worktree: "/repo".into(),
            title: None,
        };
        event(
            seq,
            ApiEventBody::Domain {
                event: DomainEvent::ChatCreated { chat },
            },
        )
    };
    let mut after = empty.clone();
    let events = [created(1, "C-1"), created(2, "C-2")];
    for e in &events {
        if let ApiEventBody::Domain { event } = &e.body {
            after.apply(event);
        }
    }
    store.commit(&events, &empty, &after).await.expect("commit");
    // C-1 is prompted later than C-2 was created; the title arrives on the way.
    let mut titled = after.clone();
    let title = DomainEvent::ChatTitled {
        chat: "C-1".into(),
        title: "hello".into(),
    };
    titled.apply(&title);
    let events = [
        event(
            3,
            ApiEventBody::Prompted {
                session: "orchestrator:C-1".into(),
                text: "x".into(),
            },
        ),
        event(4, ApiEventBody::Domain { event: title }),
    ];
    store
        .commit(&events, &after, &titled)
        .await
        .expect("commit");

    assert_eq!(store.load_state("P-1").await.expect("load"), Some(titled));
    let chats = store.chats("P-1").await.expect("chats");
    let summary: Vec<(&str, Option<&str>, u64, u64)> = chats
        .iter()
        .map(|c| {
            (
                c.id.as_str(),
                c.title.as_deref(),
                c.created_ms,
                c.last_used_ms,
            )
        })
        .collect();
    // Most recently used first.
    assert_eq!(
        summary,
        [
            ("C-1", Some("hello"), 1_001, 1_003),
            ("C-2", None, 1_002, 1_002)
        ]
    );
}

#[tokio::test]
async fn sessions_record_the_directory_they_started_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, _) = stored_rich(dir.path()).await;
    store
        .append(&[session_started(2, "orchestrator:C-1", "acp-1")])
        .await
        .expect("append");
    let sessions = store.sessions("P-1").await.expect("sessions");
    assert_eq!(sessions[0].cwd.as_deref(), Some("/work/tree"));
}

#[tokio::test]
async fn chats_are_stored_with_their_worktree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, _) = stored_rich(dir.path()).await;
    let listed = store.chats("P-1").await.expect("chats");
    let worktree_of = |id: &str| {
        listed
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.worktree.clone())
    };
    assert_eq!(worktree_of("C-1").as_deref(), Some("/repo"));
    assert_eq!(
        worktree_of("C-2").as_deref(),
        Some("/data/worktrees/P-1/branches/feature-x")
    );
}

#[tokio::test]
async fn a_changed_base_branch_is_stored_for_the_group() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, rich) = stored_rich(dir.path()).await;
    let moved = DomainEvent::GroupBaseChanged {
        group: "G-1".into(),
        from: "main".into(),
        to: "trunk".into(),
    };
    let mut after = rich.clone();
    after.apply(&moved);
    let events = [event(2, ApiEventBody::Domain { event: moved })];
    store.commit(&events, &rich, &after).await.expect("commit");

    let loaded = store.load_state("P-1").await.expect("load").expect("state");
    assert_eq!(loaded, after);
    assert_eq!(loaded.group("G-1").expect("G-1").base_branch, "trunk");
}

async fn count(store: &Store, table: &'static str) -> i64 {
    let sql = match table {
        "chats" => "SELECT COUNT(*) FROM chats",
        "task_groups" => "SELECT COUNT(*) FROM task_groups",
        "tasks" => "SELECT COUNT(*) FROM tasks",
        "steps" => "SELECT COUNT(*) FROM steps",
        "task_deps" => "SELECT COUNT(*) FROM task_deps",
        "helps" => "SELECT COUNT(*) FROM helps",
        "inbox" => "SELECT COUNT(*) FROM inbox",
        _ => panic!("no count for {table}"),
    };
    sqlx::query_scalar(sql)
        .fetch_one(&store.pool)
        .await
        .expect("count")
}

fn group_of(id: &str, chat: &str) -> Group {
    Group {
        id: id.into(),
        chat: chat.into(),
        title: id.into(),
        summary: None,
        base_branch: "main".into(),
        group_branch: format!("yhtye/{id}"),
        status: GroupStatus::Done,
        finish_summary: None,
        finish_nudges: 0,
        detail: None,
    }
}

/// Commits the deletion of `chat` as the runtime would: the event, the new state.
async fn commit_deletion(store: &Store, seq: u64, before: &State, chat: &str) -> State {
    let deleted = DomainEvent::ChatDeleted { chat: chat.into() };
    let mut after = before.clone();
    after.apply(&deleted);
    let events = [event(seq, ApiEventBody::Domain { event: deleted })];
    store.commit(&events, before, &after).await.expect("commit");
    after
}

#[tokio::test]
async fn a_deleted_chat_takes_its_rows_and_sessions_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, rich) = stored_rich(dir.path()).await;
    // C-1's orchestrator, its task's agent, and C-2's orchestrator.
    let mut c2_orchestrator = session_started(4, "orchestrator:C-2", "acp-3");
    if let ApiEventBody::SessionStarted { task, .. } = &mut c2_orchestrator.body {
        *task = None;
    }
    let mut c1_orchestrator = session_started(3, "orchestrator:C-1", "acp-2");
    if let ApiEventBody::SessionStarted { task, .. } = &mut c1_orchestrator.body {
        *task = None;
    }
    store
        .append(&[
            session_started(2, "T-1/implementer", "acp-1"),
            c1_orchestrator,
            c2_orchestrator,
        ])
        .await
        .expect("append");
    assert_eq!(store.sessions("P-1").await.expect("sessions").len(), 3);

    let after = commit_deletion(&store, 5, &rich, "C-1").await;

    assert_eq!(store.load_state("P-1").await.expect("load"), Some(after));
    let counts = [
        count(&store, "chats").await,
        count(&store, "task_groups").await,
        count(&store, "tasks").await,
        count(&store, "steps").await,
        count(&store, "task_deps").await,
        count(&store, "helps").await,
        count(&store, "inbox").await,
    ];
    assert_eq!(
        counts,
        [1, 0, 0, 0, 0, 0, 1],
        "only C-2 and its inbox remain"
    );
    let sessions: Vec<String> = store
        .sessions("P-1")
        .await
        .expect("sessions")
        .into_iter()
        .map(|s| s.session_key)
        .collect();
    assert_eq!(sessions, ["orchestrator:C-2"]);
    let chats = store.chats("P-1").await.expect("chats");
    assert_eq!(
        chats.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["C-2"]
    );
    // The log is append-only: the deletion is one more event, nothing is removed.
    assert_eq!(store.last_seq("P-1").await.expect("seq"), 5);
}

#[tokio::test]
async fn rows_added_after_a_deletion_keep_their_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, mut rich) = stored_rich(dir.path()).await;
    // C-2 gets two finished groups after C-1's G-1 ("G-10" sorts before "G-9" as text).
    let mut with_more = rich.clone();
    with_more.groups.push(group_of("G-8", "C-2"));
    with_more.groups.push(group_of("G-9", "C-2"));
    let mut t3 = rich.tasks[1].clone();
    t3.id = "T-3".into();
    t3.group = "G-8".into();
    t3.depends_on.clear();
    with_more.tasks.push(t3);
    let mut h2 = rich.helps[0].clone();
    h2.id = "H-2".into();
    h2.task = "T-3".into();
    with_more.helps.push(h2);
    store.commit(&[], &rich, &with_more).await.expect("commit");
    rich = with_more;

    let mut after = commit_deletion(&store, 2, &rich, "C-1").await;
    // A new chat and group land after the survivors, not among them.
    let created = DomainEvent::ChatCreated {
        chat: Chat {
            id: "C-3".into(),
            worktree: "/repo".into(),
            title: None,
        },
    };
    let mut next = after.clone();
    next.apply(&created);
    next.groups.push(group_of("G-10", "C-3"));
    let events = [event(3, ApiEventBody::Domain { event: created })];
    store.commit(&events, &after, &next).await.expect("commit");
    after = next;

    let loaded = store.load_state("P-1").await.expect("load").expect("state");
    let ids = |v: Vec<&str>| v.into_iter().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        ids(loaded.chats.iter().map(|c| c.id.as_str()).collect()),
        ["C-2", "C-3"]
    );
    assert_eq!(
        ids(loaded.groups.iter().map(|g| g.id.as_str()).collect()),
        ["G-8", "G-9", "G-10"]
    );
    assert_eq!(
        ids(loaded.tasks.iter().map(|t| t.id.as_str()).collect()),
        ["T-3"]
    );
    assert_eq!(loaded, after);
}

#[tokio::test]
async fn a_renamed_chat_is_stored_with_its_new_title() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (store, rich) = stored_rich(dir.path()).await;
    let title = DomainEvent::ChatTitled {
        chat: "C-2".into(),
        title: "my name".into(),
    };
    let mut after = rich.clone();
    after.apply(&title);
    let events = [event(2, ApiEventBody::Domain { event: title })];
    store.commit(&events, &rich, &after).await.expect("commit");

    let chats = store.chats("P-1").await.expect("chats");
    let renamed = chats.iter().find(|c| c.id == "C-2").expect("C-2");
    assert_eq!(renamed.title.as_deref(), Some("my name"));
    assert_eq!(store.load_state("P-1").await.expect("load"), Some(after));
}
