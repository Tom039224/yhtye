//! Store tests: migrations on an empty database, state round trips through the
//! current-state tables, atomicity of a transition, and the session table.

use std::path::{Path, PathBuf};

use super::*;
use crate::acp::AgentEvent;
use crate::acp::schema::StopReason;
use crate::agents::AgentChoice;
use crate::api::{ApiEventBody, TextKind};
use crate::domain::{
    DomainEvent, Group, GroupStatus, Help, HelpKind, HelpSource, HelpState, InboxEntry, InboxItem,
    InboxKind, Role, Step, StepKind, StepStatus, Task, TaskKind, TaskStatus, Verdict,
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
    s.groups.push(Group {
        id: "G-1".into(),
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
        item: InboxItem::new(
            InboxKind::HelpRaised,
            &[("help_id", "H-1"), ("task", "T-1")],
            "stuck",
        ),
    });
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
            event: DomainEvent::InboxDelivered { up_to: 0 },
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
    assert_eq!(applied, 4);
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
            "events",
            "helps",
            "inbox",
            "projects",
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
    assert_eq!(applied, 4);
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
        item: InboxItem::user_message("hi"),
    });
    next.counters.inbox = 4;
    next.groups[0].status = GroupStatus::MergeBlocked;
    let events = [event(
        2,
        ApiEventBody::Domain {
            event: DomainEvent::InboxDelivered { up_to: 3 },
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
                event: DomainEvent::InboxDelivered { up_to: 3 },
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
        item: InboxItem::user_message("hello"),
    };
    let mut after = empty.clone();
    let domain = DomainEvent::InboxQueued { entry };
    after.apply(&domain);
    let events = [
        event(
            1,
            ApiEventBody::Prompted {
                session: "orchestrator".into(),
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
    assert_eq!(stored[0].session.as_deref(), Some("orchestrator"));
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
