//! Sign-in for the web page (session cookie), registration, and tokens for programs
//! (JWT access + refresh, personal API tokens).

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{AppendHeaders, IntoResponse},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    auth::{self, ClientIp, CurrentUser, NewAccount},
    error::{AppError, AppResult},
    events,
    perm::{self, Perm},
    state::AppState,
    tokens,
    util::{now, sha256_hex},
};

#[derive(Serialize, ToSchema)]
pub struct AuthState {
    /// No user exists yet: the first visitor creates the admin account.
    pub needs_setup: bool,
    pub user: Option<CurrentUser>,
    pub google_enabled: bool,
    /// Anyone may create an account at `/register`.
    pub registration_open: bool,
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
        user: auth::authenticate(&state, &headers).await?,
        google_enabled: state.config.google.is_some(),
        registration_open: state.settings().auth.registration_open,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

async fn sign_in(state: &AppState, id: i64) -> AppResult<impl IntoResponse> {
    auth::touch_login(&state.db, id).await?;
    let token = auth::create_session(&state.db, id).await?;
    let user = auth::load_user(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("找不到使用者"))?;
    Ok((
        AppendHeaders([(
            header::SET_COOKIE,
            auth::session_cookie(&token, state.config.cookie_secure),
        )]),
        Json(user),
    ))
}

async fn new_password_hash(username: &str, password: String) -> AppResult<String> {
    auth::validate_username(username)?;
    auth::validate_new_password(&password)?;
    Ok(auth::hash_password(password).await?)
}

/// First run only: create the admin account and sign in.
#[utoipa::path(post, path = "/api/auth/setup", tag = "auth", request_body = Credentials,
    responses((status = 200, body = CurrentUser), (status = 409, description = "Already set up")))]
pub async fn setup(
    State(state): State<AppState>,
    Json(c): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let username = c.username.trim().to_string();
    let hash = new_password_hash(&username, c.password).await?;
    // The INSERT only happens while the table is empty, so two racing setups can't both win.
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, created_at) \
         SELECT ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users) RETURNING id",
    )
    .bind(&username)
    .bind(hash)
    .bind(auth::FIRST_USER_ROLE)
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let id = id.ok_or_else(|| AppError::Conflict("已經設定過管理員了，請直接登入".into()))?;
    events::record(&state.db, &username, "user", "建立了第一個管理員帳號").await;
    sign_in(&state, id).await
}

/// Create an account with the default role (when registration is open) and sign in.
#[utoipa::path(post, path = "/api/auth/register", tag = "auth", request_body = Credentials,
    responses((status = 200, body = CurrentUser), (status = 403, description = "Registration is closed"),
              (status = 409, description = "Name taken"), (status = 429, description = "Too many registrations")))]
pub async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(c): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let rules = state.settings().auth;
    if !rules.registration_open {
        return Err(AppError::Forbidden("目前沒有開放註冊".into()));
    }
    auth::check_register_limit(&state, &ip)?;
    let username = c.username.trim().to_string();
    let hash = new_password_hash(&username, c.password).await?;
    let id = auth::create_account(
        &state.db,
        NewAccount {
            username: &username,
            password_hash: hash,
            role: &rules.default_role,
            email: None,
            google_sub: None,
        },
    )
    .await?;
    events::record(&state.db, &username, "user", "註冊了帳號").await;
    sign_in(&state, id).await
}

#[utoipa::path(post, path = "/api/auth/login", tag = "auth", request_body = Credentials,
    responses((status = 200, body = CurrentUser), (status = 401, description = "Wrong username or password")))]
pub async fn login(
    State(state): State<AppState>,
    Json(c): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let id = auth::check_credentials(&state, &c.username, c.password).await?;
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
        StatusCode::NO_CONTENT,
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

/// Change your own password. Every other session and token of yours stops working.
#[utoipa::path(post, path = "/api/auth/password", tag = "auth", request_body = PasswordChange,
    responses((status = 204), (status = 401, description = "Current password is wrong")))]
pub async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(p): Json<PasswordChange>,
) -> AppResult<impl IntoResponse> {
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
    tokens::revoke_all_for_user(&state.db, user.id).await?;
    events::record(&state.db, &user.username, "user", "改了自己的密碼").await;
    // Keep the page that made the change signed in, with a fresh session.
    let mut cookie = None;
    if auth::read_cookie(&headers, auth::COOKIE_NAME).is_some() {
        let token = auth::create_session(&state.db, user.id).await?;
        cookie = Some(auth::session_cookie(&token, state.config.cookie_secure));
    }
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders(cookie.map(|c| (header::SET_COOKIE, c))),
    ))
}

// ───────────────────────── tokens for programs ─────────────────────────

#[derive(Serialize, ToSchema)]
pub struct TokenPair {
    /// Send as `Authorization: Bearer <access_token>`.
    pub access_token: String,
    /// Exchange at `/api/auth/refresh` for a new pair; works once.
    pub refresh_token: String,
    pub token_type: String,
    /// Seconds until the access token expires.
    pub expires_in: i64,
    pub refresh_expires_in: i64,
}

