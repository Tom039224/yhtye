//! Which harnesses the app registers (`core-design.md` §15.2): one preset per
//! installed harness ([`presets_from`], from what [`super::detect_harnesses`]
//! found), and the built-in default among them ([`default_choice`]). Pure
//! functions over an injected environment so they can be tested.

use std::ffi::OsString;

use super::catalog::{CLAUDE_CODE, CODEX, DEVIN, HarnessPreset, MINIMAX_CODE, OPENCODE};
use super::detect::HarnessDetection;
use super::settings::AgentChoice;
use crate::acp::MINIMAX_CODE_DEFAULT_MODEL;

/// Model OpenCode runs if a setting without a model ever reaches it (settings
/// are validated to name one): a free model, never OpenCode's own last-used
/// (possibly paid) model.
pub const OPENCODE_FALLBACK_MODEL: &str = "opencode/muse-spark-1.3-contributor-free";

/// The harnesses the built-in default prefers, first installed first. Codex is
/// last: it cannot run without a chosen model, so it is the default only when
/// nothing else is there.
const DEFAULT_ORDER: [&str; 5] = [CLAUDE_CODE, OPENCODE, DEVIN, MINIMAX_CODE, CODEX];

/// Inherited variables to drop from OpenCode's environment. The user's OpenCode
/// configuration stays in effect (`acp-harnesses.md` §7.2), so an
/// `OPENCODE_CONFIG_DIR` the user set is kept; but Orca's terminals inject
/// `OPENCODE_CONFIG_DIR` (equal to `ORCA_OPENCODE_CONFIG_DIR`, Orca's own hooks
/// directory, which *replaces* the user's global config directory). When Yhtye
/// is started from such a terminal, that value is removed so the agents read the
/// user's `~/.config/opencode` as they would when Yhtye is started from the desktop.
pub fn inherited_opencode_env_remove(env: impl Fn(&str) -> Option<OsString>) -> Vec<String> {
    match (env("OPENCODE_CONFIG_DIR"), env("ORCA_OPENCODE_CONFIG_DIR")) {
        (Some(dir), Some(orca)) if !dir.is_empty() && dir == orca => {
            vec!["OPENCODE_CONFIG_DIR".into()]
        }
        _ => Vec::new(),
    }
}

/// One preset per installed harness in `detections`, in their order: Claude
/// Code with `claude_model`, OpenCode, Codex, Devin and MiniMax Code, each launched through
/// the absolute path that was found (Claude Code and Codex run `npx`; Codex also
/// gets the user's `codex` as the adapter's `CODEX_PATH`). A harness that is not
/// installed is not registered.
pub fn presets_from(
    detections: &[HarnessDetection],
    claude_model: &str,
    env: impl Fn(&str) -> Option<OsString>,
) -> Vec<HarnessPreset> {
    detections
        .iter()
        .filter(|d| d.installed)
        .filter_map(|d| {
            let preset = preset_of(d, claude_model, &env);
            if preset.is_none() {
                tracing::warn!("harness {} has no launch recipe; not offered", d.id);
            }
            preset
        })
        .collect()
}

fn preset_of(
    d: &HarnessDetection,
    claude_model: &str,
    env: &impl Fn(&str) -> Option<OsString>,
) -> Option<HarnessPreset> {
    let main = d.resolved_path.as_deref()?;
    match d.id.as_str() {
        CLAUDE_CODE => Some(HarnessPreset::claude_code(claude_model).with_command(main)),
        OPENCODE => Some(
            HarnessPreset::opencode(OPENCODE_FALLBACK_MODEL, inherited_opencode_env_remove(env))
                .with_command(main),
        ),
        CODEX => Some(HarnessPreset::codex(Some(main)).with_command(d.found("npx")?)),
        DEVIN => Some(HarnessPreset::devin(main)),
        MINIMAX_CODE => Some(HarnessPreset::minimax_code(main)),
        _ => None,
    }
}

