//! Tokens for programs that talk to the API without a browser:
//!
//! - **Access token**: a short-lived JWT (HS256) naming the user and their token version.
//!   Bumping the version (disable, password change) makes every older one invalid.
//! - **Refresh token**: an opaque random string stored as a SHA-256, exchanged for a new
//!   pair (the old one stops working), revocable one by one.
//! - **API token** (`spat_…`): long-lived, made on the admin page, limited to the
//!   permissions picked when it was created.

use anyhow::Result;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    db,
    util::{now, random_hex, sha256_hex},
};

const SECRET_KEY: &str = "jwt_secret";
/// Prefix of personal API tokens, so they can't be mistaken for a JWT.
pub const API_TOKEN_PREFIX: &str = "spat_";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// User id
    pub sub: i64,
    /// The user's token version when it was issued
    pub ver: i64,
    pub iat: i64,
    pub exp: i64,
}

/// The signing key: `SP_JWT_SECRET` if set, otherwise made on first start and kept in
/// the database.
pub async fn signing_key(db: &SqlitePool, configured: Option<&str>) -> Result<Vec<u8>> {
    if let Some(k) = configured {
        return Ok(k.as_bytes().to_vec());
    }
    if let Some(k) = db::kv_get(db, SECRET_KEY).await? {
        return Ok(k.into_bytes());
    }
    let k = random_hex(32);
    db::kv_set(db, SECRET_KEY, &k).await?;
    Ok(k.into_bytes())
}

pub fn encode_claims(key: &[u8], claims: &Claims) -> Result<String> {
    Ok(encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(key),
    )?)
}

pub fn access_token(key: &[u8], user_id: i64, ver: i64, ttl_secs: i64) -> Result<String> {
    let t = now();
    encode_claims(
        key,
        &Claims {
            sub: user_id,
            ver,
            iat: t,
            exp: t + ttl_secs,
        },
    )
}

/// Claims of a valid, unexpired access token. The caller still checks the user.
pub fn verify_access(key: &[u8], token: &str) -> Option<Claims> {
    let mut v = Validation::new(Algorithm::HS256);
    v.leeway = 0;
    v.set_required_spec_claims(&["exp"]);
    decode::<Claims>(token, &DecodingKey::from_secret(key), &v)
        .ok()
        .map(|d| d.claims)
}

pub async fn new_refresh_token(db: &SqlitePool, user_id: i64, ttl_secs: i64) -> Result<String> {
    let token = random_hex(32);
    let t = now();
    sqlx::query("INSERT INTO refresh_tokens (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(sha256_hex(&token))
        .bind(user_id)
        .bind(t)
        .bind(t + ttl_secs)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < ?")
        .bind(t)
        .execute(db)
        .await?;
    Ok(token)
}

/// Use up a refresh token: it is deleted, and its user id returned if it was valid.
pub async fn take_refresh_token(db: &SqlitePool, token: &str) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "DELETE FROM refresh_tokens WHERE token_hash = ? AND expires_at > ? RETURNING user_id",
    )
    .bind(sha256_hex(token))
    .bind(now())
    .fetch_optional(db)
    .await?)
}

pub async fn revoke_refresh_token(db: &SqlitePool, token: &str) -> Result<bool> {
    Ok(
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = ?")
            .bind(sha256_hex(token))
            .execute(db)
            .await?
            .rows_affected()
            > 0,
    )
}

/// Sign a user out everywhere: sessions, refresh tokens, and (via the version) access tokens.
/// API tokens stay; they are revoked on their own, and stop working when the user is disabled.
pub async fn revoke_all_for_user(db: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query("UPDATE users SET token_version = token_version + 1 WHERE id = ?")
        .bind(user_id)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = ?")
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(())
}

pub fn new_api_token() -> String {
    format!("{API_TOKEN_PREFIX}{}", random_hex(24))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_tokens_round_trip_and_expire() {
        let key = b"test-key";
        let t = access_token(key, 7, 2, 60).unwrap();
        let c = verify_access(key, &t).unwrap();
        assert_eq!((c.sub, c.ver), (7, 2));
        assert!(verify_access(b"other-key", &t).is_none(), "wrong key");
        let expired = encode_claims(
            key,
            &Claims {
                sub: 7,
                ver: 2,
                iat: now() - 100,
                exp: now() - 1,
            },
        )
        .unwrap();
        assert!(verify_access(key, &expired).is_none(), "expired");
        let mut tampered = t.clone();
        tampered.push('x');
        assert!(verify_access(key, &tampered).is_none(), "tampered");
        assert!(verify_access(key, "not.a.jwt").is_none());
    }

    #[test]
    fn api_tokens_have_the_prefix() {
        let t = new_api_token();
        assert!(t.starts_with(API_TOKEN_PREFIX));
        assert_eq!(t.len(), API_TOKEN_PREFIX.len() + 48);
    }
}
