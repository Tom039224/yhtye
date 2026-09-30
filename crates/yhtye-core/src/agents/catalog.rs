//! The harness registry and the settings in effect (`core-design.md` §15.2):
//! [`HarnessPreset`]s say how to launch each harness for each role and how to
//! put a model into its [`HarnessConfig`]; [`AgentCatalog`] holds them with the
//! built-in default and an in-memory copy of the stored settings layers.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use serde::Serialize;
use ts_rs::TS;

use super::settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, RoleSettings, effective,
};
use crate::acp::{AgentError, CODEX_CONFIG_ENV, EFFORT_CONFIG_ID, HarnessConfig, ModelSelect};
use crate::secrets::Secrets;

/// The config id used for the model when a harness config names none.
pub const MODEL_CONFIG_ID: &str = "model";

/// Id of the OpenCode preset.
pub const OPENCODE: &str = "opencode";
/// Id of the Claude Code preset.
pub const CLAUDE_CODE: &str = "claude-code";
/// Id of the Codex preset.
pub const CODEX: &str = "codex";

/// Where the models of a harness come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// The harness's own model option, read over ACP in a short session.
    Acp,
    /// Codex: over ACP, except that with the OpenRouter provider in the user's
    /// Codex config the list is OpenRouter's public model list (the adapter
    /// only lists OpenAI's catalog), see [`super::openrouter`].
    Codex,
}

/// How to launch one harness for every role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessPreset {
    /// Stable id stored in settings (`claude-code`).
    pub id: String,
    /// Display name.
    pub label: String,
    pub orchestrator: HarnessConfig,
    pub implementer: HarnessConfig,
    pub investigator: HarnessConfig,
    pub reviewer: HarnessConfig,
    /// Session used to list the models (no MCP, nothing persisted); `None`
    /// uses the implementer config without mode / model switching.
    pub probe: Option<HarnessConfig>,
    /// Environment variable that also carries the model (the adapter's initial
    /// model, e.g. `ANTHROPIC_MODEL`), set with the `set_config_option` value.
    pub model_env: Option<String>,
    /// Environment variable holding a JSON object with the chosen `model` and
    /// `model_reasoning_effort` (Codex: `CODEX_CONFIG`), for harnesses that
    /// refuse `set_config_option` for models outside their own catalog. The
    /// model is still verified through its config option; the effort is then
    /// not sent as an option.
    pub model_config_env: Option<String>,
    /// The config id of the effort option (`effort`; Codex: `reasoning_effort`).
    pub effort_config_id: String,
    pub model_source: ModelSource,
    /// A choice of this harness must name a model (OpenCode: its own default is
    /// the last model the user used, possibly a paid one).
    pub requires_model: bool,
}

/// A registered harness, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HarnessInfo {
    pub id: String,
    pub label: String,
    /// Every choice names a model (no "harness default" entry).
    pub requires_model: bool,
}

impl HarnessPreset {
    /// Claude Code (`HarnessConfig::claude_code*`) with `model` as the config's
    /// own model (used when a choice names none).
    #[must_use]
    pub fn claude_code(model: &str) -> Self {
        Self {
            id: CLAUDE_CODE.into(),
            label: "Claude Code".into(),
            orchestrator: HarnessConfig::claude_code(model),
            implementer: HarnessConfig::claude_code(model),
            investigator: HarnessConfig::claude_code(model),
            reviewer: HarnessConfig::claude_code(model),
            probe: Some(HarnessConfig::claude_code_usage_probe()),
            model_env: Some("ANTHROPIC_MODEL".into()),
            model_config_env: None,
            effort_config_id: EFFORT_CONFIG_ID.into(),
            model_source: ModelSource::Acp,
            requires_model: false,
        }
    }

