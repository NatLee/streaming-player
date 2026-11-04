//! Activity log: requests, skips, blocks, settings changes, chat connection problems.
//! Shown on the dashboard so the streamer can see what happened while they were live.

use serde::Serialize;
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::util::now;

/// Keep this many rows; older ones are pruned.
const KEEP_EVENTS: i64 = 5000;

/// Kinds that make up the audit log (accounts, roles, API tokens). Only people who manage
/// users read them (`/api/users/audit`); the dashboard's activity log leaves them out.
pub const AUDIT_KINDS: &[&str] = &["user", "role", "token"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct Event {
    pub id: i64,
    pub at: i64,
    pub actor: String,
    pub kind: String,
    pub detail: String,
}

/// Write one line. Logging must never break the action it describes, so errors are only traced.
pub async fn record(db: &SqlitePool, actor: &str, kind: &str, detail: &str) {
    let res = sqlx::query(
        "INSERT INTO events (at, actor, kind, detail) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(now())
    .bind(actor)
    .bind(kind)
    .bind(detail)
    .fetch_one(db)
    .await;
    match res {
        Ok(row) => {
            let id: i64 = sqlx::Row::get(&row, 0);
            if id % 100 == 0 {
                let _ = sqlx::query("DELETE FROM events WHERE id <= ?")
                    .bind(id - KEEP_EVENTS)
                    .execute(db)
                    .await;
            }
        }
        Err(err) => tracing::warn!("could not record event: {err}"),
    }
}

/// The newest activity, without the audit log.
pub async fn recent(db: &SqlitePool, limit: i64) -> anyhow::Result<Vec<Event>> {
    select(db, "NOT IN", limit).await
}

/// The newest audit log entries.
pub async fn audit(db: &SqlitePool, limit: i64) -> anyhow::Result<Vec<Event>> {
    select(db, "IN", limit).await
}

async fn select(db: &SqlitePool, membership: &str, limit: i64) -> anyhow::Result<Vec<Event>> {
    let sql = format!(
        "SELECT id, at, actor, kind, detail FROM events \
         WHERE kind {membership} (SELECT value FROM json_each(?)) ORDER BY id DESC LIMIT ?"
    );
    Ok(sqlx::query_as(&sql)
        .bind(serde_json::to_string(AUDIT_KINDS)?)
        .bind(limit.clamp(1, 500))
        .fetch_all(db)
        .await?)
}
