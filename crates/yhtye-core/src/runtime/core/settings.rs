//! The settings commands of [`Core`] that several clients may run at once
//! (`core-design.md` §15.7, §16, T-65): the agent settings layers and the
//! secret environment variables. Each change runs under one lock from its
//! validation to its last step (database, then memory or the credential store),
//! so two clients cannot leave the database and what is in effect disagreeing,
//! and is announced with an app-wide `settings_changed` event.

use std::sync::atomic::Ordering;

use super::{Core, now_ms};
use crate::acp::CLAUDE_API_KEY_SECRET;
use crate::agents::{AgentRole, CLAUDE_CODE_API, RoleSettings};
use crate::api::{AgentSettingsView, ApiError, ApiEvent, ApiEventBody, SettingsKind};
use crate::secrets::{SecretError, SecretValue, validate_secret_name, validate_secret_value};

/// The first revision of the agent settings in this run: the clock in
/// microseconds, so it is larger than any revision a client kept from an
/// earlier run (unless that one changed them more than once a microsecond).
/// Stays below 2^53 (a JavaScript number) for centuries.
pub(super) fn initial_revision() -> u64 {
    now_us()
}

/// The revision after `current`: one more, and at least the clock.
fn next_revision(current: u64) -> u64 {
    (current + 1).max(now_us())
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

impl Core {
    /// Announces a settings change to every client (remote ones too).
    pub(super) fn settings_changed(
        &self,
        kind: SettingsKind,
        project: Option<String>,
        revision: Option<u64>,
    ) {
        // Nobody listening is fine.
        let _ = self
            .inner
            .events
            .send(ApiEvent::app_wide(ApiEventBody::SettingsChanged {
                kind,
                project,
                revision,
            }));
    }

    /// The settings of every role, globally or for a registered `project`.
    pub(super) async fn agent_settings(
        &self,
        project: Option<String>,
    ) -> Result<AgentSettingsView, ApiError> {
        if let Some(p) = &project {
            self.known_project(p).await?;
        }
        let agents = &self.inner.agents;
        let (global, project_layer) = agents.layers(project.as_deref());
        Ok(AgentSettingsView {
            harnesses: agents.harnesses(),
            builtin: agents.builtin(),
            global,
            effective: agents.effective(project.as_deref()),
            project,
            project_layer,
            revision: self.inner.settings_revision.load(Ordering::SeqCst),
        })
    }

    /// Validates, stores and applies one role of a settings layer, refusing
    /// with `conflict` when `expected` is not the current revision.
    pub(super) async fn set_agent_settings(
        &self,
        project: Option<&str>,
        role: AgentRole,
        settings: Option<RoleSettings>,
        expected: Option<u64>,
    ) -> Result<(), ApiError> {
        if let Some(p) = project {
            self.known_project(p).await?;
        }
        let _serial = self.inner.settings_lock.lock().await;
        let current = self.inner.settings_revision.load(Ordering::SeqCst);
        if expected.is_some_and(|e| e != current) {
            return Err(ApiError::conflict(
                "the agent settings were changed elsewhere since they were read; read them again",
            ));
        }
        let agents = &self.inner.agents;
        // The rows of this scope as the UI shows them (the layer's own, else the
        // inherited ones), which may include harnesses that are gone since.
        let existing = agents.effective(project).get(role).clone();
        let settings = settings
            .map(|s| {
                let valid = agents.validate(&s, &existing)?;
                // An effort the model does not offer (Devin's models offer none)
                // is refused when its efforts were read; see `check_efforts`.
                valid.check_efforts(&existing.candidates, |harness, model| {
                    let efforts = self.inner.models.cached_efforts(harness, model)?;
                    Some(efforts.into_iter().map(|e| e.value).collect())
                })?;
                Ok::<_, String>(valid)
            })
            .transpose()
            .map_err(|e| ApiError::invalid_argument(format!("{}: {e}", role.as_str())))?;
        self.inner
            .store
            .set_agent_settings(project, role, settings.as_ref(), now_ms())
            .await?;
        agents.set(project, role, settings);
        let revision = next_revision(current);
        self.inner
            .settings_revision
            .store(revision, Ordering::SeqCst);
        self.settings_changed(
            SettingsKind::AgentSettings,
            project.map(str::to_string),
            Some(revision),
        );
        Ok(())
    }

    /// Registers the name in the database, then stores the value in the
    /// credential store; a failure of the second undoes the first (unless the
    /// name was registered before), so the two never disagree.
    pub(super) async fn set_secret(&self, name: &str, value: SecretValue) -> Result<(), ApiError> {
        let secrets = &self.inner.secrets;
        validate_secret_name(name).map_err(secret_error)?;
        validate_secret_value(value.expose()).map_err(secret_error)?;
        let _serial = self.inner.settings_lock.lock().await;
        let was_registered = secrets.names().iter().any(|n| n == name);
        self.inner.store.add_secret_name(name, now_ms()).await?;
        if let Err(e) = secrets.set(name, value).await {
            if !was_registered && let Err(undo) = self.inner.store.remove_secret_name(name).await {
                tracing::error!("could not undo registering the secret name {name}: {undo}");
            }
            return Err(secret_error(e));
        }
        self.secret_changed(name);
        Ok(())
    }

    /// Removes the name from the database, then the value from the credential
    /// store; a failure of the second registers the name again.
    pub(super) async fn delete_secret(&self, name: &str) -> Result<(), ApiError> {
        validate_secret_name(name).map_err(secret_error)?;
        let _serial = self.inner.settings_lock.lock().await;
        self.inner.store.remove_secret_name(name).await?;
        if let Err(e) = self.inner.secrets.remove(name).await {
            if let Err(undo) = self.inner.store.add_secret_name(name, now_ms()).await {
                tracing::error!("could not undo removing the secret name {name}: {undo}");
            }
            return Err(secret_error(e));
        }
        self.secret_changed(name);
        Ok(())
    }
}

impl Core {
    /// A secret was stored or removed: announced to every client, and what
    /// was read from the harness that uses it is forgotten (the models of
    /// `Claude Code (API)` could not be listed before its key was registered).
    fn secret_changed(&self, name: &str) {
        self.settings_changed(SettingsKind::Secrets, None, None);
        if name == CLAUDE_API_KEY_SECRET {
            self.inner.models.invalidate(CLAUDE_CODE_API);
        }
    }
}

/// The message never contains a value (`SecretError` does not carry one).
fn secret_error(e: SecretError) -> ApiError {
    match e {
        SecretError::Invalid(m) => ApiError::invalid_argument(m),
        e @ SecretError::Unavailable(_) => ApiError::unavailable(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_grow_past_the_clock_and_one_by_one() {
        let start = initial_revision();
        assert!(next_revision(start) > start);
        // A revision ahead of the clock (many changes within a millisecond) still grows.
        let ahead = now_us() + 1_000_000;
        assert_eq!(next_revision(ahead), ahead + 1);
        // One from long ago (an earlier run's) jumps to the clock.
        assert!(next_revision(5) >= start);
    }
}
