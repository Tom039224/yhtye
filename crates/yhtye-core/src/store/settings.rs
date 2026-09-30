//! The `agent_settings` table (`core-design.md` §15.3): one row per layer
//! (global or a project) and role that does not inherit.

use std::collections::HashMap;

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

/// The registered secret environment variable names (the `secret_env_names`
/// table). Only names: the values are in the OS credential store.
pub(super) async fn secret_names(pool: &SqlitePool) -> Result<Vec<String>, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT name FROM secret_env_names ORDER BY name")
            .fetch_all(pool)
            .await?,
    )
}

pub(super) async fn add_secret_name(
    pool: &SqlitePool,
    name: &str,
    now_ms: u64,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO secret_env_names (name, updated_ms) VALUES (?, ?) \
         ON CONFLICT (name) DO UPDATE SET updated_ms = excluded.updated_ms",
    )
    .bind(name)
    .bind(int(now_ms)?)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn remove_secret_name(pool: &SqlitePool, name: &str) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM secret_env_names WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

/// The manual executable paths, by harness id (the `harness_paths` table).
pub(super) async fn harness_paths(
    pool: &SqlitePool,
) -> Result<HashMap<String, String>, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT harness, path FROM harness_paths")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Stores the manual path of `harness`; `None` removes it (detect automatically).
pub(super) async fn set_harness_path(
    pool: &SqlitePool,
    harness: &str,
    path: Option<&str>,
    now_ms: u64,
) -> Result<(), StoreError> {
    match path {
        Some(path) => {
            sqlx::query(
                "INSERT INTO harness_paths (harness, path, updated_ms) VALUES (?, ?, ?) \
                 ON CONFLICT (harness) DO UPDATE SET path = excluded.path, \
                 updated_ms = excluded.updated_ms",
            )
            .bind(harness)
            .bind(path)
            .bind(int(now_ms)?)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM harness_paths WHERE harness = ?")
                .bind(harness)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}
