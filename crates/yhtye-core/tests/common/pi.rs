//! Helpers for fake-agent tests that stand in for pi through `@openma/pi-acp`,
//! which needs pi's own providers and credentials and is not available to
//! automated tests.

use std::time::Duration;

use serde_json::json;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{AgentRole, HarnessPreset};

use super::fake_agent_bin;

/// A model that reasons, with the levels pi lists for it (`thinking`).
pub const HAIKU: &str = "orcarouter/anthropic/claude-haiku-4.5";

/// A model that does not reason: pi lists no `thinking` option for it.
pub const DEEPSEEK: &str = "orcarouter/deepseek/deepseek-chat";

/// A free model whose id has a colon of its own, with levels that differ from
/// [`HAIKU`]'s.
pub const NEMOTRON: &str = "orcarouter/nvidia/nemotron-3-super:free";

/// A model of another provider (the model option is grouped by provider).
pub const GPT: &str = "openai/gpt-5";

/// The levels of [`HAIKU`].
pub const HAIKU_LEVELS: [&str; 5] = ["off", "low", "medium", "high", "xhigh"];

/// The levels of [`NEMOTRON`].
pub const NEMOTRON_LEVELS: [&str; 4] = ["minimal", "low", "medium", "high"];

/// The fake agent's script for a pi-like agent: no modes, models grouped by
/// provider, the starting model read from `PI_ACP_MODEL` (`<model>[:<level>]`),
/// the `thinking` option (category `thought_level`) only for the models that
/// reason, listing their levels without a `default`, the boolean option
/// `auto_compaction` (category `model_config`) that is no effort, and steering.
/// `extra` is merged over it.
pub fn pi_script(extra: serde_json::Value) -> serde_json::Value {
    let mut script = json!({
        "modes": [],
        "models": [HAIKU, DEEPSEEK, NEMOTRON, GPT],
        "efforts": {HAIKU: HAIKU_LEVELS, NEMOTRON: NEMOTRON_LEVELS},
        "effort_id": "thinking",
        "efforts_without_default": true,
        "model_groups": true,
        "model_env": "PI_ACP_MODEL",
        "boolean_options": [{"id": "auto_compaction", "category": "model_config"}],
        "steering": true,
    });
    let (Some(base), Some(extra)) = (script.as_object_mut(), extra.as_object()) else {
        panic!("scripts are JSON objects")
    };
    base.extend(extra.clone());
    script
}

/// [`HarnessPreset::pi`] with every config (each role and the model listing
/// session) pointed at the fake agent. Only the command line changes: the fake
/// takes no `-y @openma/pi-acp@...` arguments.
pub fn fake_pi_preset(script: serde_json::Value) -> HarnessPreset {
    let command = fake_agent_bin().display().to_string();
    let mut preset = HarnessPreset::pi().with_command(&command);
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

/// The implementer role's config of [`fake_pi_preset`] for the chosen `model`
/// and `effort`.
pub fn fake_pi_harness(
    script: serde_json::Value,
    model: Option<&str>,
    effort: Option<&str>,
) -> HarnessConfig {
    fake_pi_preset(script).config(AgentRole::Implementer, model, effort)
}
