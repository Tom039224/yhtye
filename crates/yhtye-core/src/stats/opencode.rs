//! OpenCode: one SQLite database (`opencode.db`), opened read-only (it may be
//! in WAL mode with OpenCode running).
//!
//! Two layouts live side by side: the older `message` table (assistant rows
//! have `data.modelID` and `data.tokens`, sessions are in `session`) and the
//! newer `session_message` / `session_v2` (`data.model.id`, `data.tokens`),
//! which copies the older rows it replaces. A `message` row that has a
//! `session_message` twin is read from the latter only. The tokens are
//! `{input, output, cache: {read, write}}`; a sub-session counts for its parent.

use std::collections::HashSet;
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};

use super::record::{FileBuilder, ParsedFile, Tokens};

const UNKNOWN_MODEL: &str = "unknown";
/// OpenCode may hold a write lock for a moment.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(FromRow)]
struct Row {
    session: String,
    ts: i64,
    model: Option<String>,
    directory: Option<String>,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
}

fn count(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// Reads the database at `path`.
pub async fn read(path: &Path) -> Result<ParsedFile, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(BUSY_TIMEOUT);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let result = read_from(&pool).await;
    pool.close().await;
    result
}

async fn read_from(pool: &SqlitePool) -> Result<ParsedFile, sqlx::Error> {
    let tables: HashSet<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let has = |t: &str| tables.contains(t);
    let mut builder = FileBuilder::new();

    if has("session_message") && has("session_v2") {
        let dir = directory_expr(pool, "session_v2", "s").await?;
        let sql = format!(
            "SELECT COALESCE(s.parent_id, sm.session_id) AS session, sm.time_created AS ts, \
                    json_extract(sm.data, '$.model.id') AS model, {dir} AS directory, {tokens} \
             FROM session_message sm LEFT JOIN session_v2 s ON s.id = sm.session_id \
             WHERE sm.type = 'assistant' AND json_extract(sm.data, '$.tokens') IS NOT NULL",
            tokens = token_columns("sm"),
        );
        push_rows(pool, &sql, &mut builder).await?;
    }
    if has("message") && has("session") {
        let dir = directory_expr(pool, "session", "s").await?;
        let twin = if has("session_message") {
            " AND NOT EXISTS (SELECT 1 FROM session_message sm \
                             WHERE sm.id = m.id AND sm.session_id = m.session_id)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT COALESCE(s.parent_id, m.session_id) AS session, m.time_created AS ts, \
                    json_extract(m.data, '$.modelID') AS model, {dir} AS directory, {tokens} \
             FROM message m LEFT JOIN session s ON s.id = m.session_id \
             WHERE json_extract(m.data, '$.role') = 'assistant' \
               AND json_extract(m.data, '$.tokens') IS NOT NULL{twin}",
            tokens = token_columns("m"),
        );
        push_rows(pool, &sql, &mut builder).await?;
    }
    Ok(builder.finish())
}

/// The token columns of an assistant row whose JSON is `alias.data`.
fn token_columns(alias: &str) -> String {
    let n = |path: &str| {
        format!("CAST(COALESCE(json_extract({alias}.data, '$.tokens.{path}'), 0) AS INTEGER)")
    };
    format!(
        "{} AS input, {} AS output, {} AS cache_read, {} AS cache_write",
        n("input"),
        n("output"),
        n("cache.read"),
        n("cache.write")
    )
}

/// The working directory of a row: the session's `directory` (older schemas
/// have none).
async fn directory_expr(
    pool: &SqlitePool,
    table: &str,
    alias: &str,
) -> Result<String, sqlx::Error> {
    let has_directory: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = 'directory'")
            .bind(table)
            .fetch_one(pool)
            .await?;
    Ok(if has_directory > 0 {
        format!("{alias}.directory")
    } else {
        "NULL".to_string()
    })
}

