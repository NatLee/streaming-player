//! Web accounts. Admins manage everything; moderators run the queue and the player.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::{
    auth::{self, AdminUser, CurrentUser, ROLE_ADMIN, ROLE_MODERATOR},
    error::{AppError, AppResult},
    events,
    state::AppState,
    util::now,
};

fn check_role(role: &str) -> AppResult<()> {
    if role == ROLE_ADMIN || role == ROLE_MODERATOR {
        Ok(())
    } else {
        Err(AppError::bad("角色只能是 admin 或 moderator"))
    }
}

fn clean_email(email: Option<String>) -> Option<String> {
    email
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
}

#[utoipa::path(get, path = "/api/users", tag = "users", responses((status = 200, body = [CurrentUser])))]
pub async fn list(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<Vec<CurrentUser>>> {
    Ok(Json(auth::list_users(&state.db).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct NewUser {
    pub username: String,
    /// Optional when the user signs in with Google.
    pub password: Option<String>,
    pub role: String,
    /// Google sign-in is allowed for this address.
    pub email: Option<String>,
}

#[utoipa::path(post, path = "/api/users", tag = "users", request_body = NewUser, responses((status = 201)))]
pub async fn create(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(u): Json<NewUser>,
) -> AppResult<StatusCode> {
    let username = u.username.trim().to_string();
    auth::validate_username(&username)?;
    check_role(&u.role)?;
    let email = clean_email(u.email);
    let hash = match u.password.filter(|p| !p.is_empty()) {
        Some(p) => {
            auth::validate_new_password(&p)?;
            auth::hash_password(p).await?
        }
        None if email.is_some() => String::new(),
        None => return Err(AppError::bad("要設密碼，或填 Email 讓他用 Google 登入")),
    };
    sqlx::query("INSERT INTO users (username, password_hash, role, email, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&username)
        .bind(hash)
        .bind(&u.role)
        .bind(&email)
        .bind(now())
        .execute(&state.db)
        .await
        .map_err(|e| match e.as_database_error() {
            Some(d) if d.is_unique_violation() => AppError::Conflict("帳號或 Email 已經有人用了".into()),
            _ => e.into(),
        })?;
    events::record(
        &state.db,
        &admin.username,
        "user",
        &format!("新增使用者 {username}（{}）", u.role),
    )
    .await;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize, ToSchema)]
pub struct UserUpdate {
    pub role: Option<String>,
    pub password: Option<String>,
    /// Empty string removes the email.
    pub email: Option<String>,
    pub disabled: Option<bool>,
}

#[utoipa::path(put, path = "/api/users/{id}", tag = "users", request_body = UserUpdate, responses((status = 204)))]
pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
    Json(u): Json<UserUpdate>,
) -> AppResult<StatusCode> {
    if id == admin.id
        && (u.disabled == Some(true) || u.role.as_deref().is_some_and(|r| r != ROLE_ADMIN))
    {
        return Err(AppError::bad("不能停用自己或拿掉自己的管理員權限"));
    }
    if let Some(role) = &u.role {
        check_role(role)?;
        sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(p) = u.password.filter(|p| !p.is_empty()) {
        auth::validate_new_password(&p)?;
        let hash = auth::hash_password(p).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(id)
            .execute(&state.db)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(email) = u.email {
        sqlx::query("UPDATE users SET email = ?, google_sub = NULL WHERE id = ?")
            .bind(clean_email(Some(email)))
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(disabled) = u.disabled {
        sqlx::query("UPDATE users SET disabled = ? WHERE id = ?")
            .bind(disabled)
            .bind(id)
            .execute(&state.db)
            .await?;
        if disabled {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/users/{id}", tag = "users", responses((status = 204)))]
pub async fn remove(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if id == admin.id {
        return Err(AppError::bad("不能刪除自己"));
    }
    let n = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found("找不到使用者"));
    }
    Ok(StatusCode::NO_CONTENT)
}
