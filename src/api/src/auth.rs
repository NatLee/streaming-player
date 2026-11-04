//! Who is calling, and what they may do.
//!
//! One extractor (`CurrentUser`) accepts a session cookie (the web page), a JWT access
//! token or a personal API token (`Authorization: Bearer …`). `Can<P>` adds one
//! permission point on top. OBS pages and chat bots use their own access keys.

use std::{marker::PhantomData, net::SocketAddr};

use anyhow::anyhow;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header, request::Parts, HeaderMap},
};
use serde::Serialize;
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::{
    error::AppError,
    perm::{self, Perm, PermMarker},
    settings,
    state::{AppState, Inner},
    tokens,
    util::{now, random_hex, same_secret, sha256_hex},
};

pub const COOKIE_NAME: &str = "sp_session";
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 3600;
const FAILURE_WINDOW_SECS: i64 = 15 * 60;
const MAX_FAILURES: usize = 5;
/// The role of the account made on first run.
pub const FIRST_USER_ROLE: &str = "admin";

/// A user as stored, with their role's permissions.
#[derive(Debug, Clone, sqlx::FromRow)]
struct UserRow {
    id: i64,
    username: String,
    role: String,
    role_label: String,
    role_permissions: String,
    role_locked: bool,
    email: Option<String>,
    has_password: bool,
    google_linked: bool,
    disabled: bool,
    token_version: i64,
    created_at: i64,
    last_login_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub role_label: String,
    pub email: Option<String>,
    pub has_password: bool,
    pub google_linked: bool,
    pub disabled: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
    /// What this caller may do. For an API token: the token's permissions.
    pub permissions: Vec<Perm>,
    #[serde(skip)]
    pub token_version: i64,
}

impl From<UserRow> for CurrentUser {
    fn from(r: UserRow) -> Self {
        let permissions = if r.role_locked {
            Perm::all()
        } else {
            perm::parse_list(&r.role_permissions)
        };
        Self {
            id: r.id,
            username: r.username,
            role: r.role,
            role_label: r.role_label,
            email: r.email,
            has_password: r.has_password,
            google_linked: r.google_linked,
            disabled: r.disabled,
            created_at: r.created_at,
            last_login_at: r.last_login_at,
            permissions,
            token_version: r.token_version,
        }
    }
}

impl CurrentUser {
    pub fn can(&self, p: Perm) -> bool {
        self.permissions.contains(&p)
    }

    /// 403 unless the caller holds `p`.
    pub fn require(&self, p: Perm) -> Result<(), AppError> {
        if self.can(p) {
            Ok(())
        } else {
            Err(AppError::Forbidden(format!("需要權限：{}", p.info().label)))
        }
    }
}

const USER_SELECT: &str = "SELECT u.id, u.username, u.role, r.label AS role_label, \
     r.permissions AS role_permissions, r.locked AS role_locked, u.email, \
     u.password_hash <> '' AS has_password, u.google_sub IS NOT NULL AS google_linked, \
     u.disabled, u.token_version, u.created_at, u.last_login_at \
     FROM users u JOIN roles r ON r.name = u.role";

pub async fn load_user(db: &SqlitePool, id: i64) -> anyhow::Result<Option<CurrentUser>> {
    let row: Option<UserRow> = sqlx::query_as(&format!("{USER_SELECT} WHERE u.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(CurrentUser::from))
}

pub async fn list_users(db: &SqlitePool) -> anyhow::Result<Vec<CurrentUser>> {
    let rows: Vec<UserRow> = sqlx::query_as(&format!("{USER_SELECT} ORDER BY u.id"))
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(CurrentUser::from).collect())
}

pub async fn touch_login(db: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn hash_password(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow!("password hashing failed: {e}"))
    })
    .await?
}

pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .map(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

pub fn validate_new_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < 8 {
        return Err(AppError::bad("密碼至少要 8 個字元"));
    }
    if password.len() > 256 {
        return Err(AppError::bad("密碼太長了"));
    }
    Ok(())
}

pub fn validate_username(username: &str) -> Result<(), AppError> {
    let n = username.chars().count();
    if !(2..=32).contains(&n) || username.contains(char::is_whitespace) {
        return Err(AppError::bad("帳號要 2～32 個字元，不能有空白"));
    }
    Ok(())
}

