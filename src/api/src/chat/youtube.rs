//! YouTube live chat through the Live Streaming API with an API key.
//!
//! `liveChatMessages.list` costs 5 quota units per call and a project gets 10,000 units a
//! day, so the poll interval is a setting (default 20 s ≈ 900 units an hour). An API key
//! can read a public chat but can't post, so replies only go to the activity log.

use anyhow::{bail, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{report, sleep_or_cancel, YOUTUBE};
use crate::{
    commands::engine,
    events,
    media::{
        source::{self, Input},
        youtube,
    },
    models::{Level, Requester, Source},
    settings,
    state::AppState,
};

pub const UNITS_PER_POLL: u32 = 5;
/// When the video isn't live yet, check again this often (1 unit per check).
const NOT_LIVE_RETRY_SECS: u64 = 120;

/// Quota units one hour of polling uses.
pub fn units_per_hour(poll_secs: u32) -> u32 {
    3600 / poll_secs.max(1) * UNITS_PER_POLL
}

pub fn video_id(input: &str) -> Option<String> {
    match source::parse_input(input)? {
        Input::YouTube(id) => Some(id),
        _ => None,
    }
}

/// One chat item as (who, text). None for non-text events (super chats, joins…).
pub fn chat_item(item: &Value) -> Option<(Requester, String)> {
    let snippet = &item["snippet"];
    if snippet["type"].as_str() != Some("textMessageEvent") {
        return None;
    }
    let text = snippet["displayMessage"]
        .as_str()
        .or_else(|| snippet["textMessageDetails"]["messageText"].as_str())?
        .to_string();
    let author = &item["authorDetails"];
    let flags = [
        ("isChatOwner", Level::Broadcaster),
        ("isChatModerator", Level::Moderator),
        ("isChatSponsor", Level::Subscriber),
    ];
    let level = flags
        .iter()
        .filter(|(flag, _)| author[*flag].as_bool() == Some(true))
        .map(|(_, l)| *l)
        .max()
        .unwrap_or(Level::Everyone);
    let name = author["displayName"].as_str().unwrap_or("YouTube");
    let key = author["channelId"].as_str().map(|c| format!("yt:{c}"));
    Some((
        Requester::new(name, key.as_deref(), level, Source::Youtube),
        text,
    ))
}

pub async fn run(state: AppState, cancel: CancellationToken) {
    loop {
        match poll_chat(&state, &cancel).await {
            Ok(Wait::Stop) => return,
            Ok(Wait::Secs(secs)) => {
                if sleep_or_cancel(&cancel, secs).await {
                    return;
                }
            }
            Err(err) => {
                report(
                    &state,
                    YOUTUBE,
                    "error",
                    &format!("{err:#}（1 分鐘後重試）"),
                )
                .await;
                if sleep_or_cancel(&cancel, 60).await {
                    return;
                }
            }
        }
    }
}

enum Wait {
    Stop,
    Secs(u64),
}

async fn poll_chat(state: &AppState, cancel: &CancellationToken) -> Result<Wait> {
    let cfg = state.settings().chat.youtube;
    let Some(key) = settings::youtube_api_key(&state.db).await? else {
        report(state, YOUTUBE, "needs_auth", "還沒設定 YouTube API key").await;
        return Ok(Wait::Stop);
    };
    let Some(video) = video_id(&cfg.video) else {
        report(state, YOUTUBE, "needs_auth", "還沒填直播網址").await;
        return Ok(Wait::Stop);
    };
    let ep = &state.config.endpoints;
    let Some(chat_id) = youtube::active_live_chat_id(&state.http, ep, &key, &video).await? else {
        report(
            state,
            YOUTUBE,
            "waiting",
            "這部影片現在沒有在直播，稍後再檢查",
        )
        .await;
        return Ok(Wait::Secs(NOT_LIVE_RETRY_SECS));
    };
    report(
        state,
        YOUTUBE,
        "connected",
        &format!("讀取 {video} 的聊天室，每 {} 秒一次", cfg.poll_secs),
    )
    .await;

    let mut page: Option<String> = None;
    let mut first = true;
    loop {
        let mut query = vec![
            ("liveChatId", chat_id.clone()),
            ("part", "snippet,authorDetails".into()),
            ("key", key.clone()),
        ];
        if let Some(p) = &page {
            query.push(("pageToken", p.clone()));
        }
        let resp = state
            .http
            .get(format!("{}/liveChat/messages", ep.youtube_api))
            .query(&query)
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            let reason = body["error"]["errors"][0]["reason"].as_str().unwrap_or("");
            match reason {
                "liveChatEnded" | "liveChatNotFound" => {
                    report(state, YOUTUBE, "waiting", "直播結束了，稍後再檢查").await;
                    return Ok(Wait::Secs(NOT_LIVE_RETRY_SECS));
                }
                "quotaExceeded" | "rateLimitExceeded" => {
                    report(
                        state,
                        YOUTUBE,
                        "error",
                        "YouTube API 配額用完了，1 小時後再試",
                    )
                    .await;
                    return Ok(Wait::Secs(3600));
                }
                _ => bail!("YouTube API HTTP {status}: {}", youtube::api_error(&body)),
            }
        }
        // The first page is chat from before we connected; don't run old commands.
        if !first {
            for item in body["items"].as_array().into_iter().flatten() {
                if let Some((who, text)) = chat_item(item) {
                    match engine::handle_message(state, &who, &text).await {
                        Ok(Some(reply)) => {
                            events::record(&state.db, &who.name, "youtube", &reply.text).await;
                        }
                        Ok(None) => {}
                        Err(e) => tracing::error!("chat command failed: {e:#}"),
                    }
                }
            }
        }
        first = false;
        if body["offlineAt"].is_string() {
            report(state, YOUTUBE, "waiting", "直播結束了，稍後再檢查").await;
            return Ok(Wait::Secs(NOT_LIVE_RETRY_SECS));
        }
        page = body["nextPageToken"].as_str().map(str::to_string);
        let suggested = body["pollingIntervalMillis"]
            .as_u64()
            .unwrap_or(0)
            .div_ceil(1000);
        let wait = suggested.max(u64::from(cfg.poll_secs));
        if sleep_or_cancel(cancel, wait).await {
            return Ok(Wait::Stop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quota_estimate() {
        assert_eq!(units_per_hour(20), 900);
        assert_eq!(units_per_hour(5), 3600);
    }

    #[test]
    fn parses_chat_items() {
        let item = json!({
            "snippet": {"type": "textMessageEvent", "displayMessage": "!sr 小幸運"},
            "authorDetails": {"displayName": "Viewer", "channelId": "UCabc", "isChatModerator": true, "isChatSponsor": true}
        });
        let (who, text) = chat_item(&item).unwrap();
        assert_eq!(text, "!sr 小幸運");
        assert_eq!(who.key, "yt:ucabc");
        assert_eq!(who.level, Level::Moderator);
        let superchat = json!({"snippet": {"type": "superChatEvent"}});
        assert!(chat_item(&superchat).is_none());
    }

    #[test]
    fn video_ids_from_links() {
        assert_eq!(
            video_id("https://www.youtube.com/live/JJo-zUi9E5U"),
            Some("JJo-zUi9E5U".into())
        );
        assert_eq!(video_id("JJo-zUi9E5U"), Some("JJo-zUi9E5U".into()));
        assert_eq!(video_id("https://twitch.tv/x"), None);
    }
}
