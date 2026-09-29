//! Where secret values live.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use super::SecretError;

/// Service name of Yhtye's entries in the OS credential store (the account is
/// the variable name).
pub const SECRET_SERVICE: &str = "yhtye";

/// A store of secret values by variable name. Calls may block (D-Bus).
pub trait SecretBackend: Send + Sync + std::fmt::Debug {
    fn set(&self, name: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;
    /// Removing a value that is not there is not an error.
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

/// The OS credential store (Secret Service / Keychain / Credential Manager).
#[derive(Debug, Clone)]
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    /// Entries under [`SECRET_SERVICE`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_service(SECRET_SERVICE)
    }

    /// Entries under another service name (tests).
    #[must_use]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, name).map_err(unavailable)
    }
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// The message of a keyring error (these never carry the secret value).
fn unavailable(e: keyring::Error) -> SecretError {
    SecretError::Unavailable(e.to_string())
}

impl SecretBackend for KeyringBackend {
    fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
        self.entry(name)?.set_password(value).map_err(unavailable)
    }

    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        match self.entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(unavailable(e)),
        }
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(unavailable(e)),
        }
    }
}

/// Values kept in memory (unit tests). `fail_with` makes every call fail like
/// an unavailable credential store.
#[derive(Debug, Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
    fail_with: Mutex<Option<String>>,
}

impl MemoryBackend {
    /// Every later call fails with this message.
    pub fn fail_with(&self, message: &str) {
        *self
            .fail_with
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(message.into());
    }

    fn check(&self) -> Result<(), SecretError> {
        match &*self
            .fail_with
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Some(m) => Err(SecretError::Unavailable(m.clone())),
            None => Ok(()),
        }
    }

    fn values(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.values.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretBackend for MemoryBackend {
    fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
        self.check()?;
        self.values().insert(name.into(), value.into());
        Ok(())
    }

    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self.values().get(name).cloned())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.check()?;
        self.values().remove(name);
        Ok(())
    }
}
