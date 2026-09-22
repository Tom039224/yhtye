//! Helpers for real Claude Code tests (always Haiku).

use std::path::Path;
use std::time::Duration;

use yhtye_core::acp::{AgentEvent, HarnessConfig};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, OrchestrationConfig};

use super::orch::config;

pub const MODEL: &str = "haiku";
pub const REAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Every session that became ready runs Haiku in bypass mode.
pub fn assert_haiku(events: &[ApiEvent]) {
    for e in events {
        if let ApiEventBody::Agent {
            session,
            event: AgentEvent::Ready(info),
        } = &e.body
        {
            assert_eq!(
                info.config_value("model"),
                Some(MODEL),
                "{session} must run on haiku"
            );
            assert_eq!(info.current_mode(), Some("bypassPermissions"), "{session}");
        }
    }
}

pub fn is_orchestrator_turn_end(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(_) } if session == ORCHESTRATOR_SESSION)
}

pub fn real_config(dir: &Path) -> OrchestrationConfig {
    config(
        dir,
        HarnessConfig::claude_code_orchestrator(MODEL),
        HarnessConfig::claude_code(MODEL),
        HarnessConfig::claude_code(MODEL),
    )
}
