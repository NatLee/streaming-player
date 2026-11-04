//! Settings the streamer edits on the admin page. Stored as one JSON document in `kv`,
//! so adding a field only needs a `#[serde(default)]` default here.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::{db, perm::Perm, util::random_hex};

const SETTINGS_KEY: &str = "settings";
pub const OBS_KEY: &str = "obs_key";
pub const BOT_KEY: &str = "bot_key";
pub const YOUTUBE_API_KEY: &str = "youtube_api_key";

/// Queue rules. A zero limit means "no limit". Defaults follow v1
/// (30 songs, 15 minutes, 3 votes to skip, 2 votes to bump).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct QueueRules {
    /// Accept new requests from chat.
    pub requests_open: bool,
    /// Most songs waiting in the queue.
    pub max_queue: u32,
    /// Most songs one person can have waiting.
    pub max_per_user: u32,
    /// Longest video in seconds.
    pub max_duration_secs: u32,
    /// Allow a song that is already playing or queued to be requested again.
    pub allow_duplicates: bool,
    /// Votes needed to skip the current song.
    pub skip_votes: u32,
    /// Votes needed to move a queued song to the front.
    pub bump_votes: u32,
    /// How long a vote counts, in seconds.
    pub vote_window_secs: u32,
    /// Mark a video "missing" after it failed to play this many times in a row.
    pub missing_after_failures: u32,
    /// Moderators and the broadcaster ignore the per-user limit and command cooldowns.
    pub privileged_bypass: bool,
}

