//! Song library (default playlist = favorites), history, block list and activity log.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::queue::{block_user, rejection_message, resolve_message};
use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult},
    events::{self, Event},
    live,
    media::resolve,
    models::{Entry, Level, Requester, Song, Source, ENTRY_COLUMNS},
    queue::{self, Enqueued, Placement, UserStats},
    state::AppState,
};

#[derive(Deserialize, IntoParams)]
pub struct LibraryQuery {
    /// Search in title and URL.
    pub q: Option<String>,
    /// `favorite`, `blocked` or `missing`
    pub filter: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct LibrarySong {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub song: Song,
    pub plays: i64,
    pub requests: i64,
}

#[derive(Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

/// Filters for the library list. A fixed table keeps user input out of the SQL.
const LIBRARY_FILTERS: &[(&str, &str)] = &[
    ("favorite", "s.favorite = 1"),
    ("blocked", "s.blocked = 1"),
    ("missing", "s.missing = 1"),
];

#[utoipa::path(get, path = "/api/library", tag = "library", params(LibraryQuery), responses((status = 200, body = Page<LibrarySong>)))]
pub async fn list(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<LibraryQuery>,
) -> AppResult<Json<Page<LibrarySong>>> {
    let filter = q
        .filter
        .as_deref()
        .and_then(|f| LIBRARY_FILTERS.iter().find(|(k, _)| *k == f))
        .map(|(_, sql)| *sql)
        .unwrap_or("1 = 1");
    let pattern = format!("%{}%", q.q.unwrap_or_default().trim());
    let where_sql = format!("WHERE {filter} AND (s.title LIKE ?1 OR s.url LIKE ?1)");
    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM songs s {where_sql}"))
        .bind(&pattern)
        .fetch_one(&state.db)
        .await?;
    let sql = format!(
        "SELECT s.*, \
           (SELECT COUNT(*) FROM requests r WHERE r.song_id = s.id AND r.status = 'played') AS plays, \
           (SELECT COUNT(*) FROM requests r WHERE r.song_id = s.id AND r.autoplay = 0) AS requests \
         FROM songs s {where_sql} ORDER BY COALESCE(s.last_played_at, s.created_at) DESC, s.id DESC \
         LIMIT ?2 OFFSET ?3"
    );
    let items = sqlx::query_as(&sql)
        .bind(&pattern)
        .bind(q.limit.unwrap_or(50).clamp(1, 500))
        .bind(q.offset.unwrap_or(0).max(0))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(Page { items, total }))
}

#[derive(Deserialize, ToSchema)]
pub struct AddSong {
    pub input: String,
    #[serde(default)]
    pub favorite: bool,
}

#[utoipa::path(post, path = "/api/library", tag = "library", request_body = AddSong, responses((status = 200, body = Song)))]
pub async fn add(
    State(state): State<AppState>,
    _user: CurrentUser,
    Json(a): Json<AddSong>,
) -> AppResult<Json<Song>> {
    let song = resolve::song_for(&state, &a.input)
        .await?
        .map_err(|e| resolve_message(&e))?;
    if a.favorite {
        sqlx::query("UPDATE songs SET favorite = 1 WHERE id = ?")
            .bind(song.id)
            .execute(&state.db)
            .await?;
    }
    Ok(Json(load_song(&state, song.id).await?))
}

async fn load_song(state: &AppState, id: i64) -> AppResult<Song> {
    sqlx::query_as("SELECT * FROM songs WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這首歌"))
}

#[derive(Deserialize, ToSchema)]
pub struct SongPatch {
    pub favorite: Option<bool>,
    pub blocked: Option<bool>,
    /// false clears "missing" and the failure count, so the song can be requested again.
    pub missing: Option<bool>,
    pub title: Option<String>,
}

