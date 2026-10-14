//! Contract samples: one JSON value for every variant of every API enum,
//! written by the Rust types themselves (commands are read with serde first,
//! so their JSON is what Rust accepts and writes back). The Kotlin tests read
//! each one and must write the same JSON back (`ContractTest.kt`).
//!
//! `contract_samples_cover_every_variant_and_match_the_model` fails when a
//! variant has no sample: add one here when you add a variant.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::acp::{AgentError, AgentEvent, AgentInfo, AgentOutput};
use crate::agents::{
    AgentChoice, AgentSettings, AgentSettingsLayer, EffortOption, HarnessDetection, HarnessInfo,
    HarnessModels, HarnessRequirement, ModelEfforts, ModelOption, PathSource, RoleSettings,
};
use crate::api::{
    AgentSettingsView, ApiCommand, ApiError, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse,
    LoggedEvent, ProjectInfo, SettingsKind, Snapshot, TextKind, WsReply, WsRequest,
    WsServerMessage,
};
use crate::domain::{DomainEvent, Role, ToolError};
use crate::git::{GitBranch, GitCommit, GitOverview, GitWorktree};
use crate::mcp::ToolCallRecord;
use crate::remote::{
    ClientKind, Clients, PairRequest, PairResponse, PairingCode, RemoteDevice, RemoteError,
    RemoteStatus,
};
use crate::store::{ChatInfo, SessionRecord, SessionStatus};
use crate::usage::{UsageReport, UsageWindow, UsageWindowKind};

/// One JSON value of type `ty` (the Kotlin name of a definition).
pub struct Sample {
    pub ty: &'static str,
    pub name: String,
    pub json: Value,
}

pub fn all() -> Vec<Sample> {
    let mut out = Vec::new();
    out.extend(commands());
    out.extend(requests());
    out.extend(responses());
    out.extend(errors());
    out.extend(events());
    out.extend(server_messages());
    out.extend(remote_http());
    out
}

fn de<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"))
}

fn sample(ty: &'static str, name: impl Into<String>, value: &impl Serialize) -> Sample {
    Sample {
        ty,
        name: name.into(),
        json: serde_json::to_value(value).expect("serialize"),
    }
}

/// Every command: read from JSON by Rust, then written by Rust.
fn commands() -> Vec<Sample> {
    let role_settings = json!({
        "candidates": [
            {"harness": "claude-code", "model": "opus", "effort": "high", "note": "hard tasks"},
            {"harness": "codex", "model": null, "effort": null, "note": ""}
        ],
        "default": {"harness": "claude-code", "model": "opus", "effort": "high"}
    });
    let cases = [
        ("ping", json!({"type": "ping"})),
        ("list_projects", json!({"type": "list_projects"})),
        (
            "open_project",
            json!({"type": "open_project", "path": "/home/u/repo"}),
        ),
        (
            "get_snapshot",
            json!({"type": "get_snapshot", "project": "repo"}),
        ),
        (
            "list_events",
            json!({"type": "list_events", "project": "repo", "after_seq": 12, "limit": 500}),
        ),
        (
            "list_events/no_limit",
            json!({"type": "list_events", "project": "repo", "after_seq": 0}),
        ),
        (
            "send_user_message",
            json!({"type": "send_user_message", "project": "repo", "chat": "C-1",
                "text": "Fix \"the\" bug\nin $HOME \\ path ✓"}),
        ),
        (
            "open_chat",
            json!({"type": "open_chat", "project": "repo", "chat": "C-1"}),
        ),
        (
            "cancel_orchestrator_turn",
            json!({"type": "cancel_orchestrator_turn", "project": "repo", "chat": "C-1"}),
        ),
        (
            "compact_chat",
            json!({"type": "compact_chat", "project": "repo", "chat": "C-1"}),
        ),
        (
            "list_chats",
            json!({"type": "list_chats", "project": "repo"}),
        ),
        (
            "create_chat",
            json!({"type": "create_chat", "project": "repo", "branch": "feature/x"}),
        ),
        (
            "create_chat/worktree",
            json!({"type": "create_chat", "project": "repo", "worktree": "/home/u/wt"}),
        ),
        (
            "rename_chat",
            json!({"type": "rename_chat", "project": "repo", "chat": "C-1", "title": "Docs"}),
        ),
        (
            "delete_chat",
            json!({"type": "delete_chat", "project": "repo", "chat": "C-2"}),
        ),
        (
            "create_branch",
            json!({"type": "create_branch", "project": "repo", "name": "feature/y", "from": "main"}),
        ),
        (
            "create_branch/no_from",
            json!({"type": "create_branch", "project": "repo", "name": "feature/z"}),
        ),
        (
            "cancel_task",
            json!({"type": "cancel_task", "project": "repo", "task": "T-3", "reason": "wrong"}),
        ),
        (
            "cancel_group",
            json!({"type": "cancel_group", "project": "repo", "group": "G-1"}),
        ),
        (
            "retry_group_merge",
            json!({"type": "retry_group_merge", "project": "repo", "group": "G-1"}),
        ),
        (
            "get_git_overview",
            json!({"type": "get_git_overview", "project": "repo", "limit": 120}),
        ),
        ("get_usage", json!({"type": "get_usage", "refresh": true})),
        ("get_usage/default", json!({"type": "get_usage"})),
        (
            "get_agent_settings",
            json!({"type": "get_agent_settings", "project": "repo"}),
        ),
        (
            "get_agent_settings/global",
            json!({"type": "get_agent_settings"}),
        ),
        (
            "set_agent_settings/orchestrator",
            json!({"type": "set_agent_settings", "role": "orchestrator", "settings": role_settings}),
        ),
        (
            "set_agent_settings/implementer",
            json!({"type": "set_agent_settings", "project": "repo", "role": "implementer",
                "settings": null}),
        ),
        (
            "set_agent_settings/investigator",
            json!({"type": "set_agent_settings", "role": "investigator", "settings": null}),
        ),
        (
            "set_agent_settings/reviewer",
            json!({"type": "set_agent_settings", "role": "reviewer", "settings": role_settings,
                "expected_revision": 1_790_766_538_443_u64}),
        ),
        (
            "list_harness_models",
            json!({"type": "list_harness_models", "harness": "claude-code", "refresh": false}),
        ),
        (
            "list_model_efforts",
            json!({"type": "list_model_efforts", "harness": "codex", "model": "gpt-5"}),
        ),
        ("get_harnesses", json!({"type": "get_harnesses"})),
        ("detect_harnesses", json!({"type": "detect_harnesses"})),
        (
            "set_harness_path",
            json!({"type": "set_harness_path", "harness": "codex", "path": "/usr/bin/codex"}),
        ),
        (
            "set_harness_path/auto",
            json!({"type": "set_harness_path", "harness": "codex", "path": null}),
        ),
        ("list_secret_env", json!({"type": "list_secret_env"})),
        (
            "set_secret_env",
            json!({"type": "set_secret_env", "name": "OPENROUTER_API_KEY", "value": "sk-1"}),
        ),
        (
            "delete_secret_env",
            json!({"type": "delete_secret_env", "name": "OPENROUTER_API_KEY"}),
        ),
        ("get_remote_status", json!({"type": "get_remote_status"})),
        (
            "set_remote_settings",
            json!({"type": "set_remote_settings", "enabled": true, "port": 1423}),
        ),
        (
            "set_remote_settings/off",
            json!({"type": "set_remote_settings", "enabled": false}),
        ),
        (
            "create_pairing_code",
            json!({"type": "create_pairing_code"}),
        ),
        (
            "list_remote_devices",
            json!({"type": "list_remote_devices"}),
        ),
        (
            "revoke_remote_device",
            json!({"type": "revoke_remote_device", "device": "dev-0123456789abcdef"}),
        ),
        ("list_clients", json!({"type": "list_clients"})),
    ];
    cases
        .into_iter()
        .map(|(name, v)| {
            let cmd: ApiCommand = de(v);
            let s = sample("ApiCommand", name, &cmd);
            assert_eq!(de::<ApiCommand>(s.json.clone()), cmd, "{name}: round trip");
            s
        })
        .collect()
}