    /// OpenCode (`opencode acp`, `HarnessConfig::opencode`) for every role
    /// (`acp-harnesses.md` §7). A choice must name a model; `fallback_model` is
    /// only used if one without a model slips through (e.g. an old setting), so
    /// OpenCode never starts on its own last-used model. Every role, the
    /// orchestrator included, runs in `build` mode (all tools).
    /// `env_remove` is dropped from the inherited environment
    /// ([`super::inherited_opencode_env_remove`]).
    #[must_use]
    pub fn opencode(fallback_model: &str, env_remove: Vec<String>) -> Self {
        let mut h = HarnessConfig::opencode(fallback_model);
        h.env_remove = env_remove;
        let mut probe = h.clone();
        probe.mode_after_new = None;
        probe.model = None;
        Self {
            id: OPENCODE.into(),
            label: "OpenCode".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h,
            probe: Some(probe),
            model_env: None,
            model_config_env: None,
            effort_config_id: EFFORT_CONFIG_ID.into(),
            model_source: ModelSource::Acp,
            requires_model: true,
        }
    }

    /// Codex (`HarnessConfig::codex`) for every role (`acp-harnesses.md` §9).
    /// `codex_path` is the user's `codex`. A choice must name a model (the
    /// user's own default model may not even be usable); the model and effort
    /// reach Codex through `CODEX_CONFIG`. Every role runs in
    /// `agent-full-access` mode.
    #[must_use]
    pub fn codex(codex_path: Option<&str>) -> Self {
        let h = HarnessConfig::codex(codex_path);
        Self {
            id: CODEX.into(),
            label: "Codex".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h.clone(),
            probe: Some(HarnessConfig {
                mode_after_new: None,
                ..h
            }),
            model_env: None,
            model_config_env: Some(CODEX_CONFIG_ENV.into()),
            effort_config_id: "reasoning_effort".into(),
            model_source: ModelSource::Codex,
            requires_model: true,
        }
    }

    /// A harness with the given per-role configs (tests, fake agents). The
    /// investigator uses the implementer config.
    #[must_use]
    pub fn fixed(
        id: &str,
        orchestrator: HarnessConfig,
        implementer: HarnessConfig,
        reviewer: HarnessConfig,
    ) -> Self {
        Self {
            id: id.into(),
            label: id.into(),
            orchestrator,
            investigator: implementer.clone(),
            implementer,
            reviewer,
            probe: None,
            model_env: None,
            model_config_env: None,
            effort_config_id: EFFORT_CONFIG_ID.into(),
            model_source: ModelSource::Acp,
            requires_model: false,
        }
    }

    #[must_use]
    pub fn info(&self) -> HarnessInfo {
        HarnessInfo {
            id: self.id.clone(),
            label: self.label.clone(),
            requires_model: self.requires_model,
        }
    }

    fn role_config(&self, role: AgentRole) -> &HarnessConfig {
        match role {
            AgentRole::Orchestrator => &self.orchestrator,
            AgentRole::Implementer => &self.implementer,
            AgentRole::Investigator => &self.investigator,
            AgentRole::Reviewer => &self.reviewer,
        }
    }

    /// The config id of the model option (the one the configs select, else `model`).
    #[must_use]
    pub fn model_config_id(&self) -> &str {
        self.implementer
            .model
            .as_ref()
            .map_or(MODEL_CONFIG_ID, |m| m.config_id.as_str())
    }

