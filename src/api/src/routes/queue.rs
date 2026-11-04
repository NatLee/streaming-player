//! Queue and player controls for signed-in users (admins and moderators).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::{
    auth::Can,
    control,
    error::{AppError, AppResult},
    events,
    live::{self, LiveState},
    media::resolve::{self, ResolveError},
    models::{Level, Requester, Source},
    perm::{need, Perm},
    queue::{self, Enqueued, Placement, Rejection},
    state::AppState,
    util::now,
};

#[utoipa::path(get, path = "/api/queue", tag = "queue", responses((status = 200, body = LiveState)))]
pub async fn get(
    State(state): State<AppState>,
    _c: Can<need::QueueView>,
) -> AppResult<Json<LiveState>> {
    Ok(Json(live::snapshot(&state).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct AddRequest {
    /// A link, a YouTube id, or search words.
    pub input: String,
    #[serde(default)]
    pub placement: Placement,
    /// Name to show as the requester; defaults to the signed-in user.
    pub requester: Option<String>,
}

pub fn rejection_message(r: &Rejection) -> String {
    match r {
        Rejection::Closed => "現在沒有開放點歌".into(),
        Rejection::UserBlocked => "這個人在黑名單裡".into(),
        Rejection::SongBlocked => "這首歌在黑名單裡，要先到歌曲庫解除封鎖".into(),
        Rejection::Missing => "這首歌標成「不見了」".into(),
        Rejection::TooLong { max_secs } => format!("影片超過 {} 分鐘", max_secs / 60),
        Rejection::Duplicate {
            position,
            requester,
            ..
        } => format!("{requester} 已經點過，在第 {position} 位"),
        Rejection::QueueFull { max } => format!("佇列已滿（{max} 首）"),
        Rejection::UserLimit { max, .. } => format!("每人最多 {max} 首"),
    }
}

pub fn resolve_message(e: &ResolveError) -> AppError {
    AppError::bad(e.reason())
}

/// Request a song from the web page. With `queue.manage` the queue limits don't apply,
/// `placement` may be `next` or `now` (interrupt), and `requester` may name someone else.
/// With only `queue.request` it is a normal request under the rules.
#[utoipa::path(post, path = "/api/queue", tag = "queue", request_body = AddRequest,
    responses((status = 200, body = Enqueued), (status = 400, description = "Not found or refused")))]
pub async fn add(
    State(state): State<AppState>,
    Can(user, _): Can<need::QueueRequest>,
    Json(req): Json<AddRequest>,
) -> AppResult<Json<Enqueued>> {
    let manager = user.can(Perm::QueueManage);
    if !manager && (req.placement != Placement::End || req.requester.is_some()) {
        user.require(Perm::QueueManage)?;
    }
    let song = resolve::song_for(&state, &req.input)
        .await?
        .map_err(|e| resolve_message(&e))?;
    let name = req
        .requester
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| user.username.clone());
    let level = if manager {
        Level::Broadcaster
    } else {
        Level::Everyone
    };
    let who = Requester::new(&name, None, level, Source::Web);
    match queue::enqueue(&state, &who, &song, req.placement, !manager).await? {
        Ok(done) => Ok(Json(done)),
        Err(r) => Err(AppError::Conflict(rejection_message(&r))),
    }
}

#[derive(Deserialize, IntoParams)]
pub struct RemoveQuery {
    /// `song` also blocks the song, `user` also blocks the requester.
    pub block: Option<String>,
}

