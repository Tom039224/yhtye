//! Secret environment variables (Stage 7e, `core-design.md` §16): values kept in
//! the OS credential store, injected into the environment of every agent
//! process. Yhtye's own database holds only the *names*; a value is never
//! logged, returned to the UI or included in an error message.
//!
//! - [`SecretBackend`]: where values live ([`KeyringBackend`] = the OS store,
//!   [`MemoryBackend`] = tests).
//! - [`Secrets`]: the registered names plus the backend; [`Secrets::env`] reads
//!   every value for a spawn, except the secrets only one harness gets
//!   ([`HARNESS_ONLY_SECRETS`], handed over by [`HarnessConfig::secret_bindings`]).

mod backend;
mod name;

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};

use tokio::sync::mpsc;

use crate::acp::{
    AgentError, AgentEvent, AgentHandle, HARNESS_ONLY_SECRETS, HarnessConfig, SpawnOptions,
    spawn_agent,
};

pub use backend::{KeyringBackend, MemoryBackend, SECRET_SERVICE, SecretBackend};
pub use name::{MAX_SECRET_VALUE_BYTES, validate_secret_name, validate_secret_value};

/// Why a secret could not be stored, read or removed. The message never
/// contains a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// The name or value is not acceptable (charset, length, reserved).
    #[error("{0}")]
    Invalid(String),
    /// The credential store failed (not running, locked, no permission).
    #[error("the OS credential store is unavailable: {0}")]
    Unavailable(String),
}

/// A secret value whose `Debug` output is redacted. (De)serializes as a plain
/// string, which only the API command that registers it does.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SecretValue(String);

impl SecretValue {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, only for handing it to a process environment or the
    /// credential store.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

/// Environment variables to add to an agent process (`Debug` shows names only).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretEnv(Vec<(String, SecretValue)>);

impl SecretEnv {
    #[must_use]
    pub fn new(vars: Vec<(String, SecretValue)>) -> Self {
        Self(vars)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.expose()))
    }

    /// These variables plus `name` = `value`, which replaces a variable of the
    /// same name.
    #[must_use]
    pub fn with(mut self, name: &str, value: SecretValue) -> Self {
        self.0.retain(|(k, _)| k != name);
        self.0.push((name.to_string(), value));
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The variable names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
}

impl fmt::Debug for SecretEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

/// The registered secret names and the store holding their values. Cheap to
/// share (`Arc`); the backend is only called from blocking threads.
pub struct Secrets {
    backend: Arc<dyn SecretBackend>,
    names: RwLock<BTreeSet<String>>,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("names", &self.names())
            .finish_non_exhaustive()
    }
}

impl Secrets {
    /// `names` are the ones already registered (from the database).
    #[must_use]
    pub fn new(backend: Arc<dyn SecretBackend>, names: impl IntoIterator<Item = String>) -> Self {
        Self {
            backend,
            names: RwLock::new(names.into_iter().collect()),
        }
    }

    /// No secrets and an in-memory store (tests, fixed catalogs).
    #[must_use]
    pub fn none() -> Self {
        Self::new(Arc::new(MemoryBackend::default()), [])
    }

    /// The registered names, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.read().iter().cloned().collect()
    }

    /// Stores `value` under `name` (replacing an earlier one) and registers the name.
    pub async fn set(&self, name: &str, value: SecretValue) -> Result<(), SecretError> {
        validate_secret_name(name)?;
        validate_secret_value(value.expose())?;
        let backend = self.backend.clone();
        let key = name.to_string();
        blocking(move || backend.set(&key, value.expose())).await?;
        self.names
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_string());
        Ok(())
    }

    /// Removes the value and the name (removing an unknown name is fine).
    pub async fn remove(&self, name: &str) -> Result<(), SecretError> {
        validate_secret_name(name)?;
        let backend = self.backend.clone();
        let key = name.to_string();
        blocking(move || backend.delete(&key)).await?;
        self.names
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
        Ok(())
    }

    /// Every registered variable with its value, for a spawn: those every
    /// agent gets (not [`HARNESS_ONLY_SECRETS`]). A name whose value is
    /// missing from the store is skipped (with a warning).
    pub async fn env(&self) -> Result<SecretEnv, SecretError> {
        let names: Vec<String> = self
            .names()
            .into_iter()
            .filter(|n| !HARNESS_ONLY_SECRETS.contains(&n.as_str()))
            .collect();
        if names.is_empty() {
            return Ok(SecretEnv::default());
        }
        let backend = self.backend.clone();
        blocking(move || {
            let mut vars = Vec::with_capacity(names.len());
            for name in names {
                // A row edited by hand must not inject a forbidden variable.
                if validate_secret_name(&name).is_err() {
                    tracing::warn!("ignoring the registered secret name {name:?}: not allowed");
                    continue;
                }
                match backend.get(&name)? {
                    Some(v) => vars.push((name, SecretValue::new(v))),
                    None => tracing::warn!("secret {name} is registered but has no stored value"),
                }
            }
            Ok(SecretEnv::new(vars))
        })
        .await
    }

    /// The value of the registered secret `name`; `None` when the name is not
    /// registered or the store has no value for it.
    pub async fn get(&self, name: &str) -> Result<Option<SecretValue>, SecretError> {
        if !self.read().contains(name) {
            return Ok(None);
        }
        let backend = self.backend.clone();
        let key = name.to_string();
        Ok(blocking(move || backend.get(&key))
            .await?
            .map(SecretValue::new))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeSet<String>> {
        self.names.read().unwrap_or_else(PoisonError::into_inner)
    }
}

/// [`spawn_agent`] with every registered secret environment variable added to
/// the agent's environment, and the secrets `harness` binds
/// ([`HarnessConfig::secret_bindings`]) under the names it reads. Fails
/// (without starting anything) when a value cannot be read or a bound secret
/// is not registered: an agent silently missing its API key would only fail
/// later with a confusing message.
pub async fn spawn_agent_with_secrets(
    secrets: &Secrets,
    harness: &HarnessConfig,
    cwd: &Path,
    mut options: SpawnOptions,
    events: mpsc::UnboundedSender<AgentEvent>,
) -> Result<AgentHandle, AgentError> {
    options.secret_env = secret_env_for(secrets, harness).await?;
    spawn_agent(harness, cwd, options, events).await
}

/// The secret environment of one agent process: the general variables, then
/// the bound ones (which win over a general variable of the same name).
async fn secret_env_for(
    secrets: &Secrets,
    harness: &HarnessConfig,
) -> Result<SecretEnv, AgentError> {
    let unreadable = |e: SecretError| AgentError::Startup {
        step: "secret environment variables".into(),
        message: e.to_string(),
    };
    let mut env = secrets.env().await.map_err(unreadable)?;
    for binding in &harness.secret_bindings {
        let value = secrets
            .get(&binding.secret)
            .await
            .map_err(unreadable)?
            .ok_or_else(|| AgentError::Setup {
                message: binding.missing_message(),
            })?;
        env = env.with(&binding.env, value);
    }
    Ok(env)
}

/// Runs a credential-store call on a blocking thread (Secret Service talks D-Bus).
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| SecretError::Unavailable(format!("the credential store call failed: {e}")))?
}

#[cfg(test)]
mod tests;
