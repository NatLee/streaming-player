//! Accounts and roles. Every change is checked by `roles::ensure_manager_left` inside the
//! same transaction, and recorded in the audit log (`kind` = `user` or `role`).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    auth::{self, Can, CurrentUser, NewAccount},
    db,
    error::{AppError, AppResult},
    events::{self, Event},
    perm::{self, need, Perm},
    roles::{self, Role},
    state::AppState,
    tokens,
    util::now,
};

fn clean_email(email: Option<String>) -> Option<String> {
    email
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
}

async fn audit(state: &AppState, actor: &CurrentUser, kind: &str, detail: String) {
    events::record(&state.db, &actor.username, kind, &detail).await;
}

async fn username_of(state: &AppState, id: i64) -> AppResult<String> {
    sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到使用者"))
}

#[utoipa::path(get, path = "/api/users", tag = "users", responses((status = 200, body = [CurrentUser])))]
pub async fn list(
    State(state): State<AppState>,
    _c: Can<need::UsersManage>,
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
    Can(admin, _): Can<need::UsersManage>,
    Json(u): Json<NewUser>,
) -> AppResult<StatusCode> {
    let username = u.username.trim().to_string();
    auth::validate_username(&username)?;
    if roles::get(&state.db, &u.role).await?.is_none() {
        return Err(AppError::bad(format!("沒有「{}」這個角色", u.role)));
    }
    let email = clean_email(u.email);
    let password_hash = match u.password.filter(|p| !p.is_empty()) {
        Some(p) => {
            auth::validate_new_password(&p)?;
            auth::hash_password(p).await?
        }
        None if email.is_some() => String::new(),
        None => return Err(AppError::bad("要設密碼，或填 Email 讓他用 Google 登入")),
    };
    auth::create_account(
        &state.db,
        NewAccount {
            username: &username,
            password_hash,
            role: &u.role,
            email,
            google_sub: None,
        },
    )
    .await?;
    audit(
        &state,
        &admin,
        "user",
        format!("新增使用者 {username}（角色 {}）", u.role),
    )
    .await;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize, ToSchema)]
pub struct UserUpdate {
    pub role: Option<String>,
    /// A new password; signs the user out everywhere.
    pub password: Option<String>,
    /// Empty string removes the email (and the Google link).
    pub email: Option<String>,
    /// Disabling signs the user out everywhere and stops their API tokens.
    pub disabled: Option<bool>,
}