fn requests() -> Vec<Sample> {
    let cases = [
        ("ping", json!({"id": 1, "cmd": {"type": "ping"}})),
        (
            "list_events",
            json!({"id": 9_007_199_254_740_993_u64, "cmd": {"type": "list_events",
                "project": "repo", "after_seq": 3}}),
        ),
    ];
    cases
        .into_iter()
        .map(|(name, v)| {
            let req: WsRequest = de(v);
            sample("WsRequest", name, &req)
        })
        .collect()
}

fn choice() -> AgentChoice {
    de(json!({"harness": "claude-code", "model": "opus", "effort": "high"}))
}

fn role_settings() -> RoleSettings {
    de(json!({
        "candidates": [
            {"harness": "claude-code", "model": "opus", "effort": "high", "note": "hard tasks"},
            {"harness": "opencode", "model": null, "effort": null, "note": ""}
        ],
        "default": {"harness": "claude-code", "model": "opus", "effort": "high"}
    }))
}

fn project_info(open: bool) -> ProjectInfo {
    ProjectInfo {
        id: "repo".into(),
        name: "repo".into(),
        path: "/home/u/repo".into(),
        open,
        running: open,
    }
}

fn chat_info(id: &str, title: Option<&str>) -> ChatInfo {
    ChatInfo {
        id: id.into(),
        worktree: "/home/u/repo".into(),
        title: title.map(str::to_string),
        created_ms: 1_790_766_538_443,
        last_used_ms: 1_790_766_600_000,
    }
}

fn session_record(key: &str, role: &str, status: SessionStatus, lost: bool) -> SessionRecord {
    SessionRecord {
        session_key: key.into(),
        role: de(json!(role)),
        task: (role != "orchestrator").then(|| "T-1".into()),
        acp_session_id: format!("acp-{key}"),
        status,
        turn_running: status == SessionStatus::Live,
        agent: Some(choice()),
        cwd: Some("/home/u/repo".into()),
        lost_acp_session_id: lost.then(|| "acp-old".into()),
    }
}

