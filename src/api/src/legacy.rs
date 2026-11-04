//! Compatibility with v1 (the Django version). Everything v2 keeps for old setups is here.
//!
//! Why: streamers set up Nightbot commands like
//! `$(urlfetch https://<host>/player/nightbot/order?user=$(user)&url=$(querystring))`.
//! These URLs keep answering, through the same command engine as everything else, so the
//! texts, limits and permissions configured on the admin page apply to them too.
//! Differences from v1: replies are plain text (v1 sent a JSON string with quotes), and
//! the old JWT-protected `/player/playlist/*` APIs and `/api/auth/*` tokens are gone
//! (only v1's own page used them).
//!
//! It also holds the one-time import of a v1 database (`streaming-player import-v1`).
//!
//! When to remove: when nobody calls these URLs any more. The settings page shows the
//! last time one was used (`legacy_last_hit`), and the `legacy_api` setting turns them off.
//! Plan: drop this module in v3.

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{
    commands::{engine, Action},
    db,
    error::AppResult,
    live,
    models::{Level, Requester, Source},
    routes::bot::text,
    state::AppState,
    util::now,
};

const LAST_HIT_KEY: &str = "legacy_last_hit";

pub async fn last_hit(state: &AppState) -> anyhow::Result<Option<i64>> {
    Ok(db::kv_get(&state.db, LAST_HIT_KEY)
        .await?
        .and_then(|v| v.parse().ok()))
}

#[derive(Deserialize, IntoParams)]
pub struct UserQuery {
    /// Chatter name (Nightbot `$(user)`)
    pub user: Option<String>,
    /// Song link or search words (Nightbot `$(querystring)`)
    pub url: Option<String>,
}

/// Run a v1 call through the command engine. `None` user is allowed only for `current`.
async fn call(
    state: &AppState,
    action: Action,
    user: Option<&str>,
    args: &str,
) -> AppResult<Response> {
    if !state.settings().legacy_api {
        return Ok(text(
            StatusCode::GONE,
            "舊版 Nightbot 網址已經關閉，請改用管理頁上的新網址",
        ));
    }
    db::kv_set(&state.db, LAST_HIT_KEY, &now().to_string()).await?;
    let name = user.map(str::trim).filter(|u| !u.is_empty());
    let who = match (name, action) {
        (Some(n), _) => Requester::new(n, None, Level::Everyone, Source::Legacy),
        (None, Action::Current) => {
            Requester::new("nightbot", None, Level::Everyone, Source::Legacy)
        }
        (None, _) => return Ok(text(StatusCode::OK, "記得要填使用者！")),
    };
    let reply = engine::run_action(state, &who, action, args).await?;
    Ok(text(StatusCode::OK, reply.text))
}

#[utoipa::path(get, path = "/player/nightbot/current", tag = "legacy (v1)", responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn current(State(state): State<AppState>) -> AppResult<Response> {
    call(&state, Action::Current, None, "").await
}

/// Vote to skip the current song (v1: 3 votes).
#[utoipa::path(get, path = "/player/nightbot/current/poll", tag = "legacy (v1)", params(UserQuery), responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn poll(
    State(state): State<AppState>,
    Query(q): Query<UserQuery>,
) -> AppResult<Response> {
    call(&state, Action::VoteSkip, q.user.as_deref(), "").await
}

#[utoipa::path(get, path = "/player/nightbot/order", tag = "legacy (v1)", params(UserQuery), responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn order(
    State(state): State<AppState>,
    Query(q): Query<UserQuery>,
    RawQuery(raw): RawQuery,
) -> AppResult<Response> {
    // A link pasted without encoding arrives split at its "&"; put it back together.
    let url = crate::chat::bots::rejoin(raw.as_deref().unwrap_or(""), &["url"], &["user", "url"]);
    call(
        &state,
        Action::Request,
        q.user.as_deref(),
        url.as_deref().unwrap_or(""),
    )
    .await
}

#[utoipa::path(get, path = "/player/nightbot/order/{user}/count", tag = "legacy (v1)", responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn count(State(state): State<AppState>, Path(user): Path<String>) -> AppResult<Response> {
    call(&state, Action::Stats, Some(&user), &user).await
}

/// Delete your own request by its id.
#[utoipa::path(get, path = "/player/nightbot/{id}/delete", tag = "legacy (v1)", params(UserQuery), responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<UserQuery>,
) -> AppResult<Response> {
    call(&state, Action::Remove, q.user.as_deref(), &id).await
}

/// Vote to move a request to play next (v1: 2 votes).
#[utoipa::path(get, path = "/player/nightbot/{id}/insert", tag = "legacy (v1)", params(UserQuery), responses((status = 200, body = String, content_type = "text/plain")))]
pub async fn insert(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<UserQuery>,
) -> AppResult<Response> {
    call(&state, Action::VoteBump, q.user.as_deref(), &id).await
}

/// v1's public queue JSON, same field names.
#[derive(Serialize, ToSchema)]
pub struct V1QueueItem {
    pub id: i64,
    pub song_name: String,
    pub url: String,
    pub user: String,
    pub order: u32,
    pub created_at: String,
}