/// Check a username and password; the user id when they match an enabled account.
/// Counts failures per name, so a name is locked for a while after `MAX_FAILURES`.
pub async fn check_credentials(
    state: &Inner,
    username: &str,
    password: String,
) -> Result<i64, AppError> {
    check_rate_limit(state, username)?;
    let row: Option<(i64, String, bool)> =
        sqlx::query_as("SELECT id, password_hash, disabled FROM users WHERE username = ?")
            .bind(username.trim())
            .fetch_optional(&state.db)
            .await?;
    let ok = match &row {
        Some((_, hash, false)) if !hash.is_empty() => verify_password(password, hash.clone()).await,
        _ => false,
    };
    match row.filter(|_| ok) {
        Some((id, _, _)) => {
            clear_failures(state, username);
            Ok(id)
        }
        None => {
            record_failure(state, username);
            Err(AppError::Unauthorized("帳號或密碼錯誤".into()))
        }
    }
}

// ───────────────────────── sessions ─────────────────────────

pub async fn create_session(db: &SqlitePool, user_id: i64) -> anyhow::Result<String> {
    let token = random_hex(32);
    let t = now();
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(sha256_hex(&token))
    .bind(user_id)
    .bind(t)
    .bind(t + SESSION_TTL_SECS)
    .execute(db)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(t)
        .execute(db)
        .await?;
    Ok(token)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token))
        .execute(db)
        .await?;
    Ok(())
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookie(secure: bool) -> String {
    format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

async fn user_for_session(state: &Inner, token: &str) -> anyhow::Result<Option<CurrentUser>> {
    let id: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash = ? AND expires_at > ?")
            .bind(sha256_hex(token))
            .bind(now())
            .fetch_optional(&state.db)
            .await?;
    match id {
        Some(id) => load_user(&state.db, id).await,
        None => Ok(None),
    }
}

async fn user_for_api_token(state: &Inner, token: &str) -> anyhow::Result<Option<CurrentUser>> {
    let row: Option<(i64, i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, user_id, permissions, last_used_at FROM api_tokens \
         WHERE token_hash = ? AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(sha256_hex(token))
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let Some((token_id, user_id, scopes, last_used)) = row else {
        return Ok(None);
    };
    let Some(mut user) = load_user(&state.db, user_id).await? else {
        return Ok(None);
    };
    // The token can do what it was made for, and never more than its owner can now.
    let scopes = perm::parse_list(&scopes);
    user.permissions.retain(|p| scopes.contains(p));
    if last_used.is_none_or(|t| now() - t > 60) {
        sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
            .bind(now())
            .bind(token_id)
            .execute(&state.db)
            .await?;
    }
    Ok(Some(user))
}

async fn user_for_access_token(state: &Inner, token: &str) -> anyhow::Result<Option<CurrentUser>> {
    let Some(claims) = tokens::verify_access(&state.jwt_key, token) else {
        return Ok(None);
    };
    Ok(load_user(&state.db, claims.sub)
        .await?
        .filter(|u| u.token_version == claims.ver))
}

/// The signed-in caller, from a session cookie or a Bearer token. Disabled users are nobody.
pub async fn authenticate(
    state: &Inner,
    headers: &HeaderMap,
) -> anyhow::Result<Option<CurrentUser>> {
    let mut user = None;
    if let Some(token) = read_cookie(headers, COOKIE_NAME) {
        user = user_for_session(state, &token).await?;
    }
    if user.is_none() {
        if let Some(token) = bearer(headers) {
            user = if token.starts_with(tokens::API_TOKEN_PREFIX) {
                user_for_api_token(state, token).await?
            } else {
                user_for_access_token(state, token).await?
            };
        }
    }
    Ok(user.filter(|u| !u.disabled))
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        authenticate(state, &parts.headers)
            .await?
            .ok_or_else(|| AppError::Unauthorized("請先登入".into()))
    }
}

/// A signed-in caller holding the permission `P`: `Can<need::QueueManage>`.
pub struct Can<P>(pub CurrentUser, pub PhantomData<P>);

impl<P: PermMarker + Send + Sync> FromRequestParts<AppState> for Can<P> {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        user.require(P::PERM)?;
        Ok(Self(user, PhantomData))
    }
}

/// `key` from the query string (OBS browser sources can't send cookies or headers).
pub fn query_key(parts: &Parts) -> Option<String> {
    url::form_urlencoded::parse(parts.uri.query().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "key")
        .map(|(_, v)| v.into_owned())
}

/// Does `given` match the stored access key `name` (`obs_key` or `bot_key`)?
pub async fn key_matches(state: &Inner, name: &str, given: Option<&str>) -> anyhow::Result<bool> {
    let Some(given) = given.filter(|g| !g.is_empty()) else {
        return Ok(false);
    };
    let expected = settings::access_key(&state.db, name).await?;
    Ok(same_secret(given, &expected))
}

