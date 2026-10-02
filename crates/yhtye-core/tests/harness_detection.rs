//! Which harnesses are offered (`core-design.md` §15.2), through the [`Core`]
//! API: they are found in a fake `PATH` (the directory of the test), a path set
//! by hand is stored and wins, detecting again changes the catalog, and a
//! harness that is not installed leaves the orchestrator's view (`get_status`,
//! its prompt, `create_task`) while the stored settings keep it. What was read
//! from a harness (its models and efforts, Claude Code's usage) is forgotten
//! when its executable changes, without waiting for a probe that is running.

#![cfg(unix)]

mod common;

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::gate::Gate;
use common::orch::ORCHESTRATOR_SESSION;
use common::repo::TempRepo;
use common::{fake_agent_bin, fake_harness};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{
    AgentChoice, AgentRole, Candidate, HarnessDetection, HarnessModels, HarnessPreset,
    ModelService, OPENCODE_FALLBACK_MODEL, PathSource, RoleSettings,
};
use yhtye_core::api::{
    AgentSettingsView, ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, TextKind,
};
use yhtye_core::domain::DomainEvent;
use yhtye_core::runtime::{Core, CoreConfig, DetectionConfig};
use yhtye_core::secrets::Secrets;
use yhtye_core::usage::{UsageReport, UsageService, UsageWindowKind};

const TIMEOUT: Duration = Duration::from_secs(30);

/// A directory of fake executables that is the whole `PATH` of the core.
struct Bin(tempfile::TempDir);

impl Bin {
    fn new() -> Self {
        Self(tempfile::tempdir().expect("tempdir"))
    }

    fn dir(&self) -> &Path {
        self.0.path()
    }

    /// An executable that is never run (only found).
    fn install(&self, name: &str) -> PathBuf {
        write_executable(self.dir(), name, "#!/bin/sh\n")
    }

    fn uninstall(&self, name: &str) {
        std::fs::remove_file(self.dir().join(name)).expect("remove");
    }

    fn detection(&self) -> DetectionConfig {
        let path = OsString::from(self.dir());
        DetectionConfig::new("haiku", move |k| (k == "PATH").then(|| path.clone()))
            .with_known_dirs(Vec::new())
    }
}

fn write_executable(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn config(r: &TempRepo, bin: &Bin) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.usage = None;
    cfg.detection = Some(bin.detection());
    cfg
}

async fn run(core: &Core, cmd: ApiCommand) -> ApiResponse {
    core.command(cmd.clone())
        .await
        .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"))
}

async fn harnesses(core: &Core, cmd: ApiCommand) -> Vec<HarnessDetection> {
    match run(core, cmd).await {
        ApiResponse::Harnesses { harnesses } => harnesses,
        other => panic!("unexpected {other:?}"),
    }
}

async fn get(core: &Core) -> Vec<HarnessDetection> {
    harnesses(core, ApiCommand::GetHarnesses).await
}

async fn detect(core: &Core) -> Vec<HarnessDetection> {
    harnesses(core, ApiCommand::DetectHarnesses).await
}

async fn set_path(core: &Core, harness: &str, path: Option<&str>) -> Vec<HarnessDetection> {
    let cmd = ApiCommand::SetHarnessPath {
        harness: harness.into(),
        path: path.map(str::to_string),
    };
    harnesses(core, cmd).await
}

async fn settings_view(core: &Core) -> AgentSettingsView {
    match run(core, ApiCommand::GetAgentSettings { project: None }).await {
        ApiResponse::AgentSettings { settings } => *settings,
        other => panic!("unexpected {other:?}"),
    }
}

/// The ids of the registered harnesses, as the settings panel offers them.
async fn offered(core: &Core) -> Vec<String> {
    settings_view(core)
        .await
        .harnesses
        .into_iter()
        .map(|h| h.id)
        .collect()
}

fn by_id<'a>(all: &'a [HarnessDetection], id: &str) -> &'a HarnessDetection {
    all.iter().find(|d| d.id == id).expect("known harness")
}