/// A state with every group status, step kind and status, verdict, task
/// kind, help state and source.
fn state() -> Value {
    let group = |id: &str, status: &str| {
        json!({"id": id, "chat": "C-1", "title": format!("Group {id}"), "summary": null,
            "base_branch": "main", "group_branch": format!("yhtye/{id}"), "status": status,
            "finish_summary": null, "detail": null, "finish_nudges": 0})
    };
    json!({
        "project": "repo",
        "config": {"max_review_rounds": 2},
        "chats": [
            {"id": "C-1", "worktree": "/home/u/repo", "title": "Docs"},
            {"id": "C-2", "worktree": "/home/u/wt", "title": null}
        ],
        "groups": [
            group("G-1", "active"), group("G-2", "finishing"), group("G-3", "done"),
            group("G-4", "cancelled"),
            {"id": "G-5", "chat": "C-2", "title": "Merge", "summary": "a summary",
                "base_branch": "main", "group_branch": "yhtye/G-5", "status": "merge_blocked",
                "finish_summary": "done", "detail": "conflict in a.rs", "finish_nudges": 1}
        ],
        "tasks": [
            {"id": "T-1", "group": "G-1", "title": "Implement", "kind": "code",
                "depends_on": [], "instruction": "Do it", "current": 1, "status": "running",
                "review_rounds": 0, "workdir": "/home/u/wt/T-1", "cancel_reason": null,
                "agent": {"harness": "codex", "model": null, "effort": null},
                "review_agent": {"harness": "claude-code", "model": "opus", "effort": null},
                "steps": [
                    {"kind": "implement", "instruction": null, "status": "done",
                        "result": "ok", "verdict": null, "note": "first", "nudges": 1},
                    {"kind": "review", "instruction": "Review it", "status": "running",
                        "result": null, "verdict": null, "note": null, "nudges": 0},
                    {"kind": "checkpoint", "instruction": null, "status": "pending",
                        "result": null, "verdict": null, "note": null, "nudges": 0},
                    {"kind": "done", "instruction": null, "status": "pending",
                        "result": null, "verdict": null, "note": null, "nudges": 0}
                ]},
            {"id": "T-2", "group": "G-1", "title": "Look", "kind": "investigate",
                "depends_on": ["T-1"], "instruction": null, "current": 1, "status": "done",
                "review_rounds": 1, "workdir": null, "cancel_reason": null,
                "steps": [
                    {"kind": "review", "instruction": null, "status": "done",
                        "result": "fine", "verdict": "approve", "note": null, "nudges": 0},
                    {"kind": "review", "instruction": null, "status": "done",
                        "result": "fix", "verdict": "needs_changes", "note": null, "nudges": 0}
                ]}
        ],
        "helps": [
            {"id": "H-1", "task": "T-1", "step": 0, "kind": "blocked", "message": "stuck",
                "source": {"by": "agent", "role": "implementer"}, "state": "open",
                "agent_lost": false, "reply": null},
            {"id": "H-2", "task": "T-1", "step": 1, "kind": "merge_conflict", "message": "x",
                "source": {"by": "yhtye"}, "state": "answered", "agent_lost": true,
                "reply": "go on"},
            {"id": "H-3", "task": "T-2", "step": 0, "kind": "question", "message": "?",
                "source": {"by": "agent", "role": "reviewer"}, "state": "closed",
                "agent_lost": false, "reply": null}
        ],
        "inbox": [
            {"id": 4, "chat": "C-1", "item": {"kind": "user_message",
                "attrs": [["from", "user"], ["chat", "C-1"]], "body": "hello"}}
        ],
        "counters": {"chats": 2, "groups": 5, "tasks": 2, "helps": 3, "inbox": 4}
    })
}

fn snapshot() -> Snapshot {
    Snapshot {
        seq: 42,
        state: de(state()),
        sessions: vec![
            session_record(
                "orchestrator-C-1",
                "orchestrator",
                SessionStatus::Live,
                false,
            ),
            session_record("T-1/0", "implementer", SessionStatus::Stopped, false),
            session_record("T-1/1", "reviewer", SessionStatus::Suspended, true),
            session_record("T-2/0", "implementer", SessionStatus::Interrupted, false),
        ],
        starting: vec!["orchestrator:C-2".into()],
        chats: vec![chat_info("C-1", Some("Docs")), chat_info("C-2", None)],
    }
}

fn harness_detections() -> Vec<HarnessDetection> {
    let detection = |id: &str, source: PathSource, installed: bool| HarnessDetection {
        id: id.into(),
        label: id.to_uppercase(),
        installed,
        resolved_path: installed.then(|| format!("/usr/bin/{id}")),
        path_source: source,
        override_path: (source == PathSource::Override).then(|| format!("/opt/{id}")),
        override_error: None,
        requirements: vec![HarnessRequirement {
            command: id.into(),
            found: installed.then(|| format!("/usr/bin/{id}")),
        }],
    };
    let mut bad = detection("devin", PathSource::None, false);
    bad.override_error = Some("not an executable".into());
    vec![
        detection("claude-code", PathSource::Override, true),
        detection("codex", PathSource::Path, true),
        detection("grok", PathSource::KnownDir, true),
        bad,
    ]
}

fn effort(value: &str) -> EffortOption {
    EffortOption {
        value: value.into(),
        name: value.to_uppercase(),
        description: (value == "high").then(|| "Thinks longer".into()),
    }
}

fn remote_status(running: bool) -> RemoteStatus {
    RemoteStatus {
        enabled: true,
        port: 1423,
        running,
        addresses: if running {
            vec!["100.64.0.1:1423".into(), "[fd7a:115c:a1e0::1]:1423".into()]
        } else {
            vec![]
        },
        ip: running.then(|| "100.64.0.1".into()),
        dns_name: running.then(|| "pc.tailnet.ts.net".into()),
        server_id: "0123456789abcdef0123456789abcdef".into(),
        error: (!running).then(|| "Tailscale is not running".into()),
        pairing_expires_at_ms: running.then_some(1_790_766_838_443),
    }
}