async fn issue_pair(state: &AppState, user: &CurrentUser) -> AppResult<TokenPair> {
    let rules = state.settings().auth;
    let access_ttl = i64::from(rules.access_token_minutes) * 60;
    let refresh_ttl = i64::from(rules.refresh_token_days) * 86400;
    Ok(TokenPair {
        access_token: tokens::access_token(
            &state.jwt_key,
            user.id,
            user.token_version,
            access_ttl,
        )?,
        refresh_token: tokens::new_refresh_token(&state.db, user.id, refresh_ttl).await?,
        token_type: "Bearer".into(),
        expires_in: access_ttl,
        refresh_expires_in: refresh_ttl,
    })
}

/// Username and password for an access token and a refresh token.
#[utoipa::path(post, path = "/api/auth/token", tag = "auth", request_body = Credentials,
    responses((status = 200, body = TokenPair), (status = 401, description = "Wrong username or password")))]
pub async fn token(
    State(state): State<AppState>,
    Json(c): Json<Credentials>,
) -> AppResult<Json<TokenPair>> {
    let id = auth::check_credentials(&state, &c.username, c.password).await?;
    auth::touch_login(&state.db, id).await?;
    let user = auth::load_user(&state.db, id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("帳號或密碼錯誤".into()))?;
    Ok(Json(issue_pair(&state, &user).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct RefreshBody {
    pub refresh_token: String,
}

/// Trade a refresh token for a new pair. The old refresh token stops working.
#[utoipa::path(post, path = "/api/auth/refresh", tag = "auth", request_body = RefreshBody,
    responses((status = 200, body = TokenPair), (status = 401, description = "Unknown, used, expired or revoked")))]
pub async fn refresh(
    State(state): State<AppState>,
    Json(b): Json<RefreshBody>,
) -> AppResult<Json<TokenPair>> {
    let invalid = || AppError::Unauthorized("refresh token 無效、過期或已撤銷".into());
    let user_id = tokens::take_refresh_token(&state.db, &b.refresh_token)
        .await?
        .ok_or_else(invalid)?;
    let user = auth::load_user(&state.db, user_id)
        .await?
        .filter(|u| !u.disabled)
        .ok_or_else(invalid)?;
    Ok(Json(issue_pair(&state, &user).await?))
}

/// Revoke a refresh token (sign a program out).
#[utoipa::path(post, path = "/api/auth/revoke", tag = "auth", request_body = RefreshBody, responses((status = 204)))]
pub async fn revoke(
    State(state): State<AppState>,
    Json(b): Json<RefreshBody>,
) -> AppResult<StatusCode> {
    tokens::revoke_refresh_token(&state.db, &b.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct ApiTokenInfo {
    pub id: i64,
    pub name: String,
    #[sqlx(skip)]
    pub permissions: Vec<Perm>,
    #[serde(skip)]
    #[sqlx(rename = "permissions")]
    pub permissions_json: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

/// Your personal API tokens.
#[utoipa::path(get, path = "/api/auth/api-tokens", tag = "auth", responses((status = 200, body = [ApiTokenInfo])))]
pub async fn list_api_tokens(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<ApiTokenInfo>>> {
    let mut rows: Vec<ApiTokenInfo> = sqlx::query_as(
        "SELECT id, name, permissions, created_at, expires_at, last_used_at FROM api_tokens \
         WHERE user_id = ? ORDER BY id DESC",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    for r in &mut rows {
        r.permissions = perm::parse_list(&r.permissions_json);
    }
    Ok(Json(rows))
}

#[derive(Deserialize, ToSchema)]
pub struct NewApiToken {
    pub name: String,
    /// What the token may do; only permissions you have yourself.
    pub permissions: Vec<Perm>,
    /// Days until it expires; none or 0 for no expiry.
    pub expires_days: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedApiToken {
    /// Shown only now; only its hash is stored.
    pub token: String,
    pub id: i64,
}

/// Make a personal API token, e.g. for Streamer.bot or a Stream Deck to skip songs.
#[utoipa::path(post, path = "/api/auth/api-tokens", tag = "auth", request_body = NewApiToken,
    responses((status = 200, body = CreatedApiToken), (status = 403, description = "Asked for a permission you don't have")))]
pub async fn create_api_token(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(n): Json<NewApiToken>,
) -> AppResult<Json<CreatedApiToken>> {
    let name = n.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::bad("名稱要 1～64 個字"));
    }
    if n.permissions.is_empty() {
        return Err(AppError::bad("至少要選一個權限"));
    }
    for p in &n.permissions {
        user.require(*p)?;
    }
    let token = tokens::new_api_token();
    let expires_at = n
        .expires_days
        .filter(|d| *d > 0)
        .map(|d| now() + i64::from(d) * 86400);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO api_tokens (user_id, name, token_hash, permissions, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user.id)
    .bind(name)
    .bind(sha256_hex(&token))
    .bind(perm::to_json(&n.permissions))
    .bind(now())
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;
    events::record(
        &state.db,
        &user.username,
        "token",
        &format!("建立 API token「{name}」"),
    )
    .await;
    Ok(Json(CreatedApiToken { token, id }))
}

#[utoipa::path(delete, path = "/api/auth/api-tokens/{id}", tag = "auth", responses((status = 204), (status = 404)))]
pub async fn delete_api_token(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let name: Option<String> =
        sqlx::query_scalar("DELETE FROM api_tokens WHERE id = ? AND user_id = ? RETURNING name")
            .bind(id)
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    let name = name.ok_or_else(|| AppError::not_found("找不到這個 token"))?;
    events::record(
        &state.db,
        &user.username,
        "token",
        &format!("撤銷 API token「{name}」"),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
