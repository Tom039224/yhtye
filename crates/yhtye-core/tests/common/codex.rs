//! Helpers for real Codex tests (always `nvidia/nemotron-3-super-120b-a12b:free`
//! through OpenRouter, the user's `~/.codex` configuration with
//! `model_provider = "openrouter"`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yhtye_core::acp::{AgentEvent, HarnessConfig};
use yhtye_core::agents::{AgentRole, HarnessPreset, find_in_path};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::runtime::OrchestrationConfig;

use super::orch::config;

/// The only model real Codex tests may use (user decision, Stage 7e; changed
/// from `nvidia/nemotron-3.5-lightning:free` on 2026-09-29 when that one was
/// in an outage: 3 to 6 minutes per model call). OpenRouter lists its efforts
/// as `medium` and `low`.
pub const MODEL: &str = "nvidia/nemotron-3-super-120b-a12b:free";
/// Longest silence tolerated per event; a slow provider fails the test in
/// minutes instead of hanging it.
pub const REAL_TIMEOUT: Duration = Duration::from_secs(240);
/// Longest a whole orchestration run may take.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(1200);

/// The variable the user's `~/.codex/config.toml` reads the OpenRouter key
/// from (`[model_providers.openrouter.auth]`). Only its presence is checked.
const KEY_VAR: &str = "OPENROUTER_API_KEY_CODEX";

/// The user's Codex home. A terminal of Orca injects another `CODEX_HOME`
/// (its own runtime home, not the user's config), so real tests name it explicitly.
pub fn codex_home() -> PathBuf {
    Path::new(&std::env::var_os("HOME").expect("HOME")).join(".codex")
}

/// Fails at once, with the reason, when the OpenRouter key is not in the test
/// process's environment (run through `fish -c '...'`, which exports it, or
/// register it in Yhtye's secret environment variables).
pub fn require_key() {
    assert!(
        std::env::var_os(KEY_VAR).is_some_and(|v| !v.is_empty()),
        "{KEY_VAR} is not set: run the test through `fish -c '...'` (its config exports it)"
    );
}

/// [`HarnessPreset::codex`] with the user's `codex` and their `~/.codex`.
pub fn codex_preset() -> HarnessPreset {
    require_key();
    let path = find_in_path("codex", std::env::var_os("PATH").as_deref()).expect("codex on PATH");
    let mut p = HarnessPreset::codex(path.to_str());
    let home = codex_home().display().to_string();
    for h in [
        &mut p.orchestrator,
        &mut p.implementer,
        &mut p.investigator,
        &mut p.reviewer,
    ] {
        h.env.insert("CODEX_HOME".into(), home.clone());
    }
    if let Some(probe) = &mut p.probe {
        probe.env.insert("CODEX_HOME".into(), home);
    }
    p
}

/// An implementer-style Codex config on [`MODEL`].
pub fn codex_harness() -> HarnessConfig {
    codex_preset().config(AgentRole::Implementer, Some(MODEL), None)
}

/// Codex processes that belong to no user daemon: the adapter, its `codex.js`
/// launcher and `codex app-server` children. The user's managed daemon
/// (`--managed-daemon`, `pid-update-loop`) is not counted.
pub fn codex_processes() -> BTreeSet<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return BTreeSet::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            if super::process_gone(*pid) {
                return false;
            }
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|raw| {
                let cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
                let is_adapter = cmd.contains("codex-acp");
                let is_app_server = cmd.contains("codex") && cmd.contains(" app-server");
                let is_daemon = cmd.contains("--managed-daemon") || cmd.contains("pid-update-loop");
                (is_adapter || is_app_server) && !is_daemon
            })
        })
        .collect()
}

/// Polls until none of the Codex processes that were not in `before` is alive.
pub async fn assert_no_new_processes(before: &BTreeSet<u32>, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let left: Vec<u32> = codex_processes().difference(before).copied().collect();
        if left.is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "codex processes still alive: {left:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Every session that became ready runs [`MODEL`] in `agent-full-access` mode.
pub fn assert_model(events: &[ApiEvent]) {
    for e in events {
        if let ApiEventBody::Agent {
            session,
            event: AgentEvent::Ready(info),
        } = &e.body
        {
            assert_eq!(
                info.config_value("model"),
                Some(MODEL),
                "{session} must run on {MODEL}"
            );
            assert_eq!(info.current_mode(), Some("agent-full-access"), "{session}");
        }
    }
}

/// Orchestration with Codex in every role.
pub fn real_config(dir: &Path, orchestrator: HarnessConfig) -> OrchestrationConfig {
    config(dir, orchestrator, codex_harness(), codex_harness())
}