/// `sql` is assembled from this module's own constants only (no outside text).
async fn push_rows(
    pool: &SqlitePool,
    sql: &str,
    builder: &mut FileBuilder,
) -> Result<(), sqlx::Error> {
    for row in sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(pool)
        .await?
    {
        let tokens = Tokens {
            input: count(row.input),
            output: count(row.output),
            cache_read: count(row.cache_read),
            cache_creation: count(row.cache_write),
        };
        if tokens.is_zero() || row.ts < 0 {
            continue;
        }
        builder.push(
            &row.session,
            row.model
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or(UNKNOWN_MODEL),
            row.directory.as_deref().unwrap_or_default(),
            count(row.ts),
            None,
            tokens,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::Executor;

    use super::*;

    async fn database(dir: &Path, statements: &[String]) -> std::path::PathBuf {
        let path = dir.join("opencode.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        for s in statements {
            pool.execute(sqlx::AssertSqlSafe(s.clone())).await.unwrap();
        }
        pool.close().await;
        path
    }

    fn assistant(model_key: &str, input: u64, output: u64, read: u64, write: u64) -> String {
        let model = if model_key == "modelID" {
            json!({"modelID": "kimi-k2"})
        } else {
            json!({"model": {"id": "muse-spark"}})
        };
        let mut data = json!({"role": "assistant", "tokens": {
            "input": input, "output": output, "reasoning": 0, "cache": {"read": read, "write": write}}});
        data.as_object_mut()
            .unwrap()
            .extend(model.as_object().unwrap().clone());
        data.to_string().replace('\'', "''")
    }

    const LEGACY_SCHEMA: [&str; 2] = [
        "CREATE TABLE session (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL)",
        "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, \
         time_created INTEGER NOT NULL, data TEXT NOT NULL)",
    ];

    #[tokio::test]
    async fn the_older_layout_is_read_with_models_directories_and_sub_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sql: Vec<String> = LEGACY_SCHEMA.map(String::from).to_vec();
        sql.push("INSERT INTO session VALUES ('s1', NULL, '/work/app')".into());
        sql.push("INSERT INTO session VALUES ('s1-child', 's1', '/work/app')".into());
        sql.push(format!(
            "INSERT INTO message VALUES ('m1', 's1', 1000, '{}')",
            assistant("modelID", 10, 20, 30, 40)
        ));
        sql.push(format!(
            "INSERT INTO message VALUES ('m2', 's1-child', 2000, '{}')",
            assistant("modelID", 1, 2, 3, 4)
        ));
        sql.push(
            "INSERT INTO message VALUES ('m3', 's1', 3000, '{\"role\":\"user\",\"tokens\":{\"input\":9}}')"
                .into(),
        );
        sql.push(format!(
            "INSERT INTO message VALUES ('m4', 's1', 4000, '{}')",
            assistant("modelID", 0, 0, 0, 0)
        ));
        let path = database(tmp.path(), &sql).await;

        let f = read(&path).await.unwrap();
        assert_eq!(f.records.len(), 2);
        let r = &f.records[0];
        assert_eq!(
            (
                r.tokens.input,
                r.tokens.output,
                r.tokens.cache_read,
                r.tokens.cache_creation
            ),
            (10, 20, 30, 40)
        );
        assert_eq!(r.ts_ms, 1000);
        assert_eq!(&*f.models[r.model as usize], "kimi-k2");
        assert_eq!(&*f.cwds[r.cwd as usize], "/work/app");
        // The sub-session is counted for its parent.
        assert_eq!(f.sessions.len(), 1);
        assert_eq!(&*f.sessions[0], "s1");
    }

    #[tokio::test]
    async fn the_newer_layout_wins_over_the_copies_it_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sql: Vec<String> = LEGACY_SCHEMA.map(String::from).to_vec();
        sql.push("CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL)".into());
        sql.push(
            "CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, \
             type TEXT NOT NULL, time_created INTEGER NOT NULL, data TEXT NOT NULL)"
                .into(),
        );
        sql.push("INSERT INTO session VALUES ('old', NULL, '/old')".into());
        sql.push("INSERT INTO session_v2 VALUES ('old', NULL, '/old')".into());
        sql.push("INSERT INTO session_v2 VALUES ('new', NULL, '/new')".into());
        // `m1` exists in both tables: counted once. `m2` only in the old one.
        let a = assistant("modelID", 5, 5, 5, 5);
        sql.push(format!(
            "INSERT INTO message VALUES ('m1', 'old', 1000, '{a}')"
        ));
        sql.push(format!(
            "INSERT INTO message VALUES ('m2', 'old', 2000, '{a}')"
        ));
        let b = assistant("model.id", 7, 0, 0, 0);
        sql.push(format!(
            "INSERT INTO session_message VALUES ('m1', 'old', 'assistant', 1000, '{b}')"
        ));
        sql.push(format!(
            "INSERT INTO session_message VALUES ('n1', 'new', 'assistant', 3000, '{b}')"
        ));
        sql.push(
            "INSERT INTO session_message VALUES ('u1', 'new', 'user', 3500, '{\"text\":\"hi\"}')"
                .into(),
        );
        let path = database(tmp.path(), &sql).await;

        let f = read(&path).await.unwrap();
        let mut seen: Vec<(u64, u64, String, String)> = f
            .records
            .iter()
            .map(|r| {
                (
                    r.ts_ms,
                    r.tokens.input,
                    f.models[r.model as usize].to_string(),
                    f.cwds[r.cwd as usize].to_string(),
                )
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                (1000, 7, "muse-spark".into(), "/old".into()),
                (2000, 5, "kimi-k2".into(), "/old".into()),
                (3000, 7, "muse-spark".into(), "/new".into()),
            ]
        );
    }

    #[tokio::test]
    async fn a_database_without_the_tables_or_a_missing_one_contributes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = database(tmp.path(), &["CREATE TABLE other (x INTEGER)".into()]).await;
        assert!(read(&empty).await.unwrap().records.is_empty());
        assert!(read(&tmp.path().join("missing.db")).await.is_err());
    }
}
