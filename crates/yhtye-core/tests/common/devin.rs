//! Helpers for fake-agent tests that stand in for Devin (`devin acp`), which
//! is not available to automated tests.

use std::time::Duration;

use serde_json::json;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{AgentRole, HarnessPreset};

use super::fake_agent_bin;

/// Devin's bypass mode is the one a session must end up in.
pub const BYPASS: &str = "bypass";

/// The fake agent's script for a Devin-like agent: modes `default` / `bypass`
/// (the agent starts in `default`) and the effort option named `reasoning`
/// (category `thought_level`) on the `default` model. `extra` is merged over it.
pub fn devin_script(extra: serde_json::Value) -> serde_json::Value {
    let mut script = json!({
        "modes": ["default", "accept-edits", BYPASS],
        "models": ["default", "fast"],
        "efforts": {"default": ["low", "high"]},
        "effort_id": "reasoning",
    });
    let (Some(base), Some(extra)) = (script.as_object_mut(), extra.as_object()) else {
        panic!("scripts are JSON objects")
    };
    base.extend(extra.clone());
    script
}

/// [`HarnessPreset::devin`] pointed at the fake agent, the way a session of
/// the implementer role is configured for the chosen `model` and `effort`.
/// Only the command line changes: the fake takes no `acp` argument.
pub fn fake_devin_harness(
    script: serde_json::Value,
    model: Option<&str>,
    effort: Option<&str>,
) -> HarnessConfig {
    let command = fake_agent_bin().display().to_string();
    let mut h = HarnessPreset::devin(&command).config(AgentRole::Implementer, model, effort);
    h.args.clear();
    h.env.insert("YHTYE_FAKE_SCRIPT".into(), script.to_string());
    h.startup_timeout = Duration::from_secs(10);
    h
}
