//! Helpers for fake-agent tests that stand in for Grok Build
//! (`grok agent --no-leader stdio`), which is not available to automated tests
//! (it needs a grok.com login and uses its quota).

use std::time::Duration;

use serde_json::json;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{AgentRole, HarnessPreset};

use super::fake_agent_bin;

/// The only model `grok` 1.0.46 offers on the free plan.
pub const GROK_MODEL: &str = "grok-4.7";

/// The fake agent's script for a Grok-Build-like agent: no modes at all, the
/// model `grok-4.7` with the effort option `reasoning_effort` (category
/// `thought_level`), and `grok`'s vendor notifications (`_x.ai/...`, unknown to
/// Yhtye) before the `session/new` response and at the start of every turn.
/// `extra` is merged over it.
pub fn grok_build_script(extra: serde_json::Value) -> serde_json::Value {
    let mut script = json!({
        "modes": [],
        "models": [GROK_MODEL],
        "efforts": {GROK_MODEL: ["xhigh", "high", "medium", "low"]},
        "effort_id": "reasoning_effort",
        "vendor_notifications": [
            {"method": "_x.ai/session/setup", "params": {"method": "session/new", "phase": "auth"}},
            {"method": "_x.ai/session_notification", "params": {"update": {"sessionUpdate": "turn_completed"}}},
        ],
    });
    let (Some(base), Some(extra)) = (script.as_object_mut(), extra.as_object()) else {
        panic!("scripts are JSON objects")
    };
    base.extend(extra.clone());
    script
}

/// [`HarnessPreset::grok_build`] with every config (each role and the model
/// listing session) pointed at the fake agent. Only the command line changes:
/// the fake takes no `agent --no-leader stdio` arguments.
pub fn fake_grok_build_preset(script: serde_json::Value) -> HarnessPreset {
    let command = fake_agent_bin().display().to_string();
    let mut preset = HarnessPreset::grok_build(&command);
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

/// The implementer role's config of [`fake_grok_build_preset`] for the chosen
/// `model` and `effort`.
pub fn fake_grok_build_harness(
    script: serde_json::Value,
    model: Option<&str>,
    effort: Option<&str>,
) -> HarnessConfig {
    fake_grok_build_preset(script).config(AgentRole::Implementer, model, effort)
}
