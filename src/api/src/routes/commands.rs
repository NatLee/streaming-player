use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    auth::Can,
    commands::{self, Action, Command, ALL_ACTIONS},
    error::{AppError, AppResult},
    events,
    perm::need,
    state::AppState,
};

#[derive(Serialize, ToSchema)]
pub struct OutcomeInfo {
    pub key: String,
    pub default_text: String,
}

#[derive(Serialize, ToSchema)]
pub struct ActionInfo {
    pub action: Action,
    pub outcomes: Vec<OutcomeInfo>,
    pub variables: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CommandList {
    pub prefix: String,
    pub commands: Vec<Command>,
    /// What each action can answer and which variables its templates can use.
    pub actions: Vec<ActionInfo>,
}

#[utoipa::path(get, path = "/api/commands", tag = "commands", responses((status = 200, body = CommandList)))]
pub async fn list(
    State(state): State<AppState>,
    _c: Can<need::SettingsView>,
) -> AppResult<Json<CommandList>> {
    let actions = ALL_ACTIONS
        .iter()
        .map(|a| ActionInfo {
            action: *a,
            outcomes: a
                .outcomes()
                .into_iter()
                .map(|(k, t)| OutcomeInfo {
                    key: k.into(),
                    default_text: t.into(),
                })
                .collect(),
            variables: a.variables().iter().map(|v| v.to_string()).collect(),
        })
        .collect();
    Ok(Json(CommandList {
        prefix: state.settings().chat.prefix,
        commands: commands::list(&state.db).await?,
        actions,
    }))
}

async fn checked(state: &AppState, mut cmd: Command) -> AppResult<Command> {
    let all = commands::list(&state.db).await?;
    let prefix = state.settings().chat.prefix;
    commands::normalize(&mut cmd, &prefix, &all).map_err(AppError::BadRequest)?;
    Ok(cmd)
}

#[utoipa::path(post, path = "/api/commands", tag = "commands", request_body = Command, responses((status = 200, body = Command)))]
pub async fn create(
    State(state): State<AppState>,
    Can(admin, _): Can<need::CommandsManage>,
    Json(cmd): Json<Command>,
) -> AppResult<Json<Command>> {
    let mut cmd = checked(&state, Command { id: 0, ..cmd }).await?;
    cmd.id = commands::insert(&state.db, &cmd).await?;
    events::record(
        &state.db,
        &admin.username,
        "command",
        &format!("新增指令 {}", cmd.triggers.join("/")),
    )
    .await;
    Ok(Json(cmd))
}

#[utoipa::path(put, path = "/api/commands/{id}", tag = "commands", request_body = Command, responses((status = 200, body = Command)))]
pub async fn update(
    State(state): State<AppState>,
    Can(admin, _): Can<need::CommandsManage>,
    Path(id): Path<i64>,
    Json(cmd): Json<Command>,
) -> AppResult<Json<Command>> {
    let cmd = checked(&state, Command { id, ..cmd }).await?;
    if !commands::update(&state.db, &cmd).await? {
        return Err(AppError::not_found("找不到這個指令"));
    }
    events::record(
        &state.db,
        &admin.username,
        "command",
        &format!("修改指令 {}", cmd.triggers.join("/")),
    )
    .await;
    Ok(Json(cmd))
}

#[utoipa::path(delete, path = "/api/commands/{id}", tag = "commands", responses((status = 204)))]
pub async fn remove(
    State(state): State<AppState>,
    _c: Can<need::CommandsManage>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if commands::get(&state.db, id).await?.is_none() || !commands::delete(&state.db, id).await? {
        return Err(AppError::not_found("找不到這個指令"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Throw away every change and start from the built-in commands.
#[utoipa::path(post, path = "/api/commands/reset", tag = "commands", responses((status = 204)))]
pub async fn reset(
    State(state): State<AppState>,
    Can(admin, _): Can<need::CommandsManage>,
) -> AppResult<StatusCode> {
    sqlx::query("DELETE FROM commands")
        .execute(&state.db)
        .await?;
    commands::seed_defaults(&state.db).await?;
    events::record(&state.db, &admin.username, "command", "指令恢復成預設值").await;
    Ok(StatusCode::NO_CONTENT)
}