fn installed(all: &[HarnessDetection]) -> Vec<&str> {
    all.iter()
        .filter(|d| d.installed)
        .map(|d| d.id.as_str())
        .collect()
}

fn text(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

#[tokio::test]
async fn every_known_harness_is_reported_with_what_was_found() {
    let r = TempRepo::new();
    let bin = Bin::new();
    let npx = bin.install("npx");
    let devin = bin.install("devin");
    let mcode = bin.install("mcode");
    bin.install("opencode");
    let core = Core::start(config(&r, &bin)).await.expect("core");

    let all = get(&core).await;
    let ids: Vec<&str> = all.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        ids,
        ["claude-code", "opencode", "codex", "devin", "minimax-code"]
    );
    assert_eq!(
        installed(&all),
        ["claude-code", "opencode", "devin", "minimax-code"]
    );

    let claude = by_id(&all, "claude-code");
    assert_eq!(claude.label, "Claude Code");
    assert_eq!(claude.resolved_path.as_deref(), Some(text(&npx)));
    assert_eq!(claude.path_source, PathSource::Path);
    assert_eq!(claude.override_path, None);

    let codex = by_id(&all, "codex");
    assert!(!codex.installed);
    assert_eq!(codex.resolved_path, None);
    assert_eq!(codex.path_source, PathSource::None);
    let found: Vec<(&str, Option<&str>)> = codex
        .requirements
        .iter()
        .map(|r| (r.command.as_str(), r.found.as_deref()))
        .collect();
    assert_eq!(
        found,
        [("codex", None), ("npx", Some(text(&npx)))],
        "what is missing is listed, and npx was found"
    );

    assert_eq!(
        by_id(&all, "devin").resolved_path.as_deref(),
        Some(text(&devin))
    );
    let minimax = by_id(&all, "minimax-code");
    assert_eq!(minimax.label, "MiniMax Code");
    assert_eq!(minimax.resolved_path.as_deref(), Some(text(&mcode)));
    assert_eq!(
        offered(&core).await,
        ["claude-code", "opencode", "devin", "minimax-code"],
        "a harness that is not installed is not offered"
    );
    assert_eq!(
        settings_view(&core).await.builtin,
        AgentChoice::new("claude-code", Some("haiku"))
    );
    core.shutdown().await;
}

/// The rows of a harness that is gone stay in the settings panel; saving the
/// rows around them must not fail, while a new row of an unknown harness does.
#[tokio::test]
async fn rows_of_an_uninstalled_harness_do_not_block_editing_the_others() {
    let r = TempRepo::new();
    let bin = Bin::new();
    bin.install("opencode");
    bin.install("devin");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    let set = |rows: &RoleSettings| {
        core.command(ApiCommand::SetAgentSettings {
            project: None,
            role: AgentRole::Implementer,
            settings: Some(rows.clone()),
        })
    };
    let gone = AgentChoice::new("opencode", Some("x"));
    let devin = AgentChoice::new("devin", None);
    let rows = RoleSettings {
        candidates: vec![
            Candidate::from(gone.clone()).with_note("cheap"),
            Candidate::from(devin.clone()),
        ],
        default: devin.clone(),
    };
    set(&rows).await.expect("both are installed");
    bin.uninstall("opencode");
    assert_eq!(installed(&detect(&core).await), ["devin"]);

    // A note changes and a row is added; the row of OpenCode is sent back as is.
    let edited = RoleSettings {
        candidates: vec![
            Candidate::from(gone.clone()).with_note("cheap"),
            Candidate::from(devin.clone()).with_note("careful"),
            Candidate::from(AgentChoice::new("devin", Some("swe-1-6-slow"))),
        ],
        default: devin.clone(),
    };
    set(&edited).await.expect("the stored row is not new");
    assert_eq!(settings_view(&core).await.effective.implementer, edited);

    // Another model of the missing harness is a new row.
    let mut added = edited.clone();
    added
        .candidates
        .push(AgentChoice::new("opencode", Some("y")).into());
    let e = set(&added)
        .await
        .expect_err("a new row of an unknown harness");
    assert_eq!(e.code, ApiErrorCode::InvalidArgument, "{e}");
    assert!(e.message.contains("unknown harness opencode"), "{e}");
    assert_eq!(settings_view(&core).await.effective.implementer, edited);
    core.shutdown().await;
}

