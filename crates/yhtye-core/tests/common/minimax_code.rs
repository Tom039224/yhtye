//! Helpers for fake-agent tests that stand in for MiniMax Code (`mcode acp`),
//! which is not available to automated tests (it is billed per use).

use std::time::Duration;

use serde_json::json;
use yhtye_core::acp::{HarnessConfig, MINIMAX_CODE_DEFAULT_MODEL};
use yhtye_core::agents::{AgentRole, HarnessPreset};

use super::fake_agent_bin;

/// A model `mcode` offers that has no effort levels.
pub const M3_THINKING: &str = "m:minimax:MiniMax-M3:v:thinking";

/// The Flash Preview without thinking: listed, but selecting it is refused.
pub const FLASH_WITHOUT_THINKING: &str = "m:minimax:MiniMax-M3.1-Flash-Preview:v:";

/// The fake agent's script for a MiniMax-Code-like agent: modes `default` /
/// `plan` (no bypass), the `permissionMode` option (category `_permission`,
/// `auto` current), models in `mcode`'s `m:minimax:<Model>:v:<variant>` form
/// with the agent starting on a model that is not the cheapest, and the effort
/// option `thinkingEffort` (category `thought_level`) on the Flash Preview with
/// thinking only. `extra` is merged over it.
pub fn minimax_code_script(extra: serde_json::Value) -> serde_json::Value {
    let mut script = json!({
        "modes": ["default", "plan"],
        "permission_modes": ["auto", "default", "bypassPermissions"],
        "models": [
            M3_THINKING,
            MINIMAX_CODE_DEFAULT_MODEL,
            FLASH_WITHOUT_THINKING,
            "m:minimax:MiniMax-M2.7:v:thinking",
        ],
        "efforts": {MINIMAX_CODE_DEFAULT_MODEL: ["low", "medium", "high", "xhigh", "max"]},
        "effort_id": "thinkingEffort",
    });
    let (Some(base), Some(extra)) = (script.as_object_mut(), extra.as_object()) else {
        panic!("scripts are JSON objects")
    };
    base.extend(extra.clone());
    script
}

/// [`HarnessPreset::minimax_code`] with every config (each role and the model
/// listing session) pointed at the fake agent. Only the command line changes:
/// the fake takes no `acp` argument.
pub fn fake_minimax_code_preset(script: serde_json::Value) -> HarnessPreset {
    let command = fake_agent_bin().display().to_string();
    let mut preset = HarnessPreset::minimax_code(&command);
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
        h.args.clear();
        h.env.insert("YHTYE_FAKE_SCRIPT".into(), script.to_string());
        h.startup_timeout = Duration::from_secs(10);
    }
    preset
}

/// The implementer role's config of [`fake_minimax_code_preset`] for the chosen
/// `model` and `effort`.
pub fn fake_minimax_code_harness(
    script: serde_json::Value,
    model: Option<&str>,
    effort: Option<&str>,
) -> HarnessConfig {
    fake_minimax_code_preset(script).config(AgentRole::Implementer, model, effort)
}