impl Default for QueueRules {
    fn default() -> Self {
        Self {
            requests_open: true,
            max_queue: 30,
            max_per_user: 0,
            max_duration_secs: 15 * 60,
            allow_duplicates: false,
            skip_votes: 3,
            bump_votes: 2,
            vote_window_secs: 15 * 60,
            missing_after_failures: 3,
            privileged_bypass: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum YoutubeMode {
    /// Official YouTube IFrame player.
    Embed,
    /// Direct video link from yt-dlp (how v1 played everything).
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct Playback {
    /// When the queue is empty, play a random song from the default playlist (favorites).
    pub autoplay_favorites: bool,
    pub youtube_mode: YoutubeMode,
    /// When the YouTube embed refuses a video, try the yt-dlp link instead.
    pub stream_fallback: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            autoplay_favorites: true,
            youtube_mode: YoutubeMode::Embed,
            stream_fallback: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct ViewerSettings {
    /// Anyone can open `/queue` to see the queue and look up their own requests.
    pub public_queue: bool,
}

impl Default for ViewerSettings {
    fn default() -> Self {
        Self { public_queue: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct TwitchSettings {
    pub enabled: bool,
    /// Twitch application client id (public client). Empty: use `SP_TWITCH_CLIENT_ID`.
    pub client_id: String,
    /// Channel login to read. Empty: the signed-in account's own channel.
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct YoutubeSettings {
    pub enabled: bool,
    /// Live video URL or id whose chat to read.
    pub video: String,
    /// Seconds between chat polls. Each poll costs 5 of the 10,000 daily quota units.
    pub poll_secs: u32,
}

impl Default for YoutubeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            video: String::new(),
            poll_secs: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct ChatSettings {
    /// Commands start with this, e.g. `!sr`.
    pub prefix: String,
    pub twitch: TwitchSettings,
    pub youtube: YoutubeSettings,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            prefix: "!".into(),
            twitch: TwitchSettings::default(),
            youtube: YoutubeSettings::default(),
        }
    }
}

/// Accounts: registration, and how long API tokens last.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct AuthSettings {
    /// Anyone can create an account at `/register` (and Google sign-in creates one).
    pub registration_open: bool,
    /// Role of new accounts. Must not be able to manage users.
    pub default_role: String,
    /// Registrations allowed per client address per hour.
    pub register_per_ip_per_hour: u32,
    /// Registrations allowed per hour in total.
    pub register_per_hour: u32,
    /// Lifetime of an API access token (JWT), in minutes.
    pub access_token_minutes: u32,
    /// Lifetime of an API refresh token, in days.
    pub refresh_token_days: u32,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            registration_open: true,
            default_role: "viewer".into(),
            register_per_ip_per_hour: 5,
            register_per_hour: 30,
            access_token_minutes: 15,
            refresh_token_days: 30,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub queue: QueueRules,
    pub playback: Playback,
    pub viewer: ViewerSettings,
    pub chat: ChatSettings,
    pub auth: AuthSettings,
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        let prefix = self.chat.prefix.trim();
        if prefix.is_empty() || prefix.chars().count() > 3 || prefix.contains(char::is_whitespace) {
            return Err("指令前綴要是 1～3 個字元，不能有空白".into());
        }
        if self.chat.youtube.poll_secs < 5 {
            return Err("YouTube 聊天室輪詢間隔至少 5 秒".into());
        }
        if self.queue.skip_votes == 0 || self.queue.bump_votes == 0 {
            return Err("投票門檻至少要 1 票".into());
        }
        if self.auth.access_token_minutes == 0 || self.auth.refresh_token_days == 0 {
            return Err("token 的有效時間至少要 1".into());
        }
        Ok(())
    }
}

/// Which permission each settings section needs to change. One table instead of a
/// branch per field; a new section only needs a row here.
const SECTION_PERMS: &[(&str, Perm)] = &[
    ("queue", Perm::SettingsManage),
    ("playback", Perm::SettingsManage),
    ("viewer", Perm::SettingsManage),
    ("chat", Perm::CommandsManage),
    ("auth", Perm::UsersManage),
];

/// The permissions needed to go from `before` to `after`.
pub fn required_for_change(before: &Settings, after: &Settings) -> Vec<Perm> {
    let (b, a) = (
        serde_json::to_value(before).unwrap_or_default(),
        serde_json::to_value(after).unwrap_or_default(),
    );
    let mut perms: Vec<Perm> = SECTION_PERMS
        .iter()
        .filter(|(section, _)| b.get(section) != a.get(section))
        .map(|(_, p)| *p)
        .collect();
    perms.sort();
    perms.dedup();
    perms
}

pub async fn load(db: &SqlitePool) -> Result<Settings> {
    Ok(db::kv_get_json(db, SETTINGS_KEY).await?.unwrap_or_default())
}

pub async fn save(db: &SqlitePool, settings: &Settings) -> Result<()> {
    db::kv_set_json(db, SETTINGS_KEY, settings).await
}

/// A random access key (OBS pages, bot API). Created on first use.
pub async fn access_key(db: &SqlitePool, name: &str) -> Result<String> {
    if let Some(key) = db::kv_get(db, name).await? {
        return Ok(key);
    }
    rotate_key(db, name).await
}

pub async fn rotate_key(db: &SqlitePool, name: &str) -> Result<String> {
    let key = random_hex(16);
    db::kv_set(db, name, &key).await?;
    Ok(key)
}

pub async fn youtube_api_key(db: &SqlitePool) -> Result<Option<String>> {
    Ok(db::kv_get(db, YOUTUBE_API_KEY)
        .await?
        .filter(|k| !k.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_v1_limits() {
        let s = Settings::default();
        assert_eq!(s.queue.max_queue, 30, "v1 LIMIT_SONG_NUMBER");
        assert_eq!(s.queue.max_duration_secs, 900, "v1 LIMIT_MINUTES = 15");
        assert_eq!(s.queue.skip_votes, 3, "v1 cut_over_num");
        assert_eq!(s.queue.bump_votes, 2, "v1 insert_over_num");
        assert!(s.auth.registration_open, "v1 let anyone register");
        assert_eq!(s.auth.default_role, "viewer");
    }

    #[test]
    fn each_section_needs_its_permission() {
        let before = Settings::default();
        assert!(required_for_change(&before, &before).is_empty());
        let mut after = before.clone();
        after.queue.max_queue = 5;
        after.chat.prefix = "?".into();
        assert_eq!(
            required_for_change(&before, &after),
            vec![Perm::CommandsManage, Perm::SettingsManage]
        );
        let mut after = before.clone();
        after.auth.registration_open = false;
        assert_eq!(
            required_for_change(&before, &after),
            vec![Perm::UsersManage]
        );
        // every section of the document is in the table
        let doc = serde_json::to_value(&before).unwrap();
        for key in doc.as_object().unwrap().keys() {
            assert!(
                SECTION_PERMS.iter().any(|(s, _)| s == key),
                "section {key} has no permission"
            );
        }
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"queue":{"max_queue":5}}"#).unwrap();
        assert_eq!(s.queue.max_queue, 5);
        assert_eq!(s.queue.skip_votes, 3);
        assert_eq!(s.chat.prefix, "!");
    }

    #[test]
    fn validate_rejects_bad_prefix() {
        let mut s = Settings::default();
        s.chat.prefix = " ".into();
        assert!(s.validate().is_err());
        s.chat.prefix = "!".into();
        assert!(s.validate().is_ok());
    }
}