    /// The launch config of `role` with `model` and `effort` put in (`None`:
    /// unchanged / not set).
    #[must_use]
    pub fn config(
        &self,
        role: AgentRole,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> HarnessConfig {
        let mut h = self.role_config(role).clone();
        // With a config env var the effort travels in it (and is not verified
        // as an option: the adapter shows none for models it has no metadata of).
        h.effort = effort
            .filter(|_| self.model_config_env.is_none())
            .map(|value| ModelSelect {
                config_id: self.effort_config_id.clone(),
                value: value.to_string(),
            });
        if let Some(model) = model {
            let config_id = h.model.as_ref().map_or_else(
                || self.model_config_id().to_string(),
                |m| m.config_id.clone(),
            );
            h.model = Some(ModelSelect {
                config_id,
                value: model.to_string(),
            });
            if let Some(var) = &self.model_env {
                h.env.insert(var.clone(), model.to_string());
            }
        }
        if let Some(var) = &self.model_config_env
            && let Some(json) = session_config_json(model, effort)
        {
            h.env.insert(var.clone(), json);
        }
        h
    }

    /// The config of the model-listing session: [`HarnessPreset::probe`], else
    /// the implementer config without mode / model switching.
    #[must_use]
    pub fn probe_config(&self) -> HarnessConfig {
        self.probe.clone().unwrap_or_else(|| {
            let mut h = self.implementer.clone();
            h.mode_after_new = None;
            h.model = None;
            h
        })
    }
}

/// The JSON object for [`HarnessPreset::model_config_env`]: `model` and
/// `model_reasoning_effort` (only those given); `None` when neither is.
fn session_config_json(model: Option<&str>, effort: Option<&str>) -> Option<String> {
    let mut map = serde_json::Map::new();
    if let Some(m) = model {
        map.insert("model".into(), m.into());
    }
    if let Some(e) = effort {
        map.insert("model_reasoning_effort".into(), e.into());
    }
    (!map.is_empty()).then(|| serde_json::Value::Object(map).to_string())
}

/// The outcome of [`AgentCatalog::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the session runs.
    pub choice: AgentChoice,
    pub harness: HarnessConfig,
    /// The choice asked for, when its harness is not registered and `choice`
    /// runs instead (shown to the user, `core-design.md` §15.4).
    pub replaced: Option<AgentChoice>,
}

#[derive(Debug, Default)]
struct Layers {
    global: AgentSettingsLayer,
    projects: HashMap<String, AgentSettingsLayer>,
}

/// Registered harnesses, the built-in default and the stored settings (a copy
/// kept in sync by [`crate::runtime::Core`]). Shared by every project.
#[derive(Debug)]
pub struct AgentCatalog {
    presets: Vec<HarnessPreset>,
    builtin: AgentChoice,
    layers: RwLock<Layers>,
    /// The secret environment variables every agent process gets (Stage 7e).
    secrets: Arc<Secrets>,
}

impl AgentCatalog {
    /// `builtin` is used for every role without settings.
    #[must_use]
    pub fn new(presets: Vec<HarnessPreset>, builtin: AgentChoice) -> Self {
        Self {
            presets,
            builtin,
            layers: RwLock::new(Layers::default()),
            secrets: Arc::new(Secrets::none()),
        }
    }

