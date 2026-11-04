//! Twitch chat through EventSub WebSocket and the Twitch API, which Twitch documents as
//! "the preferred method of viewing and sending chats". IRC is not used.
//!
//! Sign-in uses the Device Code Grant flow: no redirect URL, so it works for a server on
//! localhost. Scopes: `user:read:chat` (read via `channel.chat.message`) and
//! `user:write:chat` (reply via Send Chat Message).

use std::{collections::VecDeque, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use super::{report, sleep_or_cancel, TWITCH};
use crate::{
    commands::engine,
    db,
    models::{Level, Requester, Source},
    state::{AppState, Inner},
    util::now,
};

pub const SCOPES: &str = "user:read:chat user:write:chat";
const TOKENS_KEY: &str = "twitch_tokens";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Twitch asks apps to validate tokens at least once an hour.
const VALIDATE_EVERY_SECS: i64 = 50 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub login: String,
    #[serde(default)]
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceLogin {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

pub fn client_id(state: &Inner) -> Option<String> {
    let configured = state.settings().chat.twitch.client_id;
    Some(configured.trim().to_string())
        .filter(|c| !c.is_empty())
        .or_else(|| state.config.twitch_client_id.clone())
}

pub async fn load_tokens(state: &Inner) -> Result<Option<Tokens>> {
    db::kv_get_json(&state.db, TOKENS_KEY).await
}

async fn save_tokens(state: &Inner, t: &Tokens) -> Result<()> {
    db::kv_set_json(&state.db, TOKENS_KEY, t).await
}

pub async fn forget_tokens(state: &Inner) -> Result<()> {
    db::kv_delete(&state.db, TOKENS_KEY).await
}

// ───────────────────────────── sign-in ─────────────────────────────

/// Start the device-code sign-in; a background task polls until the streamer approves.
pub async fn start_device(state: &AppState) -> Result<DeviceLogin> {
    let client = client_id(state).ok_or_else(|| anyhow!("先填 Twitch 應用程式的 Client ID"))?;
    let ep = &state.config.endpoints;
    let resp = state
        .http
        .post(format!("{}/oauth2/device", ep.twitch_id))
        .form(&[("client_id", client.as_str()), ("scopes", SCOPES)])
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await?;
    if !status.is_success() {
        bail!(
            "Twitch 拒絕：{}",
            body["message"].as_str().unwrap_or("unknown")
        );
    }
    let device_code = body["device_code"].as_str().unwrap_or_default().to_string();
    let login = DeviceLogin {
        user_code: body["user_code"].as_str().unwrap_or_default().to_string(),
        verification_uri: body["verification_uri"]
            .as_str()
            .unwrap_or("https://www.twitch.tv/activate")
            .to_string(),
        expires_at: now() + body["expires_in"].as_i64().unwrap_or(1800),
    };
    let interval = body["interval"].as_u64().unwrap_or(5).max(1);
    *state.chat.device.lock().unwrap() = Some(login.clone());
    tokio::spawn(poll_device(
        state.clone(),
        client,
        device_code,
        interval,
        login.expires_at,
    ));
    Ok(login)
}

async fn poll_device(
    state: AppState,
    client: String,
    device_code: String,
    mut interval: u64,
    expires_at: i64,
) {
    let ep = state.config.endpoints.clone();
    while now() < expires_at {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        // A newer sign-in replaced this one.
        if state
            .chat
            .device
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.expires_at)
            != Some(expires_at)
        {
            return;
        }
        let resp = state
            .http
            .post(format!("{}/oauth2/token", ep.twitch_id))
            .form(&[
                ("client_id", client.as_str()),
                ("scopes", SCOPES),
                ("device_code", device_code.as_str()),
                ("grant_type", DEVICE_GRANT),
            ])
            .send()
            .await;
        let Ok(resp) = resp else { continue };
        let ok = resp.status().is_success();
        let body: Value = resp.json().await.unwrap_or_default();
        if ok {
            let tokens = Tokens {
                access_token: body["access_token"].as_str().unwrap_or_default().into(),
                refresh_token: body["refresh_token"].as_str().unwrap_or_default().into(),
                login: String::new(),
                user_id: String::new(),
            };
            let result = async {
                let tokens = identify(&state, tokens).await?;
                save_tokens(&state, &tokens).await?;
                anyhow::Ok(tokens)
            }
            .await;
            *state.chat.device.lock().unwrap() = None;
            match result {
                Ok(t) => {
                    crate::events::record(
                        &state.db,
                        "twitch",
                        "chat",
                        &format!("已用 {} 登入 Twitch", t.login),
                    )
                    .await;
                    super::restart(&state);
                }
                Err(e) => report(&state, TWITCH, "error", &format!("登入失敗：{e}")).await,
            }
            return;
        }
        match body["message"].as_str().unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            other => {
                *state.chat.device.lock().unwrap() = None;
                report(
                    &state,
                    TWITCH,
                    "needs_auth",
                    &format!("Twitch 登入沒有完成：{other}"),
                )
                .await;
                return;
            }
        }
    }
    *state.chat.device.lock().unwrap() = None;
}

