//! Helpers for fake-agent tests that stand in for Google Antigravity's ACP
//! server (`agy_acp_server`), which needs a Google sign-in and is not available
//! to automated tests.

use std::time::Duration;

use serde_json::json;
use yhtye_core::acp::{ANTIGRAVITY_DEFAULT_MODEL, HarnessConfig};
use yhtye_core::agents::{AgentRole, HarnessPreset};

use super::fake_agent_bin;

/// The server's own default model, which is not the cheapest.
pub const FLASH_HIGH: &str = "gemini-3.8-flash-high";

/// A model with a thinking level of its own, as the server lists them.
pub const PRO_HIGH: &str = "gemini-3.1-pro-high";

/// The fake agent's script for an Antigravity-like agent: modes `default` /
/// `auto_edit` / `yolo`, models whose thinking level is part of their id with the
/// agent starting on its own default (`gemini-3.8-flash-high`), and no effort
/// option at all. `extra` is merged over it.
pub fn antigravity_script(extra: serde_json::Value) -> serde_json::Value {
    let mut script = json!({
        "modes": ["default", "auto_edit", "yolo"],
        "models": [
            FLASH_HIGH,
            "gemini-3.8-flash-medium",
            ANTIGRAVITY_DEFAULT_MODEL,
            PRO_HIGH,
            "gemini-3.1-pro-low",
        ],
    });
    let (Some(base), Some(extra)) = (script.as_object_mut(), extra.as_object()) else {
        panic!("scripts are JSON objects")
    };
    base.extend(extra.clone());
    script
}

/// [`HarnessPreset::antigravity`] with every config (each role and the model
/// listing session) pointed at the fake agent. The server takes no arguments, so
/// only the environment changes.
pub fn fake_antigravity_preset(script: serde_json::Value) -> HarnessPreset {
    let command = fake_agent_bin().display().to_string();
    let mut preset = HarnessPreset::antigravity(&command);
    let probe = preset.probe.as_mut();
    for h in [
        &mut preset.orchestrator,
        &mut preset.implementer,
        &mut preset.investigator,
        &mut preset.reviewer,
    ]
    .into_iter()
    .chain(probe)
    {
        h.env.insert("YHTYE_FAKE_SCRIPT".into(), script.to_string());
        h.startup_timeout = Duration::from_secs(10);
    }
    preset
}

/// The implementer role's config of [`fake_antigravity_preset`] for the chosen
/// `model` and `effort`.
pub fn fake_antigravity_harness(
    script: serde_json::Value,
    model: Option<&str>,
    effort: Option<&str>,
) -> HarnessConfig {
    fake_antigravity_preset(script).config(AgentRole::Implementer, model, effort)
}