#[utoipa::path(patch, path = "/api/library/{id}", tag = "library", request_body = SongPatch, responses((status = 200, body = Song)))]
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(p): Json<SongPatch>,
) -> AppResult<Json<Song>> {
    let song = load_song(&state, id).await?;
    let flags = [
        ("favorite", p.favorite),
        ("blocked", p.blocked),
        ("missing", p.missing),
    ];
    for (column, value) in flags {
        if let Some(v) = value {
            // column names come from the fixed list above
            sqlx::query(&format!("UPDATE songs SET {column} = ? WHERE id = ?"))
                .bind(v)
                .bind(id)
                .execute(&state.db)
                .await?;
            events::record(
                &state.db,
                &user.username,
                column,
                &format!("『{}』{column} = {v}", song.title),
            )
            .await;
        }
    }
    if p.missing == Some(false) {
        sqlx::query("UPDATE songs SET fail_count = 0 WHERE id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(title) = p
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        sqlx::query("UPDATE songs SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    live::publish(&state).await;
    Ok(Json(load_song(&state, id).await?))
}

#[utoipa::path(delete, path = "/api/library/{id}", tag = "library", responses((status = 204), (status = 409)))]
pub async fn remove(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM requests WHERE song_id = ? AND status IN ('queued', 'playing')",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if active > 0 {
        return Err(AppError::Conflict("這首歌還在佇列裡，先從佇列刪掉".into()));
    }
    sqlx::query("DELETE FROM songs WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct EnqueueSong {
    #[serde(default)]
    pub placement: Placement,
}

#[utoipa::path(post, path = "/api/library/{id}/enqueue", tag = "library", request_body = EnqueueSong, responses((status = 200, body = Enqueued)))]
pub async fn enqueue(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(e): Json<EnqueueSong>,
) -> AppResult<Json<Enqueued>> {
    let song = load_song(&state, id).await?;
    let who = Requester::new(&user.username, None, Level::Broadcaster, Source::Web);
    match queue::enqueue(&state, &who, &song, e.placement, false).await? {
        Ok(done) => Ok(Json(done)),
        Err(r) => Err(AppError::Conflict(rejection_message(&r))),
    }
}

/// One library row in an export file. Import accepts the same shape.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SongRecord {
    pub url: String,
    pub title: String,
    pub duration_secs: Option<i64>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub blocked: bool,
}

#[derive(Deserialize, IntoParams)]
pub struct ExportQuery {
    /// `json` (default) or `csv`
    pub format: Option<String>,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// v1's admin could export the playlist; this downloads the whole library.
#[utoipa::path(get, path = "/api/library/export", tag = "library", params(ExportQuery), responses((status = 200, body = [SongRecord])))]
pub async fn export(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<ExportQuery>,
) -> AppResult<impl IntoResponse> {
    let songs: Vec<Song> = sqlx::query_as("SELECT * FROM songs ORDER BY id")
        .fetch_all(&state.db)
        .await?;
    let records: Vec<SongRecord> = songs
        .into_iter()
        .map(|s| SongRecord {
            url: s.url,
            title: s.title,
            duration_secs: s.duration_secs,
            favorite: s.favorite,
            blocked: s.blocked,
        })
        .collect();
    let (body, mime, ext) = if q.format.as_deref() == Some("csv") {
        let mut out = String::from("url,title,duration_secs,favorite,blocked\n");
        for r in &records {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                csv_field(&r.url),
                csv_field(&r.title),
                r.duration_secs.map(|d| d.to_string()).unwrap_or_default(),
                r.favorite,
                r.blocked
            ));
        }
        (out, "text/csv; charset=utf-8", "csv")
    } else {
        (
            serde_json::to_string_pretty(&records)?,
            "application/json",
            "json",
        )
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"streaming-player-library.{ext}\""),
            ),
        ],
        body,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
}

/// Import exported JSON without looking every song up again.
#[utoipa::path(post, path = "/api/library/import", tag = "library", request_body = [SongRecord], responses((status = 200, body = ImportResult)))]
pub async fn import(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(records): Json<Vec<SongRecord>>,
) -> AppResult<Json<ImportResult>> {
    let (mut imported, mut skipped) = (0, 0);
    for r in records {
        let Some(resolved) = resolve::offline(&r.url, &r.title, r.duration_secs) else {
            skipped += 1;
            continue;
        };
        let song = resolve::upsert(&state.db, &resolved).await?;
        sqlx::query("UPDATE songs SET favorite = ?, blocked = ? WHERE id = ?")
            .bind(r.favorite)
            .bind(r.blocked)
            .bind(song.id)
            .execute(&state.db)
            .await?;
        imported += 1;
    }
    events::record(
        &state.db,
        &user.username,
        "import",
        &format!("匯入 {imported} 首（略過 {skipped} 筆）"),
    )
    .await;
    Ok(Json(ImportResult { imported, skipped }))
}