#[utoipa::path(get, path = "/player/playlist/queue", tag = "legacy (v1)", responses((status = 200, body = [V1QueueItem])))]
pub async fn queue_json(State(state): State<AppState>) -> AppResult<Response> {
    if !state.settings().legacy_api {
        return Ok(text(StatusCode::GONE, "legacy API is off"));
    }
    let snap = live::snapshot(&state).await?;
    let iso = |t: i64| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|d| d.to_rfc3339())
            .unwrap_or_default()
    };
    let mut items: Vec<V1QueueItem> = Vec::new();
    if let Some(n) = &snap.now {
        items.push(V1QueueItem {
            id: n.entry.id,
            song_name: n.entry.song.title.clone(),
            url: n.entry.song.url.clone(),
            user: n.entry.requester.clone(),
            order: 1,
            created_at: iso(n.entry.requested_at),
        });
    }
    items.extend(snap.queue.iter().map(|i| V1QueueItem {
        id: i.entry.id,
        song_name: i.entry.song.title.clone(),
        url: i.entry.song.url.clone(),
        user: i.entry.requester.clone(),
        order: i.position,
        created_at: iso(i.entry.requested_at),
    }));
    Ok(Json(items).into_response())
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "response": "pong", "detail": "you got it! ;)" }))
}

/// v1 page addresses that people may have bookmarked.
const REDIRECTS: &[(&str, &str)] = &[
    ("/player", "/admin"),
    ("/player/", "/admin"),
    ("/player/login", "/login"),
    ("/api/__hidden_admin", "/admin"),
    ("/api/__hidden_admin/", "/admin"),
    ("/api/__hidden_swagger", "/api/docs/"),
    ("/api/__hidden_swagger/", "/api/docs/"),
    ("/api/__hidden_redoc", "/api/docs/"),
    ("/api/__hidden_redoc/", "/api/docs/"),
];

/// Routes outside the OpenAPI router: the ping and the page redirects.
pub fn extra_routes() -> Router<AppState> {
    let mut r = Router::new().route("/player/ping", get(ping));
    for (from, to) in REDIRECTS {
        let to = *to;
        r = r.route(from, get(move || async move { Redirect::permanent(to) }));
    }
    r
}

// ───────────────────────────── v1 database import ─────────────────────────────

const IMPORTED_KEY: &str = "v1_imported_at";

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct V1Import {
    pub songs: usize,
    pub requests: usize,
    pub skipped: usize,
}

/// Django stored times as naive UTC text, e.g. `2023-01-22 12:34:56.123456`.
fn v1_time(s: &str) -> i64 {
    let s = s.trim().trim_end_matches("+00:00");
    [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ]
    .iter()
    .find_map(|f| chrono::NaiveDateTime::parse_from_str(s, f).ok())
    .map(|t| t.and_utc().timestamp())
    .unwrap_or_else(now)
}

/// Copy v1's song library (with favorite / blocked / missing flags) and its request
/// history from v1's `backend/db.sqlite3`, so favorites keep playing and `!songstats`
/// keeps counting. v1's pending queue is not copied. Runs once per database.
pub async fn import_v1(state: &AppState, path: &std::path::Path) -> anyhow::Result<V1Import> {
    if db::kv_get(&state.db, IMPORTED_KEY).await?.is_some() {
        anyhow::bail!("v1 data was already imported into this database");
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true);
    let old = sqlx::SqlitePool::connect_with(options).await?;
    let songs: Vec<(i64, String, String, i64, bool, bool, bool)> = sqlx::query_as(
        "SELECT id, song_name, url, duration, favorite, block, missing FROM playlist ORDER BY id",
    )
    .fetch_all(&old)
    .await?;
    let mut report = V1Import::default();
    let mut ids = std::collections::HashMap::new();
    for (old_id, title, url, duration, favorite, blocked, missing) in songs {
        let Some(r) =
            crate::media::resolve::offline(&url, &title, (duration > 0).then_some(duration))
        else {
            report.skipped += 1;
            continue;
        };
        let song = crate::media::resolve::upsert(&state.db, &r).await?;
        sqlx::query("UPDATE songs SET favorite = ?, blocked = ?, missing = ? WHERE id = ?")
            .bind(favorite)
            .bind(blocked)
            .bind(missing)
            .bind(song.id)
            .execute(&state.db)
            .await?;
        ids.insert(old_id, song.id);
        report.songs += 1;
    }
    let history: Vec<(i64, String, String, bool, bool, bool)> = sqlx::query_as(
        "SELECT playlist_id, user, created_at, played, cut, autoplay FROM playlist_order_history ORDER BY id",
    )
    .fetch_all(&old)
    .await?;
    let mut tx = db::write_tx(&state.db).await?;
    for (old_song, user, created, played, cut, autoplay) in history {
        let Some(song_id) = ids.get(&old_song) else {
            report.skipped += 1;
            continue;
        };
        let status = if played {
            "played"
        } else if cut {
            "skipped"
        } else {
            "removed"
        };
        let at = v1_time(&created);
        let who = Requester::new(&user, None, Level::Everyone, Source::Legacy);
        sqlx::query(
            "INSERT INTO requests (song_id, requester, requester_key, source, status, autoplay, requested_at, ended_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(song_id)
        .bind(&who.name)
        .bind(&who.key)
        .bind(if autoplay { Source::Autoplay } else { Source::Legacy }.as_str())
        .bind(status)
        .bind(autoplay)
        .bind(at)
        .bind(at)
        .execute(&mut *tx)
        .await?;
        report.requests += 1;
    }
    tx.commit().await?;
    db::kv_set(&state.db, IMPORTED_KEY, &now().to_string()).await?;
    crate::events::record(
        &state.db,
        "import",
        "import",
        &format!(
            "從 v1 匯入 {} 首歌、{} 筆點歌紀錄",
            report.songs, report.requests
        ),
    )
    .await;
    Ok(report)
}
