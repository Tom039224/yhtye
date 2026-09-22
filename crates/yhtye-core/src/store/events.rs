//! The `events` table: every durable [`ApiEvent`], append-only.

use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

use super::StoreError;
use super::codec::{int, uint};
use crate::api::ApiEvent;
use crate::domain::DomainEvent;

/// One stored event. `body` is the [`crate::api::ApiEventBody`] as JSON.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredEvent {
    pub seq: u64,
    pub ts_ms: u64,
    /// `<body type>` or `<body type>.<inner type>` (e.g. `domain.task_created`,
    /// `agent.turn_ended`, `agent_text`).
    pub kind: String,
    pub session: Option<String>,
    pub body: Value,
}

#[derive(FromRow)]
struct Row {
    seq: i64,
    ts_ms: i64,
    kind: String,
    session: Option<String>,
    payload: String,
}

impl Row {
    fn into_event(self) -> Result<StoredEvent, StoreError> {
        Ok(StoredEvent {
            seq: uint(self.seq)?,
            ts_ms: uint(self.ts_ms)?,
            kind: self.kind,
            session: self.session,
            body: serde_json::from_str(&self.payload)?,
        })
    }
}

/// `domain.task_created`, `agent.turn_ended`, `prompted`, ...
fn kind_of(body: &Value) -> String {
    let outer = body["type"].as_str().unwrap_or("unknown");
    let inner = match outer {
        "domain" => body["event"]["type"].as_str(),
        "agent" => body["event"]["type"].as_str(),
        _ => None,
    };
    inner.map_or_else(|| outer.to_string(), |i| format!("{outer}.{i}"))
}

pub(super) async fn insert(
    tx: &mut Transaction<'static, Sqlite>,
    event: &ApiEvent,
) -> Result<(), StoreError> {
    let body = serde_json::to_value(&event.body)?;
    sqlx::query(
        "INSERT INTO events (project_id, seq, ts_ms, kind, session, payload) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.project)
    .bind(int(event.seq)?)
    .bind(int(event.ts_ms)?)
    .bind(kind_of(&body))
    .bind(event.body.session())
    .bind(body.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn last_seq(pool: &SqlitePool, project: &str) -> Result<u64, StoreError> {
    let seq: Option<i64> = sqlx::query_scalar("SELECT MAX(seq) FROM events WHERE project_id = ?")
        .bind(project)
        .fetch_one(pool)
        .await?;
    seq.map_or(Ok(0), uint)
}

pub(super) async fn after(
    pool: &SqlitePool,
    project: &str,
    after_seq: u64,
    limit: u32,
) -> Result<Vec<StoredEvent>, StoreError> {
    sqlx::query_as::<_, Row>(
        "SELECT seq, ts_ms, kind, session, payload FROM events \
         WHERE project_id = ? AND seq > ? ORDER BY seq LIMIT ?",
    )
    .bind(project)
    .bind(int(after_seq)?)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Row::into_event)
    .collect()
}

pub(super) async fn of_session(
    pool: &SqlitePool,
    project: &str,
    session: &str,
) -> Result<Vec<StoredEvent>, StoreError> {
    sqlx::query_as::<_, Row>(
        "SELECT seq, ts_ms, kind, session, payload FROM events \
         WHERE project_id = ? AND session = ? ORDER BY seq",
    )
    .bind(project)
    .bind(session)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Row::into_event)
    .collect()
}

/// Every domain event of `project`, in order.
pub(super) async fn domain_events(
    pool: &SqlitePool,
    project: &str,
) -> Result<Vec<DomainEvent>, StoreError> {
    let payloads: Vec<String> = sqlx::query_scalar(
        // A range instead of LIKE so the (project_id, kind, seq) index is used.
        "SELECT payload FROM events WHERE project_id = ? AND kind >= 'domain.' AND kind < 'domain/' \
         ORDER BY seq",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    payloads
        .iter()
        .map(|p| {
            let mut body: Value = serde_json::from_str(p)?;
            Ok(serde_json::from_value(body["event"].take())?)
        })
        .collect()
}
