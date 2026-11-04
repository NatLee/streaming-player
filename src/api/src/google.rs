//! Google sign-in (OAuth 2.0 authorization code flow with PKCE, state and nonce).
//!
//! v1 let any gmail.com account in. Now only a user whose email an admin entered on the
//! Users page can sign in; the first sign-in remembers the Google account (`sub`).
//!
//! The ID token comes straight from Google's token endpoint over TLS with our client
//! secret, so per OpenID Connect Core 3.1.3.7 TLS authenticates the issuer; we still
//! check iss, aud, exp, nonce and email_verified.

use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap},
    response::{AppendHeaders, IntoResponse, Redirect, Response},
};
use data_encoding::BASE64URL_NOPAD;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    auth,
    config::GoogleConfig,
    state::AppState,
    util::{now, random_hex},
};

const STATE_COOKIE: &str = "sp_oauth_state";
const STATE_TTL_SECS: i64 = 600;
pub const CALLBACK_PATH: &str = "/api/auth/google/callback";

#[derive(Debug, Clone)]
pub struct PendingLogin {
    verifier: String,
    nonce: String,
    created_at: i64,
}

pub fn redirect_uri(public_url: &str) -> String {
    format!("{}{CALLBACK_PATH}", public_url.trim_end_matches('/'))
}

fn state_cookie(value: &str, max_age: i64, secure: bool) -> String {
    format!(
        "{STATE_COOKIE}={value}; Path=/api/auth/google; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn fail(code: &str, secure: bool) -> Response {
    (
        AppendHeaders([(header::SET_COOKIE, state_cookie("", 0, secure))]),
        Redirect::to(&format!("/login?error={code}")),
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/auth/google/start", tag = "auth",
    responses((status = 303, description = "Redirect to Google")))]
pub async fn start(State(state): State<AppState>) -> Response {
    let secure = state.config.cookie_secure;
    let Some(google) = &state.config.google else {
        return fail("google_disabled", secure);
    };
    let oauth_state = random_hex(16);
    let nonce = random_hex(16);
    let verifier = BASE64URL_NOPAD.encode(random_hex(32).as_bytes());
    let challenge = BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()));
    {
        let mut pending = state.oauth_pending.lock().unwrap();
        let t = now();
        pending.retain(|_, p| t - p.created_at < STATE_TTL_SECS);
        pending.insert(
            oauth_state.clone(),
            PendingLogin {
                verifier,
                nonce: nonce.clone(),
                created_at: t,
            },
        );
    }
    let redirect = redirect_uri(&state.config.public_url);
    let url = reqwest::Url::parse_with_params(
        &google.auth_url,
        &[
            ("client_id", google.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", "openid email profile"),
            ("state", oauth_state.as_str()),
            ("nonce", nonce.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ],
    );
    match url {
        Ok(url) => (
            AppendHeaders([(
                header::SET_COOKIE,
                state_cookie(&oauth_state, STATE_TTL_SECS, secure),
            )]),
            Redirect::to(url.as_str()),
        )
            .into_response(),
        Err(err) => {
            tracing::error!("bad Google auth URL: {err}");
            fail("google_failed", secure)
        }
    }
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[utoipa::path(get, path = "/api/auth/google/callback", tag = "auth",
    responses((status = 303, description = "Signed in; redirect to the admin page")))]
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let secure = state.config.cookie_secure;
    let pending = q
        .state
        .as_ref()
        .and_then(|s| state.oauth_pending.lock().unwrap().remove(s));
    let Some(google) = state.config.google.clone() else {
        return fail("google_disabled", secure);
    };
    if q.error.is_some() {
        return fail("google_denied", secure);
    }
    let cookie_state = auth::read_cookie(&headers, STATE_COOKIE);
    let pending = match (pending, q.state.as_deref()) {
        (Some(p), Some(s))
            if cookie_state.as_deref() == Some(s) && now() - p.created_at < STATE_TTL_SECS =>
        {
            p
        }
        _ => return fail("google_state", secure),
    };
    let Some(code) = q.code else {
        return fail("google_state", secure);
    };
    let claims = match exchange_code(&state, &google, &code, &pending).await {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!("Google sign-in failed: {err:#}");
            return fail("google_failed", secure);
        }
    };
    if claims.get("email_verified").and_then(Value::as_bool) != Some(true) {
        return fail("google_unverified", secure);
    }
    let (Some(sub), Some(email)) = (
        claims.get("sub").and_then(Value::as_str),
        claims.get("email").and_then(Value::as_str),
    ) else {
        return fail("google_failed", secure);
    };
    match find_user(&state, sub, email).await {
        Ok(Some((_, true))) => fail("user_disabled", secure),
        Ok(Some((user_id, false))) => {
            let token = async {
                auth::touch_login(&state.db, user_id).await?;
                auth::create_session(&state.db, user_id).await
            }
            .await;
            match token {
                Ok(token) => (
                    AppendHeaders([
                        (header::SET_COOKIE, auth::session_cookie(&token, secure)),
                        (header::SET_COOKIE, state_cookie("", 0, secure)),
                    ]),
                    Redirect::to("/admin"),
                )
                    .into_response(),
                Err(err) => {
                    tracing::error!("Google sign-in session failed: {err:#}");
                    fail("google_failed", secure)
                }
            }
        }
        Ok(None) => {
            tracing::info!("Google account {email} has no matching user");
            fail("google_not_allowed", secure)
        }
        Err(err) => {
            tracing::error!("Google sign-in lookup failed: {err:#}");
            fail("google_failed", secure)
        }
    }
}