#[utoipa::path(delete, path = "/api/queue/{id}", tag = "queue", params(RemoveQuery), responses((status = 204), (status = 404)))]
pub async fn remove(
    State(state): State<AppState>,
    Can(user, _): Can<need::QueueManage>,
    Path(id): Path<i64>,
    Query(q): Query<RemoveQuery>,
) -> AppResult<StatusCode> {
    if q.block.is_some() {
        user.require(Perm::BlocklistManage)?;
    }
    let e = queue::remove(&state, id)
        .await?
        .ok_or_else(|| AppError::not_found("這首歌已經不在佇列裡"))?;
    let mut what = "刪除";
    match q.block.as_deref() {
        Some("song") => {
            sqlx::query("UPDATE songs SET blocked = 1 WHERE id = ?")
                .bind(e.song.id)
                .execute(&state.db)
                .await?;
            what = "刪除並封鎖歌曲";
        }
        Some("user") => {
            block_user(
                &state,
                &e.requester_key,
                &format!("{} 從佇列封鎖", user.username),
            )
            .await?;
            what = "刪除並封鎖點歌者";
        }
        _ => {}
    }
    events::record(
        &state.db,
        &user.username,
        "remove",
        &format!("{what}：{} 點的『{}』", e.requester, e.song.title),
    )
    .await;
    live::publish(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn block_user(state: &AppState, key: &str, note: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO blocked_users (name_key, note, created_at) VALUES (?, ?, ?)",
    )
    .bind(key.trim().to_lowercase())
    .bind(note)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(())
}

#[derive(Deserialize, ToSchema)]
pub struct MoveTo {
    /// 0 = plays next
    pub index: usize,
}

#[utoipa::path(post, path = "/api/queue/{id}/move", tag = "queue", request_body = MoveTo, responses((status = 204), (status = 404)))]
pub async fn move_to(
    State(state): State<AppState>,
    _c: Can<need::QueueManage>,
    Path(id): Path<i64>,
    Json(m): Json<MoveTo>,
) -> AppResult<StatusCode> {
    if queue::move_to(&state, id, m.index).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("這首歌已經不在佇列裡"))
    }
}

#[derive(Deserialize, ToSchema)]
pub struct Order {
    pub ids: Vec<i64>,
}

#[utoipa::path(put, path = "/api/queue/order", tag = "queue", request_body = Order, responses((status = 204)))]
pub async fn reorder(
    State(state): State<AppState>,
    _c: Can<need::QueueManage>,
    Json(o): Json<Order>,
) -> AppResult<StatusCode> {
    queue::reorder(&state, &o.ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Interrupt: play this request now; the current song returns to the front.
#[utoipa::path(post, path = "/api/queue/{id}/play-now", tag = "queue", responses((status = 204), (status = 404)))]
pub async fn play_now(
    State(state): State<AppState>,
    Can(user, _): Can<need::QueueManage>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if queue::play_now(&state, id).await? {
        events::record(
            &state.db,
            &user.username,
            "play_now",
            &format!("插播佇列 #{id}"),
        )
        .await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("這首歌已經不在佇列裡"))
    }
}

#[utoipa::path(post, path = "/api/queue/clear", tag = "queue", responses((status = 204)))]
pub async fn clear(
    State(state): State<AppState>,
    Can(user, _): Can<need::QueueManage>,
) -> AppResult<StatusCode> {
    let n = queue::clear(&state).await?;
    events::record(
        &state.db,
        &user.username,
        "clear",
        &format!("清空佇列（{n} 首）"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/player/pause", tag = "player", responses((status = 204)))]
pub async fn pause(
    State(state): State<AppState>,
    Can(user, _): Can<need::PlayerControl>,
) -> AppResult<StatusCode> {
    control::set_paused(&state, true, &user.username).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/player/resume", tag = "player", responses((status = 204)))]
pub async fn resume(
    State(state): State<AppState>,
    Can(user, _): Can<need::PlayerControl>,
) -> AppResult<StatusCode> {
    control::set_paused(&state, false, &user.username).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema, Default)]
pub struct SkipBody {
    /// Also put the song on the block list.
    #[serde(default)]
    pub block: bool,
}

#[utoipa::path(post, path = "/api/player/skip", tag = "player", request_body = SkipBody, responses((status = 204), (status = 404)))]
pub async fn skip(
    State(state): State<AppState>,
    Can(user, _): Can<need::PlayerControl>,
    body: Option<Json<SkipBody>>,
) -> AppResult<StatusCode> {
    let block = body.map(|b| b.block).unwrap_or(false);
    if block {
        user.require(Perm::BlocklistManage)?;
    }
    match control::skip(&state, block, &user.username).await? {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(AppError::not_found("現在沒有在播的歌")),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct VolumeBody {
    /// 0–100
    pub volume: Option<u8>,
    pub muted: Option<bool>,
}

#[utoipa::path(put, path = "/api/player/volume", tag = "player", request_body = VolumeBody, responses((status = 204)))]
pub async fn volume(
    State(state): State<AppState>,
    _c: Can<need::PlayerControl>,
    Json(v): Json<VolumeBody>,
) -> AppResult<StatusCode> {
    control::set_volume(&state, v.volume, v.muted).await?;
    Ok(StatusCode::NO_CONTENT)
}
