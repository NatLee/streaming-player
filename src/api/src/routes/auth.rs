use axum::{
    extract::State,
    http::{header, HeaderMap},
    response::{AppendHeaders, IntoResponse},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    auth::{self, CurrentUser},
    error::{AppError, AppResult},
    events,
    state::AppState,
    util::now,
};

#[derive(Serialize, ToSchema)]
pub struct AuthState {
    /// No user exists yet: the first visitor creates the admin account.
    pub needs_setup: bool,
    pub user: Option<CurrentUser>,
    pub google_enabled: bool,
}

#[utoipa::path(get, path = "/api/auth/state", tag = "auth", responses((status = 200, body = AuthState)))]
pub async fn state(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<AuthState>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(AuthState {
        needs_setup: users == 0,
        user: auth::session_user(&state, &headers).await?,
        google_enabled: state.config.google.is_some(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// First run only: create the admin account and sign in (v1's first-login page).
#[utoipa::path(post, path = "/api/auth/setup", tag = "auth", request_body = Credentials,
    responses((status = 200, body = CurrentUser), (status = 409, description = "Already set up")))]
pub async fn setup(
    State(state): State<AppState>,
    Json(c): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let username = c.username.trim().to_string();
    auth::validate_username(&username)?;
    auth::validate_new_password(&c.password)?;
    let hash = auth::hash_password(c.password).await?;
    // The INSERT only happens while the table is empty, so two racing setups can't both win.
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, created_at) \
         SELECT ?, ?, 'admin', ? WHERE NOT EXISTS (SELECT 1 FROM users) RETURNING id",
    )
    .bind(&username)
    .bind(hash)
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let id = id.ok_or_else(|| AppError::Conflict("已經設定過管理員了，請直接登入".into()))?;
    events::record(&state.db, &username, "user", "建立了第一個管理員帳號").await;
    sign_in(&state, id).await
}

async fn sign_in(state: &AppState, id: i64) -> AppResult<impl IntoResponse> {
    auth::touch_login(&state.db, id).await?;
    let token = auth::create_session(&state.db, id).await?;
    let user = auth::list_users(&state.db)
        .await?
        .into_iter()
        .find(|u| u.id == id)
        .ok_or_else(|| AppError::not_found("找不到使用者"))?;
    Ok((
        AppendHeaders([(
            header::SET_COOKIE,
            auth::session_cookie(&token, state.config.cookie_secure),
        )]),
        Json(user),
    ))
}

#[utoipa::path(post, path = "/api/auth/login", tag = "auth", request_body = Credentials,
    responses((status = 200, body = CurrentUser), (status = 401, description = "Wrong username or password")))]
pub async fn login(
    State(state): State<AppState>,
    Json(c): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    auth::check_rate_limit(&state, &c.username)?;
    let row: Option<(i64, String, bool)> =
        sqlx::query_as("SELECT id, password_hash, disabled FROM users WHERE username = ?")
            .bind(c.username.trim())
            .fetch_optional(&state.db)
            .await?;
    let ok = match &row {
        Some((_, hash, false)) if !hash.is_empty() => {
            auth::verify_password(c.password, hash.clone()).await
        }
        _ => false,
    };
    let Some((id, _, _)) = row.filter(|_| ok) else {
        auth::record_failure(&state, &c.username);
        return Err(AppError::Unauthorized("帳號或密碼錯誤".into()));
    };
    auth::clear_failures(&state, &c.username);
    sign_in(&state, id).await
}

#[utoipa::path(post, path = "/api/auth/logout", tag = "auth", responses((status = 204)))]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    if let Some(token) = auth::read_cookie(&headers, auth::COOKIE_NAME) {
        auth::delete_session(&state.db, &token).await?;
    }
    Ok((
        axum::http::StatusCode::NO_CONTENT,
        AppendHeaders([(
            header::SET_COOKIE,
            auth::clear_cookie(state.config.cookie_secure),
        )]),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordChange {
    /// Required when the account already has a password.
    pub current: Option<String>,
    pub new: String,
}

#[utoipa::path(post, path = "/api/auth/password", tag = "auth", request_body = PasswordChange,
    responses((status = 204), (status = 401, description = "Current password is wrong")))]
pub async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(p): Json<PasswordChange>,
) -> AppResult<axum::http::StatusCode> {
    auth::validate_new_password(&p.new)?;
    if user.has_password {
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
        if !auth::verify_password(p.current.unwrap_or_default(), hash).await {
            return Err(AppError::Unauthorized("目前的密碼不對".into()));
        }
    }
    let hash = auth::hash_password(p.new).await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
