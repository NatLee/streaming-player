//! Web sign-in: argon2 password hashes, HttpOnly session cookies, a failed-login limit,
//! and the access keys that OBS pages and chat bots use instead of a session.

use anyhow::anyhow;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap},
};
use serde::Serialize;
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::{
    error::AppError,
    settings,
    state::{AppState, Inner},
    util::{now, random_hex, same_secret, sha256_hex},
};

pub const COOKIE_NAME: &str = "sp_session";
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 3600;
const FAILURE_WINDOW_SECS: i64 = 15 * 60;
const MAX_FAILURES: usize = 5;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_MODERATOR: &str = "moderator";

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    /// `admin` (everything) or `moderator` (queue and player, not settings or users)
    pub role: String,
    pub email: Option<String>,
    pub has_password: bool,
    pub google_linked: bool,
}

impl CurrentUser {
    pub fn is_admin(&self) -> bool {
        self.role == ROLE_ADMIN
    }
}

const USER_COLUMNS: &str =
    "u.id, u.username, u.role, u.email, u.password_hash <> '' AS has_password, \
     u.google_sub IS NOT NULL AS google_linked";

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

pub async fn user_for_token(db: &SqlitePool, token: &str) -> anyhow::Result<Option<CurrentUser>> {
    let sql = format!(
        "SELECT {USER_COLUMNS} FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = ? AND s.expires_at > ? AND u.disabled = 0"
    );
    Ok(sqlx::query_as(&sql)
        .bind(sha256_hex(token))
        .bind(now())
        .fetch_optional(db)
        .await?)
}

pub async fn list_users(db: &SqlitePool) -> anyhow::Result<Vec<CurrentUser>> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users u ORDER BY u.id");
    Ok(sqlx::query_as(&sql).fetch_all(db).await?)
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

pub async fn session_user(
    state: &Inner,
    headers: &HeaderMap,
) -> anyhow::Result<Option<CurrentUser>> {
    match read_cookie(headers, COOKIE_NAME) {
        Some(token) => user_for_token(&state.db, &token).await,
        None => Ok(None),
    }
}

/// Handlers that need a signed-in user take this; without a session they answer 401.
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        session_user(state, &parts.headers)
            .await?
            .ok_or_else(|| AppError::Unauthorized("請先登入".into()))
    }
}

/// Admin-only handlers take this; moderators get 403.
pub struct AdminUser(pub CurrentUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(AppError::Forbidden("需要管理員權限".into()));
        }
        Ok(Self(user))
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

/// OBS pages (player, overlay, media streams): a signed-in user or `?key=<obs key>`.
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
        if session_user(state, &parts.headers).await?.is_some() {
            return Ok(Self);
        }
        Err(AppError::Unauthorized(
            "需要 OBS 金鑰（網址裡的 key）或先登入".into(),
        ))
    }
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
    fn reads_cookie_header() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; sp_session=abc; b=2"),
        );
        assert_eq!(read_cookie(&h, COOKIE_NAME).as_deref(), Some("abc"));
        assert_eq!(read_cookie(&h, "missing"), None);
    }
}