#[tokio::test]
async fn the_harnesses_are_found_again_when_asked() {
    let r = TempRepo::new();
    let bin = Bin::new();
    bin.install("opencode");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    assert_eq!(offered(&core).await, ["opencode"]);
    assert_eq!(
        settings_view(&core).await.builtin,
        AgentChoice::new("opencode", Some(OPENCODE_FALLBACK_MODEL)),
        "no npx: the built-in default is the first one that is there"
    );

    // Nothing is looked at again until asked.
    bin.install("devin");
    assert_eq!(offered(&core).await, ["opencode"]);
    // Opening the settings (GetHarnesses) finds it.
    assert_eq!(installed(&get(&core).await), ["opencode", "devin"]);
    assert_eq!(offered(&core).await, ["opencode", "devin"]);

    // The detect button does the same for what disappeared.
    bin.uninstall("opencode");
    assert_eq!(offered(&core).await, ["opencode", "devin"]);
    assert_eq!(installed(&detect(&core).await), ["devin"]);
    assert_eq!(offered(&core).await, ["devin"]);
    assert_eq!(
        settings_view(&core).await.builtin,
        AgentChoice::new("devin", None)
    );

    bin.install("npx");
    detect(&core).await;
    assert_eq!(offered(&core).await, ["claude-code", "devin"]);
    assert_eq!(
        settings_view(&core).await.builtin,
        AgentChoice::new("claude-code", Some("haiku"))
    );

    bin.uninstall("npx");
    bin.uninstall("devin");
    assert!(installed(&detect(&core).await).is_empty());
    assert!(offered(&core).await.is_empty());
    core.shutdown().await;
}

#[tokio::test]
async fn a_path_set_by_hand_is_checked_stored_and_kept_across_a_restart() {
    let r = TempRepo::new();
    let bin = Bin::new();
    bin.install("npx");
    let tools = tempfile::tempdir().expect("tempdir");
    let mine = write_executable(tools.path(), "my-devin", "#!/bin/sh\n");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    assert!(!by_id(&get(&core).await, "devin").installed);

    let all = set_path(&core, "devin", Some(text(&mine))).await;
    let devin = by_id(&all, "devin");
    assert!(devin.installed);
    assert_eq!(devin.resolved_path.as_deref(), Some(text(&mine)));
    assert_eq!(devin.path_source, PathSource::Override);
    assert_eq!(devin.override_path.as_deref(), Some(text(&mine)));
    assert_eq!(devin.override_error, None);
    assert_eq!(
        offered(&core).await,
        ["claude-code", "devin"],
        "registered right away"
    );

    // What is not an absolute path of an executable file is refused, and
    // nothing changes.
    let plain = tools.path().join("plain");
    std::fs::write(&plain, "data").expect("write");
    let bad = [
        "relative/devin".to_string(),
        text(&tools.path().join("missing")).to_string(),
        text(&plain).to_string(),
        text(tools.path()).to_string(),
        String::new(),
    ];
    for path in &bad {
        let cmd = ApiCommand::SetHarnessPath {
            harness: "devin".into(),
            path: Some(path.clone()),
        };
        let e = core.command(cmd).await.expect_err(path);
        assert_eq!(e.code, ApiErrorCode::InvalidArgument, "{path:?}: {e}");
    }
    let cmd = ApiCommand::SetHarnessPath {
        harness: "nope".into(),
        path: Some(text(&mine).into()),
    };
    assert_eq!(
        core.command(cmd).await.expect_err("unknown").code,
        ApiErrorCode::NotFound
    );
    let devin = get(&core).await;
    assert_eq!(
        by_id(&devin, "devin").override_path.as_deref(),
        Some(text(&mine))
    );
    core.shutdown().await;

    // After a restart the path is in effect from the start.
    let core = Core::start(config(&r, &bin)).await.expect("restarted");
    assert_eq!(offered(&core).await, ["claude-code", "devin"]);
    let all = get(&core).await;
    assert_eq!(by_id(&all, "devin").path_source, PathSource::Override);

    // Removing it goes back to the search.
    let all = set_path(&core, "devin", None).await;
    let devin = by_id(&all, "devin");
    assert!(!devin.installed);
    assert_eq!(
        (devin.override_path.as_deref(), devin.path_source),
        (None, PathSource::None)
    );
    assert_eq!(offered(&core).await, ["claude-code"]);
    core.shutdown().await;
    let core = Core::start(config(&r, &bin)).await.expect("restarted");
    assert_eq!(offered(&core).await, ["claude-code"]);
    core.shutdown().await;
}

