//! Chat connections. Twitch and YouTube run as background tasks that turn chat messages
//! into commands; chat bots call the HTTP API in `bots.rs` instead.

pub mod bots;
pub mod twitch;
pub mod youtube;

use std::{collections::BTreeMap, sync::Mutex};

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use crate::{events, state::AppState, util::now};

pub const TWITCH: &str = "twitch";
pub const YOUTUBE: &str = "youtube";

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct Status {
    /// `off`, `connecting`, `connected`, `waiting`, `needs_auth` or `error`
    pub state: String,
    pub detail: String,
    pub since: i64,
}

#[derive(Default)]
pub struct Manager {
    tasks: Mutex<Vec<CancellationToken>>,
    status: Mutex<BTreeMap<&'static str, Status>>,
    /// Twitch device-code sign-in waiting for the streamer.
    pub device: Mutex<Option<twitch::DeviceLogin>>,
}

impl Manager {
    pub fn set(&self, platform: &'static str, state: &str, detail: impl Into<String>) {
        let detail = detail.into();
        let mut status = self.status.lock().unwrap();
        let unchanged = status
            .get(platform)
            .is_some_and(|s| s.state == state && s.detail == detail);
        if !unchanged {
            status.insert(
                platform,
                Status {
                    state: state.into(),
                    detail,
                    since: now(),
                },
            );
        }
    }

    pub fn get(&self, platform: &'static str) -> Status {
        self.status
            .lock()
            .unwrap()
            .get(platform)
            .cloned()
            .unwrap_or(Status {
                state: "off".into(),
                detail: String::new(),
                since: 0,
            })
    }

    fn stop_all(&self) {
        for t in self.tasks.lock().unwrap().drain(..) {
            t.cancel();
        }
    }
}

/// Apply the chat settings: stop every connection, then start the enabled ones.
pub fn restart(state: &AppState) {
    state.chat.stop_all();
    let settings = state.settings().chat;
    let start =
        |platform: &'static str,
         enabled: bool,
         run: fn(AppState, CancellationToken) -> tokio::task::JoinHandle<()>| {
            if enabled {
                let token = CancellationToken::new();
                state.chat.tasks.lock().unwrap().push(token.clone());
                state.chat.set(platform, "connecting", "");
                run(state.clone(), token);
            } else {
                state.chat.set(platform, "off", "");
            }
        };
    start(TWITCH, settings.twitch.enabled, |s, t| {
        tokio::spawn(twitch::run(s, t))
    });
    start(YOUTUBE, settings.youtube.enabled, |s, t| {
        tokio::spawn(youtube::run(s, t))
    });
}

/// Record a chat problem once in the activity log (not on every retry).
pub async fn report(state: &AppState, platform: &'static str, state_name: &str, detail: &str) {
    let before = state.chat.get(platform);
    state.chat.set(platform, state_name, detail);
    if before.state != state_name || before.detail != detail {
        events::record(
            &state.db,
            platform,
            "chat",
            &format!("{state_name}：{detail}"),
        )
        .await;
    }
}

/// Wait, but wake up early when the connection is being shut down. True if cancelled.
pub async fn sleep_or_cancel(cancel: &CancellationToken, secs: u64) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => true,
        _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => false,
    }
}
