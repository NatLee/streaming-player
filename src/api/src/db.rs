use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Sqlite, SqlitePool, Transaction,
};

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .with_context(|| format!("cannot open database {}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("database migration failed")?;
    Ok(pool)
}

/// A transaction that takes the write lock up front. A plain BEGIN starts as a reader;
/// if another connection writes before ours upgrades, SQLite fails at once with
/// "database is locked" instead of waiting for the busy timeout.
pub async fn write_tx(db: &SqlitePool) -> Result<Transaction<'static, Sqlite>> {
    Ok(db.begin_with("BEGIN IMMEDIATE").await?)
}

pub async fn kv_get(db: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("SELECT value FROM kv WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await?)
}

pub async fn kv_set(db: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO kv (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn kv_delete(db: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM kv WHERE key = ?")
        .bind(key)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn kv_get_json<T: DeserializeOwned>(db: &SqlitePool, key: &str) -> Result<Option<T>> {
    match kv_get(db, key).await? {
        Some(raw) => Ok(Some(
            serde_json::from_str(&raw).with_context(|| format!("kv {key} is not valid JSON"))?,
        )),
        None => Ok(None),
    }
}

pub async fn kv_set_json<T: Serialize>(db: &SqlitePool, key: &str, value: &T) -> Result<()> {
    kv_set(db, key, &serde_json::to_string(value)?).await
}
