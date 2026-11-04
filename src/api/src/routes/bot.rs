//! `GET /api/bot/{command}`: the chat-bot API. See `chat/bots.rs` and `docs/chat-bots.md`.

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    auth, chat::bots, commands::engine, error::AppResult, settings::BOT_KEY, state::AppState,
};

#[derive(Deserialize, IntoParams)]
pub struct BotQuery {
    /// The bot key from the admin page.
    pub key: Option<String>,
    /// Chatter name. Nightbot and Fossabot send it in headers, so it's optional for them.
    pub user: Option<String>,
    /// Chatter level: everyone, subscriber, vip, moderator, broadcaster (bot names like
    /// `owner`, `regular`, `twitch_vip` work too).
    pub level: Option<String>,
    /// What came after the command, e.g. the song link. `q` and `query` work too.
    pub args: Option<String>,
    pub q: Option<String>,
    pub query: Option<String>,
}

/// Names that carry the command arguments, and every parameter this endpoint reads.
const ARG_PARAMS: &[&str] = &["args", "q", "query"];
const KNOWN_PARAMS: &[&str] = &["key", "user", "level", "args", "q", "query"];

pub fn text(status: StatusCode, body: impl Into<String>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body.into(),
    )
        .into_response()
}

/// Run a chat command for a bot and answer with the reply as plain text.
#[utoipa::path(get, path = "/api/bot/{command}", tag = "bot", params(("command" = String, Path, description = "A command word such as `sr`"), BotQuery),
    responses((status = 200, description = "Reply text to post in chat", body = String, content_type = "text/plain"),
              (status = 401, description = "Wrong or missing key"), (status = 404, description = "Unknown command")))]
pub async fn run(
    State(state): State<AppState>,
    Path(command): Path<String>,
    Query(q): Query<BotQuery>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> AppResult<Response> {
    if !auth::key_matches(&state, BOT_KEY, q.key.as_deref()).await? {
        return Ok(text(
            StatusCode::UNAUTHORIZED,
            "bot key 不對，請到管理頁複製網址",
        ));
    }
    let Some(who) = bots::requester(&headers, q.user.as_deref(), q.level.as_deref()) else {
        return Ok(text(StatusCode::BAD_REQUEST, "缺少 user 參數"));
    };
    let args =
        bots::rejoin(raw.as_deref().unwrap_or(""), ARG_PARAMS, KNOWN_PARAMS).unwrap_or_default();
    Ok(
        match engine::run_trigger(&state, &who, &command, &args).await? {
            Some(reply) => text(StatusCode::OK, reply.text),
            None => text(StatusCode::NOT_FOUND, format!("沒有「{command}」這個指令")),
        },
    )
}
