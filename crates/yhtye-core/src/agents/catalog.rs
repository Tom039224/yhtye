//! The harness registry and the settings in effect (`core-design.md` §15.2):
//! [`HarnessPreset`]s say how to launch each harness for each role and how to
//! put a model into its [`HarnessConfig`]; [`AgentCatalog`] holds them with the
//! built-in default and an in-memory copy of the stored settings layers.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use ts_rs::TS;

use super::settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, Candidate, RoleSettings, effective,
};
use crate::acp::{
    AgentError, CODEX_CONFIG_ENV, EFFORT_CONFIG_ID, GROK_BUILD_EFFORT_CONFIG_ID, HarnessConfig,
    MINIMAX_CODE_EFFORT_CONFIG_ID, MINIMAX_CODE_UNUSABLE_MODELS, ModelSelect,
};
use crate::secrets::Secrets;

/// Shown when an agent is started and no harness is registered.
pub const NO_HARNESS_MESSAGE: &str = "使えるハーネスがありません (設定 › ハーネス を確認)";

/// The config id used for the model when a harness config names none.
pub const MODEL_CONFIG_ID: &str = "model";

/// Id of the OpenCode preset.
pub const OPENCODE: &str = "opencode";
/// Id of the Claude Code preset.
pub const CLAUDE_CODE: &str = "claude-code";
/// Id of the Codex preset.
pub const CODEX: &str = "codex";
/// Id of the Devin preset.
pub const DEVIN: &str = "devin";
/// Id of the MiniMax Code preset (`mcode`).
pub const MINIMAX_CODE: &str = "minimax-code";
/// Id of the Google Antigravity preset (`agy_acp_server`).
pub const ANTIGRAVITY: &str = "antigravity";
/// Id of the Grok Build preset (`grok`).
pub const GROK_BUILD: &str = "grok-build";

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
    /// The config id of the effort option (`effort`; Codex: `reasoning_effort`);
    /// `None`: the harness has no effort option (Google Antigravity, whose
    /// thinking levels are separate models), so no model has an effort.
    pub effort_config_id: Option<String>,
    pub model_source: ModelSource,
    /// A choice of this harness must name a model (OpenCode: its own default is
    /// the last model the user used, possibly a paid one).
    pub requires_model: bool,
    /// Models the harness lists but cannot run (selecting them is refused):
    /// left out of the model listing.
    pub unusable_models: Vec<String>,
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
            effort_config_id: Some(EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: false,
            unusable_models: Vec::new(),
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
            effort_config_id: Some(EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: true,
            unusable_models: Vec::new(),
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
            effort_config_id: Some("reasoning_effort".into()),
            model_source: ModelSource::Codex,
            requires_model: true,
            unusable_models: Vec::new(),
        }
    }

    /// Devin (`HarnessConfig::devin`) at `command` for every role. Devin's own
    /// default model applies unless a choice names one (a `model` option read
    /// over ACP, like Claude Code's), and the effort goes to its `thought_level`
    /// option (whatever id it has, see [`crate::acp::effort_option`]). The
    /// model listing session does no mode / model switching.
    #[must_use]
    pub fn devin(command: &str) -> Self {
        let h = HarnessConfig::devin(command);
        Self {
            id: DEVIN.into(),
            label: "Devin".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h.clone(),
            probe: Some(HarnessConfig {
                mode_after_new: None,
                ..h
            }),
            model_env: None,
            model_config_env: None,
            effort_config_id: Some(EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: false,
            unusable_models: Vec::new(),
        }
    }

    /// MiniMax Code (`mcode acp`, `HarnessConfig::minimax_code`) at `command`
    /// for every role (`acp-harnesses.md` §11). A choice must name a model
    /// (one without runs on the config's own default, the cheapest, so the
    /// user's own default model is never what a role runs on by accident). The
    /// effort goes to its `thinkingEffort` option, which only the models with
    /// effort levels have, so it is set after the model. The model listing
    /// session does no model switching and leaves out the models `mcode` lists
    /// but refuses ([`MINIMAX_CODE_UNUSABLE_MODELS`]).
    #[must_use]
    pub fn minimax_code(command: &str) -> Self {
        let h = HarnessConfig::minimax_code(command);
        Self {
            id: MINIMAX_CODE.into(),
            label: "MiniMax Code".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h.clone(),
            probe: Some(HarnessConfig { model: None, ..h }),
            model_env: None,
            model_config_env: None,
            effort_config_id: Some(MINIMAX_CODE_EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: true,
            unusable_models: MINIMAX_CODE_UNUSABLE_MODELS
                .iter()
                .map(|m| (*m).to_string())
                .collect(),
        }
    }

    /// Google Antigravity (`agy_acp_server`, `HarnessConfig::antigravity`) at
    /// `command` for every role (`acp-harnesses.md` §13). Its own default model
    /// is a constant of the binary, not the user's setting, so a choice need not
    /// name a model: one without runs on the config's default, the cheapest
    /// (`gemini-3.8-flash-low`). There is no effort option (the thinking level
    /// is part of the model id), so no model has an effort and an effort in a
    /// choice is not sent. The model listing session does no model switching.
    #[must_use]
    pub fn antigravity(command: &str) -> Self {
        let h = HarnessConfig::antigravity(command);
        Self {
            id: ANTIGRAVITY.into(),
            label: "Google Antigravity".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h.clone(),
            probe: Some(HarnessConfig { model: None, ..h }),
            model_env: None,
            model_config_env: None,
            effort_config_id: None,
            model_source: ModelSource::Acp,
            requires_model: false,
            unusable_models: Vec::new(),
        }
    }

    /// Grok Build (`grok agent --no-leader stdio`, `HarnessConfig::grok_build`)
    /// at `command` for every role (`acp-harnesses.md` §12). `grok`'s own
    /// default model applies unless a choice names one (its `model` option,
    /// read over ACP), and the effort goes to its `reasoning_effort` option
    /// ([`GROK_BUILD_EFFORT_CONFIG_ID`]). There are no modes to switch, so the
    /// model listing session differs from the roles' only in setting no model.
    #[must_use]
    pub fn grok_build(command: &str) -> Self {
        let h = HarnessConfig::grok_build(command);
        Self {
            id: GROK_BUILD.into(),
            label: "Grok Build".into(),
            orchestrator: h.clone(),
            implementer: h.clone(),
            investigator: h.clone(),
            reviewer: h.clone(),
            probe: Some(HarnessConfig { model: None, ..h }),
            model_env: None,
            model_config_env: None,
            effort_config_id: Some(GROK_BUILD_EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: false,
            unusable_models: Vec::new(),
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
            effort_config_id: Some(EFFORT_CONFIG_ID.into()),
            model_source: ModelSource::Acp,
            requires_model: false,
            unusable_models: Vec::new(),
        }
    }

    /// Runs `command` (an absolute path found on this machine) instead of the
    /// one the configs name, for every role and the model listing session.
    #[must_use]
    pub fn with_command(mut self, command: &str) -> Self {
        for h in [
            &mut self.orchestrator,
            &mut self.implementer,
            &mut self.investigator,
            &mut self.reviewer,
        ]
        .into_iter()
        .chain(self.probe.as_mut())
        {
            h.command = command.to_string();
        }
        self
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
            .zip(self.effort_config_id.as_ref())
            .map(|(value, config_id)| ModelSelect {
                config_id: config_id.clone(),
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

/// The harnesses that are registered and what a role without settings runs;
/// replaced as a whole when the installed harnesses are detected again.
#[derive(Debug)]
struct Registry {
    presets: Vec<HarnessPreset>,
    builtin: AgentChoice,
}

/// Registered harnesses, the built-in default and the stored settings (a copy
/// kept in sync by [`crate::runtime::Core`]). Shared by every project. The
/// registered harnesses can change while the app runs ([`AgentCatalog::refresh`]);
/// sessions already running are not affected.
#[derive(Debug)]
pub struct AgentCatalog {
    registry: RwLock<Registry>,
    layers: RwLock<Layers>,
    /// The secret environment variables every agent process gets (Stage 7e).
    secrets: Arc<Secrets>,
}

impl AgentCatalog {
    /// `builtin` is used for every role without settings.
    #[must_use]
    pub fn new(presets: Vec<HarnessPreset>, builtin: AgentChoice) -> Self {
        Self {
            registry: RwLock::new(Registry { presets, builtin }),
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

    /// Replaces the registered harnesses and the built-in default (after the
    /// installed harnesses were detected again).
    pub fn refresh(&self, presets: Vec<HarnessPreset>, builtin: AgentChoice) {
        *self.registry_mut() = Registry { presets, builtin };
    }

    #[must_use]
    pub fn builtin(&self) -> AgentChoice {
        self.registry().builtin.clone()
    }

    #[must_use]
    pub fn harnesses(&self) -> Vec<HarnessInfo> {
        self.registry()
            .presets
            .iter()
            .map(HarnessPreset::info)
            .collect()
    }

    #[must_use]
    pub fn harness_ids(&self) -> Vec<String> {
        self.registry()
            .presets
            .iter()
            .map(|p| p.id.clone())
            .collect()
    }

    #[must_use]
    pub fn preset(&self, id: &str) -> Option<HarnessPreset> {
        self.registry().presets.iter().find(|p| p.id == id).cloned()
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
            &self.builtin(),
            &l.global,
            project.and_then(|id| l.projects.get(id)),
        )
    }

    /// [`AgentCatalog::effective`] as far as the registered harnesses go
    /// ([`AgentCatalog::available`], per role): what the orchestrator may choose
    /// from. The stored settings are not changed.
    #[must_use]
    pub fn available_settings(&self, project: Option<&str>) -> AgentSettings {
        let s = self.effective(project);
        AgentSettings {
            orchestrator: self.available(&s.orchestrator),
            implementer: self.available(&s.implementer),
            investigator: self.available(&s.investigator),
            reviewer: self.available(&s.reviewer),
        }
    }

    /// `settings` without the rows of harnesses that are not registered. When
    /// the default is one of them, the built-in default takes its place if it is
    /// still a row, else the first row left; with no row left, only the built-in
    /// default remains.
    #[must_use]
    pub fn available(&self, settings: &RoleSettings) -> RoleSettings {
        let reg = self.registry();
        let candidates: Vec<Candidate> = settings
            .candidates
            .iter()
            .filter(|c| reg.presets.iter().any(|p| p.id == c.harness))
            .cloned()
            .collect();
        let has = |choice: &AgentChoice| candidates.iter().any(|c| &c.choice() == choice);
        let default = if has(&settings.default) {
            settings.default.clone()
        } else if has(&reg.builtin) {
            reg.builtin.clone()
        } else if let Some(first) = candidates.first() {
            first.choice()
        } else {
            return RoleSettings::only(reg.builtin.clone());
        };
        RoleSettings {
            candidates,
            default,
        }
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
    /// ([`RoleSettings::validate_keeping`]) and that every choice of a harness
    /// that [requires a model](HarnessPreset::requires_model) names one.
    /// `existing` is what the role has now: its rows of a harness that is not
    /// registered (any more) may stay, but no such row can be added.
    pub fn validate(
        &self,
        settings: &RoleSettings,
        existing: &RoleSettings,
    ) -> Result<RoleSettings, String> {
        let ids = self.harness_ids();
        let valid = settings.validate_keeping(
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
            &existing.candidates,
        )?;
        if let Some(c) = valid.candidates.iter().find(|c| {
            c.model.is_none() && self.preset(&c.harness).is_some_and(|p| p.requires_model)
        }) {
            return Err(format!("{}: a model is required", c.harness));
        }
        Ok(valid)
    }

    /// What a new session of `role` in `project` runs: `over` (an allowed
    /// override of the task) if its harness is registered, else the role's
    /// default in effect now as far as the registered harnesses go
    /// ([`AgentCatalog::available`]: the default the orchestrator is shown). A
    /// choice that was skipped because its harness is not registered (any
    /// more) is reported in [`Resolved::replaced`]. Fails with [`AgentError::Setup`] when no
    /// harness is registered, or the choice is of a harness that needs a model
    /// and names none (Codex as the built-in default).
    pub fn resolve(
        &self,
        project: &str,
        role: AgentRole,
        over: Option<&AgentChoice>,
    ) -> Result<Resolved, AgentError> {
        if self.registry().presets.is_empty() {
            return Err(AgentError::Setup {
                message: NO_HARNESS_MESSAGE.into(),
            });
        }
        let resolved = self.resolve_registered(project, role, over)?;
        if resolved.choice.model.is_none()
            && self
                .preset(&resolved.choice.harness)
                .is_some_and(|p| p.requires_model)
        {
            return Err(AgentError::Setup {
                message: format!(
                    "{} にはモデルの指定が必要です (設定 › ハーネス でモデルを選んでください)",
                    resolved.choice.harness
                ),
            });
        }
        Ok(resolved)
    }

    fn resolve_registered(
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
        // The default the orchestrator is shown ([`AgentCatalog::available`]),
        // so a task that names no harness runs what the prompt and `get_status`
        // call the default.
        let settings = self.effective(Some(project));
        let stored = &settings.get(role).default;
        let default = self.available(settings.get(role)).default;
        if &default != stored {
            tracing::warn!(
                "default harness {} of {} is not registered; using {}",
                stored.harness,
                role.as_str(),
                default.harness
            );
            replaced = replaced.or_else(|| Some(stored.clone()));
        }
        self.launch_config(role, &default)
            .map(|harness| Resolved {
                choice: default.clone(),
                harness,
                replaced,
            })
            .ok_or_else(|| AgentError::Unsupported {
                requested: format!("harness {}", default.harness),
                available: self.harness_ids().join(", "),
            })
    }

    /// The launch config of `choice` for `role`, if its harness is registered.
    #[must_use]
    pub fn launch_config(&self, role: AgentRole, choice: &AgentChoice) -> Option<HarnessConfig> {
        self.preset(&choice.harness)
            .map(|p| p.config(role, choice.model.as_deref(), choice.effort.as_deref()))
    }

    fn read(&self) -> RwLockReadGuard<'_, Layers> {
        self.layers.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn registry(&self) -> RwLockReadGuard<'_, Registry> {
        self.registry.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn registry_mut(&self) -> RwLockWriteGuard<'_, Registry> {
        self.registry
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{PermissionPolicy, SystemPromptStyle};

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
    fn devin_preset_runs_every_role_the_same_and_selects_model_and_effort_over_acp() {
        let p = HarnessPreset::devin("/usr/bin/devin");
        assert_eq!((p.id.as_str(), p.label.as_str()), ("devin", "Devin"));
        assert!(!p.requires_model);
        assert_eq!(p.model_source, ModelSource::Acp);
        assert_eq!(p.effort_config_id.as_deref(), Some("effort"));
        assert_eq!(
            (p.model_env.as_ref(), p.model_config_env.as_ref()),
            (None, None)
        );
        let expected = HarnessConfig::devin("/usr/bin/devin");
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            assert_eq!(p.config(role, None, None), expected);
        }
        let h = p.config(AgentRole::Implementer, Some("claude-opus"), Some("high"));
        assert_eq!(
            h.model,
            Some(ModelSelect {
                config_id: "model".into(),
                value: "claude-opus".into()
            })
        );
        assert_eq!(
            h.effort,
            Some(ModelSelect {
                config_id: "effort".into(),
                value: "high".into()
            })
        );
        assert!(h.env.is_empty(), "the model travels as an option only");
        assert_eq!(p.info().id, "devin");
    }

    #[test]
    fn minimax_code_preset_never_touches_the_mode_and_defaults_to_its_cheapest_model() {
        let command = "/home/u/.minimax-code/bin/mcode";
        let p = HarnessPreset::minimax_code(command);
        assert_eq!(
            (p.id.as_str(), p.label.as_str()),
            ("minimax-code", "MiniMax Code")
        );
        assert!(
            p.requires_model,
            "its own default is the user's, maybe paid"
        );
        assert_eq!(p.model_config_id(), "model");
        assert_eq!(p.effort_config_id.as_deref(), Some("thinkingEffort"));
        assert_eq!(
            (p.model_env.as_ref(), p.model_config_env.as_ref()),
            (None, None)
        );
        let expected = HarnessConfig::minimax_code(command);
        assert_eq!(
            (expected.command.as_str(), expected.args.as_slice()),
            (command, &["acp".to_string()][..])
        );
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            let h = p.config(role, None, None);
            assert_eq!(h, expected);
            // `permissionMode` and the mode are saved to the user's own settings.
            assert_eq!(h.mode_after_new, None);
            assert_eq!(
                h.model.as_ref().map(|m| m.value.as_str()),
                Some("m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking"),
                "a choice without a model runs on the cheapest"
            );
            assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
            assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
            assert_eq!(h.system_prompt_note, None);
        }
        let h = p.config(
            AgentRole::Implementer,
            Some("m:minimax:MiniMax-M3:v:thinking"),
            Some("high"),
        );
        assert_eq!(
            (
                h.model.as_ref().map(|m| m.value.as_str()),
                h.effort
                    .as_ref()
                    .map(|e| (e.config_id.as_str(), e.value.as_str()))
            ),
            (
                Some("m:minimax:MiniMax-M3:v:thinking"),
                Some(("thinkingEffort", "high"))
            )
        );
        assert!(h.env.is_empty(), "the model travels as an option only");

        // The listing session switches nothing, and leaves out the model that
        // `mcode` lists but refuses (never the default).
        let probe = p.probe_config();
        assert_eq!(probe.command, command);
        assert!(probe.mode_after_new.is_none() && probe.model.is_none());
        assert_eq!(
            p.unusable_models,
            ["m:minimax:MiniMax-M3.1-Flash-Preview:v:"]
        );
        assert!(
            !p.unusable_models
                .contains(&crate::acp::MINIMAX_CODE_DEFAULT_MODEL.to_string())
        );
        assert!(
            HarnessPreset::devin("devin").unusable_models.is_empty(),
            "only MiniMax Code names models to leave out"
        );
    }

    #[test]
    fn antigravity_preset_runs_on_its_cheapest_model_and_offers_no_effort() {
        let command = "/home/u/.local/share/agy-acp-server/agy_acp_server.par";
        let p = HarnessPreset::antigravity(command);
        assert_eq!(
            (p.id.as_str(), p.label.as_str()),
            ("antigravity", "Google Antigravity")
        );
        assert!(
            !p.requires_model,
            "its default is a constant of the binary, not the user's setting"
        );
        assert_eq!(p.model_config_id(), "model");
        assert_eq!(p.effort_config_id, None, "the thinking level is the model");
        assert_eq!(
            (p.model_env.as_ref(), p.model_config_env.as_ref()),
            (None, None)
        );
        assert!(p.unusable_models.is_empty());
        let expected = HarnessConfig::antigravity(command);
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            let h = p.config(role, None, None);
            assert_eq!(h, expected);
            assert!(h.args.is_empty() && h.env.is_empty() && h.mode_after_new.is_none());
            assert_eq!(
                h.model.as_ref().map(|m| m.value.as_str()),
                Some("gemini-3.8-flash-low"),
                "a choice without a model runs on the cheapest"
            );
            assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
            assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        }

        // A chosen model replaces the default; an effort is not sent anywhere.
        let h = p.config(
            AgentRole::Implementer,
            Some("gemini-3.1-pro-high"),
            Some("high"),
        );
        assert_eq!(
            h.model.as_ref().map(|m| m.value.as_str()),
            Some("gemini-3.1-pro-high")
        );
        assert!(h.effort.is_none() && h.env.is_empty(), "no effort option");

        // The listing session switches nothing.
        let probe = p.probe_config();
        assert_eq!(probe.command, command);
        assert!(probe.mode_after_new.is_none() && probe.model.is_none());
        for other in [
            HarnessPreset::claude_code("haiku"),
            HarnessPreset::devin("devin"),
            HarnessPreset::minimax_code("mcode"),
        ] {
            assert!(other.effort_config_id.is_some(), "{} has efforts", other.id);
        }
    }

    #[test]
    fn grok_build_preset_runs_every_role_the_same_and_sets_reasoning_effort() {
        let command = "/home/u/.grok/bin/grok";
        let p = HarnessPreset::grok_build(command);
        assert_eq!(
            (p.id.as_str(), p.label.as_str()),
            ("grok-build", "Grok Build")
        );
        assert!(!p.requires_model, "grok's own default model is usable");
        assert_eq!(p.model_source, ModelSource::Acp);
        assert_eq!(p.model_config_id(), "model");
        assert_eq!(p.effort_config_id.as_deref(), Some("reasoning_effort"));
        assert_eq!(
            (p.model_env.as_ref(), p.model_config_env.as_ref()),
            (None, None)
        );
        let expected = HarnessConfig::grok_build(command);
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            let h = p.config(role, None, None);
            assert_eq!(h, expected);
            assert_eq!(h.mode_after_new, None, "grok advertises no modes");
        }
        let h = p.config(AgentRole::Implementer, Some("grok-4.7"), Some("low"));
        assert_eq!(
            (h.model, h.effort),
            (
                Some(ModelSelect {
                    config_id: "model".into(),
                    value: "grok-4.7".into()
                }),
                Some(ModelSelect {
                    config_id: "reasoning_effort".into(),
                    value: "low".into()
                })
            )
        );
        assert!(h.env.is_empty(), "the model travels as an option only");
        let probe = p.probe_config();
        assert_eq!(
            (probe.command.as_str(), probe.args.as_slice()),
            (command, &expected.args[..])
        );
        assert!(probe.mode_after_new.is_none() && probe.model.is_none());
    }

    #[test]
    fn devin_probe_switches_neither_mode_nor_model() {
        let probe = HarnessPreset::devin("devin").probe_config();
        assert_eq!(
            (probe.command.as_str(), probe.args.as_slice()),
            ("devin", &["acp".to_string()][..])
        );
        assert!(probe.mode_after_new.is_none() && probe.model.is_none());
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
    }

    #[test]
    fn without_a_registered_harness_starting_an_agent_says_what_to_check() {
        let broken = AgentCatalog::new(vec![], AgentChoice::new("claude-code", Some("haiku")));
        let e = broken
            .resolve("P", AgentRole::Implementer, None)
            .expect_err("nothing to run");
        assert_eq!(
            e,
            AgentError::Setup {
                message: NO_HARNESS_MESSAGE.into()
            }
        );
        assert_eq!(
            e.to_string(),
            "使えるハーネスがありません (設定 › ハーネス を確認)"
        );
    }

    #[test]
    fn a_builtin_default_that_needs_a_model_asks_for_one() {
        let c = AgentCatalog::new(
            vec![HarnessPreset::codex(Some("/c/codex"))],
            AgentChoice::new("codex", None),
        );
        let e = c
            .resolve("P", AgentRole::Orchestrator, None)
            .expect_err("codex has no default model");
        let AgentError::Setup { message } = e else {
            panic!("{e:?}");
        };
        assert!(
            message.contains("codex") && message.contains("設定"),
            "{message}"
        );
        // Once the settings name a model it runs.
        c.set(
            None,
            AgentRole::Orchestrator,
            Some(RoleSettings::only(AgentChoice::new("codex", Some("gpt-x")))),
        );
        let r = c
            .resolve("P", AgentRole::Orchestrator, None)
            .expect("named");
        assert_eq!(r.choice.model.as_deref(), Some("gpt-x"));
    }

    #[test]
    fn refresh_replaces_the_registered_harnesses_and_the_builtin_default() {
        let c = catalog();
        assert_eq!(c.harness_ids(), ["claude-code", "other"]);
        assert!(c.preset("other").is_some());
        let devin = AgentChoice::new("devin", None);
        c.refresh(vec![HarnessPreset::devin("/d/devin")], devin.clone());
        assert_eq!(c.harness_ids(), ["devin"]);
        assert!(c.preset("other").is_none(), "gone");
        assert_eq!(c.builtin(), devin);
        assert_eq!(c.harnesses()[0].label, "Devin");
        let r = c.resolve("P", AgentRole::Reviewer, None).expect("builtin");
        assert_eq!((r.choice, r.harness.command.as_str()), (devin, "/d/devin"));
        assert!(
            c.launch_config(AgentRole::Reviewer, &AgentChoice::new("other", None))
                .is_none()
        );
        let s = RoleSettings::only(AgentChoice::new("other", None));
        let current = RoleSettings::only(AgentChoice::new("devin", None));
        assert!(
            c.validate(&s, &current).is_err(),
            "no longer a known harness"
        );
    }

    #[test]
    fn a_default_that_is_gone_resolves_to_the_default_the_orchestrator_is_shown() {
        // The built-in default (Claude Code) is not among the rows: the first
        // row left is both what the prompt calls the default and what runs.
        let c = catalog();
        let gone = AgentChoice::new("gone", Some("x"));
        let other = AgentChoice::new("other", None);
        let rows = RoleSettings {
            candidates: vec![gone.clone().into(), other.clone().into()],
            default: gone.clone(),
        };
        c.set(None, AgentRole::Implementer, Some(rows));
        assert_eq!(c.available_settings(None).implementer.default, other);
        let r = c
            .resolve("P", AgentRole::Implementer, None)
            .expect("the row left");
        assert_eq!((r.choice, r.harness.command.as_str()), (other, "i"));
        assert_eq!(r.replaced, Some(gone), "the stored default is reported");
        // A role without settings still runs the built-in default.
        let r = c
            .resolve("P", AgentRole::Reviewer, None)
            .expect("no settings: the builtin default");
        assert_eq!(
            (r.choice.harness.as_str(), r.replaced),
            ("claude-code", None)
        );
    }

    #[test]
    fn validation_keeps_the_rows_of_a_harness_that_is_gone_but_refuses_new_ones() {
        let c = catalog();
        let cc = AgentChoice::new("claude-code", Some("haiku"));
        let gone = AgentChoice::new("gone", Some("x"));
        let existing = RoleSettings {
            candidates: vec![
                Candidate::from(gone.clone()).with_note("cheap"),
                cc.clone().into(),
            ],
            default: cc.clone(),
        };
        // Another row is added and a note is changed; the row of `gone` stays.
        let edited = RoleSettings {
            candidates: vec![
                Candidate::from(gone.clone()).with_note("cheap"),
                Candidate::from(cc.clone()).with_note("quick edits"),
                AgentChoice::new("other", None).into(),
            ],
            default: cc.clone(),
        };
        assert_eq!(c.validate(&edited, &existing), Ok(edited.clone()));
        assert!(
            c.validate(&edited, &RoleSettings::only(cc.clone()))
                .unwrap_err()
                .contains("unknown harness gone"),
            "a row that is not already there is new"
        );
        // A different row of an unregistered harness is new, kept ones or not.
        let mut added = edited.clone();
        added
            .candidates
            .push(AgentChoice::new("gone", Some("y")).into());
        assert!(c.validate(&added, &existing).is_err());
        // Registered harnesses are still checked for what a row needs.
        let codex = AgentCatalog::new(
            vec![HarnessPreset::codex(Some("/c/codex"))],
            AgentChoice::new("codex", Some("m")),
        );
        let no_model = RoleSettings::only(AgentChoice::new("codex", None));
        assert_eq!(
            codex.validate(&no_model, &no_model),
            Err("codex: a model is required".into())
        );
    }

    #[test]
    fn a_preset_can_run_a_command_found_elsewhere() {
        let p = HarnessPreset::claude_code("haiku").with_command("/n/npx");
        for role in [
            AgentRole::Orchestrator,
            AgentRole::Implementer,
            AgentRole::Investigator,
            AgentRole::Reviewer,
        ] {
            assert_eq!(p.config(role, None, None).command, "/n/npx");
        }
        assert_eq!(p.probe_config().command, "/n/npx");
        let expected = HarnessConfig {
            command: "/n/npx".into(),
            ..HarnessConfig::claude_code("haiku")
        };
        assert_eq!(p.config(AgentRole::Reviewer, None, None), expected);
        let fixed = HarnessPreset::fixed(
            "x",
            HarnessConfig::plain("o", vec![]),
            HarnessConfig::plain("i", vec![]),
            HarnessConfig::plain("r", vec![]),
        )
        .with_command("/x");
        assert_eq!(
            fixed.probe_config().command,
            "/x",
            "no probe: the implementer's"
        );
    }

    #[test]
    fn available_settings_leave_out_rows_of_unregistered_harnesses() {
        let c = catalog();
        let cc = AgentChoice::new("claude-code", Some("haiku"));
        let gone = AgentChoice::new("gone", Some("x"));
        let other = AgentChoice::new("other", None);
        let both = |default: &AgentChoice| RoleSettings {
            candidates: vec![
                Candidate::from(gone.clone()).with_note("cheap"),
                Candidate::from(cc.clone()),
                Candidate::from(other.clone()),
            ],
            default: default.clone(),
        };
        // The default survives.
        let a = c.available(&both(&other));
        assert_eq!(a.default, other);
        assert_eq!(
            a.candidates
                .iter()
                .map(Candidate::choice)
                .collect::<Vec<_>>(),
            [cc.clone(), other.clone()]
        );
        // The default is gone: the builtin default if it is still a row.
        assert_eq!(c.available(&both(&gone)).default, cc);
        // ... else the first row left.
        let rows = RoleSettings {
            candidates: vec![gone.clone().into(), other.clone().into()],
            default: gone.clone(),
        };
        let a = c.available(&rows);
        assert_eq!((a.default, a.candidates.len()), (other.clone(), 1));
        // Every row gone: only the builtin default.
        let a = c.available(&RoleSettings::only(gone.clone()));
        assert_eq!(a, RoleSettings::only(cc.clone()));
        // The stored settings are untouched.
        c.set(None, AgentRole::Implementer, Some(both(&gone)));
        assert_eq!(c.effective(None).implementer, both(&gone));
        assert_eq!(c.available_settings(None).implementer.candidates.len(), 2);
        assert_eq!(
            c.available_settings(None).orchestrator,
            RoleSettings::only(cc)
        );
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
