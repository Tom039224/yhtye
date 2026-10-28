//! Where a host's device token lives. The token is a secret (it lets whoever
//! holds it into the host's API): it goes in the OS credential store under
//! the account `remote-host:<server id>`, never into the database or a file.
//! When the credential store cannot be used, the token is kept in memory for
//! this process only (the host has to be paired again after a restart) and
//! [`TokenPlace::MemoryOnly`] says so, for the UI to show.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use crate::secrets::{KeyringBackend, SecretBackend, SecretError};

/// Prefix of the credential store's account for a host's token.
const ACCOUNT_PREFIX: &str = "remote-host:";

/// Where a token was put.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum TokenPlace {
    /// The OS credential store: survives a restart.
    Keyring,
    /// Memory of this process only, because the credential store failed
    /// (`reason`, English): gone at the next start, then pair again.
    MemoryOnly { reason: String },
}

/// The device tokens of the hosts.
pub struct TokenVault {
    backend: Arc<dyn SecretBackend>,
    /// Tokens the credential store would not take, with why.
    memory: Mutex<HashMap<String, (String, String)>>,
}

impl std::fmt::Debug for TokenVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenVault").finish_non_exhaustive()
    }
}

fn account(server_id: &str) -> String {
    format!("{ACCOUNT_PREFIX}{server_id}")
}

impl TokenVault {
    /// Tokens in the OS credential store (service `yhtye`).
    #[must_use]
    pub fn keyring() -> Self {
        Self::new(Arc::new(KeyringBackend::new()))
    }

    #[must_use]
    pub fn new(backend: Arc<dyn SecretBackend>) -> Self {
        Self {
            backend,
            memory: Mutex::new(HashMap::new()),
        }
    }

    fn memory(&self) -> MutexGuard<'_, HashMap<String, (String, String)>> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps the token of `server_id`: in the credential store, or if that
    /// fails, in memory (see [`TokenPlace`]).
    pub async fn save(&self, server_id: &str, token: &str) -> TokenPlace {
        let backend = Arc::clone(&self.backend);
        let (key, value) = (account(server_id), token.to_string());
        let stored = blocking(move || backend.set(&key, &value)).await;
        match stored {
            Ok(()) => {
                self.memory().remove(server_id);
                TokenPlace::Keyring
            }
            Err(e) => {
                let reason = e.to_string();
                tracing::warn!("the token of host {server_id} is kept in memory only: {reason}");
                self.memory()
                    .insert(server_id.to_string(), (token.to_string(), reason.clone()));
                TokenPlace::MemoryOnly { reason }
            }
        }
    }

    /// The token of `server_id`: from memory, else from the credential store;
    /// `None` if there is none. `Err`: the credential store could not be read.
    pub async fn load(&self, server_id: &str) -> Result<Option<String>, SecretError> {
        if let Some((token, _)) = self.memory().get(server_id) {
            return Ok(Some(token.clone()));
        }
        let backend = Arc::clone(&self.backend);
        let key = account(server_id);
        blocking(move || backend.get(&key)).await
    }

    /// Why the token of `server_id` is kept in memory only; `None` if it is not.
    #[must_use]
    pub fn memory_only_reason(&self, server_id: &str) -> Option<String> {
        self.memory()
            .get(server_id)
            .map(|(_, reason)| reason.clone())
    }

    /// Forgets the token (memory first, always). `Err`: the credential store
    /// could not remove it, so a copy may be left there.
    pub async fn forget(&self, server_id: &str) -> Result<(), SecretError> {
        self.memory().remove(server_id);
        let backend = Arc::clone(&self.backend);
        let key = account(server_id);
        blocking(move || backend.delete(&key)).await
    }
}

/// A credential store call on a blocking thread (the Secret Service talks D-Bus).
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| SecretError::Unavailable(format!("the credential store call failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;

    #[tokio::test]
    async fn a_token_goes_to_the_credential_store_under_the_hosts_account() {
        let backend = Arc::new(MemoryBackend::default());
        let vault = TokenVault::new(backend.clone());
        assert_eq!(vault.save("sid1", "tok").await, TokenPlace::Keyring);
        assert_eq!(
            backend.get("remote-host:sid1").unwrap().as_deref(),
            Some("tok")
        );
        assert_eq!(vault.load("sid1").await.unwrap().as_deref(), Some("tok"));
        assert_eq!(vault.load("other").await.unwrap(), None);
        assert_eq!(vault.memory_only_reason("sid1"), None);
        vault.forget("sid1").await.unwrap();
        assert_eq!(vault.load("sid1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn without_a_credential_store_the_token_lives_in_memory_and_says_so() {
        let backend = Arc::new(MemoryBackend::default());
        backend.fail_with("no secret service");
        let vault = TokenVault::new(backend.clone());
        let place = vault.save("sid1", "tok").await;
        let TokenPlace::MemoryOnly { reason } = place else {
            panic!("expected memory only, got {place:?}")
        };
        assert!(reason.contains("no secret service"), "{reason}");
        // Readable now (without touching the broken store) ...
        assert_eq!(vault.load("sid1").await.unwrap().as_deref(), Some("tok"));
        assert_eq!(vault.memory_only_reason("sid1"), Some(reason));
        // ... and gone when the process is: a new vault finds nothing (the store is still broken).
        let after_restart = TokenVault::new(backend);
        assert!(after_restart.load("sid1").await.is_err());
        // Forgetting drops the memory copy even though the store cannot be reached.
        assert!(vault.forget("sid1").await.is_err());
        assert_eq!(vault.memory_only_reason("sid1"), None);
    }

    #[tokio::test]
    async fn a_token_that_later_reaches_the_store_leaves_memory() {
        let backend = Arc::new(MemoryBackend::default());
        let vault = TokenVault::new(backend);
        vault
            .memory()
            .insert("sid1".into(), ("old".into(), "why".into()));
        assert_eq!(vault.save("sid1", "new").await, TokenPlace::Keyring);
        assert_eq!(vault.memory_only_reason("sid1"), None);
        assert_eq!(vault.load("sid1").await.unwrap().as_deref(), Some("new"));
    }
}
