//! SQLite persistence (`core-design.md` §6): an append-only event log plus
//! current-state tables of each project's domain [`State`].
//!
//! - [`Store::commit`] writes one domain transition — its events and the changed
//!   current-state rows — in a single transaction, so a crash can never leave one
//!   without the other.
//! - [`Store::append`] writes other durable [`ApiEvent`]s (session lifecycle,
//!   prompts, tool calls, coalesced agent text). Live events are never stored.
//! - On startup [`Store::load_state`] rebuilds the state from the current-state
//!   tables; [`Store::replay_state`] rebuilds it from the event log (for checks).
//! - Agent sessions (ACP session ids for `session/load`) are kept in
//!   `agent_sessions`, updated from the session events in the same transaction.

mod codec;
mod events;
mod projection;
mod sessions;
mod settings;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::agents::{AgentRole, RoleSettings};
use crate::api::ApiEvent;
use crate::domain::{DomainConfig, State};

pub use events::StoredEvent;
pub use sessions::{SessionRecord, SessionStatus};
pub use settings::StoredAgentSettings;

/// File name of the database inside the app data directory.
pub const DB_FILE_NAME: &str = "yhtye.sqlite3";

/// Path of the database for an app data directory.
#[must_use]
pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_FILE_NAME)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("could not encode or decode stored JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not create the database directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
}

/// A registered project (a row of `projects`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    /// The project's main worktree, as given when it was registered.
    pub path: PathBuf,
    pub created_ms: u64,
}

/// Handle to the database (cheap to clone; a small connection pool).
#[derive(Debug, Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and applies migrations.
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::from_str("sqlite://")?
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // WAL + NORMAL: a committed transaction survives an app crash (not
            // necessarily a power loss); never a torn transaction.
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Registers a new project with its initial (usually empty) state. Call it
    /// only when [`Store::load_state`] found no project (it writes every row).
    pub async fn create_project(&self, state: &State, path: &Path) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        projection::insert_project(&mut tx, state, path).await?;
        projection::write(&mut tx, &State::new(&state.project, state.config), state).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The current state of `project` from the current-state tables, or `None`
    /// if the project was never created.
    pub async fn load_state(&self, project: &str) -> Result<Option<State>, StoreError> {
        let mut conn = self.pool.acquire().await?;
        projection::load(&mut conn, project).await
    }

    /// The state of `project` rebuilt by replaying its domain events on an empty
    /// state with `config` (the event log alone; used to check the tables).
    pub async fn replay_state(
        &self,
        project: &str,
        config: DomainConfig,
    ) -> Result<State, StoreError> {
        let mut state = State::new(project, config);
        for event in events::domain_events(&self.pool, project).await? {
            state.apply(&event);
        }
        Ok(state)
    }

    /// Sequence number of the last stored event of `project` (0 if none).
    pub async fn last_seq(&self, project: &str) -> Result<u64, StoreError> {
        events::last_seq(&self.pool, project).await
    }

    /// Appends durable events (not a domain transition) in one transaction.
    pub async fn append(&self, events: &[ApiEvent]) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        write_events(&mut tx, events).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Writes one domain transition atomically: its `events` and the
    /// current-state rows that differ between `before` and `after`.
    pub async fn commit(
        &self,
        events: &[ApiEvent],
        before: &State,
        after: &State,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        write_events(&mut tx, events).await?;
        projection::write(&mut tx, before, after).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Durable events of `project` with `seq > after_seq`, oldest first.
    pub async fn events_after(
        &self,
        project: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        events::after(&self.pool, project, after_seq, limit).await
    }

    /// Durable events of one session of `project`, oldest first (its history).
    pub async fn session_events(
        &self,
        project: &str,
        session: &str,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        events::of_session(&self.pool, project, session).await
    }

    /// Every registered project, oldest first.
    pub async fn projects(&self) -> Result<Vec<ProjectRecord>, StoreError> {
        let rows: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT id, path, created_ms FROM projects ORDER BY created_ms, id")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|(id, path, created_ms)| {
                Ok(ProjectRecord {
                    id,
                    path: PathBuf::from(path),
                    created_ms: codec::uint(created_ms)?,
                })
            })
            .collect()
    }

    /// Every agent session recorded for `project`.
    pub async fn sessions(&self, project: &str) -> Result<Vec<SessionRecord>, StoreError> {
        sessions::list(&self.pool, project).await
    }

    /// Every stored agent settings row (global and per project).
    pub async fn agent_settings(&self) -> Result<Vec<StoredAgentSettings>, StoreError> {
        settings::list(&self.pool).await
    }

    /// Stores (or with `None` removes) the settings of `role` in the global
    /// layer (`project: None`) or a project's layer.
    pub async fn set_agent_settings(
        &self,
        project: Option<&str>,
        role: AgentRole,
        settings: Option<&RoleSettings>,
        now_ms: u64,
    ) -> Result<(), StoreError> {
        settings::set(&self.pool, project, role, settings, now_ms).await
    }
}

async fn write_events(
    tx: &mut Transaction<'static, Sqlite>,
    events: &[ApiEvent],
) -> Result<(), StoreError> {
    for event in events {
        events::insert(tx, event).await?;
        sessions::apply(tx, event).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
