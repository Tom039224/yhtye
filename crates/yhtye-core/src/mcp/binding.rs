//! Per-session MCP tokens: `token → SessionBinding` (`core-design.md` §4).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::domain::Role;

/// Who is calling: the session's role and what it is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBinding {
    /// Yhtye's key of the agent session (not the ACP session id).
    pub session: String,
    pub role: Role,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Index of the task step this session runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
}

impl SessionBinding {
    #[must_use]
    pub fn orchestrator(session: impl Into<String>, project: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            role: Role::Orchestrator,
            project: project.into(),
            group: None,
            task: None,
            step: None,
        }
    }
}

/// Unguessable per-session token (128 random bits, hex). Part of the MCP URL.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct McpToken(String);

impl McpToken {
    /// A fresh random token.
    ///
    /// # Panics
    /// If the OS random source is unavailable (nothing sensible can run then).
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; 16];
        #[allow(clippy::expect_used)]
        getrandom::fill(&mut bytes).expect("OS random source must be available");
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for McpToken {
    // Never print the full secret in logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "McpToken({}…)", &self.0[..self.0.len().min(4)])
    }
}

/// Shared `token → binding` map. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct TokenRegistry {
    inner: Arc<RwLock<HashMap<String, SessionBinding>>>,
}

impl TokenRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Issues a new token for `binding`.
    pub fn issue(&self, binding: SessionBinding) -> McpToken {
        let token = McpToken::generate();
        self.write().insert(token.0.clone(), binding);
        token
    }

    #[must_use]
    pub fn get(&self, token: &str) -> Option<SessionBinding> {
        self.read().get(token).cloned()
    }

    /// Replaces the binding of a live token (e.g. a reused session moved to its
    /// next step). Returns `false` if the token is unknown.
    pub fn rebind(&self, token: &McpToken, binding: SessionBinding) -> bool {
        match self.write().get_mut(&token.0) {
            Some(b) => {
                *b = binding;
                true
            }
            None => false,
        }
    }

    /// Invalidates `token` (the session ended). Returns its binding if it existed.
    pub fn revoke(&self, token: &McpToken) -> Option<SessionBinding> {
        self.write().remove(&token.0)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.read().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, SessionBinding>> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, SessionBinding>> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_128_bit_hex() {
        let a = McpToken::generate();
        let b = McpToken::generate();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!format!("{a:?}").contains(a.as_str()));
    }

    #[test]
    fn issue_get_revoke() {
        let reg = TokenRegistry::new();
        let binding = SessionBinding::orchestrator("orch", "P-1");
        let token = reg.issue(binding.clone());
        assert_eq!(reg.get(token.as_str()), Some(binding.clone()));
        assert_eq!(reg.get("nope"), None);
        let mut moved = binding.clone();
        moved.step = Some(2);
        assert!(reg.rebind(&token, moved.clone()));
        assert_eq!(reg.get(token.as_str()), Some(moved.clone()));
        assert_eq!(reg.revoke(&token), Some(moved.clone()));
        assert!(!reg.rebind(&token, moved));
        assert_eq!(reg.get(token.as_str()), None);
        assert!(reg.is_empty());
    }
}