#[utoipa::path(put, path = "/api/users/{id}", tag = "users", request_body = UserUpdate, responses((status = 204), (status = 400, description = "Would leave nobody who can manage users")))]
pub async fn update(
    State(state): State<AppState>,
    Can(admin, _): Can<need::UsersManage>,
    Path(id): Path<i64>,
    Json(u): Json<UserUpdate>,
) -> AppResult<StatusCode> {
    let name = username_of(&state, id).await?;
    let password_hash = match u.password.filter(|p| !p.is_empty()) {
        Some(p) => {
            auth::validate_new_password(&p)?;
            Some(auth::hash_password(p).await?)
        }
        None => None,
    };
    if let Some(role) = &u.role {
        if roles::get(&state.db, role).await?.is_none() {
            return Err(AppError::bad(format!("沒有「{role}」這個角色")));
        }
    }
    let mut changes = Vec::new();
    let mut tx = db::write_tx(&state.db).await?;
    if let Some(role) = &u.role {
        sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        changes.push(format!("角色改成 {role}"));
    }
    if let Some(hash) = &password_hash {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        changes.push("重設密碼".to_string());
    }
    if let Some(email) = u.email {
        sqlx::query("UPDATE users SET email = ?, google_sub = NULL WHERE id = ?")
            .bind(clean_email(Some(email)))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        changes.push("改 Email".to_string());
    }
    if let Some(disabled) = u.disabled {
        sqlx::query("UPDATE users SET disabled = ? WHERE id = ?")
            .bind(disabled)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        changes.push(if disabled { "停用" } else { "啟用" }.to_string());
    }
    roles::ensure_manager_left(&mut tx).await?;
    tx.commit().await?;
    if password_hash.is_some() || u.disabled == Some(true) {
        tokens::revoke_all_for_user(&state.db, id).await?;
    }
    if !changes.is_empty() {
        audit(
            &state,
            &admin,
            "user",
            format!("{name}：{}", changes.join("、")),
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/users/{id}", tag = "users", responses((status = 204), (status = 400, description = "Would leave nobody who can manage users")))]
pub async fn remove(
    State(state): State<AppState>,
    Can(admin, _): Can<need::UsersManage>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let name = username_of(&state, id).await?;
    let mut tx = db::write_tx(&state.db).await?;
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    roles::ensure_manager_left(&mut tx).await?;
    tx.commit().await?;
    audit(&state, &admin, "user", format!("刪除使用者 {name}")).await;
    Ok(StatusCode::NO_CONTENT)
}

// ───────────────────────────── roles ─────────────────────────────

#[derive(Serialize, ToSchema)]
pub struct PermissionInfo {
    pub id: Perm,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
pub struct RoleList {
    pub roles: Vec<Role>,
    /// Every permission point, for the editor.
    pub permissions: Vec<PermissionInfo>,
    pub default_role: String,
}

#[utoipa::path(get, path = "/api/roles", tag = "users", responses((status = 200, body = RoleList)))]
pub async fn list_roles(
    State(state): State<AppState>,
    _c: Can<need::UsersManage>,
) -> AppResult<Json<RoleList>> {
    Ok(Json(RoleList {
        roles: roles::list(&state.db).await?,
        permissions: perm::ALL
            .iter()
            .map(|i| PermissionInfo {
                id: i.perm,
                label: i.label.into(),
            })
            .collect(),
        default_role: state.settings().auth.default_role,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct RoleBody {
    /// Id; only when creating.
    pub name: Option<String>,
    pub label: String,
    pub permissions: Vec<Perm>,
}

#[utoipa::path(post, path = "/api/roles", tag = "users", request_body = RoleBody, responses((status = 201), (status = 409)))]
pub async fn create_role(
    State(state): State<AppState>,
    Can(admin, _): Can<need::UsersManage>,
    Json(b): Json<RoleBody>,
) -> AppResult<StatusCode> {
    let name = b.name.unwrap_or_default().trim().to_string();
    roles::validate_name(&name)?;
    let label = b.label.trim();
    if label.is_empty() {
        return Err(AppError::bad("要填角色名稱"));
    }
    sqlx::query("INSERT INTO roles (name, label, permissions, created_at) VALUES (?, ?, ?, ?)")
        .bind(&name)
        .bind(label)
        .bind(perm::to_json(&b.permissions))
        .bind(now())
        .execute(&state.db)
        .await
        .map_err(|e| match e.as_database_error() {
            Some(d) if d.is_unique_violation() => AppError::Conflict("已經有這個角色代號".into()),
            _ => e.into(),
        })?;
    audit(
        &state,
        &admin,
        "role",
        format!("新增角色 {name}：{}", ids(&b.permissions)),
    )
    .await;
    Ok(StatusCode::CREATED)
}

fn ids(perms: &[Perm]) -> String {
    if perms.is_empty() {
        "（沒有權限）".into()
    } else {
        perms.iter().map(|p| p.id()).collect::<Vec<_>>().join(", ")
    }
}

#[utoipa::path(put, path = "/api/roles/{name}", tag = "users", request_body = RoleBody, responses((status = 204), (status = 400)))]
pub async fn update_role(
    State(state): State<AppState>,
    Can(admin, _): Can<need::UsersManage>,
    Path(name): Path<String>,
    Json(b): Json<RoleBody>,
) -> AppResult<StatusCode> {
    let role = roles::get(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個角色"))?;
    if role.locked {
        return Err(AppError::bad("這個角色固定擁有全部權限，不能修改"));
    }
    if name == state.settings().auth.default_role && b.permissions.contains(&Perm::UsersManage) {
        return Err(AppError::bad("新帳號的預設角色不能有「管理使用者」權限"));
    }
    let label = b.label.trim();
    if label.is_empty() {
        return Err(AppError::bad("要填角色名稱"));
    }
    let mut tx = db::write_tx(&state.db).await?;
    sqlx::query("UPDATE roles SET label = ?, permissions = ? WHERE name = ?")
        .bind(label)
        .bind(perm::to_json(&b.permissions))
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    roles::ensure_manager_left(&mut tx).await?;
    tx.commit().await?;
    audit(
        &state,
        &admin,
        "role",
        format!("修改角色 {name} 的權限：{}", ids(&b.permissions)),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/roles/{name}", tag = "users", responses((status = 204), (status = 400, description = "Built in, in use, or the default role")))]
pub async fn delete_role(
    State(state): State<AppState>,
    Can(admin, _): Can<need::UsersManage>,
    Path(name): Path<String>,
) -> AppResult<StatusCode> {
    let role = roles::get(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個角色"))?;
    if role.builtin {
        return Err(AppError::bad("內建角色不能刪除"));
    }
    if role.users > 0 {
        return Err(AppError::bad(format!(
            "還有 {} 個帳號是這個角色，先改他們的角色",
            role.users
        )));
    }
    if name == state.settings().auth.default_role {
        return Err(AppError::bad("這是新帳號的預設角色，先在設定改掉"));
    }
    sqlx::query("DELETE FROM roles WHERE name = ?")
        .bind(&name)
        .execute(&state.db)
        .await?;
    audit(&state, &admin, "role", format!("刪除角色 {name}")).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Changes to users, roles and API tokens (the audit log).
#[utoipa::path(get, path = "/api/users/audit", tag = "users", responses((status = 200, body = [Event])))]
pub async fn audit_log(
    State(state): State<AppState>,
    _c: Can<need::UsersManage>,
) -> AppResult<Json<Vec<Event>>> {
    Ok(Json(events::audit(&state.db, 200).await?))
}