#[tokio::test]
async fn a_path_that_broke_later_makes_the_harness_unavailable_with_the_reason() {
    let r = TempRepo::new();
    let bin = Bin::new();
    bin.install("devin"); // on the PATH, but the path by hand wins
    let tools = tempfile::tempdir().expect("tempdir");
    let mine = write_executable(tools.path(), "my-devin", "#!/bin/sh\n");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    set_path(&core, "devin", Some(text(&mine))).await;
    core.shutdown().await;

    std::fs::remove_file(&mine).expect("remove");
    let core = Core::start(config(&r, &bin)).await.expect("restarted");
    assert!(offered(&core).await.is_empty(), "no fallback to the PATH");
    let all = get(&core).await;
    let devin = by_id(&all, "devin");
    assert!(!devin.installed);
    assert_eq!(devin.resolved_path, None);
    assert_eq!(devin.override_path.as_deref(), Some(text(&mine)));
    let reason = devin.override_error.as_deref().expect("a reason");
    assert!(reason.contains("my-devin"), "{reason}");
    core.shutdown().await;
}

#[tokio::test]
async fn a_path_for_codex_does_not_replace_its_npx() {
    let r = TempRepo::new();
    let bin = Bin::new();
    let tools = tempfile::tempdir().expect("tempdir");
    let codex = write_executable(tools.path(), "codex", "#!/bin/sh\n");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    let all = set_path(&core, "codex", Some(text(&codex))).await;
    let d = by_id(&all, "codex");
    assert!(!d.installed, "npx is still missing");
    assert_eq!(d.resolved_path.as_deref(), Some(text(&codex)));
    bin.install("npx");
    assert!(by_id(&detect(&core).await, "codex").installed);
    assert_eq!(offered(&core).await, ["claude-code", "codex"]);
    core.shutdown().await;
}

#[tokio::test]
async fn without_a_detection_the_commands_answer_an_empty_list() {
    let r = TempRepo::new();
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.usage = None;
    let core = Core::start(cfg).await.expect("core");
    assert!(get(&core).await.is_empty());
    assert!(detect(&core).await.is_empty());
    let tools = tempfile::tempdir().expect("tempdir");
    let mine = write_executable(tools.path(), "devin", "#!/bin/sh\n");
    assert!(set_path(&core, "devin", Some(text(&mine))).await.is_empty());
    assert_eq!(
        offered(&core).await,
        ["claude-code"],
        "the configured harnesses stay"
    );
    core.shutdown().await;
}

#[tokio::test]
async fn detecting_forgets_the_models_read_from_a_harness() {
    let dir = tempfile::tempdir().expect("tempdir");
    let preset = |models: &[&str]| {
        let script = json!({"turns": [], "models": models});
        HarnessPreset::fixed(
            "h",
            fake_harness(script.clone()),
            fake_harness(script.clone()),
            fake_harness(script),
        )
    };
    let service = ModelService::new(dir.path().join("probe"), Arc::new(Secrets::none()));
    let values = |m: yhtye_core::agents::HarnessModels| -> Vec<String> {
        m.models.into_iter().map(|m| m.value).collect()
    };
    let first = service
        .get(&preset(&["a", "b"]), false)
        .await
        .expect("read");
    assert_eq!(values(first), ["a", "b"]);
    let cached = service.get(&preset(&["c"]), false).await.expect("cached");
    assert_eq!(values(cached), ["a", "b"], "read again only after a while");
    service.invalidate("other");
    let cached = service.get(&preset(&["c"]), false).await.expect("cached");
    assert_eq!(
        values(cached),
        ["a", "b"],
        "another harness is not affected"
    );
    service.invalidate("h");
    let fresh = service.get(&preset(&["c"]), false).await.expect("read");
    assert_eq!(values(fresh), ["c"]);
    service.close().await;
}

