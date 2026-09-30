//! The models a harness offers, read over ACP (`core-design.md` §15.6): a
//! short-lived session is opened without sending a prompt (no model call) and
//! its model option in `config_options` is read. The efforts of a model
//! (Stage 7d) depend on the model, so they are read by selecting that model in
//! the same kind of session and reading the `effort` option again.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::catalog::{HarnessPreset, ModelSource};
use super::codex_config::configured_provider;
use super::openrouter::{OPENROUTER_MODELS_URL, OPENROUTER_PROVIDER, fetch_openrouter_models};
use crate::acp::schema::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelect,
    SessionConfigSelectOption, SessionConfigSelectOptions,
};
use crate::acp::{AgentHandle, SpawnOptions, effort_option};
use crate::secrets::{Secrets, spawn_agent_with_secrets};

/// Upper bound for starting the listing session.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// A listing younger than this is returned without asking again.
const FRESH_FOR: Duration = Duration::from_secs(600);
/// Failures, and explicit refreshes, reuse a result younger than this.
const MIN_REFRESH: Duration = Duration::from_secs(10);
/// Upper bound for selecting one model in the listing session.
const SET_MODEL_TIMEOUT: Duration = Duration::from_secs(30);

/// Harnesses with at most this many models get every model's efforts read
/// with the model list (one `set_config_option` each, no model call); bigger
/// lists (OpenCode: hundreds) are read per model on demand
/// ([`ModelService::get_efforts`]).
const EAGER_EFFORT_MODELS: usize = 12;

/// Reads an environment variable (the process's own, or a fake in tests).
type EnvLookup = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// One model a harness offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ModelOption {
    /// The value to select (`haiku`).
    pub value: String,
    /// Display name.
    pub name: String,
    pub description: Option<String>,
    /// The efforts this model supports; `None`: not read yet (ask with
    /// `list_model_efforts`), empty: the model has no effort option.
    pub efforts: Option<Vec<EffortOption>>,
}

/// One effort (thought level) a model supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct EffortOption {
    /// The value to select (`high`).
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

/// The efforts of one model of a harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ModelEfforts {
    pub harness: String,
    pub model: String,
    pub efforts: Vec<EffortOption>,
}

/// The models of one harness. Empty `models`: it has no model option (only its
/// own default can be used).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HarnessModels {
    pub harness: String,
    pub models: Vec<ModelOption>,
    /// The harness's current (default) model, if it reports one.
    pub current: Option<String>,
    /// When the harness was asked (ms since the Unix epoch).
    pub fetched_at_ms: u64,
}

/// The model option in `options`: the select of category `model`, else the
/// select with id `config_id`.
fn model_option<'a>(
    options: &'a [SessionConfigOption],
    config_id: &str,
) -> Option<&'a SessionConfigOption> {
    let is_select = |o: &&SessionConfigOption| matches!(o.kind, SessionConfigKind::Select(_));
    options
        .iter()
        .filter(is_select)
        .find(|o| o.category == Some(SessionConfigOptionCategory::Model))
        .or_else(|| {
            options
                .iter()
                .filter(is_select)
                .find(|o| &*o.id.0 == config_id)
        })
}

/// The options of a select, groups flattened.
fn flat_options(select: &SessionConfigSelect) -> Vec<&SessionConfigSelectOption> {
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(v) => v.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|g| g.options.iter()).collect()
        }
        _ => Vec::new(),
    }
}

/// The efforts listed in `options`: the select with id `config_id`, else the one
/// of category `thought_level` ([`effort_option`], the rule a session start uses
/// to apply the effort). Empty when there is none. A value `default`
/// (Claude Code adds it for clients that predate the option) is left out: an
/// unset effort is "not specified" in Yhtye.
#[must_use]
pub fn efforts_from_options(options: &[SessionConfigOption], config_id: &str) -> Vec<EffortOption> {
    let Some(SessionConfigKind::Select(select)) =
        effort_option(options, config_id).map(|o| &o.kind)
    else {
        return Vec::new();
    };
    let mut efforts: Vec<EffortOption> = Vec::new();
    for o in flat_options(select) {
        let value = o.value.0.to_string();
        if value != "default" && efforts.iter().all(|e| e.value != value) {
            efforts.push(EffortOption {
                value,
                name: o.name.clone(),
                description: o.description.clone(),
            });
        }
    }
    efforts
}

