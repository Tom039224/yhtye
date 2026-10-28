//! The `harness_prompt_caps` table: what each harness last said it accepts in a
//! prompt, written from the `session_started` events of the log in the same
//! transaction.

use std::collections::HashMap;

use sqlx::{Sqlite, SqlitePool, Transaction};

use super::StoreError;
use super::codec::int;
use crate::api::{ApiEvent, ApiEventBody};
use crate::attachments::PromptCaps;

/// The last observed capabilities of every harness that has started a session.
pub(super) async fn list(pool: &SqlitePool) -> Result<HashMap<String, PromptCaps>, StoreError> {
    let rows: Vec<(String, i64, i64)> =
        sqlx::query_as("SELECT harness, image, embedded_context FROM harness_prompt_caps")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(harness, image, embedded)| {
            let caps = PromptCaps {
                image: image != 0,
                embedded_context: embedded != 0,
            };
            (harness, caps)
        })
        .collect())
}

/// Records the capabilities a started session reported.
pub(super) async fn apply(
    tx: &mut Transaction<'static, Sqlite>,
    event: &ApiEvent,
) -> Result<(), StoreError> {
    let ApiEventBody::SessionStarted {
        agent: Some(agent),
        prompt_caps: Some(caps),
        ..
    } = &event.body
    else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO harness_prompt_caps (harness, image, embedded_context, updated_ms) \
         VALUES (?, ?, ?, ?) ON CONFLICT (harness) DO UPDATE SET image = excluded.image, \
         embedded_context = excluded.embedded_context, updated_ms = excluded.updated_ms",
    )
    .bind(&agent.harness)
    .bind(i64::from(caps.image))
    .bind(i64::from(caps.embedded_context))
    .bind(int(event.ts_ms)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
