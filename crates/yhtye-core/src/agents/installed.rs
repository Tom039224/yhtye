//! Which harnesses the app registers (Stage 7c-2, `core-design.md` §15.2):
//! Claude Code always, OpenCode / Codex only when `opencode` / `codex` is installed. Pure
//! functions over an injected environment so they can be tested.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::catalog::HarnessPreset;

/// The OpenCode executable looked up on `PATH`.
pub const OPENCODE_COMMAND: &str = "opencode";

/// The Codex executable looked up on `PATH` (the adapter runs it through
/// `CODEX_PATH`; the adapter itself is fetched by `npx`).
pub const CODEX_COMMAND: &str = "codex";

/// Model OpenCode runs if a setting without a model ever reaches it (settings
/// are validated to name one): a free model, never OpenCode's own last-used
/// (possibly paid) model.
pub const OPENCODE_FALLBACK_MODEL: &str = "opencode/muse-spark-1.3-contributor-free";

/// The first executable regular file named `command` in the directories of
/// `path` (a `PATH` value). Relative entries are ignored.
#[must_use]
pub fn find_in_path(command: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(command))
        .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

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

/// The presets to register: Claude Code with `model` (always; a missing `npx`
/// shows up when a session starts), OpenCode if `opencode` is on `PATH`, and
/// Codex if `codex` is (the found path becomes the adapter's `CODEX_PATH`).
pub fn installed_presets(
    claude_model: &str,
    env: impl Fn(&str) -> Option<OsString>,
) -> Vec<HarnessPreset> {
    let mut presets = vec![HarnessPreset::claude_code(claude_model)];
    match find_in_path(OPENCODE_COMMAND, env("PATH").as_deref()) {
        Some(path) => {
            tracing::info!("OpenCode found at {}", path.display());
            presets.push(HarnessPreset::opencode(
                OPENCODE_FALLBACK_MODEL,
                inherited_opencode_env_remove(&env),
            ));
        }
        None => tracing::info!("OpenCode (`opencode`) is not installed; not offered"),
    }
    match find_in_path(CODEX_COMMAND, env("PATH").as_deref()) {
        Some(path) => {
            tracing::info!("Codex found at {}", path.display());
            if path.to_str().is_none() {
                tracing::warn!(
                    "{} is not valid UTF-8; the adapter's own codex is used",
                    path.display()
                );
            }
            presets.push(HarnessPreset::codex(path.to_str()));
        }
        None => tracing::info!("Codex (`codex`) is not installed; not offered"),
    }
    presets
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[cfg(unix)]
    fn fake_bin(dir: &Path, name: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    #[cfg(unix)]
    #[test]
    fn opencode_is_registered_only_when_an_executable_is_on_the_path() {
        let with = tempfile::tempdir().expect("tempdir");
        let without = tempfile::tempdir().expect("tempdir");
        fake_bin(with.path(), "opencode", 0o755);
        fake_bin(without.path(), "opencode", 0o644); // not executable
        let path = |d: &Path| format!("relative:{}", d.display());

        let ids = |presets: Vec<HarnessPreset>| -> Vec<String> {
            presets.into_iter().map(|p| p.id).collect()
        };
        let found = installed_presets("haiku", env(&[("PATH", &path(with.path()))]));
        assert_eq!(ids(found), ["claude-code", "opencode"]);
        fake_bin(with.path(), "codex", 0o755);
        fake_bin(without.path(), "codex", 0o644);
        let found = installed_presets("haiku", env(&[("PATH", &path(with.path()))]));
        assert_eq!(ids(found.clone()), ["claude-code", "opencode", "codex"]);
        let codex = found.last().expect("codex");
        let expected = with.path().join("codex").display().to_string();
        assert_eq!(
            codex
                .config(super::super::AgentRole::Implementer, Some("m"), None)
                .env
                .get("CODEX_PATH"),
            Some(&expected),
            "the adapter runs the user's codex"
        );
        let missing = installed_presets("haiku", env(&[("PATH", &path(without.path()))]));
        assert_eq!(ids(missing), ["claude-code"], "not executable: not offered");
        assert_eq!(ids(installed_presets("haiku", env(&[]))), ["claude-code"]);
        assert_eq!(
            find_in_path("opencode", Some(OsStr::new("relative"))),
            None,
            "relative PATH entries are ignored"
        );
    }

    #[test]
    fn the_codex_preset_passes_model_and_effort_through_codex_config() {
        use super::super::AgentRole;
        let p = HarnessPreset::codex(Some("/bin/codex"));
        assert!(p.requires_model && !p.orchestrator_read_only);
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
    fn the_opencode_preset_always_sets_a_model_and_warns_about_the_orchestrator() {
        let p =
            HarnessPreset::opencode(OPENCODE_FALLBACK_MODEL, vec!["OPENCODE_CONFIG_DIR".into()]);
        assert!(p.requires_model);
        assert!(!p.orchestrator_read_only);
        assert!(p.model_env.is_none());
        let info = p.info();
        assert!(info.requires_model && !info.orchestrator_read_only);
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