/// Player media: `?key=<obs key>` (OBS) or a caller who may see the queue.
pub struct PlayerAccess;

impl FromRequestParts<AppState> for PlayerAccess {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if key_matches(state, settings::OBS_KEY, query_key(parts).as_deref()).await? {
            return Ok(Self);
        }
        match authenticate(state, &parts.headers).await? {
            Some(u) => u.require(Perm::QueueView).map(|_| Self),
            None => Err(AppError::Unauthorized(
                "需要 OBS 金鑰（網址裡的 key）或先登入".into(),
            )),
        }
    }
}

/// The caller's address for rate limits: the socket peer, or the first
/// `X-Forwarded-For` hop when `SP_TRUST_PROXY` says a reverse proxy sets it.
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let forwarded = state
            .config
            .trust_proxy
            .then(|| parts.headers.get("x-forwarded-for")?.to_str().ok())
            .flatten()
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        Ok(Self(forwarded.or(peer).unwrap_or_else(|| "unknown".into())))
    }
}

// ───────────────────────── registration ─────────────────────────

const REGISTER_WINDOW_SECS: i64 = 3600;

/// Refuse a registration when this address, or everyone together, made too many in
/// the last hour. Every attempt counts, so a flood of bad requests is limited too.
pub fn check_register_limit(state: &Inner, ip: &str) -> Result<(), AppError> {
    let limits = state.settings().auth;
    let t = now();
    let mut attempts = state.register_attempts.lock().unwrap();
    attempts.retain(|(_, at)| t - at < REGISTER_WINDOW_SECS);
    let from_ip = attempts.iter().filter(|(a, _)| a == ip).count() as u32;
    if from_ip >= limits.register_per_ip_per_hour
        || attempts.len() as u32 >= limits.register_per_hour
    {
        return Err(AppError::TooManyRequests("註冊太頻繁，請晚點再試".into()));
    }
    attempts.push((ip.to_string(), t));
    Ok(())
}

pub struct NewAccount<'a> {
    pub username: &'a str,
    pub password_hash: String,
    pub role: &'a str,
    pub email: Option<String>,
    pub google_sub: Option<&'a str>,
}

/// Insert an account. The very first account becomes the admin, whatever was asked.
pub async fn create_account(db: &SqlitePool, a: NewAccount<'_>) -> Result<i64, AppError> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?;
    let role = if users == 0 { FIRST_USER_ROLE } else { a.role };
    sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, email, google_sub, created_at)          VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(a.username)
    .bind(a.password_hash)
    .bind(role)
    .bind(a.email)
    .bind(a.google_sub)
    .bind(now())
    .fetch_one(db)
    .await
    .map_err(|e| match e.as_database_error() {
        Some(d) if d.is_unique_violation() => AppError::Conflict("帳號或 Email 已經有人用了".into()),
        _ => e.into(),
    })
}

// ───────────────────────── failed sign-in limit ─────────────────────────

pub fn check_rate_limit(state: &Inner, username: &str) -> Result<(), AppError> {
    let key = username.trim().to_lowercase();
    let t = now();
    let mut map = state.login_failures.lock().unwrap();
    let entry = map.entry(key).or_default();
    entry.retain(|ts| t - ts < FAILURE_WINDOW_SECS);
    if entry.len() >= MAX_FAILURES {
        return Err(AppError::TooManyRequests(
            "登入失敗太多次，請 15 分鐘後再試".into(),
        ));
    }
    Ok(())
}

pub fn record_failure(state: &Inner, username: &str) {
    state
        .login_failures
        .lock()
        .unwrap()
        .entry(username.trim().to_lowercase())
        .or_default()
        .push(now());
}

pub fn clear_failures(state: &Inner, username: &str) {
    state
        .login_failures
        .lock()
        .unwrap()
        .remove(&username.trim().to_lowercase());
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[tokio::test]
    async fn hash_and_verify() {
        let h = hash_password("correct horse".into()).await.unwrap();
        assert!(verify_password("correct horse".into(), h.clone()).await);
        assert!(!verify_password("wrong".into(), h).await);
        assert!(!verify_password("x".into(), "not-a-hash".into()).await);
    }

    #[test]
    fn reads_cookie_and_bearer_headers() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; sp_session=abc; b=2"),
        );
        assert_eq!(read_cookie(&h, COOKIE_NAME).as_deref(), Some("abc"));
        assert_eq!(read_cookie(&h, "missing"), None);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer  xyz "),
        );
        assert_eq!(bearer(&h), Some("xyz"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer(&h), None);
    }
}