fn model_values(models: HarnessModels) -> Vec<String> {
    models.models.into_iter().map(|m| m.value).collect()
}

/// A model listing session of harness `h` that is held until the gate opens.
fn gated_preset(gate: &Gate, script: Value) -> HarnessPreset {
    let harness = gate.harness(script);
    HarnessPreset::fixed("h", harness.clone(), harness.clone(), harness)
}

#[tokio::test]
async fn invalidating_does_not_wait_for_a_probe_and_what_it_reads_is_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gate = Gate::new(dir.path());
    let preset = |models: &[&str]| gated_preset(&gate, json!({"turns": [], "models": models}));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));

    // A probe of the executable that is about to be replaced is running.
    let old = tokio::spawn({
        let service = service.clone();
        let preset = preset(&["old"]);
        async move { service.get(&preset, false).await }
    });
    gate.started().await;
    service.invalidate("h");
    assert!(!old.is_finished(), "invalidate returned while it still ran");
    gate.open();
    let stale = old
        .await
        .expect("join")
        .expect("the caller gets its answer");
    assert_eq!(model_values(stale), ["old"]);

    let fresh = service.get(&preset(&["new"]), false).await.expect("read");
    assert_eq!(model_values(fresh), ["new"], "the old result was not kept");
    let cached = service
        .get(&preset(&["newer"]), false)
        .await
        .expect("cached");
    assert_eq!(model_values(cached), ["new"], "a probe after it is kept");
    service.close().await;
}

#[tokio::test]
async fn invalidating_does_not_keep_the_efforts_a_running_probe_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gate = Gate::new(dir.path());
    let preset = |efforts: &[&str]| {
        let script = json!({"turns": [], "models": ["m"], "efforts": {"m": efforts}});
        gated_preset(&gate, script)
    };
    let values = |efforts: Vec<yhtye_core::agents::EffortOption>| -> Vec<String> {
        efforts.into_iter().map(|e| e.value).collect()
    };
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let old = tokio::spawn({
        let service = service.clone();
        let preset = preset(&["low"]);
        async move { service.get_efforts(&preset, "m").await }
    });
    gate.started().await;
    service.invalidate("h");
    assert!(!old.is_finished(), "invalidate returned while it still ran");
    gate.open();
    let stale = old
        .await
        .expect("join")
        .expect("the caller gets its answer");
    assert_eq!(values(stale), ["low"]);

    let fresh = service.get_efforts(&preset(&["high"]), "m").await;
    assert_eq!(values(fresh.expect("read")), ["high"]);
    service.close().await;
}

/// The core's detection looks in the directory `path` holds when it runs.
fn detection_at(path: &Arc<Mutex<OsString>>) -> DetectionConfig {
    let path = path.clone();
    DetectionConfig::new("haiku", move |k| {
        (k == "PATH").then(|| path.lock().expect("lock").clone())
    })
    .with_known_dirs(Vec::new())
}

