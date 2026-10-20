//! Remote access (`core-design.md` §18): the paired devices (`remote_devices`)
//! and app-wide settings (`app_settings`, key → JSON text).

use sqlx::SqlitePool;

use super::StoreError;
use super::codec::{int, name, parse, uint};
use crate::remote::ClientKind;

/// A paired device. `token_hash` is the SHA-256 of its token (hex); the token
/// itself is never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    pub kind: ClientKind,
    pub token_hash: String,
    pub created_ms: u64,
    pub last_seen_ms: Option<u64>,
}

type DeviceRow = (String, String, String, String, i64, Option<i64>);

fn device(row: DeviceRow) -> Result<DeviceRecord, StoreError> {
    let (id, device_name, kind, token_hash, created_ms, last_seen_ms) = row;
    Ok(DeviceRecord {
        id,
        name: device_name,
        kind: parse(&kind)?,
        token_hash,
        created_ms: uint(created_ms)?,
        last_seen_ms: last_seen_ms.map(uint).transpose()?,
    })
}

/// Every paired device, oldest first.
pub(super) async fn devices(pool: &SqlitePool) -> Result<Vec<DeviceRecord>, StoreError> {
    let rows: Vec<DeviceRow> = sqlx::query_as(
        "SELECT id, name, kind, token_hash, created_ms, last_seen_ms FROM remote_devices \
         ORDER BY created_ms, id",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(device).collect()
}

pub(super) async fn add_device(pool: &SqlitePool, d: &DeviceRecord) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO remote_devices (id, name, kind, token_hash, created_ms, last_seen_ms) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&d.id)
    .bind(&d.name)
    .bind(name(&d.kind)?)
    .bind(&d.token_hash)
    .bind(int(d.created_ms)?)
    .bind(d.last_seen_ms.map(int).transpose()?)
    .execute(pool)
    .await?;
    Ok(())
}

/// Removes a device; whether it existed.
pub(super) async fn remove_device(pool: &SqlitePool, id: &str) -> Result<bool, StoreError> {
    let done = sqlx::query("DELETE FROM remote_devices WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}

pub(super) async fn touch_device(
    pool: &SqlitePool,
    id: &str,
    now_ms: u64,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE remote_devices SET last_seen_ms = ? WHERE id = ?")
        .bind(int(now_ms)?)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn app_setting(
    pool: &SqlitePool,
    key: &str,
) -> Result<Option<String>, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

pub(super) async fn set_app_setting(
    pool: &SqlitePool,
    key: &str,
    value: &str,
    now_ms: u64,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_ms) VALUES (?, ?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms",
    )
    .bind(key)
    .bind(value)
    .bind(int(now_ms)?)
    .execute(pool)
    .await?;
    Ok(())
}

/// The value of `key`, storing `value` first if there is none (the first
/// writer wins; later calls return what it stored).
pub(super) async fn app_setting_or_insert(
    pool: &SqlitePool,
    key: &str,
    value: &str,
    now_ms: u64,
) -> Result<String, StoreError> {
    sqlx::query("INSERT OR IGNORE INTO app_settings (key, value, updated_ms) VALUES (?, ?, ?)")
        .bind(key)
        .bind(value)
        .bind(int(now_ms)?)
        .execute(pool)
        .await?;
    app_setting(pool, key)
        .await?
        .ok_or_else(|| StoreError::Corrupt(format!("app setting {key} vanished")))
}
