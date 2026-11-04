//! WebSocket for live pages, media for the player, and the public viewer API.

use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::HeaderMap,
    response::Response,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use utoipa::{IntoParams, ToSchema};

use crate::{
    auth::{self, PlayerAccess},
    error::{AppError, AppResult},
    live::{self, HubMsg, LiveState},
    media::stream::{self, PlaySource},
    models::{EndReason, Entry, Song},
    perm::Perm,
    queue,
    settings::OBS_KEY,
    state::AppState,
    util::now_ms,
};

use super::library::{history_page, Page};

const PING_EVERY: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Plays the video; the oldest one leads and reports progress.
    Player,
    Overlay,
    Admin,
    Viewer,
}

#[derive(Deserialize, IntoParams)]
pub struct WsQuery {
    /// `player`, `overlay`, `admin` or `viewer`
    pub role: Option<String>,
    /// OBS key, for player and overlay pages in OBS.
    pub key: Option<String>,
}

async fn can_view_public(state: &AppState, headers: &HeaderMap) -> anyhow::Result<bool> {
    if state.settings().viewer.public_queue {
        return Ok(true);
    }
    Ok(auth::authenticate(state, headers)
        .await?
        .is_some_and(|u| u.can(Perm::QueueView)))
}

/// What each page type needs to connect. Viewers need nothing while the public page is on.
fn role_needs(role: Role, public_queue: bool) -> Option<Perm> {
    match role {
        Role::Player => Some(Perm::PlayerControl),
        Role::Overlay | Role::Admin => Some(Perm::QueueView),
        Role::Viewer if public_queue => None,
        Role::Viewer => Some(Perm::QueueView),
    }
}

/// Messages a page may send, and the permission each one needs.
const MESSAGE_PERMS: &[(&str, Perm)] = &[
    ("started", Perm::PlayerControl),
    ("progress", Perm::PlayerControl),
    ("ended", Perm::PlayerControl),
    ("error", Perm::PlayerControl),
    ("duration", Perm::PlayerControl),
];

fn message_allowed(kind: &str, grants: &[Perm]) -> bool {
    MESSAGE_PERMS
        .iter()
        .find(|(k, _)| *k == kind)
        .is_some_and(|(_, p)| grants.contains(p))
}

/// Live state stream. Messages: `hello`, `state`, `notice`, `leader`; players send
/// `started`, `progress`, `ended`, `error` and `duration` reports.
#[utoipa::path(get, path = "/api/ws", tag = "live", params(WsQuery), responses((status = 101, description = "WebSocket")))]
pub async fn ws(
    State(state): State<AppState>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> AppResult<Response> {
    let role = match q.role.as_deref().unwrap_or("viewer") {
        "player" => Role::Player,
        "overlay" => Role::Overlay,
        "admin" => Role::Admin,
        _ => Role::Viewer,
    };
    // The OBS key stands for an OBS page: it may play and report. Otherwise the
    // signed-in caller's permissions decide.
    let grants: Vec<Perm> = if auth::key_matches(&state, OBS_KEY, q.key.as_deref()).await? {
        vec![Perm::PlayerControl, Perm::QueueView]
    } else {
        auth::authenticate(&state, &headers)
            .await?
            .map(|u| u.permissions)
            .unwrap_or_default()
    };
    if let Some(need) = role_needs(role, state.settings().viewer.public_queue) {
        if !grants.contains(&need) {
            return Err(AppError::Forbidden(format!(
                "需要權限：{}（或 OBS 金鑰）",
                need.info().label
            )));
        }
    }
    Ok(upgrade.on_upgrade(move |socket| client(state, socket, role, grants)))
}

async fn send_state(state: &AppState) -> Option<String> {
    let snap = live::snapshot(state).await.ok()?;
    let mut v = serde_json::to_value(snap).ok()?;
    v["type"] = json!("state");
    Some(v.to_string())
}

async fn client(state: AppState, socket: WebSocket, role: Role, grants: Vec<Perm>) {
    let id = state.hub.connection_id();
    let mut events = state.hub.subscribe();
    let (mut tx, mut rx) = socket.split();
    let is_player = role == Role::Player;

    let leader = if is_player {
        let leader = state.hub.join_player(id);
        if let Err(e) = queue::ensure_playing(&state).await {
            tracing::warn!("could not start autoplay: {e:#}");
        }
        leader
    } else {
        None
    };
    let hello = json!({ "type": "hello", "conn": id, "leader": is_player && leader == Some(id) });
    if tx
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        if is_player {
            state.hub.leave_player(id);
        }
        return;
    }
    if is_player {
        // Everyone's "players connected" count changes.
        live::publish(&state).await;
    } else if let Some(s) = send_state(&state).await {
        let _ = tx.send(Message::Text(s.into())).await;
    }

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            msg = events.recv() => {
                let out = match msg {
                    Ok(HubMsg::State(s)) => Some(s.to_string()),
                    Ok(HubMsg::Notice(n)) => Some(n.to_string()),
                    Ok(HubMsg::Leader(l)) if is_player => {
                        Some(json!({ "type": "leader", "leader": l == Some(id) }).to_string())
                    }
                    Ok(HubMsg::Leader(_)) => None,
                    // Missed some updates: a fresh snapshot replaces all of them.
                    Err(RecvError::Lagged(_)) => send_state(&state).await,
                    Err(RecvError::Closed) => break,
                };
                if let Some(text) = out {
                    if tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(text))) if is_player => {
                    if state.hub.leader() == Some(id) {
                        if let Err(e) = handle_report(&state, text.as_str(), &grants).await {
                            tracing::warn!("player report failed: {e:#}");
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = ping.tick() => {
                if tx.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }
    if is_player {
        state.hub.leave_player(id);
        live::publish(&state).await;
    }
}

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(rename = "type")]
    kind: String,
    request_id: i64,
    #[serde(default)]
    position_ms: i64,
    #[serde(default)]
    secs: i64,
    #[serde(default)]
    reason: String,
}

/// Reports from the leading player.
async fn handle_report(state: &AppState, text: &str, grants: &[Perm]) -> anyhow::Result<()> {
    let r: Report = serde_json::from_str(text)?;
    if !message_allowed(&r.kind, grants) {
        anyhow::bail!("not allowed to send {}", r.kind);
    }
    match r.kind.as_str() {
        "started" | "progress" => {
            let synced = state
                .player
                .lock()
                .unwrap()
                .sync(r.request_id, r.position_ms, now_ms());
            if synced {
                if r.kind == "started" {
                    queue::mark_started(state, r.request_id).await?;
                }
                live::publish(state).await;
            }
        }
        "ended" => {
            queue::finish_current(state, Some(r.request_id), EndReason::Played).await?;
        }
        "error" => {
            let reason = if r.reason.is_empty() {
                "unknown".to_string()
            } else {
                r.reason
            };
            queue::report_failure(state, r.request_id, &reason).await?;
        }
        "duration" => queue::report_duration(state, r.request_id, r.secs).await?,
        _ => {}
    }
    Ok(())
}

async fn load_song(state: &AppState, id: i64) -> AppResult<Song> {
    sqlx::query_as("SELECT * FROM songs WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這首歌"))
}