/// What a role runs when its settings do not say: the first of Claude Code,
/// OpenCode, Devin, MiniMax Code and Codex that is in `presets`, with Claude
/// Code's `claude_model`, OpenCode's [`OPENCODE_FALLBACK_MODEL`], MiniMax Code's
/// [`MINIMAX_CODE_DEFAULT_MODEL`] and no model for the others (Codex has no
/// usable default: choosing its model is asked for in the settings when it is
/// started). With none installed the choice names Claude
/// Code, which starting an agent reports as unavailable.
#[must_use]
pub fn default_choice(presets: &[HarnessPreset], claude_model: &str) -> AgentChoice {
    let claude = AgentChoice::new(CLAUDE_CODE, Some(claude_model));
    let Some(id) = DEFAULT_ORDER
        .iter()
        .find(|id| presets.iter().any(|p| p.id == **id))
    else {
        return claude;
    };
    match *id {
        CLAUDE_CODE => claude,
        OPENCODE => AgentChoice::new(OPENCODE, Some(OPENCODE_FALLBACK_MODEL)),
        MINIMAX_CODE => AgentChoice::new(MINIMAX_CODE, Some(MINIMAX_CODE_DEFAULT_MODEL)),
        other => AgentChoice::new(other, None),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::detect::{HarnessRequirement, PathSource, harness_spec};
    use super::*;
    use crate::acp::HarnessConfig;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn detection(
        id: &str,
        installed: bool,
        main: Option<&str>,
        npx: Option<&str>,
    ) -> HarnessDetection {
        let spec = harness_spec(id).expect("known harness");
        let mut requirements = vec![HarnessRequirement {
            command: spec.main.into(),
            found: main.map(Into::into),
        }];
        requirements.extend(spec.also.iter().map(|c| HarnessRequirement {
            command: (*c).into(),
            found: npx.map(Into::into),
        }));
        HarnessDetection {
            id: id.into(),
            label: spec.label.into(),
            installed,
            resolved_path: main.map(Into::into),
            path_source: PathSource::Path,
            override_path: None,
            override_error: None,
            requirements,
        }
    }

    fn all_installed() -> Vec<HarnessDetection> {
        vec![
            detection("claude-code", true, Some("/n/npx"), None),
            detection("opencode", true, Some("/o/opencode"), None),
            detection("codex", true, Some("/c/codex"), Some("/n/npx")),
            detection("devin", true, Some("/d/devin"), None),
            detection("minimax-code", true, Some("/m/mcode"), None),
        ]
    }

    fn ids(presets: &[HarnessPreset]) -> Vec<&str> {
        presets.iter().map(|p| p.id.as_str()).collect()
    }

    fn command(p: &HarnessPreset) -> String {
        p.config(super::super::AgentRole::Implementer, None, None)
            .command
    }

    #[test]
    fn only_installed_harnesses_are_registered_through_the_paths_that_were_found() {
        let presets = presets_from(&all_installed(), "haiku", env(&[]));
        assert_eq!(
            ids(&presets),
            ["claude-code", "opencode", "codex", "devin", "minimax-code"]
        );
        let commands: Vec<String> = presets.iter().map(command).collect();
        assert_eq!(
            commands,
            ["/n/npx", "/o/opencode", "/n/npx", "/d/devin", "/m/mcode"],
            "claude-code and codex run npx; the others their own executable"
        );
        let codex = &presets[2];
        for role in [
            super::super::AgentRole::Orchestrator,
            super::super::AgentRole::Implementer,
            super::super::AgentRole::Investigator,
            super::super::AgentRole::Reviewer,
        ] {
            for p in &presets {
                assert_eq!(
                    p.config(role, None, None).command,
                    match p.id.as_str() {
                        "claude-code" | "codex" => "/n/npx",
                        "opencode" => "/o/opencode",
                        "minimax-code" => "/m/mcode",
                        _ => "/d/devin",
                    }
                );
            }
        }
        assert_eq!(
            codex
                .config(super::super::AgentRole::Implementer, Some("m"), None)
                .env
                .get("CODEX_PATH")
                .map(String::as_str),
            Some("/c/codex"),
            "the adapter runs the user's codex"
        );
        assert_eq!(
            codex.probe_config().command,
            "/n/npx",
            "the model listing session runs the found npx too"
        );

        let mut some = all_installed();
        some[1].installed = false;
        some[3].installed = false;
        some[4].installed = false;
        assert_eq!(
            ids(&presets_from(&some, "haiku", env(&[]))),
            ["claude-code", "codex"]
        );
        assert!(presets_from(&[], "haiku", env(&[])).is_empty());
    }

    #[test]
    fn claude_code_is_registered_with_the_model_it_is_given() {
        let presets = presets_from(&all_installed()[..1], "sonnet", env(&[]));
        let h = presets[0].config(super::super::AgentRole::Orchestrator, None, None);
        assert_eq!(h.model.as_ref().map(|m| m.value.as_str()), Some("sonnet"));
        assert_eq!(h.args, HarnessConfig::claude_code("sonnet").args);
    }

    #[test]
    fn the_opencode_preset_drops_the_orca_config_dir_from_the_environment() {
        let orca = "/home/u/.config/orca/opencode-hooks/shared";
        let vars = [
            ("OPENCODE_CONFIG_DIR", orca),
            ("ORCA_OPENCODE_CONFIG_DIR", orca),
        ];
        let presets = presets_from(&all_installed()[1..2], "haiku", env(&vars));
        let h = presets[0].config(super::super::AgentRole::Implementer, None, None);
        assert_eq!(h.env_remove, ["OPENCODE_CONFIG_DIR"]);
    }

    #[test]
    fn a_codex_without_a_found_npx_is_not_registered() {
        let d = detection("codex", true, Some("/c/codex"), None);
        assert!(presets_from(&[d], "haiku", env(&[])).is_empty());
    }

    #[test]
    fn the_builtin_default_is_the_first_installed_harness_in_order() {
        let preset = |id: &str| match id {
            "claude-code" => HarnessPreset::claude_code("haiku"),
            "opencode" => HarnessPreset::opencode("m", Vec::new()),
            "codex" => HarnessPreset::codex(None),
            "minimax-code" => HarnessPreset::minimax_code("mcode"),
            _ => HarnessPreset::devin("devin"),
        };
        let choice = |installed: &[&str]| {
            let presets: Vec<HarnessPreset> = installed.iter().map(|id| preset(id)).collect();
            default_choice(&presets, "haiku")
        };
        assert_eq!(
            choice(&["devin", "codex", "opencode", "claude-code"]),
            AgentChoice::new("claude-code", Some("haiku"))
        );
        assert_eq!(
            choice(&["codex", "devin", "opencode"]),
            AgentChoice::new("opencode", Some(OPENCODE_FALLBACK_MODEL))
        );
        assert_eq!(choice(&["codex", "devin"]), AgentChoice::new("devin", None));
        assert_eq!(
            choice(&["codex", "minimax-code"]),
            AgentChoice::new(
                "minimax-code",
                Some("m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking")
            ),
            "before Codex, and on the cheapest model rather than the user's own"
        );
        assert_eq!(
            choice(&["codex"]),
            AgentChoice::new("codex", None),
            "the user is asked to choose the model when it starts"
        );
        assert_eq!(
            choice(&[]),
            AgentChoice::new("claude-code", Some("haiku")),
            "nothing installed: a placeholder that starting an agent reports"
        );
    }

    #[test]
    fn the_codex_preset_passes_model_and_effort_through_codex_config() {
        use super::super::AgentRole;
        let p = HarnessPreset::codex(Some("/bin/codex"));
        assert!(p.requires_model);
        assert_eq!(p.model_config_id(), "model");
        assert_eq!(p.effort_config_id, "reasoning_effort");
        let info = p.info();
        assert_eq!((info.id.as_str(), info.label.as_str()), ("codex", "Codex"));
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            let h = p.config(
                role,
                Some("nvidia/nemotron-3.5-lightning:free"),
                Some("high"),
            );
            assert_eq!(h.mode_after_new.as_deref(), Some("agent-full-access"));
            let config: serde_json::Value =
                serde_json::from_str(h.env.get("CODEX_CONFIG").expect("CODEX_CONFIG"))
                    .expect("JSON");
            assert_eq!(
                config,
                serde_json::json!({
                    "model": "nvidia/nemotron-3.5-lightning:free",
                    "model_reasoning_effort": "high"
                })
            );
            assert_eq!(
                h.model.as_ref().map(|m| m.value.as_str()),
                Some("nvidia/nemotron-3.5-lightning:free"),
                "the model is still verified through its config option"
            );
            assert!(
                h.effort.is_none(),
                "the effort is not sent as an option: the adapter shows none for models it has no metadata of"
            );
        }
        let h = p.config(AgentRole::Implementer, Some("m"), None);
        let config: serde_json::Value =
            serde_json::from_str(h.env.get("CODEX_CONFIG").expect("env")).expect("JSON");
        assert_eq!(config, serde_json::json!({"model": "m"}), "no effort key");
        assert!(
            !p.config(AgentRole::Implementer, None, None)
                .env
                .contains_key("CODEX_CONFIG")
        );
        let probe = p.probe_config();
        assert!(probe.mode_after_new.is_none() && probe.model.is_none());
        assert!(!probe.env.contains_key("CODEX_CONFIG"));
        assert_eq!(
            probe.env.get("CODEX_PATH").map(String::as_str),
            Some("/bin/codex")
        );
    }

    #[test]
    fn an_orca_injected_config_dir_is_removed_but_a_users_own_is_kept() {
        let orca = "/home/u/.config/orca/opencode-hooks/shared";
        assert_eq!(
            inherited_opencode_env_remove(env(&[
                ("OPENCODE_CONFIG_DIR", orca),
                ("ORCA_OPENCODE_CONFIG_DIR", orca),
            ])),
            ["OPENCODE_CONFIG_DIR"]
        );
        assert!(
            inherited_opencode_env_remove(env(&[
                ("OPENCODE_CONFIG_DIR", "/home/u/my-opencode"),
                ("ORCA_OPENCODE_CONFIG_DIR", orca),
            ]))
            .is_empty()
        );
        assert!(
            inherited_opencode_env_remove(env(&[("OPENCODE_CONFIG_DIR", "/home/u/x")])).is_empty()
        );
        assert!(inherited_opencode_env_remove(env(&[])).is_empty());
    }

    #[test]
    fn the_opencode_preset_always_sets_a_model() {
        let p =
            HarnessPreset::opencode(OPENCODE_FALLBACK_MODEL, vec!["OPENCODE_CONFIG_DIR".into()]);
        assert!(p.requires_model);
        assert!(p.model_env.is_none());
        let info = p.info();
        assert!(info.requires_model);
        for role in [
            super::super::AgentRole::Orchestrator,
            super::super::AgentRole::Implementer,
            super::super::AgentRole::Investigator,
            super::super::AgentRole::Reviewer,
        ] {
            let h = p.config(role, Some("opencode/x"), None);
            assert_eq!(
                (h.command.as_str(), h.args.as_slice()),
                ("opencode", &["acp".to_string()][..])
            );
            assert_eq!(h.mode_after_new.as_deref(), Some("build"));
            assert_eq!(
                h.model.as_ref().map(|m| m.value.as_str()),
                Some("opencode/x")
            );
            assert_eq!(h.env_remove, ["OPENCODE_CONFIG_DIR"]);
            let fallback = p.config(role, None, None);
            assert_eq!(
                fallback.model.as_ref().map(|m| m.value.as_str()),
                Some(OPENCODE_FALLBACK_MODEL),
                "never started without a model"
            );
        }
        let probe = p.probe_config();
        assert!(probe.model.is_none() && probe.mode_after_new.is_none());
        assert_eq!(probe.env_remove, ["OPENCODE_CONFIG_DIR"]);
        assert_eq!(p.model_config_id(), "model");
    }
}