async fn listed_models(core: &Core, harness: &str) -> Vec<String> {
    let cmd = ApiCommand::ListHarnessModels {
        harness: harness.into(),
        refresh: None,
    };
    match run(core, cmd).await {
        ApiResponse::HarnessModels { models } => model_values(models),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn opening_the_settings_forgets_the_models_of_a_harness_whose_executable_changed() {
    let r = TempRepo::new();
    let (first, second) = (Bin::new(), Bin::new());
    fake_devin(&first, &json!({"models": ["a", "b"]}));
    fake_devin(&second, &json!({"models": ["c"]}));
    let path = Arc::new(Mutex::new(OsString::from(first.dir())));
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.usage = None;
    cfg.detection = Some(detection_at(&path));
    let core = Core::start(cfg).await.expect("core");
    assert_eq!(listed_models(&core, "devin").await, ["a", "b"]);

    // Found again at the same place: what was read is kept.
    fake_devin(&first, &json!({"models": ["x"]}));
    get(&core).await;
    assert_eq!(listed_models(&core, "devin").await, ["a", "b"]);

    // Another Devin comes first in the PATH: the settings opened are enough.
    *path.lock().expect("lock") = OsString::from(second.dir());
    get(&core).await;
    assert_eq!(listed_models(&core, "devin").await, ["c"]);
    core.shutdown().await;
}

/// `/usage` output with the 5-hour window at `five` percent.
fn usage_script(five: u32) -> Value {
    let text = format!(
        "## Usage\n\n> Claude max subscription usage\n\n### Limits\n\n\
**5-hour limit** — **{five}%** · Resets Sep 22, 10:50 PM UTC\n\n`███████████████░░░░░`\n\n\
**Weekly · all models** — **40%** · Resets Sep 24, 5:00 PM UTC\n\n`████████░░░░░░░░░░░░`\n"
    );
    json!({"turns": [{"match": "/usage", "actions": [{"message": text}]}]})
}

fn five_hour_percent(report: &UsageReport) -> f64 {
    report
        .window(UsageWindowKind::FiveHour)
        .expect("5h")
        .percent
}

async fn usage(core: &Core) -> UsageReport {
    let cmd = ApiCommand::GetUsage { refresh: None };
    match run(core, cmd).await {
        ApiResponse::Usage { usage } => usage,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn the_usage_probe_follows_the_npx_of_claude_code() {
    let r = TempRepo::new();
    let (none, a, b) = (Bin::new(), Bin::new(), Bin::new());
    fake_wrapper(&a, "npx", &usage_script(55));
    fake_wrapper(&b, "npx", &usage_script(20));
    let mut cfg = config(&r, &none);
    cfg.usage = Some(HarnessConfig::claude_code_usage_probe());
    // No npx at start; the user gives one later.
    let core = Core::start(cfg).await.expect("core");
    assert!(!by_id(&get(&core).await, "claude-code").installed);

    set_path(&core, "claude-code", Some(text(&a.dir().join("npx")))).await;
    let report = usage(&core).await;
    assert!((five_hour_percent(&report) - 55.0).abs() < f64::EPSILON);

    // Changing the path drops the report that was read through the old one.
    set_path(&core, "claude-code", Some(text(&b.dir().join("npx")))).await;
    let report = usage(&core).await;
    assert!((five_hour_percent(&report) - 20.0).abs() < f64::EPSILON);
    core.shutdown().await;
}

#[tokio::test]
async fn changing_the_usage_harness_does_not_wait_for_a_probe_and_its_report_is_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gate = Gate::new(dir.path());
    let service = Arc::new(UsageService::new(
        Some(gate.harness(usage_script(55))),
        dir.path().join("usage"),
    ));
    let old = tokio::spawn({
        let service = service.clone();
        async move { service.get(false).await }
    });
    gate.started().await;
    let other = fake_harness(usage_script(20));
    service.set_harness(other.clone());
    assert!(
        !old.is_finished(),
        "set_harness returned while it still ran"
    );
    gate.open();
    let stale = old
        .await
        .expect("join")
        .expect("the caller gets its answer");
    assert!((five_hour_percent(&stale) - 55.0).abs() < f64::EPSILON);

    let fresh = service.get(false).await.expect("read");
    assert!(
        (five_hour_percent(&fresh) - 20.0).abs() < f64::EPSILON,
        "the old report was not kept"
    );
    // The same harness again changes nothing: the report stays cached.
    service.set_harness(other);
    assert_eq!(service.get(false).await.expect("cached"), fresh);
    service.close().await;
}

async fn until(
    rx: &mut broadcast::Receiver<ApiEvent>,
    seen: &mut Vec<ApiEvent>,
    done: impl Fn(&ApiEvent) -> bool,
) {
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out after {} events", seen.len()))
            .expect("event stream open");
        let stop = done(&ev);
        seen.push(ev);
        if stop {
            return;
        }
    }
}

async fn open_project_with_chat(core: &Core, r: &TempRepo) -> String {
    let cmd = ApiCommand::OpenProject {
        path: r.repo.display().to_string(),
    };
    let ApiResponse::Project { project } = run(core, cmd).await else {
        panic!("project");
    };
    let cmd = ApiCommand::CreateChat {
        project: project.id.clone(),
        branch: Some("main".into()),
        worktree: None,
    };
    run(core, cmd).await;
    project.id
}

fn send(project: &str) -> ApiCommand {
    ApiCommand::SendUserMessage {
        project: project.into(),
        chat: "C-1".into(),
        text: "go".into(),
    }
}

#[tokio::test]
async fn starting_an_agent_with_nothing_installed_says_what_to_check() {
    let r = TempRepo::new();
    let bin = Bin::new();
    let core = Core::start(config(&r, &bin))
        .await
        .expect("the core starts without any harness");
    assert!(installed(&get(&core).await).is_empty());
    let mut rx = core.subscribe();
    let project = open_project_with_chat(&core, &r).await;
    run(&core, send(&project)).await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::SessionFailed { .. })
    })
    .await;
    let Some(ApiEventBody::SessionFailed { session, error }) = seen.last().map(|e| &e.body) else {
        panic!("failed");
    };
    assert_eq!(session, ORCHESTRATOR_SESSION);
    assert_eq!(error, "使えるハーネスがありません (設定 › ハーネス を確認)");
    core.shutdown().await;
}