/// How the player should play a non-embedded song (asks yt-dlp; may take a few seconds).
#[utoipa::path(get, path = "/api/media/{id}/source", tag = "live", params(("key" = Option<String>, Query, description = "OBS key")),
    responses((status = 200, body = PlaySource), (status = 502, description = "yt-dlp failed")))]
pub async fn media_source(
    State(state): State<AppState>,
    _a: PlayerAccess,
    Path(id): Path<i64>,
) -> AppResult<Json<PlaySource>> {
    let song = load_song(&state, id).await?;
    stream::play_source(&state, &song)
        .await
        .map(Json)
        .map_err(|e| AppError::BadGateway(e.to_string()))
}

/// The media file, streamed through this server (Range requests supported).
#[utoipa::path(get, path = "/api/media/{id}/stream", tag = "live", params(("key" = Option<String>, Query, description = "OBS key")),
    responses((status = 200, description = "Media bytes"), (status = 206, description = "Partial content")))]
pub async fn media_stream(
    State(state): State<AppState>,
    _a: PlayerAccess,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let song = load_song(&state, id).await?;
    stream::proxy(&state, &song, &headers)
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))
}

/// Queue and now playing for the public viewer page (`/queue`).
#[utoipa::path(get, path = "/api/public/state", tag = "public", responses((status = 200, body = LiveState), (status = 404, description = "Viewer page is off")))]
pub async fn public_state(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<LiveState>> {
    if !can_view_public(&state, &headers).await? {
        return Err(AppError::not_found("觀眾頁沒有開放"));
    }
    Ok(Json(live::snapshot(&state).await?))
}

#[derive(Deserialize, IntoParams)]
pub struct MineQuery {
    /// Chat name to look up.
    pub user: String,
}

#[derive(Serialize, ToSchema)]
pub struct MyRequests {
    pub pending: Vec<Entry>,
    pub history: Page<Entry>,
}

/// A viewer's waiting songs and recent requests ("我點的歌").
#[utoipa::path(get, path = "/api/public/requests", tag = "public", params(MineQuery), responses((status = 200, body = MyRequests)))]
pub async fn public_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MineQuery>,
) -> AppResult<Json<MyRequests>> {
    if !can_view_public(&state, &headers).await? {
        return Err(AppError::not_found("觀眾頁沒有開放"));
    }
    let who = q.user.trim().to_lowercase();
    if who.is_empty() {
        return Err(AppError::bad("要填名字"));
    }
    let snap = live::snapshot(&state).await?;
    let mine = |e: &Entry| e.requester_key == who || e.requester.to_lowercase() == who;
    let mut pending: Vec<Entry> = snap
        .now
        .iter()
        .map(|n| n.entry.clone())
        .filter(mine)
        .collect();
    pending.extend(snap.queue.into_iter().map(|i| i.entry).filter(mine));
    let history = history_page(&state, Some(&who), 20, 0).await?;
    Ok(Json(MyRequests { pending, history }))
}

#[derive(Serialize, ToSchema)]
pub struct Meta {
    pub version: String,
    pub public_queue: bool,
    pub google_enabled: bool,
}

#[utoipa::path(get, path = "/api/meta", tag = "public", responses((status = 200, body = Meta)))]
pub async fn meta(State(state): State<AppState>) -> Json<Meta> {
    Json(Meta {
        version: env!("CARGO_PKG_VERSION").into(),
        public_queue: state.settings().viewer.public_queue,
        google_enabled: state.config.google.is_some(),
    })
}

#[utoipa::path(get, path = "/api/health", tag = "public", responses((status = 200, body = String)))]
pub async fn health() -> &'static str {
    "ok"
}
