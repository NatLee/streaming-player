use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    auth::AdminUser,
    chat, db,
    error::{AppError, AppResult},
    events, legacy, live,
    media::ytdlp,
    queue,
    settings::{self, Settings, BOT_KEY, OBS_KEY, YOUTUBE_API_KEY},
    state::AppState,
};

#[derive(Serialize, ToSchema)]
pub struct Links {
    pub player: String,
    pub overlay: String,
    pub viewer: String,
    pub bot_base: String,
}

#[derive(Serialize, ToSchema)]
pub struct SettingsView {
    pub settings: Settings,
    pub obs_key: String,
    pub bot_key: String,
    pub youtube_api_key_set: bool,
    pub public_url: String,
    pub links: Links,
}

#[utoipa::path(get, path = "/api/settings", tag = "settings", responses((status = 200, body = SettingsView)))]
pub async fn get(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<SettingsView>> {
    let obs_key = settings::access_key(&state.db, OBS_KEY).await?;
    let bot_key = settings::access_key(&state.db, BOT_KEY).await?;
    let base = state.config.public_url.clone();
    Ok(Json(SettingsView {
        settings: state.settings(),
        youtube_api_key_set: settings::youtube_api_key(&state.db).await?.is_some(),
        links: Links {
            player: format!("{base}/obs/player?key={obs_key}"),
            overlay: format!("{base}/obs/overlay?key={obs_key}"),
            viewer: format!("{base}/queue"),
            bot_base: format!("{base}/api/bot"),
        },
        public_url: base,
        obs_key,
        bot_key,
    }))
}

#[utoipa::path(put, path = "/api/settings", tag = "settings", request_body = Settings, responses((status = 204)))]
pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(s): Json<Settings>,
) -> AppResult<StatusCode> {
    s.validate().map_err(AppError::BadRequest)?;
    let before = state.settings();
    state.save_settings(s.clone()).await?;
    events::record(&state.db, &admin.username, "settings", "更新設定").await;
    if before.chat != s.chat {
        chat::restart(&state);
    }
    if !before.playback.autoplay_favorites && s.playback.autoplay_favorites {
        queue::ensure_playing(&state).await?;
    }
    live::publish(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct NewKey {
    pub key: String,
}

/// Make a new OBS key (`obs`) or bot key (`bot`). Old URLs stop working.
#[utoipa::path(post, path = "/api/settings/keys/{name}/rotate", tag = "settings", responses((status = 200, body = NewKey)))]
pub async fn rotate_key(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(name): Path<String>,
) -> AppResult<Json<NewKey>> {
    let kv = match name.as_str() {
        "obs" => OBS_KEY,
        "bot" => BOT_KEY,
        _ => return Err(AppError::bad("只能換 obs 或 bot 金鑰")),
    };
    let key = settings::rotate_key(&state.db, kv).await?;
    events::record(
        &state.db,
        &admin.username,
        "settings",
        &format!("重新產生 {name} 金鑰"),
    )
    .await;
    Ok(Json(NewKey { key }))
}

#[derive(Deserialize, ToSchema)]
pub struct ApiKeyBody {
    /// Empty or null removes the key.
    pub api_key: Option<String>,
}

#[utoipa::path(put, path = "/api/settings/youtube-key", tag = "settings", request_body = ApiKeyBody, responses((status = 204)))]
pub async fn youtube_key(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(b): Json<ApiKeyBody>,
) -> AppResult<StatusCode> {
    match b
        .api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        Some(k) => db::kv_set(&state.db, YOUTUBE_API_KEY, &k).await?,
        None => db::kv_delete(&state.db, YOUTUBE_API_KEY).await?,
    }
    events::record(
        &state.db,
        &admin.username,
        "settings",
        "更新 YouTube API key",
    )
    .await;
    chat::restart(&state);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct SystemInfo {
    pub version: String,
    pub ytdlp_path: String,
    /// None when yt-dlp can't be run.
    pub ytdlp_version: Option<String>,
    pub ytdlp_error: Option<String>,
    /// Last time someone used a v1 Nightbot URL (Unix seconds).
    pub legacy_last_hit: Option<i64>,
    pub players_connected: usize,
}

#[utoipa::path(get, path = "/api/system", tag = "settings", responses((status = 200, body = SystemInfo)))]
pub async fn system(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<SystemInfo>> {
    let (ytdlp_version, ytdlp_error) = match ytdlp::version(&state.config.ytdlp).await {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        ytdlp_path: state.config.ytdlp.path.clone(),
        ytdlp_version,
        ytdlp_error,
        legacy_last_hit: legacy::last_hit(&state).await?,
        players_connected: state.hub.player_count(),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct UpdateOutput {
    pub output: String,
    pub version: Option<String>,
}

/// Update yt-dlp to the latest stable release (sites change often; old versions break).
#[utoipa::path(post, path = "/api/system/ytdlp/update", tag = "settings", responses((status = 200, body = UpdateOutput), (status = 502)))]
pub async fn update_ytdlp(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
) -> AppResult<Json<UpdateOutput>> {
    let output = ytdlp::update(&state.config.ytdlp)
        .await
        .map_err(|e| AppError::BadGateway(format!("yt-dlp 更新失敗：{e}")))?;
    let version = ytdlp::version(&state.config.ytdlp).await.ok();
    events::record(
        &state.db,
        &admin.username,
        "settings",
        &format!("更新 yt-dlp：{}", version.clone().unwrap_or_default()),
    )
    .await;
    Ok(Json(UpdateOutput { output, version }))
}
