//! Roles: named sets of permission points. Built-in roles can be edited (except the
//! locked admin role, which always has everything) but not deleted.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};
use utoipa::ToSchema;

use crate::{
    error::AppError,
    perm::{self, Perm},
};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Role {
    pub name: String,
    pub label: String,
    pub permissions: Vec<Perm>,
    pub builtin: bool,
    /// Always every permission; can't be edited.
    pub locked: bool,
    /// Accounts with this role.
    pub users: i64,
}

#[derive(sqlx::FromRow)]
struct RoleRow {
    name: String,
    label: String,
    permissions: String,
    builtin: bool,
    locked: bool,
    users: i64,
}

impl From<RoleRow> for Role {
    fn from(r: RoleRow) -> Self {
        Self {
            permissions: if r.locked {
                Perm::all()
            } else {
                perm::parse_list(&r.permissions)
            },
            name: r.name,
            label: r.label,
            builtin: r.builtin,
            locked: r.locked,
            users: r.users,
        }
    }
}

const ROLE_SELECT: &str = "SELECT r.name, r.label, r.permissions, r.builtin, r.locked, \
     (SELECT COUNT(*) FROM users u WHERE u.role = r.name) AS users FROM roles r";

pub async fn list(db: &SqlitePool) -> Result<Vec<Role>> {
    let rows: Vec<RoleRow> = sqlx::query_as(&format!(
        "{ROLE_SELECT} ORDER BY r.builtin DESC, r.created_at, r.name"
    ))
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Role::from).collect())
}

pub async fn get(db: &SqlitePool, name: &str) -> Result<Option<Role>> {
    let row: Option<RoleRow> = sqlx::query_as(&format!("{ROLE_SELECT} WHERE r.name = ?"))
        .bind(name)
        .fetch_optional(db)
        .await?;
    Ok(row.map(Role::from))
}

/// Role names are ids: lower-case letters, digits, `-` and `_`.
pub fn validate_name(name: &str) -> Result<(), AppError> {
    let ok = (2..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AppError::bad(
            "角色代號要 2～32 個小寫英文字母、數字、- 或 _",
        ))
    }
}

/// Enabled accounts that can still manage users. Every change to users or roles runs
/// this inside its transaction and rolls back when it would reach zero, so nobody can
/// lock the admins out: not by disabling, deleting, demoting, or editing a role.
pub async fn managers_left(tx: &mut Transaction<'_, Sqlite>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM users u JOIN roles r ON r.name = u.role WHERE u.disabled = 0 \
         AND (r.locked = 1 OR EXISTS (SELECT 1 FROM json_each(r.permissions) WHERE value = ?))",
    )
    .bind(Perm::UsersManage.id())
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn ensure_manager_left(tx: &mut Transaction<'_, Sqlite>) -> Result<(), AppError> {
    if managers_left(tx).await? == 0 {
        return Err(AppError::bad(
            "至少要留一個能管理使用者的帳號（沒有停用、角色有「管理使用者」權限）",
        ));
    }
    Ok(())
}

/// A role new accounts may get: it exists and can't manage users.
pub async fn check_default_role(db: &SqlitePool, name: &str) -> Result<(), AppError> {
    match get(db, name).await? {
        None => Err(AppError::bad(format!("沒有「{name}」這個角色"))),
        Some(r) if r.permissions.contains(&Perm::UsersManage) => {
            Err(AppError::bad("新帳號的預設角色不能有「管理使用者」權限"))
        }
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names() {
        assert!(validate_name("dj").is_ok());
        assert!(validate_name("song_helper-2").is_ok());
        assert!(validate_name("D J").is_err());
        assert!(validate_name("x").is_err());
        assert!(validate_name("管理").is_err());
    }
}