/// Fill in login and user id from `/oauth2/validate`.
async fn identify(state: &Inner, mut t: Tokens) -> Result<Tokens> {
    let (login, user_id) = validate(state, &t.access_token)
        .await?
        .ok_or_else(|| anyhow!("Twitch 說這個 token 無效"))?;
    t.login = login;
    t.user_id = user_id;
    Ok(t)
}

/// `Some((login, user_id))` if the token is valid, `None` if Twitch says 401.
async fn validate(state: &Inner, access: &str) -> Result<Option<(String, String)>> {
    let resp = state
        .http
        .get(format!(
            "{}/oauth2/validate",
            state.config.endpoints.twitch_id
        ))
        .header("Authorization", format!("OAuth {access}"))
        .send()
        .await?;
    if resp.status().as_u16() == 401 {
        return Ok(None);
    }
    let body: Value = resp.error_for_status()?.json().await?;
    Ok(Some((
        body["login"].as_str().unwrap_or_default().to_string(),
        body["user_id"].as_str().unwrap_or_default().to_string(),
    )))
}

/// Public clients refresh without a secret; the refresh token is single-use.
async fn refresh(state: &Inner, client: &str, t: &Tokens) -> Result<Tokens> {
    let resp = state
        .http
        .post(format!("{}/oauth2/token", state.config.endpoints.twitch_id))
        .form(&[
            ("client_id", client),
            ("grant_type", "refresh_token"),
            ("refresh_token", t.refresh_token.as_str()),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await?;
    if !status.is_success() {
        bail!(
            "Twitch 拒絕更新 token：{}",
            body["message"].as_str().unwrap_or("unknown")
        );
    }
    let fresh = Tokens {
        access_token: body["access_token"].as_str().unwrap_or_default().into(),
        refresh_token: body["refresh_token"]
            .as_str()
            .unwrap_or(&t.refresh_token)
            .into(),
        login: t.login.clone(),
        user_id: t.user_id.clone(),
    };
    save_tokens(state, &fresh).await?;
    Ok(fresh)
}

// ───────────────────────────── Twitch API ─────────────────────────────

struct Session {
    client: String,
    tokens: Tokens,
    broadcaster_id: String,
    validated_at: i64,
    /// Our own replies come back as chat messages; skip them by id.
    sent: VecDeque<String>,
}

impl Session {
    async fn helix(
        &mut self,
        state: &Inner,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        for attempt in 0..2 {
            let mut req = state
                .http
                .request(
                    method.clone(),
                    format!("{}{path}", state.config.endpoints.twitch_api),
                )
                .header("Client-Id", &self.client)
                .bearer_auth(&self.tokens.access_token)
                .query(query);
            if let Some(b) = &body {
                req = req.json(b);
            }
            let resp = req.send().await?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt == 0 {
                self.tokens = refresh(state, &self.client, &self.tokens).await?;
                continue;
            }
            let value: Value = resp.json().await.unwrap_or_default();
            if !status.is_success() {
                bail!(
                    "Twitch API {path} HTTP {status}: {}",
                    value["message"].as_str().unwrap_or("")
                );
            }
            return Ok(value);
        }
        bail!("Twitch API {path}: unauthorized")
    }

    async fn keep_valid(&mut self, state: &Inner) -> Result<()> {
        if now() - self.validated_at < VALIDATE_EVERY_SECS {
            return Ok(());
        }
        if validate(state, &self.tokens.access_token).await?.is_none() {
            self.tokens = refresh(state, &self.client, &self.tokens).await?;
        }
        self.validated_at = now();
        Ok(())
    }

    async fn send(&mut self, state: &Inner, text: &str, reply_to: Option<&str>) -> Result<()> {
        let mut body = json!({
            "broadcaster_id": self.broadcaster_id,
            "sender_id": self.tokens.user_id,
            "message": text,
        });
        if let Some(id) = reply_to {
            body["reply_parent_message_id"] = json!(id);
        }
        let resp = self
            .helix(
                state,
                reqwest::Method::POST,
                "/chat/messages",
                Some(body),
                &[],
            )
            .await?;
        let data = &resp["data"][0];
        if let Some(id) = data["message_id"].as_str() {
            self.sent.push_back(id.to_string());
            if self.sent.len() > 50 {
                self.sent.pop_front();
            }
        }
        if data["is_sent"].as_bool() == Some(false) {
            let reason = data["drop_reason"]["message"].as_str().unwrap_or("dropped");
            tracing::warn!("Twitch dropped our reply: {reason}");
        }
        Ok(())
    }
}

/// Parse a `channel.chat.message` event into who said what. None for messages we skip.
pub fn chat_event(event: &Value, broadcaster_id: &str) -> Option<(Requester, String, String)> {
    // Shared chat: messages from the other channels in the session aren't ours to handle.
    if let Some(src) = event["source_broadcaster_user_id"].as_str() {
        if src != broadcaster_id {
            return None;
        }
    }
    let text = event["message"]["text"].as_str()?.to_string();
    let login = event["chatter_user_login"].as_str()?;
    let name = event["chatter_user_name"].as_str().unwrap_or(login);
    let badges: Vec<&str> = event["badges"]
        .as_array()
        .map(|b| b.iter().filter_map(|x| x["set_id"].as_str()).collect())
        .unwrap_or_default();
    let mut level = Level::highest(badges);
    if event["chatter_user_id"].as_str() == Some(broadcaster_id) {
        level = Level::Broadcaster;
    }
    let id = event["message_id"].as_str().unwrap_or_default().to_string();
    Some((
        Requester::new(name, Some(login), level, Source::Twitch),
        text,
        id,
    ))
}

// ───────────────────────────── the connection ─────────────────────────────

pub async fn run(state: AppState, cancel: CancellationToken) {
    let mut backoff = 2;
    loop {
        let result = tokio::select! {
            _ = cancel.cancelled() => return,
            r = connect_once(&state, &cancel) => r,
        };
        match result {
            Ok(()) => return,
            Err(Stop::NeedsAuth(msg)) => {
                report(&state, TWITCH, "needs_auth", &msg).await;
                return;
            }
            Err(Stop::Retry(err)) => {
                report(
                    &state,
                    TWITCH,
                    "error",
                    &format!("{err:#}（{backoff} 秒後重連）"),
                )
                .await;
            }
        }
        if sleep_or_cancel(&cancel, backoff).await {
            return;
        }
        backoff = (backoff * 2).min(300);
    }
}

enum Stop {
    NeedsAuth(String),
    Retry(anyhow::Error),
}

impl From<anyhow::Error> for Stop {
    fn from(e: anyhow::Error) -> Self {
        Stop::Retry(e)
    }
}

async fn start_session(state: &Inner) -> Result<Session, Stop> {
    let client =
        client_id(state).ok_or_else(|| Stop::NeedsAuth("還沒填 Twitch Client ID".into()))?;
    let tokens = load_tokens(state)
        .await?
        .ok_or_else(|| Stop::NeedsAuth("還沒登入 Twitch".into()))?;
    let tokens = match validate(state, &tokens.access_token).await? {
        Some(_) => tokens,
        None => refresh(state, &client, &tokens)
            .await
            .map_err(|e| Stop::NeedsAuth(format!("Twitch 登入過期，請重新登入（{e}）")))?,
    };
    let tokens = identify(state, tokens).await?;
    let mut session = Session {
        client,
        broadcaster_id: tokens.user_id.clone(),
        tokens,
        validated_at: now(),
        sent: VecDeque::new(),
    };
    let channel = state.settings().chat.twitch.channel.trim().to_lowercase();
    if !channel.is_empty() && channel != session.tokens.login {
        let users = session
            .helix(
                state,
                reqwest::Method::GET,
                "/users",
                None,
                &[("login", channel.as_str())],
            )
            .await?;
        session.broadcaster_id = users["data"][0]["id"]
            .as_str()
            .ok_or_else(|| Stop::NeedsAuth(format!("找不到 Twitch 頻道 {channel}")))?
            .to_string();
    }
    Ok(session)
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Read until the welcome message; returns (session id, keepalive seconds).
async fn welcome(ws: &mut Ws) -> Result<(String, u64)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .context("EventSub 沒有送出 welcome")?
            .ok_or_else(|| anyhow!("EventSub 連線關閉了"))??;
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text)?;
            if v["metadata"]["message_type"] == "session_welcome" {
                let s = &v["payload"]["session"];
                return Ok((
                    s["id"].as_str().unwrap_or_default().to_string(),
                    s["keepalive_timeout_seconds"].as_u64().unwrap_or(10),
                ));
            }
        }
    }
}