    /// Uses `secrets` for the environment of every agent this catalog launches.
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<Secrets>) -> Self {
        self.secrets = secrets;
        self
    }

    /// The secret environment variables (names and values) for a spawn.
    #[must_use]
    pub fn secrets(&self) -> Arc<Secrets> {
        self.secrets.clone()
    }

    /// One harness `default` with fixed configs and no model selection (tests).
    #[must_use]
    pub fn fixed(
        orchestrator: HarnessConfig,
        implementer: HarnessConfig,
        reviewer: HarnessConfig,
    ) -> Self {
        let preset = HarnessPreset::fixed("default", orchestrator, implementer, reviewer);
        Self::new(vec![preset], AgentChoice::new("default", None))
    }

    #[must_use]
    pub fn builtin(&self) -> &AgentChoice {
        &self.builtin
    }

    #[must_use]
    pub fn harnesses(&self) -> Vec<HarnessInfo> {
        self.presets.iter().map(HarnessPreset::info).collect()
    }

    #[must_use]
    pub fn harness_ids(&self) -> Vec<&str> {
        self.presets.iter().map(|p| p.id.as_str()).collect()
    }

    #[must_use]
    pub fn preset(&self, id: &str) -> Option<&HarnessPreset> {
        self.presets.iter().find(|p| p.id == id)
    }

    /// The global layer and, for `project`, its layer.
    #[must_use]
    pub fn layers(
        &self,
        project: Option<&str>,
    ) -> (AgentSettingsLayer, Option<AgentSettingsLayer>) {
        let l = self.read();
        let p = project.map(|id| l.projects.get(id).cloned().unwrap_or_default());
        (l.global.clone(), p)
    }

    /// The settings in effect for `project` (the global ones for `None`).
    #[must_use]
    pub fn effective(&self, project: Option<&str>) -> AgentSettings {
        let l = self.read();
        effective(
            &self.builtin,
            &l.global,
            project.and_then(|id| l.projects.get(id)),
        )
    }

    /// Replaces the in-memory copy of one role of a layer (after it is stored).
    pub fn set(&self, project: Option<&str>, role: AgentRole, settings: Option<RoleSettings>) {
        let mut l = self.layers.write().unwrap_or_else(PoisonError::into_inner);
        match project {
            None => l.global.set(role, settings),
            Some(id) => l
                .projects
                .entry(id.to_string())
                .or_default()
                .set(role, settings),
        }
    }

    /// Checks `settings` against the registered harnesses
    /// ([`RoleSettings::validate`]) and that every choice of a harness that
    /// [requires a model](HarnessPreset::requires_model) names one.
    pub fn validate(&self, settings: &RoleSettings) -> Result<RoleSettings, String> {
        let valid = settings.validate(&self.harness_ids())?;
        if let Some(c) = valid.candidates.iter().find(|c| {
            c.model.is_none() && self.preset(&c.harness).is_some_and(|p| p.requires_model)
        }) {
            return Err(format!("{}: a model is required", c.harness));
        }
        Ok(valid)
    }

    /// What a new session of `role` in `project` runs: `over` (an allowed
    /// override of the task) if its harness is registered, else the role's
    /// default in effect now, else the built-in default. A choice that was
    /// skipped because its harness is not registered (any more) is reported
    /// in [`Resolved::replaced`].
    pub fn resolve(
        &self,
        project: &str,
        role: AgentRole,
        over: Option<&AgentChoice>,
    ) -> Result<Resolved, AgentError> {
        let mut replaced = None;
        if let Some(choice) = over {
            match self.launch_config(role, choice) {
                Some(harness) => {
                    return Ok(Resolved {
                        choice: choice.clone(),
                        harness,
                        replaced,
                    });
                }
                None => {
                    tracing::warn!(
                        "harness {} of the task's override is not registered; using the default",
                        choice.harness
                    );
                    replaced = Some(choice.clone());
                }
            }
        }
        let default = self.effective(Some(project)).get(role).default.clone();
        if let Some(harness) = self.launch_config(role, &default) {
            return Ok(Resolved {
                choice: default,
                harness,
                replaced,
            });
        }
        tracing::warn!(
            "default harness {} of {} is not registered; using the built-in default",
            default.harness,
            role.as_str()
        );
        let replaced = replaced.or(Some(default));
        self.launch_config(role, &self.builtin)
            .map(|harness| Resolved {
                choice: self.builtin.clone(),
                harness,
                replaced,
            })
            .ok_or_else(|| AgentError::Unsupported {
                requested: format!("harness {}", self.builtin.harness),
                available: self.harness_ids().join(", "),
            })
    }

    /// The launch config of `choice` for `role`, if its harness is registered.
    #[must_use]
    pub fn launch_config(&self, role: AgentRole, choice: &AgentChoice) -> Option<HarnessConfig> {
        self.preset(&choice.harness)
            .map(|p| p.config(role, choice.model.as_deref(), choice.effort.as_deref()))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Layers> {
        self.layers.read().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> AgentCatalog {
        let other = HarnessPreset::fixed(
            "other",
            HarnessConfig::plain("o", vec![]),
            HarnessConfig::plain("i", vec![]),
            HarnessConfig::plain("r", vec![]),
        );
        AgentCatalog::new(
            vec![HarnessPreset::claude_code("haiku"), other],
            AgentChoice::new("claude-code", Some("haiku")),
        )
    }

    #[test]
    fn claude_code_configs_carry_the_chosen_model() {
        let p = HarnessPreset::claude_code("haiku");
        let h = p.config(AgentRole::Implementer, Some("sonnet"), Some("high"));
        assert_eq!(h.model.as_ref().map(|m| m.value.as_str()), Some("sonnet"));
        assert_eq!(
            h.effort,
            Some(ModelSelect {
                config_id: "effort".into(),
                value: "high".into()
            }),
            "the effort is a separate option, applied after the model"
        );
        assert_eq!(
            h.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("sonnet")
        );
        let o = p.config(AgentRole::Orchestrator, Some("haiku"), None);
        assert_eq!(o, HarnessConfig::claude_code("haiku"));
        assert_eq!(
            p.config(AgentRole::Reviewer, None, None),
            HarnessConfig::claude_code("haiku")
        );
        assert_eq!(p.model_config_id(), "model");
        assert!(p.probe_config().model.is_none());
    }

    #[test]
    fn a_model_is_added_to_a_config_without_one() {
        let p = HarnessPreset::fixed(
            "x",
            HarnessConfig::plain("o", vec![]),
            HarnessConfig::plain("i", vec![]),
            HarnessConfig::plain("r", vec![]),
        );
        let h = p.config(AgentRole::Investigator, Some("m"), None);
        assert_eq!(h.command, "i");
        assert_eq!(
            h.model,
            Some(ModelSelect {
                config_id: "model".into(),
                value: "m".into()
            })
        );
        assert!(h.env.is_empty(), "no model env var for this harness");
        assert_eq!(p.config(AgentRole::Reviewer, None, None).command, "r");
    }

    #[test]
    fn resolve_uses_the_override_else_the_current_default() {
        let c = catalog();
        let r = c.resolve("P", AgentRole::Reviewer, None).expect("builtin");
        assert_eq!(r.choice, AgentChoice::new("claude-code", Some("haiku")));
        assert_eq!(r.harness.command, "npx");
        assert_eq!(r.replaced, None);

        let other = AgentChoice::new("other", None);
        c.set(
            Some("P"),
            AgentRole::Reviewer,
            Some(RoleSettings::only(other.clone())),
        );
        let r = c.resolve("P", AgentRole::Reviewer, None).expect("project");
        assert_eq!((r.choice, r.harness.command.as_str()), (other.clone(), "r"));
        let r = c
            .resolve("Q", AgentRole::Reviewer, None)
            .expect("other project");
        assert_eq!(r.choice.harness, "claude-code", "layers are per project");

        let over = AgentChoice::new("claude-code", Some("sonnet"));
        let r = c
            .resolve("P", AgentRole::Reviewer, Some(&over))
            .expect("override");
        assert_eq!((r.choice, r.replaced), (over, None));
        let gone = AgentChoice::new("gone", None);
        let r = c
            .resolve("P", AgentRole::Reviewer, Some(&gone))
            .expect("fallback");
        assert_eq!(
            r.choice, other,
            "an unregistered override falls back to the default"
        );
        assert_eq!(r.replaced, Some(gone), "and says what it replaced");
    }

    #[test]
    fn a_default_naming_an_unregistered_harness_falls_back_to_the_builtin() {
        let c = catalog();
        c.set(
            None,
            AgentRole::Implementer,
            Some(RoleSettings::only(AgentChoice::new("gone", None))),
        );
        let r = c
            .resolve("P", AgentRole::Implementer, None)
            .expect("builtin");
        assert_eq!(r.choice, AgentChoice::new("claude-code", Some("haiku")));
        assert_eq!(r.replaced, Some(AgentChoice::new("gone", None)));
        let broken = AgentCatalog::new(vec![], AgentChoice::new("none", None));
        assert!(broken.resolve("P", AgentRole::Implementer, None).is_err());
    }

    #[test]
    fn layers_are_reported_per_scope() {
        let c = catalog();
        let s = RoleSettings::only(AgentChoice::new("other", None));
        c.set(None, AgentRole::Orchestrator, Some(s.clone()));
        let (global, project) = c.layers(Some("P"));
        assert_eq!(global.orchestrator, Some(s.clone()));
        assert_eq!(project, Some(AgentSettingsLayer::default()));
        assert_eq!(c.effective(Some("P")).orchestrator, s);
        assert_eq!(c.layers(None).1, None);
    }
}