/// The models and current value listed in `options` (groups are flattened).
/// `efforts` of every model is `None`.
#[must_use]
pub fn models_from_options(
    options: &[SessionConfigOption],
    config_id: &str,
) -> (Vec<ModelOption>, Option<String>) {
    let Some(SessionConfigKind::Select(select)) = model_option(options, config_id).map(|o| &o.kind)
    else {
        return (Vec::new(), None);
    };
    let flat = flat_options(select);
    let mut models: Vec<ModelOption> = Vec::new();
    for o in flat {
        let value = o.value.0.to_string();
        if models.iter().all(|m| m.value != value) {
            models.push(ModelOption {
                value,
                name: o.name.clone(),
                description: o.description.clone(),
                efforts: None,
            });
        }
    }
    (models, Some(select.current_value.0.to_string()))
}

/// Opens a listing session of `preset` in `cwd`; gives up (stopping the agent)
/// when `cancel` fires.
async fn open_probe(
    secrets: &Secrets,
    preset: &HarnessPreset,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<AgentHandle, String> {
    let harness = preset.probe_config();
    let (tx, _rx) = mpsc::unbounded_channel();
    // Dropping a pending spawn kills the new process group (GroupKillGuard).
    let started = tokio::select! {
        r = tokio::time::timeout(PROBE_TIMEOUT, spawn_agent_with_secrets(secrets, &harness, cwd, SpawnOptions::default(), tx)) => r,
        () = cancel.cancelled() => return Err("Yhtye is shutting down".into()),
    };
    started
        .map_err(|_| "the harness did not start in time".to_string())?
        .map_err(|e| format!("could not start the harness: {e}"))
}

/// Selects `model` in the listing session and reads the efforts it offers.
/// `Err` when the harness refuses the model.
async fn efforts_of(
    handle: &AgentHandle,
    preset: &HarnessPreset,
    model: &str,
) -> Result<Vec<EffortOption>, String> {
    let options = tokio::time::timeout(
        SET_MODEL_TIMEOUT,
        handle.set_config_option(preset.model_config_id(), model),
    )
    .await
    .map_err(|_| format!("selecting {model} timed out"))?
    .map_err(|e| format!("could not select {model}: {e}"))?;
    Ok(efforts_from_options(&options, &preset.effort_config_id))
}

/// Opens a session of `preset`'s listing harness in `cwd`, reads its models and
/// stops it. Harnesses with at most [`EAGER_EFFORT_MODELS`] models also get
/// every model's efforts read (a model the harness refuses keeps `efforts:
/// None`). Gives up (stopping the agent) when `cancel` fires.
pub async fn probe_models(
    secrets: &Secrets,
    preset: &HarnessPreset,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<HarnessModels, String> {
    let handle = open_probe(secrets, preset, cwd, cancel).await?;
    let config_id = preset.model_config_id();
    let (mut models, current) = models_from_options(&handle.info().config_options, config_id);
    if models.len() <= EAGER_EFFORT_MODELS {
        for m in &mut models {
            if cancel.is_cancelled() {
                break;
            }
            match efforts_of(&handle, preset, &m.value).await {
                Ok(efforts) => m.efforts = Some(efforts),
                Err(e) => tracing::warn!("efforts of {}/{}: {e}", preset.id, m.value),
            }
        }
    }
    handle.shutdown().await;
    Ok(HarnessModels {
        harness: preset.id.clone(),
        models,
        current,
        fetched_at_ms: now_ms(),
    })
}

/// Opens a listing session, selects `model` and reads its efforts.
pub async fn probe_efforts(
    secrets: &Secrets,
    preset: &HarnessPreset,
    model: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<EffortOption>, String> {
    let handle = open_probe(secrets, preset, cwd, cancel).await?;
    let efforts = efforts_of(&handle, preset, model).await;
    handle.shutdown().await;
    efforts
}

type Cached = (Instant, Result<HarnessModels, String>);
type CachedEfforts = (Instant, Result<Vec<EffortOption>, String>);

/// Cached, one-at-a-time access to [`probe_models`] for the API.
pub struct ModelService {
    cwd: PathBuf,
    secrets: Arc<Secrets>,
    /// The environment the user's Codex home is found in (the process's own).
    env: EnvLookup,
    /// Where the OpenRouter model list is downloaded from.
    openrouter_url: String,
    /// Held while probing, so concurrent callers share the result.
    cache: Mutex<HashMap<String, Cached>>,
    /// Efforts read for one model on demand, by `(harness, model)`.
    efforts: Mutex<HashMap<(String, String), CachedEfforts>>,
    cancel: CancellationToken,
}

impl ModelService {
    /// `cwd` (the listing sessions' working directory) is created on first use.
    #[must_use]
    pub fn new(cwd: PathBuf, secrets: Arc<Secrets>) -> Self {
        Self {
            cwd,
            secrets,
            env: Arc::new(|k| std::env::var_os(k)),
            openrouter_url: OPENROUTER_MODELS_URL.to_string(),
            cache: Mutex::new(HashMap::new()),
            efforts: Mutex::new(HashMap::new()),
            cancel: CancellationToken::new(),
        }
    }

    /// Finds the user's Codex home (`CODEX_HOME`, `HOME`) in `env` instead of
    /// the process environment (tests).
    #[must_use]
    pub fn with_env(
        mut self,
        env: impl Fn(&str) -> Option<OsString> + Send + Sync + 'static,
    ) -> Self {
        self.env = Arc::new(env);
        self
    }

    /// Downloads OpenRouter's model list from `url` (tests).
    #[must_use]
    pub fn with_openrouter_url(mut self, url: impl Into<String>) -> Self {
        self.openrouter_url = url.into();
        self
    }

    /// Whether `preset`'s models come from OpenRouter: Codex with the
    /// `openrouter` provider in the user's Codex config.
    async fn uses_openrouter(&self, preset: &HarnessPreset) -> bool {
        preset.model_source == ModelSource::Codex
            && configured_provider(&*self.env).await.as_deref() == Some(OPENROUTER_PROVIDER)
    }

    /// The models of `preset` from where they come (ACP or OpenRouter).
    async fn load(&self, preset: &HarnessPreset) -> Result<HarnessModels, String> {
        if !self.uses_openrouter(preset).await {
            return probe_models(&self.secrets, preset, &self.cwd, &self.cancel).await;
        }
        let models = tokio::select! {
            r = fetch_openrouter_models(&self.openrouter_url) => r?,
            () = self.cancel.cancelled() => return Err("Yhtye is shutting down".into()),
        };
        Ok(HarnessModels {
            harness: preset.id.clone(),
            models,
            current: None,
            fetched_at_ms: now_ms(),
        })
    }

    /// Stops a running probe and refuses new ones; waits until none runs.
    pub async fn close(&self) {
        self.cancel.cancel();
        drop(self.cache.lock().await);
        drop(self.efforts.lock().await);
    }

    /// The models of `preset`: cached if fresh enough, otherwise probed.
    pub async fn get(
        &self,
        preset: &HarnessPreset,
        refresh: bool,
    ) -> Result<HarnessModels, String> {
        let mut cache = self.cache.lock().await;
        if self.cancel.is_cancelled() {
            return Err("Yhtye is shutting down".into());
        }
        if let Some((at, outcome)) = cache.get(&preset.id) {
            let max_age = if refresh || outcome.is_err() {
                MIN_REFRESH
            } else {
                FRESH_FOR
            };
            if at.elapsed() < max_age {
                return outcome.clone();
            }
        }
        let outcome = match std::fs::create_dir_all(&self.cwd) {
            Ok(()) => self.load(preset).await,
            Err(e) => Err(format!("could not create {}: {e}", self.cwd.display())),
        };
        cache.insert(preset.id.clone(), (Instant::now(), outcome.clone()));
        outcome
    }
}

impl ModelService {
    /// The efforts of one model: from the harness's cached listing when that
    /// has them, else cached per model, else read with a listing session that
    /// selects the model (about as slow as starting the harness: ~1.5–2 s).
    pub async fn get_efforts(
        &self,
        preset: &HarnessPreset,
        model: &str,
    ) -> Result<Vec<EffortOption>, String> {
        if self.uses_openrouter(preset).await {
            // OpenRouter's list carries every model's efforts.
            let listed = self.get(preset, false).await?;
            return Ok(listed
                .models
                .into_iter()
                .find(|m| m.value == model)
                .and_then(|m| m.efforts)
                .unwrap_or_default());
        }
        if let Some((at, Ok(listed))) = self.cache.lock().await.get(&preset.id)
            && at.elapsed() < FRESH_FOR
            && let Some(e) = listed
                .models
                .iter()
                .find(|m| m.value == model)
                .and_then(|m| m.efforts.clone())
        {
            return Ok(e);
        }
        let mut cache = self.efforts.lock().await;
        if self.cancel.is_cancelled() {
            return Err("Yhtye is shutting down".into());
        }
        let key = (preset.id.clone(), model.to_string());
        if let Some((at, outcome)) = cache.get(&key) {
            let max_age = if outcome.is_err() {
                MIN_REFRESH
            } else {
                FRESH_FOR
            };
            if at.elapsed() < max_age {
                return outcome.clone();
            }
        }
        let outcome = match std::fs::create_dir_all(&self.cwd) {
            Ok(()) => probe_efforts(&self.secrets, preset, model, &self.cwd, &self.cancel).await,
            Err(e) => Err(format!("could not create {}: {e}", self.cwd.display())),
        };
        cache.insert(key, (Instant::now(), outcome.clone()));
        outcome
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::schema::SessionConfigSelectGroup;

    fn opt(v: &str) -> SessionConfigSelectOption {
        SessionConfigSelectOption::new(v.to_string(), v.to_uppercase())
    }

    #[test]
    fn reads_the_model_select_by_id() {
        let options = vec![
            SessionConfigOption::select("mode", "Mode", "a", vec![opt("a")]),
            SessionConfigOption::select(
                "model",
                "Model",
                "haiku",
                vec![opt("default"), opt("haiku"), opt("haiku")],
            ),
        ];
        let (models, current) = models_from_options(&options, "model");
        let values: Vec<&str> = models.iter().map(|m| m.value.as_str()).collect();
        assert_eq!(values, ["default", "haiku"], "duplicates dropped");
        assert_eq!(models[1].name, "HAIKU");
        assert_eq!(current.as_deref(), Some("haiku"));
    }

    #[test]
    fn prefers_the_model_category_and_flattens_groups() {
        let grouped = SessionConfigSelectOptions::Grouped(vec![
            SessionConfigSelectGroup::new("a", "A", vec![opt("x")]),
            SessionConfigSelectGroup::new("b", "B", vec![opt("y")]),
        ]);
        let options = vec![
            SessionConfigOption::select("model", "Other", "q", vec![opt("q")]),
            SessionConfigOption::select("llm", "LLM", "x", grouped)
                .category(SessionConfigOptionCategory::Model),
        ];
        let (models, current) = models_from_options(&options, "model");
        let values: Vec<&str> = models.iter().map(|m| m.value.as_str()).collect();
        assert_eq!(values, ["x", "y"]);
        assert_eq!(current.as_deref(), Some("x"));
    }

    #[test]
    fn efforts_come_from_the_named_option_else_the_only_thought_level_option() {
        let by_id = vec![SessionConfigOption::select(
            "effort",
            "Effort",
            "high",
            vec![opt("default"), opt("low"), opt("high")],
        )];
        let values = |efforts: Vec<EffortOption>| -> Vec<String> {
            efforts.into_iter().map(|e| e.value).collect()
        };
        assert_eq!(
            values(efforts_from_options(&by_id, "effort")),
            ["low", "high"],
            "`default` is left out"
        );

        let by_category = vec![
            SessionConfigOption::select("model", "Model", "m", vec![opt("m")]),
            SessionConfigOption::select(
                "thinking",
                "Thinking",
                "low",
                vec![opt("low"), opt("max")],
            )
            .category(SessionConfigOptionCategory::ThoughtLevel),
        ];
        assert_eq!(
            values(efforts_from_options(&by_category, "effort")),
            ["low", "max"]
        );

        let ambiguous = vec![
            SessionConfigOption::select("a", "A", "x", vec![opt("x")])
                .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::select("b", "B", "y", vec![opt("y")])
                .category(SessionConfigOptionCategory::ThoughtLevel),
        ];
        assert!(efforts_from_options(&ambiguous, "effort").is_empty());
    }

    #[test]
    fn a_harness_without_a_model_option_lists_nothing() {
        let options = vec![SessionConfigOption::select(
            "mode",
            "Mode",
            "a",
            vec![opt("a")],
        )];
        assert_eq!(models_from_options(&options, "model"), (Vec::new(), None));
        assert_eq!(models_from_options(&[], "model"), (Vec::new(), None));
    }
}
