//! Helpers for real OpenCode tests (always `opencode/muse-spark-1.3-contributor-free`).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use yhtye_core::acp::{AgentEvent, HarnessConfig};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::runtime::OrchestrationConfig;

use super::orch::config;

/// The only model real OpenCode tests may use (user decision, Stage 7c).
pub const MODEL: &str = "opencode/muse-spark-1.3-contributor-free";
pub const REAL_TIMEOUT: Duration = Duration::from_secs(300);

/// `opencode acp` starts its private server as `opencode serve --stdio` in a
/// *new session* (its own process group), so killing the agent's process group
/// does not reach it; it exits when its stdin (a pipe from `opencode acp`) closes.
/// Returns the pids of such servers that are alive now.
pub fn opencode_servers() -> BTreeSet<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return BTreeSet::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            if super::process_gone(*pid) {
                return false;
            }
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|raw| {
                let args: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
                args.iter().any(|a| a.ends_with(b"opencode"))
                    && args.iter().any(|a| *a == b"serve")
                    && args.iter().any(|a| *a == b"--stdio")
            })
        })
        .collect()
}

/// Polls until none of the servers that were not in `before` is alive.
pub async fn assert_no_new_servers(before: &BTreeSet<u32>, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let left: Vec<u32> = opencode_servers().difference(before).copied().collect();
        if left.is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "opencode servers still alive: {left:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Every session that became ready runs [`MODEL`].
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
            eprintln!("{session}: mode {:?}", info.current_mode());
        }
    }
}

/// Orchestration with OpenCode in every role.
pub fn real_config(dir: &Path, orchestrator: HarnessConfig) -> OrchestrationConfig {
    config(
        dir,
        orchestrator,
        HarnessConfig::opencode(MODEL),
        HarnessConfig::opencode(MODEL),
    )
}