fn devices() -> Vec<RemoteDevice> {
    vec![
        RemoteDevice {
            id: "dev-0123456789abcdef".into(),
            name: "Pixel 9".into(),
            kind: ClientKind::Android,
            created_ms: 1_790_766_538_443,
            last_seen_ms: Some(1_790_766_600_000),
            connected: true,
        },
        RemoteDevice {
            id: "dev-fedcba9876543210".into(),
            name: "Tablet".into(),
            kind: ClientKind::Android,
            created_ms: 1_790_766_538_443,
            last_seen_ms: None,
            connected: false,
        },
        RemoteDevice {
            id: "dev-0f1e2d3c4b5a6978".into(),
            name: "Workstation".into(),
            kind: ClientKind::Desktop,
            created_ms: 1_790_766_538_443,
            last_seen_ms: Some(1_790_766_650_000),
            connected: true,
        },
    ]
}

fn clients() -> Clients {
    Clients::new("pc".into(), &devices())
}

fn responses() -> Vec<Sample> {
    let settings = AgentSettings {
        orchestrator: role_settings(),
        implementer: role_settings(),
        investigator: role_settings(),
        reviewer: role_settings(),
    };
    let layer = AgentSettingsLayer {
        orchestrator: Some(role_settings()),
        ..AgentSettingsLayer::default()
    };
    let view = AgentSettingsView {
        harnesses: vec![HarnessInfo {
            id: "claude-code".into(),
            label: "Claude Code".into(),
            requires_model: false,
        }],
        builtin: choice(),
        global: layer.clone(),
        project: Some("repo".into()),
        project_layer: Some(AgentSettingsLayer::default()),
        effective: settings,
        revision: 1_790_766_538_443,
    };
    let global_view = AgentSettingsView {
        project: None,
        project_layer: None,
        ..view.clone()
    };
    let logged = |seq: u64, body: &ApiEventBody| LoggedEvent {
        seq,
        ts_ms: 1_790_766_538_443,
        project: "repo".into(),
        live: false,
        body: serde_json::to_value(body).expect("body"),
    };
    let git = GitOverview {
        head: Some("main".into()),
        head_sha: Some("a1b2c3d".into()),
        branches: vec![GitBranch {
            name: "main".into(),
            sha: "a1b2c3d".into(),
        }],
        worktrees: vec![
            GitWorktree {
                path: "/home/u/repo".into(),
                branch: Some("main".into()),
                head_sha: Some("a1b2c3d".into()),
                is_main: true,
                missing: false,
            },
            GitWorktree {
                path: "/home/u/gone".into(),
                branch: None,
                head_sha: None,
                is_main: false,
                missing: true,
            },
        ],
        commits: vec![GitCommit {
            sha: "a1b2c3d".into(),
            parents: vec!["0f0f0f0".into(), "1e1e1e1".into()],
            branches: vec!["main".into()],
            subject: "Merge yhtye/G-1".into(),
            ts_ms: 1_790_766_538_000,
        }],
        truncated: false,
    };
    let usage = UsageReport {
        plan: Some("Max".into()),
        windows: vec![
            UsageWindow {
                kind: UsageWindowKind::FiveHour,
                label: "5-hour limit".into(),
                percent: 42.5,
                resets_at_ms: Some(1_790_770_000_000),
            },
            UsageWindow {
                kind: UsageWindowKind::Week,
                label: "Weekly · all models".into(),
                percent: 7.0,
                resets_at_ms: None,
            },
            UsageWindow {
                kind: UsageWindowKind::Other,
                label: "Weekly · Opus".into(),
                percent: 100.0,
                resets_at_ms: None,
            },
        ],
        fetched_at_ms: 1_790_766_538_443,
    };
    let models = HarnessModels {
        harness: "claude-code".into(),
        models: vec![
            ModelOption {
                value: "opus".into(),
                name: "Opus".into(),
                description: Some("Most capable".into()),
                efforts: Some(vec![effort("low"), effort("high")]),
            },
            ModelOption {
                value: "haiku".into(),
                name: "Haiku".into(),
                description: None,
                efforts: None,
            },
        ],
        current: Some("opus".into()),
        fetched_at_ms: 1_790_766_538_443,
    };
    let efforts = ModelEfforts {
        harness: "codex".into(),
        model: "gpt-5".into(),
        efforts: vec![effort("medium")],
    };
    let prompted = ApiEventBody::Prompted {
        session: "orchestrator-C-1".into(),
        text: "hello".into(),
    };
    let cases = [
        (
            "pong",
            ApiResponse::Pong {
                host: "pc".into(),
                version: "0.2.1".into(),
            },
        ),
        (
            "projects",
            ApiResponse::Projects {
                projects: vec![project_info(true), project_info(false)],
            },
        ),
        (
            "project",
            ApiResponse::Project {
                project: project_info(true),
            },
        ),
        (
            "snapshot",
            ApiResponse::Snapshot {
                snapshot: snapshot(),
            },
        ),
        (
            "events",
            ApiResponse::Events {
                events: vec![logged(1, &prompted), logged(2, &domain(chat_created()))],
                more: true,
            },
        ),
        ("accepted", ApiResponse::Accepted),
        (
            "chats",
            ApiResponse::Chats {
                chats: vec![chat_info("C-1", Some("Docs"))],
            },
        ),
        (
            "chat",
            ApiResponse::Chat {
                chat: chat_info("C-2", None),
            },
        ),
        ("chat_opened", ApiResponse::ChatOpened { starting: true }),
        ("git_overview", ApiResponse::GitOverview { git }),
        ("usage", ApiResponse::Usage { usage }),
        (
            "agent_settings",
            ApiResponse::AgentSettings {
                settings: Box::new(view),
            },
        ),
        (
            "agent_settings/global",
            ApiResponse::AgentSettings {
                settings: Box::new(global_view),
            },
        ),
        ("harness_models", ApiResponse::HarnessModels { models }),
        ("model_efforts", ApiResponse::ModelEfforts { efforts }),
        (
            "secret_env",
            ApiResponse::SecretEnv {
                names: vec!["OPENROUTER_API_KEY".into()],
            },
        ),
        (
            "harnesses",
            ApiResponse::Harnesses {
                harnesses: harness_detections(),
            },
        ),
        (
            "remote_status",
            ApiResponse::RemoteStatus {
                status: remote_status(true),
            },
        ),
        (
            "remote_status/stopped",
            ApiResponse::RemoteStatus {
                status: remote_status(false),
            },
        ),
        (
            "pairing_code",
            ApiResponse::PairingCode {
                pairing: PairingCode {
                    code: "ABCD2345".into(),
                    expires_at_ms: 1_790_766_838_443,
                    uri: "yhtye://pair?v=1&host=pc.tailnet.ts.net&ip=100.64.0.1&port=1423\
                          &code=ABCD2345&sid=0123456789abcdef0123456789abcdef"
                        .into(),
                },
            },
        ),
        (
            "remote_devices",
            ApiResponse::RemoteDevices { devices: devices() },
        ),
        ("clients", ApiResponse::Clients { clients: clients() }),
        (
            "clients/no_devices",
            ApiResponse::Clients {
                clients: Clients::new("pc".into(), &[]),
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, r)| sample("ApiResponse", name, &r))
        .collect()
}

fn errors() -> Vec<Sample> {
    [
        ApiErrorCode::InvalidArgument,
        ApiErrorCode::NotFound,
        ApiErrorCode::InvalidState,
        ApiErrorCode::Conflict,
        ApiErrorCode::Forbidden,
        ApiErrorCode::Unavailable,
        ApiErrorCode::Internal,
    ]
    .into_iter()
    .map(|code| {
        let e = ApiError::new(code, format!("{code:?} happened"));
        let name = serde_json::to_value(code).expect("code");
        sample("ApiError", name.as_str().expect("str"), &e)
    })
    .collect()
}

fn domain(event: Value) -> ApiEventBody {
    ApiEventBody::Domain {
        event: de::<DomainEvent>(event),
    }
}

fn chat_created() -> Value {
    json!({"type": "chat_created", "chat": {"id": "C-1", "worktree": "/home/u/repo", "title": null}})
}

/// Every domain event, every task status, help kind and inbox kind.
fn domain_events() -> Vec<(String, Value)> {
    let step = json!([{"kind": "implement"}, {"kind": "review", "instruction": "Check it"}]);
    let state = state();
    let mut out: Vec<(String, Value)> = [
        ("chat_created", chat_created()),
        (
            "chat_titled",
            json!({"type": "chat_titled", "chat": "C-1", "title": "Docs"}),
        ),
        (
            "chat_deleted",
            json!({"type": "chat_deleted", "chat": "C-2"}),
        ),
        (
            "group_created",
            json!({"type": "group_created", "group": state["groups"][4]}),
        ),
        (
            "group_base_changed",
            json!({"type": "group_base_changed", "group": "G-1", "from": "main", "to": "dev"}),
        ),
        (
            "group_finishing",
            json!({"type": "group_finishing", "group": "G-1", "summary": "All done"}),
        ),
        (
            "group_finish_reminded",
            json!({"type": "group_finish_reminded", "group": "G-1"}),
        ),
        (
            "group_merge_finished",
            json!({"type": "group_merge_finished", "group": "G-1", "ok": false,
                "detail": "conflict"}),
        ),
        (
            "group_cancelled",
            json!({"type": "group_cancelled", "group": "G-1", "reason": "user"}),
        ),
        (
            "task_created",
            json!({"type": "task_created", "task": state["tasks"][0]}),
        ),
        (
            "task_created/minimal",
            json!({"type": "task_created", "task": state["tasks"][1]}),
        ),
        (
            "instruction_set",
            json!({"type": "instruction_set", "task": "T-1", "instruction": "Do it"}),
        ),
        (
            "task_cancelled",
            json!({"type": "task_cancelled", "task": "T-1", "reason": "obsolete"}),
        ),
        (
            "workspace_ready",
            json!({"type": "workspace_ready", "task": "T-1", "path": "/home/u/wt/T-1"}),
        ),
        (
            "step_started",
            json!({"type": "step_started", "task": "T-1", "step": 0, "note": "again"}),
        ),
        (
            "step_started/no_note",
            json!({"type": "step_started", "task": "T-1", "step": 1, "note": null}),
        ),
        (
            "step_completed",
            json!({"type": "step_completed", "task": "T-1", "step": 1, "result": "ok",
                "verdict": "approve"}),
        ),
        (
            "step_completed/needs_changes",
            json!({"type": "step_completed", "task": "T-1", "step": 1, "result": "no",
                "verdict": "needs_changes"}),
        ),
        (
            "step_completed/no_verdict",
            json!({"type": "step_completed", "task": "T-1", "step": 0, "result": "ok",
                "verdict": null}),
        ),
        (
            "step_reset",
            json!({"type": "step_reset", "task": "T-1", "step": 1}),
        ),
        (
            "steps_replaced",
            json!({"type": "steps_replaced", "task": "T-1", "from": 1, "steps": step}),
        ),
        (
            "review_steps_inserted",
            json!({"type": "review_steps_inserted", "task": "T-1", "after": 2,
                "steps": [{"kind": "checkpoint"}, {"kind": "done"}]}),
        ),
        (
            "nudge_sent",
            json!({"type": "nudge_sent", "task": "T-1", "step": 0}),
        ),
        (
            "task_message_sent",
            json!({"type": "task_message_sent", "task": "T-1", "step": 0, "text": "Also do X"}),
        ),
        (
            "task_message_resolved/returned",
            json!({"type": "task_message_resolved", "task": "T-1", "step": 0,
                "outcome": "returned", "via": null, "detail": "the step already reported"}),
        ),
        (
            "help_answered",
            json!({"type": "help_answered", "help": "H-1", "action": "resume", "reply": "go"}),
        ),
        (
            "help_answered/cancel_task",
            json!({"type": "help_answered", "help": "H-1", "action": "cancel_task",
                "reply": null}),
        ),
        ("help_closed", json!({"type": "help_closed", "help": "H-1"})),
        (
            "help_agent_lost",
            json!({"type": "help_agent_lost", "help": "H-1"}),
        ),
        (
            "inbox_delivered",
            json!({"type": "inbox_delivered", "chat": "C-1", "up_to": 7}),
        ),
    ]
    .into_iter()
    .map(|(n, v)| (n.to_string(), v))
    .collect();
    for via in ["steering", "interrupt", "prompt"] {
        out.push((
            format!("task_message_resolved/delivered/{via}"),
            json!({"type": "task_message_resolved", "task": "T-1", "step": 0,
                "outcome": "delivered", "via": via, "detail": ""}),
        ));
    }
    for status in [
        "pending",
        "awaiting_instruction",
        "running",
        "checkpoint",
        "handling",
        "merging",
        "done",
        "cancelled",
        "interrupted",
    ] {
        out.push((
            format!("task_status_changed/{status}"),
            json!({"type": "task_status_changed", "task": "T-1", "status": status}),
        ));
    }
    for (i, kind) in [
        "blocked",
        "question",
        "policy",
        "merge_conflict",
        "review_rounds_exhausted",
        "protocol_violation",
        "agent_stopped",
        "agent_crashed",
        "dirty_readonly_tree",
        "git_failed",
    ]
    .into_iter()
    .enumerate()
    {
        let source = if i < 3 {
            json!({"by": "agent", "role": "orchestrator"})
        } else {
            json!({"by": "yhtye"})
        };
        out.push((
            format!("help_raised/{kind}"),
            json!({"type": "help_raised", "help": {"id": format!("H-{i}"), "task": "T-1",
                "step": 0, "kind": kind, "message": "help", "source": source, "state": "open",
                "agent_lost": false, "reply": null}}),
        ));
    }
    for (i, kind) in [
        "user_message",
        "checkpoint_reached",
        "help_raised",
        "instruction_needed",
        "group_settled",
        "merge_result",
        "restarted",
    ]
    .into_iter()
    .enumerate()
    {
        out.push((
            format!("inbox_queued/{kind}"),
            json!({"type": "inbox_queued", "entry": {"id": i, "chat": "C-1",
                "item": {"kind": kind, "attrs": [["task", "T-1"]], "body": "text"}}}),
        ));
    }
    out
}

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

fn agent_info() -> AgentInfo {
    AgentInfo {
        acp_session_id: de(json!("acp-1")),
        resumed: false,
        modes: Some(de(json!({"currentModeId": "default",
            "availableModes": [{"id": "default", "name": "Default"}]}))),
        config_options: de(json!([{"id": "model", "name": "Model", "type": "select",
            "currentValue": "opus", "options": [{"value": "opus", "name": "Opus"}]}])),
        capabilities: de(json!({"loadSession": true})),
        agent: Some(de(json!({"name": "fake", "version": "1.0.0"}))),
        supports_steering: true,
    }
}

/// Every agent event and output kind (ACP payloads are opaque JSON in Kotlin).
fn agent_events() -> Vec<(String, AgentEvent)> {
    let output = |kind: &str, update: Value| {
        let out = AgentOutput::from_update_json(update);
        let unknown = matches!(out, AgentOutput::Unknown(_));
        assert_eq!(
            unknown,
            kind == "unknown",
            "{kind}: ACP did not read it as expected"
        );
        (format!("output/{kind}"), AgentEvent::Output(out))
    };
    let mut out = vec![
        (
            "ready".to_string(),
            AgentEvent::Ready(Box::new(agent_info())),
        ),
        output(
            "user_message_chunk",
            json!({"sessionUpdate": "user_message_chunk", "content": text("hi")}),
        ),
        output(
            "message_chunk",
            json!({"sessionUpdate": "agent_message_chunk", "content": text("I will")}),
        ),
        output(
            "thought_chunk",
            json!({"sessionUpdate": "agent_thought_chunk", "content": text("Hmm")}),
        ),
        output(
            "tool_call",
            json!({"sessionUpdate": "tool_call", "toolCallId": "t-1", "title": "Read a.rs",
                "kind": "read", "status": "pending"}),
        ),
        output(
            "tool_call_update",
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t-1",
                "status": "completed"}),
        ),
        output(
            "plan",
            json!({"sessionUpdate": "plan", "entries": [{"content": "Write tests",
                "priority": "high", "status": "in_progress"}]}),
        ),
        output(
            "available_commands",
            json!({"sessionUpdate": "available_commands_update", "availableCommands": [
                {"name": "compact", "description": "Compact the context"}]}),
        ),
        output(
            "mode_changed",
            json!({"sessionUpdate": "current_mode_update", "currentModeId": "plan"}),
        ),
        output(
            "config_options",
            json!({"sessionUpdate": "config_option_update", "configOptions": []}),
        ),
        output(
            "session_info",
            json!({"sessionUpdate": "session_info_update", "title": "Fix the bug"}),
        ),
        output(
            "usage",
            json!({"sessionUpdate": "usage_update", "used": 10, "size": 100}),
        ),
        output(
            "unknown",
            json!({"sessionUpdate": "brand_new_kind", "x": [1, 2.5, null, {"y": true}]}),
        ),
        (
            "permission_auto_answered".into(),
            AgentEvent::PermissionAutoAnswered {
                tool_call: Box::new(de(json!({"toolCallId": "t-2", "title": "Edit a.rs"}))),
                options: de(json!([{"optionId": "allow", "name": "Allow",
                    "kind": "allow_once"}])),
                chosen: Some(de(json!("allow_once"))),
            },
        ),
        (
            "permission_auto_answered/cancelled".into(),
            AgentEvent::PermissionAutoAnswered {
                tool_call: Box::new(de(json!({"toolCallId": "t-3"}))),
                options: vec![],
                chosen: None,
            },
        ),
        (
            "turn_ended".into(),
            AgentEvent::TurnEnded(Ok(de(json!("end_turn")))),
        ),
        ("stderr".into(), AgentEvent::Stderr("warning: x".into())),
        (
            "exited".into(),
            AgentEvent::Exited {
                code: Some(0),
                signal: None,
            },
        ),
        (
            "exited/signal".into(),
            AgentEvent::Exited {
                code: None,
                signal: Some(9),
            },
        ),
    ];
    let errors = [
        AgentError::Spawn {
            command: "npx".into(),
            message: "not found".into(),
        },
        AgentError::Startup {
            step: "initialize".into(),
            message: "bad".into(),
        },
        AgentError::Timeout {
            step: "session/new".into(),
            millis: 60_000,
        },
        AgentError::Unsupported {
            requested: "opus".into(),
            available: "haiku".into(),
        },
        AgentError::Setup {
            message: "install a harness".into(),
        },
        AgentError::Busy,
        AgentError::SteeringUnsupported,
        AgentError::Request {
            method: "session/prompt".into(),
            message: "failed".into(),
        },
        AgentError::Closed,
    ];
    for e in errors {
        let code = serde_json::to_value(&e).expect("error")["code"]
            .as_str()
            .expect("code")
            .to_string();
        out.push((format!("turn_ended/{code}"), AgentEvent::TurnEnded(Err(e))));
    }
    out
}

