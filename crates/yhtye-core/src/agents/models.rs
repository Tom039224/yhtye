//! The models a harness offers, read over ACP (`core-design.md` §15.6): a
//! short-lived session is opened without sending a prompt (no model call) and
//! its model option in `config_options` is read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::catalog::HarnessPreset;
use crate::acp::schema::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions,
};
use crate::acp::{SpawnOptions, spawn_agent};

/// Upper bound for starting the listing session.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// A listing younger than this is returned without asking again.
const FRESH_FOR: Duration = Duration::from_secs(600);
/// Failures, and explicit refreshes, reuse a result younger than this.
const MIN_REFRESH: Duration = Duration::from_secs(10);

/// One model a harness offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ModelOption {
    /// The value to select (`haiku`).
    pub value: String,
    /// Display name.
    pub name: String,
    pub description: Option<String>,
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

/// The models and current value listed in `options` (groups are flattened).
#[must_use]
pub fn models_from_options(
    options: &[SessionConfigOption],
    config_id: &str,
) -> (Vec<ModelOption>, Option<String>) {
    let Some(SessionConfigKind::Select(select)) = model_option(options, config_id).map(|o| &o.kind)
    else {
        return (Vec::new(), None);
    };
    let flat: Vec<&SessionConfigSelectOption> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(v) => v.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => {
            groups.iter().flat_map(|g| g.options.iter()).collect()
        }
        _ => Vec::new(),
    };
    let mut models: Vec<ModelOption> = Vec::new();
    for o in flat {
        let value = o.value.0.to_string();
        if models.iter().all(|m| m.value != value) {
            models.push(ModelOption {
                value,
                name: o.name.clone(),
                description: o.description.clone(),
            });
        }
    }
    (models, Some(select.current_value.0.to_string()))
}

/// Opens a session of `preset`'s listing harness in `cwd`, reads its models and
/// stops it. Gives up (stopping the agent) when `cancel` fires.
pub async fn probe_models(
    preset: &HarnessPreset,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<HarnessModels, String> {
    let harness = preset.probe_config();
    let (tx, _rx) = mpsc::unbounded_channel();
    // Dropping a pending spawn kills the new process group (GroupKillGuard).
    let started = tokio::select! {
        r = tokio::time::timeout(PROBE_TIMEOUT, spawn_agent(&harness, cwd, SpawnOptions::default(), tx)) => r,
        () = cancel.cancelled() => return Err("Yhtye is shutting down".into()),
    };
    let handle = started
        .map_err(|_| "the harness did not start in time".to_string())?
        .map_err(|e| format!("could not start the harness: {e}"))?;
    let (models, current) =
        models_from_options(&handle.info().config_options, preset.model_config_id());
    handle.shutdown().await;
    Ok(HarnessModels {
        harness: preset.id.clone(),
        models,
        current,
        fetched_at_ms: now_ms(),
    })
}

type Cached = (Instant, Result<HarnessModels, String>);

/// Cached, one-at-a-time access to [`probe_models`] for the API.
pub struct ModelService {
    cwd: PathBuf,
    /// Held while probing, so concurrent callers share the result.
    cache: Mutex<HashMap<String, Cached>>,
    cancel: CancellationToken,
}

impl ModelService {
    /// `cwd` (the listing sessions' working directory) is created on first use.
    #[must_use]
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            cache: Mutex::new(HashMap::new()),
            cancel: CancellationToken::new(),
        }
    }

    /// Stops a running probe and refuses new ones; waits until none runs.
    pub async fn close(&self) {
        self.cancel.cancel();
        drop(self.cache.lock().await);
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
            Ok(()) => probe_models(preset, &self.cwd, &self.cancel).await,
            Err(e) => Err(format!("could not create {}: {e}", self.cwd.display())),
        };
        cache.insert(preset.id.clone(), (Instant::now(), outcome.clone()));
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