async fn connect_once(state: &AppState, cancel: &CancellationToken) -> Result<(), Stop> {
    let mut session = start_session(state).await?;
    let url = state.config.endpoints.twitch_eventsub_ws.clone();
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .context("連不上 Twitch EventSub")?;
    let (session_id, mut keepalive) = welcome(&mut ws).await?;
    let sub = json!({
        "type": "channel.chat.message",
        "version": "1",
        "condition": { "broadcaster_user_id": session.broadcaster_id, "user_id": session.tokens.user_id },
        "transport": { "method": "websocket", "session_id": session_id },
    });
    session
        .helix(
            state,
            reqwest::Method::POST,
            "/eventsub/subscriptions",
            Some(sub),
            &[],
        )
        .await
        .map_err(|e| match e.to_string() {
            m if m.contains("403") => {
                Stop::NeedsAuth(format!("權限不足，請重新登入 Twitch（{m}）"))
            }
            _ => Stop::Retry(e),
        })?;
    let channel_name = state.settings().chat.twitch.channel;
    let channel_name = if channel_name.trim().is_empty() {
        session.tokens.login.clone()
    } else {
        channel_name
    };
    report(
        state,
        TWITCH,
        "connected",
        &format!("{} → #{}", session.tokens.login, channel_name),
    )
    .await;

    loop {
        // Twitch sends a keepalive when quiet; silence past the timeout means the link is dead.
        let wait = Duration::from_secs(keepalive + 10);
        let next = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            n = tokio::time::timeout(wait, ws.next()) => n,
        };
        let msg = match next {
            Err(_) => return Err(Stop::Retry(anyhow!("EventSub 超過 {keepalive} 秒沒有消息"))),
            Ok(None) => return Err(Stop::Retry(anyhow!("EventSub 連線關閉了"))),
            Ok(Some(m)) => m.map_err(|e| Stop::Retry(e.into()))?,
        };
        let text = match msg {
            Message::Text(t) => t.as_str().to_string(),
            Message::Close(_) => return Err(Stop::Retry(anyhow!("Twitch 關閉了 EventSub 連線"))),
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v["metadata"]["message_type"].as_str().unwrap_or("") {
            "notification" => {
                let event = &v["payload"]["event"];
                if let Some((who, text, id)) = chat_event(event, &session.broadcaster_id) {
                    if session.sent.contains(&id) {
                        continue;
                    }
                    if let Err(e) = session.keep_valid(state).await {
                        tracing::warn!("Twitch token check failed: {e:#}");
                    }
                    match engine::handle_message(state, &who, &text).await {
                        Ok(Some(reply)) if !reply.text.is_empty() => {
                            if let Err(e) = session.send(state, &reply.text, Some(&id)).await {
                                tracing::warn!("Twitch reply failed: {e:#}");
                            }
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!("chat command failed: {e:#}"),
                    }
                }
            }
            "session_reconnect" => {
                let Some(next_url) = v["payload"]["session"]["reconnect_url"].as_str() else {
                    continue;
                };
                // Subscriptions move to the new connection; it starts with its own welcome.
                let (mut fresh, _) = tokio_tungstenite::connect_async(next_url)
                    .await
                    .context("EventSub reconnect failed")?;
                let (_, ka) = welcome(&mut fresh).await?;
                keepalive = ka;
                ws = fresh;
            }
            "revocation" => {
                let reason = v["payload"]["subscription"]["status"]
                    .as_str()
                    .unwrap_or("revoked");
                return Err(Stop::NeedsAuth(format!("Twitch 取消了訂閱：{reason}")));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chat_events() {
        let ev = json!({
            "broadcaster_user_id": "100", "chatter_user_id": "200", "chatter_user_login": "viewer",
            "chatter_user_name": "Viewer", "message_id": "m1", "message": {"text": "!sr abc"},
            "badges": [{"set_id": "subscriber"}, {"set_id": "moderator"}],
            "source_broadcaster_user_id": null
        });
        let (who, text, id) = chat_event(&ev, "100").unwrap();
        assert_eq!(who.name, "Viewer");
        assert_eq!(who.key, "viewer");
        assert_eq!(who.level, Level::Moderator);
        assert_eq!((text.as_str(), id.as_str()), ("!sr abc", "m1"));

        let owner = json!({"chatter_user_id": "100", "chatter_user_login": "me", "message": {"text": "hi"}, "badges": []});
        assert_eq!(
            chat_event(&owner, "100").unwrap().0.level,
            Level::Broadcaster
        );

        let shared = json!({"chatter_user_login": "x", "message": {"text": "!sr"}, "source_broadcaster_user_id": "999"});
        assert!(
            chat_event(&shared, "100").is_none(),
            "shared-chat messages from other channels"
        );
    }
}