fn tool_call_records() -> Vec<(String, ApiEventBody)> {
    let binding = |chat: bool| {
        de(if chat {
            json!({"session": "orchestrator-C-1", "role": "orchestrator", "project": "repo",
                "chat": "C-1"})
        } else {
            json!({"session": "T-1/0", "role": "implementer", "project": "repo",
                "group": "G-1", "task": "T-1", "step": 0})
        })
    };
    let mut out = vec![(
        "tool_called".to_string(),
        ApiEventBody::ToolCalled {
            record: ToolCallRecord {
                binding: binding(true),
                tool: "create_group".into(),
                args: json!({"title": "x", "n": 1.5, "list": [null, false]}),
                result: Ok(json!({"group_id": "G-1"})),
            },
        },
    )];
    for code in [
        "invalid_argument",
        "not_found",
        "invalid_state",
        "forbidden",
        "conflict",
        "internal",
    ] {
        out.push((
            format!("tool_called/{code}"),
            ApiEventBody::ToolCalled {
                record: ToolCallRecord {
                    binding: binding(false),
                    tool: "report_step_done".into(),
                    args: json!({}),
                    result: Err(de::<ToolError>(json!({"code": code, "message": "no"}))),
                },
            },
        ));
    }
    out
}

fn bodies() -> Vec<(String, ApiEventBody)> {
    let started = |role: &str, full: bool| ApiEventBody::SessionStarted {
        session: format!("{role}-1"),
        role: de::<Role>(json!(role)),
        task: (role != "orchestrator").then(|| "T-1".into()),
        pid: full.then_some(4242),
        acp_session_id: "acp-1".into(),
        resumed: full,
        agent: full.then(choice),
        replaced: full.then(|| de(json!({"harness": "gone", "model": null, "effort": null}))),
        cwd: full.then(|| "/home/u/repo".into()),
        lost_acp_session_id: full.then(|| "acp-0".into()),
    };
    let mut out: Vec<(String, ApiEventBody)> = domain_events()
        .into_iter()
        .map(|(n, e)| (format!("domain/{n}"), domain(e)))
        .collect();
    out.extend([
        ("session_started".into(), started("orchestrator", true)),
        (
            "session_started/implementer".into(),
            started("implementer", false),
        ),
        (
            "session_started/reviewer".into(),
            started("reviewer", false),
        ),
        (
            "session_failed".into(),
            ApiEventBody::SessionFailed {
                session: "T-1/0".into(),
                error: "spawn failed".into(),
                retrying: true,
            },
        ),
        (
            "session_stopped".into(),
            ApiEventBody::SessionStopped {
                session: "T-1/0".into(),
                suspended: false,
            },
        ),
        (
            "session_interrupted".into(),
            ApiEventBody::SessionInterrupted {
                session: "T-1/0".into(),
            },
        ),
        (
            "agent_text".into(),
            ApiEventBody::AgentText {
                session: "T-1/0".into(),
                kind: TextKind::Message,
                text: "Done.".into(),
            },
        ),
        (
            "agent_text/thought".into(),
            ApiEventBody::AgentText {
                session: "T-1/0".into(),
                kind: TextKind::Thought,
                text: "Thinking".into(),
            },
        ),
        (
            "prompted".into(),
            ApiEventBody::Prompted {
                session: "orchestrator-C-1".into(),
                text: "<user_message>hi</user_message>".into(),
            },
        ),
        (
            "session_starting".into(),
            ApiEventBody::SessionStarting {
                session: "orchestrator:C-2".into(),
            },
        ),
        (
            "projects_changed".into(),
            ApiEventBody::ProjectsChanged {
                project: "repo".into(),
            },
        ),
        (
            "settings_changed/agent_settings".into(),
            ApiEventBody::SettingsChanged {
                kind: SettingsKind::AgentSettings,
                project: Some("repo".into()),
                revision: Some(1_790_766_538_444),
            },
        ),
        (
            "settings_changed/harnesses".into(),
            ApiEventBody::SettingsChanged {
                kind: SettingsKind::Harnesses,
                project: None,
                revision: None,
            },
        ),
        (
            "settings_changed/secrets".into(),
            ApiEventBody::SettingsChanged {
                kind: SettingsKind::Secrets,
                project: None,
                revision: None,
            },
        ),
        ("resync".into(), ApiEventBody::Resync),
        (
            "remote_status_changed".into(),
            ApiEventBody::RemoteStatusChanged {
                status: remote_status(true),
            },
        ),
        (
            "remote_devices_changed".into(),
            ApiEventBody::RemoteDevicesChanged { devices: devices() },
        ),
        (
            "clients_changed".into(),
            ApiEventBody::ClientsChanged { clients: clients() },
        ),
    ]);
    out.extend(tool_call_records());
    out.extend(agent_events().into_iter().map(|(n, event)| {
        (
            format!("agent/{n}"),
            ApiEventBody::Agent {
                session: "T-1/0".into(),
                event,
            },
        )
    }));
    out
}

