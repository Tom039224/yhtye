//! The `chats` table (`core-design.md` §17.2): the domain [`Chat`]s plus the two
//! times the store derives from the event log in the same transaction
//! (`created_ms` from `chat_created`, `last_used_ms` from the chat's `prompted`).

use std::path::PathBuf;

use serde::Serialize;
use sqlx::{FromRow, Sqlite, SqliteConnection, SqlitePool, Transaction};
use ts_rs::TS;

use super::StoreError;
use super::codec::{int, uint};
use crate::api::{ApiEvent, ApiEventBody};
use crate::domain::{Chat, DomainEvent, chat_of_session};

/// A chat as the UI lists it (`Snapshot.chats`, `list_chats`). Its branch is
/// not stored: the UI reads what the worktree has checked out (Stage 8e).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ChatInfo {
    pub id: String,
    /// The chat's worktree, as `git worktree list` shows it.
    pub worktree: String,
    pub title: Option<String>,
    pub created_ms: u64,
    /// When the chat's orchestrator was last prompted (its creation until then).
    pub last_used_ms: u64,
}

#[derive(FromRow)]
struct Row {
    id: String,
    worktree: String,
    title: Option<String>,
    created_ms: i64,
    last_used_ms: i64,
}

/// The chats of `project`, most recently used first.
pub(super) async fn list(pool: &SqlitePool, project: &str) -> Result<Vec<ChatInfo>, StoreError> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT id, worktree, title, created_ms, last_used_ms FROM chats \
         WHERE project_id = ? ORDER BY last_used_ms DESC, ord DESC",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(ChatInfo {
                id: r.id,
                worktree: r.worktree,
                title: r.title,
                created_ms: uint(r.created_ms)?,
                last_used_ms: uint(r.last_used_ms)?,
            })
        })
        .collect()
}

/// The domain chats of `project`, in creation order.
pub(super) async fn load(
    conn: &mut SqliteConnection,
    project: &str,
) -> Result<Vec<Chat>, StoreError> {
    let rows: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT id, worktree, title FROM chats WHERE project_id = ? ORDER BY ord")
            .bind(project)
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, worktree, title)| Chat {
            id,
            worktree: PathBuf::from(worktree),
            title,
        })
        .collect())
}

/// Writes the chats of `after` that differ from `before` and deletes the ones
/// that are gone (a chat created by an event was inserted with its time
/// already; this only keeps the table equal to the state).
pub(super) async fn write(
    tx: &mut Transaction<'static, Sqlite>,
    project: &str,
    before: &[Chat],
    after: &[Chat],
) -> Result<(), StoreError> {
    for gone in before
        .iter()
        .filter(|b| !after.iter().any(|a| a.id == b.id))
    {
        sqlx::query("DELETE FROM chats WHERE project_id = ? AND id = ?")
            .bind(project)
            .bind(&gone.id)
            .execute(&mut **tx)
            .await?;
    }
    let now = now_ms();
    for (ord, c) in after.iter().enumerate() {
        if before.iter().any(|b| b == c) {
            continue;
        }
        sqlx::query(
            "INSERT INTO chats (project_id, id, ord, worktree, title, created_ms, last_used_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (project_id, id) DO UPDATE SET \
             worktree = excluded.worktree, title = excluded.title",
        )
        .bind(project)
        .bind(&c.id)
        .bind(int(ord)?)
        .bind(c.worktree.to_string_lossy().into_owned())
        .bind(&c.title)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Updates the times of `chats` for an event.
pub(super) async fn apply(
    tx: &mut Transaction<'static, Sqlite>,
    event: &ApiEvent,
) -> Result<(), StoreError> {
    let (project, ts) = (&event.project, int(event.ts_ms)?);
    match &event.body {
        ApiEventBody::Domain {
            event: DomainEvent::ChatCreated { chat },
        } => {
            sqlx::query(
                "INSERT INTO chats (project_id, id, ord, worktree, title, created_ms, last_used_ms) \
                 VALUES (?, ?, (SELECT COALESCE(MAX(ord) + 1, 0) FROM chats WHERE project_id = ?), ?, ?, ?, ?) \
                 ON CONFLICT (project_id, id) DO NOTHING",
            )
            .bind(project)
            .bind(&chat.id)
            .bind(project)
            .bind(chat.worktree.to_string_lossy().into_owned())
            .bind(&chat.title)
            .bind(ts)
            .bind(ts)
            .execute(&mut **tx)
            .await?;
        }
        ApiEventBody::Prompted { session, .. } => {
            let Some(chat) = chat_of_session(session) else {
                return Ok(());
            };
            sqlx::query(
                "UPDATE chats SET last_used_ms = ? WHERE project_id = ? AND id = ? \
                 AND last_used_ms < ?",
            )
            .bind(ts)
            .bind(project)
            .bind(chat)
            .bind(ts)
            .execute(&mut **tx)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
