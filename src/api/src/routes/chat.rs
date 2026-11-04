use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    auth::Can,
    chat::{self, twitch, youtube, Status},
    error::{AppError, AppResult},
    events,
    perm::need,
    state::AppState,
};

#[derive(Serialize, ToSchema)]
pub struct TwitchInfo {
    pub status: Status,
    pub client_id_set: bool,
    /// The account signed in for chat, if any.
    pub login: Option<String>,
    /// A device-code sign-in waiting for the streamer to approve.
    pub device: Option<twitch::DeviceLogin>,
    pub scopes: String,
}

#[derive(Serialize, ToSchema)]
pub struct YoutubeInfo {
    pub status: Status,
    pub api_key_set: bool,
    /// Quota one hour of chat polling uses at the current interval.
    pub units_per_hour: u32,
}

#[derive(Serialize, ToSchema)]
pub struct ChatInfo {
    pub twitch: TwitchInfo,
    pub youtube: YoutubeInfo,
}

#[utoipa::path(get, path = "/api/chat", tag = "chat", responses((status = 200, body = ChatInfo)))]
pub async fn info(
    State(state): State<AppState>,
    _c: Can<need::SettingsView>,
) -> AppResult<Json<ChatInfo>> {
    let settings = state.settings();
    let device = state.chat.device.lock().unwrap().clone();
    let login = twitch::load_tokens(&state).await?.map(|t| t.login);
    let api_key_set = crate::settings::youtube_api_key(&state.db).await?.is_some();
    Ok(Json(ChatInfo {
        twitch: TwitchInfo {
            status: state.chat.get(chat::TWITCH),
            client_id_set: twitch::client_id(&state).is_some(),
            login,
            device,
            scopes: twitch::SCOPES.into(),
        },
        youtube: YoutubeInfo {
            status: state.chat.get(chat::YOUTUBE),
            api_key_set,
            units_per_hour: youtube::units_per_hour(settings.chat.youtube.poll_secs),
        },
    }))
}

/// Start the Twitch device-code sign-in. Open `verification_uri` and enter `user_code`.
#[utoipa::path(post, path = "/api/chat/twitch/device", tag = "chat", responses((status = 200, body = twitch::DeviceLogin)))]
pub async fn twitch_device(
    State(state): State<AppState>,
    _c: Can<need::CommandsManage>,
) -> AppResult<Json<twitch::DeviceLogin>> {
    twitch::start_device(&state)
        .await
        .map(Json)
        .map_err(|e| AppError::BadGateway(e.to_string()))
}

#[utoipa::path(delete, path = "/api/chat/twitch", tag = "chat", responses((status = 204)))]
pub async fn twitch_forget(
    State(state): State<AppState>,
    Can(admin, _): Can<need::CommandsManage>,
) -> AppResult<StatusCode> {
    twitch::forget_tokens(&state).await?;
    events::record(&state.db, &admin.username, "chat", "登出 Twitch").await;
    chat::restart(&state);
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/chat/restart", tag = "chat", responses((status = 204)))]
pub async fn restart(
    State(state): State<AppState>,
    _c: Can<need::CommandsManage>,
) -> AppResult<StatusCode> {
    chat::restart(&state);
    Ok(StatusCode::NO_CONTENT)
}
