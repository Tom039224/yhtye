//! The harness registry and the settings in effect (`core-design.md` §15.2):
//! [`HarnessPreset`]s say how to launch each harness for each role and how to
//! put a model into its [`HarnessConfig`]; [`AgentCatalog`] holds them with the
//! built-in default and an in-memory copy of the stored settings layers.

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use serde::Serialize;
use ts_rs::TS;

use super::settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, RoleSettings, effective,
};
use crate::acp::{AgentError, HarnessConfig, ModelSelect};

/// The config id used for the model when a harness config names none.
pub const MODEL_CONFIG_ID: &str = "model";

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
}

/// A registered harness, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HarnessInfo {
    pub id: String,
    pub label: String,
}

impl HarnessPreset {
    /// Claude Code (`HarnessConfig::claude_code*`) with `model` as the config's
    /// own model (used when a choice names none).
    #[must_use]
    pub fn claude_code(model: &str) -> Self {
        Self {
            id: "claude-code".into(),
            label: "Claude Code".into(),
            orchestrator: HarnessConfig::claude_code_orchestrator(model),
            implementer: HarnessConfig::claude_code(model),
            investigator: HarnessConfig::claude_code(model),
            reviewer: HarnessConfig::claude_code(model),
            probe: Some(HarnessConfig::claude_code_usage_probe()),
            model_env: Some("ANTHROPIC_MODEL".into()),
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
        }
    }

    #[must_use]
    pub fn info(&self) -> HarnessInfo {
        HarnessInfo {
            id: self.id.clone(),
            label: self.label.clone(),
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

    /// The launch config of `role` with `model` put in (`None`: unchanged).
    #[must_use]
    pub fn config(&self, role: AgentRole, model: Option<&str>) -> HarnessConfig {
        let mut h = self.role_config(role).clone();
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
}

impl AgentCatalog {
    /// `builtin` is used for every role without settings.
    #[must_use]
    pub fn new(presets: Vec<HarnessPreset>, builtin: AgentChoice) -> Self {
        Self {
            presets,
            builtin,
            layers: RwLock::new(Layers::default()),
        }
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

    /// What a new session of `role` in `project` runs: `over` (an allowed
    /// override of the task) if its harness is registered, else the role's
    /// default in effect now. Returns the choice and its launch config.
    pub fn resolve(
        &self,
        project: &str,
        role: AgentRole,
        over: Option<&AgentChoice>,
    ) -> Result<(AgentChoice, HarnessConfig), AgentError> {
        if let Some(choice) = over {
            match self.launch_config(role, choice) {
                Some(h) => return Ok((choice.clone(), h)),
                None => tracing::warn!(
                    "harness {} of the task's override is not registered; using the default",
                    choice.harness
                ),
            }
        }
        let default = self.effective(Some(project)).get(role).default.clone();
        if let Some(h) = self.launch_config(role, &default) {
            return Ok((default, h));
        }
        tracing::warn!(
            "default harness {} of {} is not registered; using the built-in default",
            default.harness,
            role.as_str()
        );
        self.launch_config(role, &self.builtin)
            .map(|h| (self.builtin.clone(), h))
            .ok_or_else(|| AgentError::Unsupported {
                requested: format!("harness {}", self.builtin.harness),
                available: self.harness_ids().join(", "),
            })
    }

    /// The launch config of `choice` for `role`, if its harness is registered.
    #[must_use]
    pub fn launch_config(&self, role: AgentRole, choice: &AgentChoice) -> Option<HarnessConfig> {
        self.preset(&choice.harness)
            .map(|p| p.config(role, choice.model.as_deref()))
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
        let h = p.config(AgentRole::Implementer, Some("sonnet"));
        assert_eq!(h.model.as_ref().map(|m| m.value.as_str()), Some("sonnet"));
        assert_eq!(
            h.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("sonnet")
        );
        let o = p.config(AgentRole::Orchestrator, Some("haiku"));
        assert_eq!(o, HarnessConfig::claude_code_orchestrator("haiku"));
        assert_eq!(
            p.config(AgentRole::Reviewer, None),
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
        let h = p.config(AgentRole::Investigator, Some("m"));
        assert_eq!(h.command, "i");
        assert_eq!(
            h.model,
            Some(ModelSelect {
                config_id: "model".into(),
                value: "m".into()
            })
        );
        assert!(h.env.is_empty(), "no model env var for this harness");
        assert_eq!(p.config(AgentRole::Reviewer, None).command, "r");
    }

    #[test]
    fn resolve_uses_the_override_else_the_current_default() {
        let c = catalog();
        let (choice, h) = c.resolve("P", AgentRole::Reviewer, None).expect("builtin");
        assert_eq!(choice, AgentChoice::new("claude-code", Some("haiku")));
        assert_eq!(h.command, "npx");

        let other = AgentChoice::new("other", None);
        c.set(
            Some("P"),
            AgentRole::Reviewer,
            Some(RoleSettings::only(other.clone())),
        );
        let (choice, h) = c.resolve("P", AgentRole::Reviewer, None).expect("project");
        assert_eq!((choice, h.command.as_str()), (other.clone(), "r"));
        let (choice, _) = c
            .resolve("Q", AgentRole::Reviewer, None)
            .expect("other project");
        assert_eq!(choice.harness, "claude-code", "layers are per project");

        let over = AgentChoice::new("claude-code", Some("sonnet"));
        let (choice, _) = c
            .resolve("P", AgentRole::Reviewer, Some(&over))
            .expect("override");
        assert_eq!(choice, over);
        let gone = AgentChoice::new("gone", None);
        let (choice, _) = c
            .resolve("P", AgentRole::Reviewer, Some(&gone))
            .expect("fallback");
        assert_eq!(
            choice, other,
            "an unregistered override falls back to the default"
        );
    }

    #[test]
    fn a_default_naming_an_unregistered_harness_falls_back_to_the_builtin() {
        let c = catalog();
        c.set(
            None,
            AgentRole::Implementer,
            Some(RoleSettings::only(AgentChoice::new("gone", None))),
        );
        let (choice, _) = c
            .resolve("P", AgentRole::Implementer, None)
            .expect("builtin");
        assert_eq!(choice, AgentChoice::new("claude-code", Some("haiku")));
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
