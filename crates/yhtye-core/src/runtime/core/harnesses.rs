//! The harness commands of [`Core`] (`GetHarnesses`, `DetectHarnesses`,
//! `SetHarnessPath`) and the detection that fills the agent catalog at start:
//! the harnesses that are installed are found again on request and registered
//! in place of the ones before; sessions already running keep what they run.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use super::{Core, now_ms};
use crate::acp::HarnessConfig;
use crate::agents::{
    AgentChoice, CLAUDE_CODE, HARNESS_SPECS, HarnessDetection, HarnessPreset, NPX_COMMAND,
    check_executable_path, default_choice, detect_harnesses, harness_spec, known_dirs,
    presets_from,
};
use crate::api::{ApiError, ApiResponse};

type EnvLookup = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// How the installed harnesses are found: the environment whose `PATH` and
/// `HOME` are searched, the well-known directories after `PATH`, and the model
/// of Claude Code's preset. The app has one ([`CoreConfig::installed`]); the
/// tests' cores have none and keep their configured harnesses.
///
/// [`CoreConfig::installed`]: super::CoreConfig::installed
#[derive(Clone)]
pub struct DetectionConfig {
    env: EnvLookup,
    known_dirs: Vec<PathBuf>,
    claude_model: String,
}

impl DetectionConfig {
    /// Looks in `env` (a fake in tests), and in the well-known directories
    /// ([`crate::agents::KNOWN_DIRS`], `~` being the `HOME` of `env`).
    #[must_use]
    pub fn new(
        claude_model: &str,
        env: impl Fn(&str) -> Option<OsString> + Send + Sync + 'static,
    ) -> Self {
        Self {
            known_dirs: known_dirs(&env),
            env: Arc::new(env),
            claude_model: claude_model.to_string(),
        }
    }

    /// The process's own environment.
    #[must_use]
    pub fn from_process(claude_model: &str) -> Self {
        Self::new(claude_model, |k| std::env::var_os(k))
    }

    /// Searches `dirs` after `PATH` instead of the well-known directories.
    #[must_use]
    pub fn with_known_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.known_dirs = dirs;
        self
    }

    /// The state of every harness, the presets of the installed ones and the
    /// built-in default among them.
    pub(super) fn detect(&self, overrides: &HashMap<String, String>) -> Detected {
        let detections = detect_harnesses(&*self.env, &self.known_dirs, overrides);
        let presets = presets_from(&detections, &self.claude_model, &*self.env);
        let builtin = default_choice(&presets, &self.claude_model);
        Detected {
            detections,
            presets,
            builtin,
        }
    }
}

impl fmt::Debug for DetectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DetectionConfig")
            .field("known_dirs", &self.known_dirs)
            .field("claude_model", &self.claude_model)
            .finish_non_exhaustive()
    }
}

/// The result of one detection.
pub(super) struct Detected {
    pub(super) detections: Vec<HarnessDetection>,
    pub(super) presets: Vec<HarnessPreset>,
    pub(super) builtin: AgentChoice,
}

impl Detected {
    /// What was found, one line per harness.
    pub(super) fn log(&self) {
        for d in &self.detections {
            if let Some(path) = d.resolved_path.as_deref().filter(|_| d.installed) {
                tracing::info!("{} found at {path}", d.label);
            } else if let Some(error) = &d.override_error {
                tracing::warn!("{} is not available: {error}", d.label);
            } else {
                tracing::info!("{} is not installed; not offered", d.label);
            }
        }
    }

    /// `usage` (Claude Code's `/usage` probe) runs the `npx` that was found,
    /// like the Claude Code preset does (the literal `npx` when none was).
    pub(super) fn with_found_npx(&self, mut usage: HarnessConfig) -> HarnessConfig {
        let npx = self
            .detections
            .iter()
            .find(|d| d.id == CLAUDE_CODE)
            .and_then(|d| d.resolved_path.as_deref());
        if usage.command == NPX_COMMAND
            && let Some(npx) = npx
        {
            usage.command = npx.to_string();
        }
        usage
    }
}

impl Core {
    /// Finds the harnesses again (`overrides` are read from the database now)
    /// and registers those that are installed. Concurrent detections are
    /// serialized so the last one to finish also read the newest paths. What
    /// was read from a harness that now runs another executable (its models
    /// and efforts) is forgotten, and Claude Code's usage probe follows the
    /// `npx` that was found. `None` without a detection config.
    async fn detect_and_register(&self) -> Result<Option<Vec<HarnessDetection>>, ApiError> {
        let Some(detection) = &self.inner.cfg.detection else {
            return Ok(None);
        };
        let _serial = self.inner.detecting.lock().await;
        let overrides = self.inner.store.harness_paths().await?;
        let found = detection.detect(&overrides);
        found.log();
        let changed = self.changed_harnesses(&found.presets);
        let usage = self
            .inner
            .cfg
            .usage
            .clone()
            .map(|u| found.with_found_npx(u));
        self.inner.agents.refresh(found.presets, found.builtin);
        // After the registration: a request that reads a preset from now on
        // gets the new one, and a probe of the old one that is still running
        // does not fill the caches again.
        for id in changed {
            self.inner.models.invalidate(id);
        }
        if let Some(usage) = usage {
            self.inner.usage.set_harness(usage);
        }
        Ok(Some(found.detections))
    }

    /// The harnesses whose registered preset is not the one in `presets`
    /// (found again, no longer found, or newly found).
    fn changed_harnesses(&self, presets: &[HarnessPreset]) -> Vec<&'static str> {
        HARNESS_SPECS
            .iter()
            .filter(|spec| {
                self.inner.agents.preset(spec.id).as_ref()
                    != presets.iter().find(|p| p.id == spec.id)
            })
            .map(|spec| spec.id)
            .collect()
    }

    /// `GetHarnesses`: detects again (the settings were opened). The models
    /// of a harness whose executable changed are forgotten.
    pub(super) async fn get_harnesses(&self) -> Result<ApiResponse, ApiError> {
        let harnesses = self.detect_and_register().await?.unwrap_or_default();
        Ok(ApiResponse::Harnesses { harnesses })
    }

    /// `DetectHarnesses`: detects again and forgets the models read from every
    /// harness (the user asked for a fresh look).
    pub(super) async fn detect_harnesses(&self) -> Result<ApiResponse, ApiError> {
        let harnesses = self.detect_and_register().await?.unwrap_or_default();
        for spec in &HARNESS_SPECS {
            self.inner.models.invalidate(spec.id);
        }
        Ok(ApiResponse::Harnesses { harnesses })
    }

    /// `SetHarnessPath`: checks, stores and applies the path of a harness's
    /// main executable (`None` removes it). Detecting again forgets what was
    /// read from the executable that is replaced.
    pub(super) async fn set_harness_path(
        &self,
        harness: String,
        path: Option<String>,
    ) -> Result<ApiResponse, ApiError> {
        if harness_spec(&harness).is_none() {
            let known: Vec<&str> = HARNESS_SPECS.iter().map(|s| s.id).collect();
            return Err(ApiError::not_found(format!(
                "unknown harness {harness} (known: {})",
                known.join(", ")
            )));
        }
        let path = path.map(|p| p.trim().to_string());
        if let Some(p) = &path {
            check_executable_path(p)
                .map_err(|e| ApiError::invalid_argument(format!("{harness}: {e}")))?;
        }
        self.inner
            .store
            .set_harness_path(&harness, path.as_deref(), now_ms())
            .await?;
        let harnesses = self.detect_and_register().await?.unwrap_or_default();
        Ok(ApiResponse::Harnesses { harnesses })
    }
}