// ───────────────────────────── history ─────────────────────────────

#[derive(Deserialize, IntoParams)]
pub struct HistoryQuery {
    /// Only this requester (name or identity).
    pub user: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(get, path = "/api/history", tag = "history", params(HistoryQuery), responses((status = 200, body = Page<Entry>)))]
pub async fn history(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<HistoryQuery>,
) -> AppResult<Json<Page<Entry>>> {
    Ok(Json(
        history_page(
            &state,
            q.user.as_deref(),
            q.limit.unwrap_or(50),
            q.offset.unwrap_or(0),
        )
        .await?,
    ))
}

pub async fn history_page(
    state: &AppState,
    user: Option<&str>,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Page<Entry>> {
    let user = user
        .map(|u| u.trim().to_lowercase())
        .filter(|u| !u.is_empty());
    let where_sql = "WHERE r.status NOT IN ('queued', 'playing') \
                     AND (?1 IS NULL OR r.requester_key = ?1 OR lower(r.requester) = ?1)";
    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM requests r {where_sql}"))
        .bind(&user)
        .fetch_one(&state.db)
        .await?;
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM requests r JOIN songs s ON s.id = r.song_id {where_sql} \
         ORDER BY COALESCE(r.ended_at, r.requested_at) DESC, r.id DESC LIMIT ?2 OFFSET ?3"
    );
    let items = sqlx::query_as(&sql)
        .bind(&user)
        .bind(limit.clamp(1, 500))
        .bind(offset.max(0))
        .fetch_all(&state.db)
        .await?;
    Ok(Page { items, total })
}

#[derive(Deserialize, IntoParams)]
pub struct StatsQuery {
    pub user: String,
}

#[utoipa::path(get, path = "/api/history/stats", tag = "history", params(StatsQuery), responses((status = 200, body = UserStats), (status = 404)))]
pub async fn stats(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<StatsQuery>,
) -> AppResult<Json<UserStats>> {
    queue::user_stats(&state.db, &q.user)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("這個人沒有點過歌"))
}

// ───────────────────────────── block list ─────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct BlockedUser {
    pub id: i64,
    pub name_key: String,
    pub note: String,
    pub created_at: i64,
}

#[utoipa::path(get, path = "/api/blocklist", tag = "blocklist", responses((status = 200, body = [BlockedUser])))]
pub async fn blocklist(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> AppResult<Json<Vec<BlockedUser>>> {
    Ok(Json(
        sqlx::query_as("SELECT * FROM blocked_users ORDER BY id DESC")
            .fetch_all(&state.db)
            .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct NewBlock {
    /// Chat name, Twitch login, or `yt:<channel id>`.
    pub name: String,
    #[serde(default)]
    pub note: String,
}

#[utoipa::path(post, path = "/api/blocklist", tag = "blocklist", request_body = NewBlock, responses((status = 204)))]
pub async fn block(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(b): Json<NewBlock>,
) -> AppResult<StatusCode> {
    let name = b.name.trim();
    if name.is_empty() {
        return Err(AppError::bad("要填名字"));
    }
    let note = if b.note.trim().is_empty() {
        format!("{} 加入", user.username)
    } else {
        b.note.trim().to_string()
    };
    block_user(&state, name, &note).await?;
    events::record(
        &state.db,
        &user.username,
        "block",
        &format!("封鎖點歌者 {name}"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/blocklist/{id}", tag = "blocklist", responses((status = 204)))]
pub async fn unblock(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let name: Option<String> =
        sqlx::query_scalar("DELETE FROM blocked_users WHERE id = ? RETURNING name_key")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let name = name.ok_or_else(|| AppError::not_found("找不到"))?;
    events::record(
        &state.db,
        &user.username,
        "unblock",
        &format!("解除封鎖 {name}"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams)]
pub struct EventsQuery {
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/events", tag = "history", params(EventsQuery), responses((status = 200, body = [Event])))]
pub async fn recent_events(
    State(state): State<AppState>,
    _user: CurrentUser,
    Query(q): Query<EventsQuery>,
) -> AppResult<Json<Vec<Event>>> {
    Ok(Json(
        events::recent(&state.db, q.limit.unwrap_or(50)).await?,
    ))
}
