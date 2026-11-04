use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    auth::{Can, CurrentUser},
    chat,
    commands::{self, Command},
    db,
    error::{AppError, AppResult},
    events, live,
    media::ytdlp,
    perm::{need, Perm},
    queue, roles,
    settings::{self, Settings, BOT_KEY, OBS_KEY, YOUTUBE_API_KEY},
    state::AppState,
    util::now,
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
    _c: Can<need::SettingsView>,
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

/// Save new settings. Each changed section needs its permission (see
/// `settings::required_for_change`): rules and playback `settings.manage`, chat
/// `commands.manage`, accounts and registration `users.manage`.
#[utoipa::path(put, path = "/api/settings", tag = "settings", request_body = Settings, responses((status = 204), (status = 403)))]
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(s): Json<Settings>,
) -> AppResult<StatusCode> {
    apply_settings(&state, &user, s).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn apply_settings(state: &AppState, user: &CurrentUser, s: Settings) -> AppResult<()> {
    s.validate().map_err(AppError::BadRequest)?;
    let before = state.settings();
    let needed = settings::required_for_change(&before, &s);
    if needed.is_empty() {
        user.require(Perm::SettingsView)?;
        return Ok(());
    }
    for p in &needed {
        user.require(*p)?;
    }
    if before.auth.default_role != s.auth.default_role {
        roles::check_default_role(&state.db, &s.auth.default_role).await?;
    }
    state.save_settings(s.clone()).await?;
    events::record(&state.db, &user.username, "settings", "更新設定").await;
    if before.chat != s.chat {
        chat::restart(state);
    }
    if !before.playback.autoplay_favorites && s.playback.autoplay_favorites {
        queue::ensure_playing(state).await?;
    }
    live::publish(state).await;
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct NewKey {
    pub key: String,
}

/// Make a new OBS key (`obs`) or bot key (`bot`). Old URLs stop working.
#[utoipa::path(post, path = "/api/settings/keys/{name}/rotate", tag = "settings", responses((status = 200, body = NewKey)))]
pub async fn rotate_key(
    State(state): State<AppState>,
    admin: CurrentUser,
    Path(name): Path<String>,
) -> AppResult<Json<NewKey>> {
    // which key, and who may change it
    let (kv, need) = match name.as_str() {
        "obs" => (OBS_KEY, Perm::SettingsManage),
        "bot" => (BOT_KEY, Perm::CommandsManage),
        _ => return Err(AppError::bad("只能換 obs 或 bot 金鑰")),
    };
    admin.require(need)?;
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
    Can(admin, _): Can<need::CommandsManage>,
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
    pub players_connected: usize,
}

#[utoipa::path(get, path = "/api/system", tag = "settings", responses((status = 200, body = SystemInfo)))]
pub async fn system(
    State(state): State<AppState>,
    _c: Can<need::SettingsView>,
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
    Can(admin, _): Can<need::SettingsManage>,
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

// ───────────────────────────── backup ─────────────────────────────

pub const BACKUP_FORMAT: &str = "streaming-player-backup";
pub const BACKUP_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct BlockEntry {
    pub name_key: String,
    #[serde(default)]
    pub note: String,
}

/// Settings, chat commands and blocked chatters in one file, to back up or move to
/// another server. Keys and tokens are not included. The song library has its own
/// export (`/api/library/export`).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub exported_at: i64,
    pub settings: Settings,
    pub commands: Vec<Command>,
    #[serde(default)]
    pub blocklist: Vec<BlockEntry>,
}

#[utoipa::path(get, path = "/api/backup", tag = "settings", responses((status = 200, body = Backup)))]
pub async fn export_backup(
    State(state): State<AppState>,
    Can(user, _): Can<need::SettingsView>,
) -> AppResult<Json<Backup>> {
    let blocklist = if user.can(Perm::BlocklistManage) {
        sqlx::query_as::<_, (String, String)>(
            "SELECT name_key, note FROM blocked_users ORDER BY id",
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|(name_key, note)| BlockEntry { name_key, note })
        .collect()
    } else {
        Vec::new()
    };
    Ok(Json(Backup {
        format: BACKUP_FORMAT.into(),
        version: BACKUP_VERSION,
        exported_at: now(),
        settings: state.settings(),
        commands: commands::list(&state.db).await?,
        blocklist,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct RestoreResult {
    pub commands: usize,
    pub blocked_added: usize,
}

/// Restore a backup: settings and commands are replaced, blocked chatters are added.
/// Each part needs the permission that editing it by hand would need.
#[utoipa::path(post, path = "/api/backup", tag = "settings", request_body = Backup, responses((status = 200, body = RestoreResult), (status = 403)))]
pub async fn import_backup(
    State(state): State<AppState>,
    Can(user, _): Can<need::SettingsManage>,
    Json(b): Json<Backup>,
) -> AppResult<Json<RestoreResult>> {
    if b.format != BACKUP_FORMAT || b.version > BACKUP_VERSION {
        return Err(AppError::bad("這不是這個版本能讀的備份檔"));
    }
    // Check every part before changing anything.
    let current = commands::list(&state.db).await?;
    let commands_changed = strip_ids(&current) != strip_ids(&b.commands);
    if commands_changed {
        user.require(Perm::CommandsManage)?;
    }
    if !b.blocklist.is_empty() {
        user.require(Perm::BlocklistManage)?;
    }
    let mut cleaned: Vec<Command> = Vec::new();
    for mut c in b.commands {
        // distinct temporary ids, so each command is checked against the ones before it
        c.id = -(cleaned.len() as i64) - 1;
        commands::normalize(&mut c, &b.settings.chat.prefix, &cleaned)
            .map_err(AppError::BadRequest)?;
        cleaned.push(c);
    }
    apply_settings(&state, &user, b.settings).await?;
    if commands_changed {
        commands::replace_all(&state.db, &cleaned).await?;
    }
    let mut blocked_added = 0;
    for e in &b.blocklist {
        let key = e.name_key.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        blocked_added += sqlx::query(
            "INSERT OR IGNORE INTO blocked_users (name_key, note, created_at) VALUES (?, ?, ?)",
        )
        .bind(&key)
        .bind(&e.note)
        .bind(now())
        .execute(&state.db)
        .await?
        .rows_affected() as usize;
    }
    events::record(&state.db, &user.username, "settings", "從備份檔還原設定").await;
    Ok(Json(RestoreResult {
        commands: cleaned.len(),
        blocked_added,
    }))
}

/// Commands compared without their database ids.
fn strip_ids(list: &[Command]) -> Vec<Command> {
    list.iter()
        .cloned()
        .map(|c| Command { id: 0, ..c })
        .collect()
}