async fn exchange_code(
    state: &AppState,
    google: &GoogleConfig,
    code: &str,
    pending: &PendingLogin,
) -> anyhow::Result<Value> {
    let redirect = redirect_uri(&state.config.public_url);
    let resp = state
        .http
        .post(&google.token_url)
        .timeout(Duration::from_secs(15))
        .form(&[
            ("code", code),
            ("client_id", google.client_id.as_str()),
            ("client_secret", google.client_secret.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", pending.verifier.as_str()),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await?;
    if !status.is_success() {
        anyhow::bail!("token endpoint HTTP {status}: {body}");
    }
    let id_token = body
        .get("id_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("no id_token in the token response"))?;
    let claims = decode_claims(id_token)?;
    check_claims(&claims, &google.client_id, &pending.nonce, now())?;
    Ok(claims)
}

pub fn decode_claims(id_token: &str) -> anyhow::Result<Value> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed id_token"))?;
    let bytes = BASE64URL_NOPAD
        .decode(payload.trim_end_matches('=').as_bytes())
        .map_err(|_| anyhow::anyhow!("id_token is not base64url"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn check_claims(claims: &Value, client_id: &str, nonce: &str, now: i64) -> anyhow::Result<()> {
    let iss = claims.get("iss").and_then(Value::as_str).unwrap_or("");
    if iss != "https://accounts.google.com" && iss != "accounts.google.com" {
        anyhow::bail!("iss is not Google: {iss}");
    }
    let aud_ok = match claims.get("aud") {
        Some(Value::String(a)) => a == client_id,
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    if !aud_ok {
        anyhow::bail!("aud is not this client");
    }
    if claims.get("exp").and_then(Value::as_i64).unwrap_or(0) <= now {
        anyhow::bail!("id_token expired");
    }
    if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
        anyhow::bail!("nonce mismatch");
    }
    Ok(())
}

/// By Google `sub` first; otherwise by email for a user not yet linked (and link it).
async fn find_user(
    state: &AppState,
    sub: &str,
    email: &str,
) -> anyhow::Result<Option<(i64, bool)>> {
    let by_sub: Option<(i64, bool)> =
        sqlx::query_as("SELECT id, disabled FROM users WHERE google_sub = ?")
            .bind(sub)
            .fetch_optional(&state.db)
            .await?;
    if by_sub.is_some() {
        return Ok(by_sub);
    }
    let by_email: Option<(i64, bool)> = sqlx::query_as(
        "SELECT id, disabled FROM users WHERE email = ? COLLATE NOCASE AND google_sub IS NULL",
    )
    .bind(email.trim())
    .fetch_optional(&state.db)
    .await?;
    if let Some((id, false)) = by_email {
        sqlx::query("UPDATE users SET google_sub = ? WHERE id = ?")
            .bind(sub)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(by_email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn token(claims: &Value) -> String {
        format!(
            "x.{}.y",
            BASE64URL_NOPAD.encode(claims.to_string().as_bytes())
        )
    }

    #[test]
    fn decodes_and_checks_claims() {
        let claims = json!({"iss":"https://accounts.google.com","aud":"cid","exp":2000,"nonce":"n1","sub":"1","email":"a@b.com","email_verified":true});
        let decoded = decode_claims(&token(&claims)).unwrap();
        assert_eq!(decoded, claims);
        assert!(check_claims(&decoded, "cid", "n1", 1000).is_ok());
        assert!(check_claims(&decoded, "other", "n1", 1000).is_err(), "aud");
        assert!(check_claims(&decoded, "cid", "n2", 1000).is_err(), "nonce");
        assert!(check_claims(&decoded, "cid", "n1", 3000).is_err(), "exp");
        let bad_iss = json!({"iss":"https://evil.example","aud":"cid","exp":2000,"nonce":"n1"});
        assert!(check_claims(&bad_iss, "cid", "n1", 1000).is_err(), "iss");
    }

    #[test]
    fn builds_redirect_uri() {
        assert_eq!(
            redirect_uri("https://player.example.com/"),
            "https://player.example.com/api/auth/google/callback"
        );
    }
}
