//! The `agent_settings` table (`core-design.md` §15.3): one row per layer
//! (global or a project) and role that does not inherit.

use sqlx::SqlitePool;

use super::StoreError;
use super::codec::{from_json, int, json, name, parse};
use crate::agents::{AgentRole, RoleSettings};

/// Scope of the global layer.
const GLOBAL: &str = "";

/// One stored row: `project: None` is the global layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentSettings {
    pub project: Option<String>,
    pub role: AgentRole,
    pub settings: RoleSettings,
}

pub(super) async fn list(pool: &SqlitePool) -> Result<Vec<StoredAgentSettings>, StoreError> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT scope, role, settings FROM agent_settings ORDER BY scope, role")
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|(scope, role, settings)| {
            Ok(StoredAgentSettings {
                project: (scope != GLOBAL).then_some(scope),
                role: parse(&role)?,
                settings: from_json(&settings)?,
            })
        })
        .collect()
}

pub(super) async fn set(
    pool: &SqlitePool,
    project: Option<&str>,
    role: AgentRole,
    settings: Option<&RoleSettings>,
    now_ms: u64,
) -> Result<(), StoreError> {
    let scope = project.unwrap_or(GLOBAL);
    match settings {
        Some(s) => {
            sqlx::query(
                "INSERT INTO agent_settings (scope, role, settings, updated_ms) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (scope, role) DO UPDATE SET settings = excluded.settings, \
                 updated_ms = excluded.updated_ms",
            )
            .bind(scope)
            .bind(name(&role)?)
            .bind(json(s)?)
            .bind(int(now_ms)?)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM agent_settings WHERE scope = ? AND role = ?")
                .bind(scope)
                .bind(name(&role)?)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}