fn events() -> Vec<Sample> {
    bodies()
        .into_iter()
        .map(|(name, body)| {
            let app_wide = body.is_app_wide();
            let event = ApiEvent {
                seq: if app_wide { 0 } else { 17 },
                ts_ms: 1_790_766_538_443,
                project: if app_wide {
                    String::new()
                } else {
                    "repo".into()
                },
                live: body.is_live(),
                body,
            };
            sample("ApiEvent", name, &event)
        })
        .collect()
}

fn server_messages() -> Vec<Sample> {
    let event = ApiEvent {
        seq: 3,
        ts_ms: 1_790_766_538_443,
        project: "repo".into(),
        live: false,
        body: domain(chat_created()),
    };
    let cases = [
        (
            "reply/ok",
            WsServerMessage::Reply(WsReply::Ok {
                id: 1,
                ok: ApiResponse::Pong {
                    host: "pc".into(),
                    version: "0.2.1".into(),
                },
            }),
        ),
        (
            "reply/err",
            WsServerMessage::Reply(WsReply::Err {
                id: 2,
                err: ApiError::not_found("no such project"),
            }),
        ),
        ("event", WsServerMessage::Event { event }),
    ];
    cases
        .into_iter()
        .map(|(name, m)| sample("WsServerMessage", name, &m))
        .collect()
}

fn remote_http() -> Vec<Sample> {
    let request: PairRequest = de(json!({"code": "ABCD2345", "device_name": "Pixel 9"}));
    let desktop: PairRequest = de(json!({
        "code": "ABCD2345",
        "device_name": "Workstation",
        "kind": "desktop",
    }));
    vec![
        sample("PairRequest", "pair", &request),
        sample("PairRequest", "pair_desktop", &desktop),
        sample(
            "PairResponse",
            "pair",
            &PairResponse {
                device_id: "dev-0123456789abcdef".into(),
                token: "ab".repeat(32),
                server_id: "0123456789abcdef0123456789abcdef".into(),
            },
        ),
        sample(
            "RemoteError",
            "wrong_code",
            &RemoteError {
                error: "wrong pairing code".into(),
            },
        ),
    ]
}
