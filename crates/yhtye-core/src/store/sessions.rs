//! The `agent_sessions` table, derived from the session events of the log in the
//! same transaction: which ACP session each Yhtye session key last used, whether
//! it can be restored, and whether a turn was running.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use ts_rs::TS;

use super::StoreError;
use super::codec::{int, name, parse};
use crate::acp::AgentEvent;
use crate::agents::AgentChoice;
use crate::api::{ApiEvent, ApiEventBody};
use crate::domain::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Live,
    /// Stopped by Yhtye (task finished, review done) or its process exited.
    Stopped,
    /// Stopped because Yhtye shut down.
    Suspended,
    /// Found live or suspended on startup.
    Interrupted,
}

impl SessionStatus {
    /// Whether the session should be restored (`session/load`) when used again.
    #[must_use]
    pub fn is_resumable(self) -> bool {
        !matches!(self, Self::Stopped)
    }
}

/// One row of `agent_sessions` (also part of [`crate::api::Snapshot`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct SessionRecord {
    pub session_key: String,
    pub role: Role,
    pub task: Option<String>,
    pub acp_session_id: String,
    pub status: SessionStatus,
    /// A prompt was sent and its turn had not ended.
    pub turn_running: bool,
    /// The harness × model × effort it ran (Stage 7b; `None` for older sessions).
    pub agent: Option<AgentChoice>,
}

#[derive(FromRow)]
struct Row {
    session_key: String,
    role: String,
    task_id: Option<String>,
    acp_session_id: String,
    status: String,
    turn_running: i64,
    harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

pub(super) async fn list(
    pool: &SqlitePool,
    project: &str,
) -> Result<Vec<SessionRecord>, StoreError> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT session_key, role, task_id, acp_session_id, status, turn_running, harness, model, effort \
         FROM agent_sessions WHERE project_id = ? ORDER BY session_key",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(SessionRecord {
                session_key: r.session_key,
                role: parse(&r.role)?,
                task: r.task_id,
                acp_session_id: r.acp_session_id,
                status: parse(&r.status)?,
                turn_running: r.turn_running != 0,
                agent: r.harness.map(|harness| AgentChoice {
                    harness,
                    model: r.model,
                    effort: r.effort,
                }),
            })
        })
        .collect()
}

/// Updates `agent_sessions` for a session event (other events change nothing).
pub(super) async fn apply(
    tx: &mut Transaction<'static, Sqlite>,
    event: &ApiEvent,
) -> Result<(), StoreError> {
    let (project, ts) = (&event.project, int(event.ts_ms)?);
    match &event.body {
        ApiEventBody::SessionStarted {
            session,
            role,
            task,
            acp_session_id,
            agent,
            ..
        } => {
            sqlx::query(
                "INSERT INTO agent_sessions \
                 (project_id, session_key, role, task_id, acp_session_id, status, turn_running, updated_ms, \
                 harness, model, effort) \
                 VALUES (?, ?, ?, ?, ?, 'live', 0, ?, ?, ?, ?) \
                 ON CONFLICT (project_id, session_key) DO UPDATE SET role = excluded.role, \
                 task_id = excluded.task_id, acp_session_id = excluded.acp_session_id, \
                 status = 'live', turn_running = 0, updated_ms = excluded.updated_ms, \
                 harness = excluded.harness, model = excluded.model, effort = excluded.effort",
            )
            .bind(project)
            .bind(session)
            .bind(name(role)?)
            .bind(task)
            .bind(acp_session_id)
            .bind(ts)
            .bind(agent.as_ref().map(|a| a.harness.as_str()))
            .bind(agent.as_ref().and_then(|a| a.model.as_deref()))
            .bind(agent.as_ref().and_then(|a| a.effort.as_deref()))
            .execute(&mut **tx)
            .await?;
        }
        ApiEventBody::SessionStopped { session, suspended } => {
            let status = if *suspended {
                SessionStatus::Suspended
            } else {
                SessionStatus::Stopped
            };
            // A suspended session keeps `turn_running` (it tells whether the
            // turn was cut off by the shutdown).
            let sql = if *suspended {
                "UPDATE agent_sessions SET status = ?, updated_ms = ? WHERE project_id = ? AND session_key = ?"
            } else {
                "UPDATE agent_sessions SET status = ?, turn_running = 0, updated_ms = ? \
                 WHERE project_id = ? AND session_key = ?"
            };
            update(tx, sql, &name(&status)?, ts, project, session).await?;
        }
        ApiEventBody::SessionInterrupted { session } => {
            let sql = "UPDATE agent_sessions SET status = ?, updated_ms = ? WHERE project_id = ? AND session_key = ?";
            update(
                tx,
                sql,
                &name(&SessionStatus::Interrupted)?,
                ts,
                project,
                session,
            )
            .await?;
        }
        ApiEventBody::Prompted { session, .. } => set_turn(tx, project, session, true, ts).await?,
        ApiEventBody::Agent {
            session,
            event: AgentEvent::TurnEnded(_),
        } => set_turn(tx, project, session, false, ts).await?,
        _ => {}
    }
    Ok(())
}

async fn update(
    tx: &mut Transaction<'static, Sqlite>,
    sql: &'static str,
    status: &str,
    ts: i64,
    project: &str,
    session: &str,
) -> Result<(), StoreError> {
    sqlx::query(sql)
        .bind(status)
        .bind(ts)
        .bind(project)
        .bind(session)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn set_turn(
    tx: &mut Transaction<'static, Sqlite>,
    project: &str,
    session: &str,
    running: bool,
    ts: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE agent_sessions SET turn_running = ?, updated_ms = ? WHERE project_id = ? AND session_key = ?",
    )
    .bind(i64::from(running))
    .bind(ts)
    .bind(project)
    .bind(session)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