/// The wrapper the detected `name` runs: the fake agent with `script`.
fn fake_wrapper(bin: &Bin, name: &str, script: &Value) {
    let script_path = bin.dir().join(format!("{name}-script.json"));
    std::fs::write(&script_path, script.to_string()).expect("write the script");
    let wrapper = format!(
        "#!/bin/sh\nYHTYE_FAKE_SCRIPT=\"$(cat '{}')\"\nexport YHTYE_FAKE_SCRIPT\nexec '{}' \"$@\"\n",
        script_path.display(),
        fake_agent_bin().display()
    );
    write_executable(bin.dir(), name, &wrapper);
}

/// The wrapper the detected Devin runs: the fake agent with `script`.
fn fake_devin(bin: &Bin, script: &Value) {
    fake_wrapper(bin, "devin", script);
}

fn tool_results(seen: &[ApiEvent], tool: &str) -> Vec<Result<Value, String>> {
    seen.iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::ToolCalled { record }
                if record.tool == tool && record.binding.session == ORCHESTRATOR_SESSION =>
            {
                Some(
                    record
                        .result
                        .clone()
                        .map_err(|e| format!("{:?}: {}", e.code, e.message)),
                )
            }
            _ => None,
        })
        .collect()
}

fn messages(seen: &[ApiEvent], key: &str) -> Vec<String> {
    seen.iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::AgentText {
                session,
                kind: TextKind::Message,
                text,
            } if session == key => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A harness that is no longer installed is left out of what the orchestrator
/// sees and may choose (its prompt, `get_status`, `create_task`), but its rows
/// stay in the stored settings.
#[tokio::test]
async fn rows_of_a_harness_that_is_not_installed_are_hidden_from_the_orchestrator() {
    let r = TempRepo::new();
    let bin = Bin::new();
    let group_and_tasks = |harness: &str, model: Option<&str>| {
        let mut args = json!({
            "group_id": "${group_id}", "title": harness, "kind": "code",
            "steps": [{"kind": "implement"}], "instruction": "go", "harness": harness});
        if let Some(m) = model {
            args["model"] = json!(m);
        }
        json!({"mcp_call": {"tool": "create_task", "args": args}})
    };
    let script = json!({
        "modes": ["default", "bypass"],
        "turns": [
            {"match": "[yhtye:user_message]", "actions": [
                "report_state",
                {"mcp_call": {"tool": "create_group", "args": {"title": "hidden"}}},
                group_and_tasks("opencode", Some("x")),
                group_and_tasks("devin", None),
                {"mcp_call": {"tool": "get_status", "args": {}}}
            ]},
            {"match": "[yhtye:group_settled]", "actions": [
                {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "ok"}}}
            ]},
            {"match": "[yhtye:step]", "actions": [
                {"mcp_call": {"tool": "report_step_done", "args": {"result": "done"}}}
            ]}
        ]
    });
    fake_devin(&bin, &script);
    bin.install("opencode");
    let core = Core::start(config(&r, &bin)).await.expect("core");
    assert_eq!(offered(&core).await, ["opencode", "devin"]);

    // Every role may use OpenCode's model x or Devin; the orchestrator and the
    // reviewer run Devin.
    let devin = AgentChoice::new("devin", None);
    let rows = RoleSettings {
        candidates: vec![
            Candidate::from(AgentChoice::new("opencode", Some("x"))).with_note("cheap"),
            Candidate::from(devin.clone()).with_note("careful"),
        ],
        default: devin.clone(),
    };
    for role in [
        AgentRole::Orchestrator,
        AgentRole::Implementer,
        AgentRole::Investigator,
        AgentRole::Reviewer,
    ] {
        let cmd = ApiCommand::SetAgentSettings {
            project: None,
            role,
            settings: Some(rows.clone()),
        };
        run(&core, cmd).await;
    }
    bin.uninstall("opencode");
    assert_eq!(installed(&detect(&core).await), ["devin"]);
    assert_eq!(offered(&core).await, ["devin"]);
    let view = settings_view(&core).await;
    assert_eq!(
        view.effective.implementer, rows,
        "the stored settings keep the row of the missing harness"
    );

    let mut rx = core.subscribe();
    let project = open_project_with_chat(&core, &r).await;
    run(&core, send(&project)).await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        matches!(
            &e.body,
            ApiEventBody::Domain {
                event: DomainEvent::GroupMergeFinished { ok: true, .. }
            }
        ) || matches!(&e.body, ApiEventBody::SessionFailed { .. })
    })
    .await;
    assert!(
        !seen
            .iter()
            .any(|e| matches!(&e.body, ApiEventBody::SessionFailed { .. })),
        "{seen:?}"
    );

    // The orchestrator's prompt lists only Devin.
    let reported = messages(&seen, ORCHESTRATOR_SESSION).join("\n");
    let prompt = reported.split("mcp:").next().expect("the state report");
    assert!(prompt.contains("harness=devin"), "{prompt}");
    assert!(!prompt.contains("harness=opencode"), "{prompt}");

    // create_task refuses the missing harness and lists what is left.
    let created = tool_results(&seen, "create_task");
    assert_eq!(created.len(), 2);
    let refused = created[0].as_ref().expect_err("opencode is not installed");
    assert!(
        refused.starts_with(
            "InvalidArgument: harness: harness=opencode model=x is not an allowed implementer choice"
        ),
        "{refused}"
    );
    let listed = refused.split("candidate rows:").nth(1).expect("the rows");
    assert!(listed.contains("harness=devin"), "{refused}");
    assert!(!listed.contains("opencode"), "{refused}");
    assert!(created[1].is_ok(), "{:?}", created[1]);

    // get_status lists the same.
    let status = tool_results(&seen, "get_status").remove(0).expect("status");
    let devin_row = json!({"harness": "devin", "model": null, "effort": null, "note": "careful"});
    for role in ["implementer", "investigator", "reviewer"] {
        assert_eq!(
            status["agents"][role]["allowed"],
            json!([devin_row]),
            "{role}"
        );
        assert_eq!(
            status["agents"][role]["default"],
            json!({"harness": "devin", "model": null, "effort": null})
        );
    }
    // The agent of the task that was created runs.
    assert!(
        seen.iter().any(|e| matches!(
            &e.body,
            ApiEventBody::SessionStarted { session, agent, .. }
                if session == "T-1/implementer" && agent.as_ref() == Some(&devin)
        )),
        "the task ran on Devin"
    );
    core.shutdown().await;
}
