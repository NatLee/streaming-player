//! Shared data types: who is asking (level, source) and what is playing (songs, queue entries).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Chat permission level, lowest first. Every platform's roles map onto these.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Everyone,
    Subscriber,
    Vip,
    Moderator,
    Broadcaster,
}

/// Role names used by Twitch badges, YouTube flags and the bots' user-level headers.
/// One table instead of a branch per platform; unknown names mean "everyone".
const LEVEL_ALIASES: &[(&str, Level)] = &[
    ("broadcaster", Level::Broadcaster),
    ("owner", Level::Broadcaster),
    ("streamer", Level::Broadcaster),
    ("admin", Level::Broadcaster),
    ("moderator", Level::Moderator),
    ("mod", Level::Moderator),
    ("vip", Level::Vip),
    ("twitch_vip", Level::Vip),
    ("regular", Level::Vip),
    ("subscriber", Level::Subscriber),
    ("sub", Level::Subscriber),
    ("member", Level::Subscriber),
    ("sponsor", Level::Subscriber),
    ("founder", Level::Subscriber),
    ("everyone", Level::Everyone),
];

impl Level {
    pub fn from_alias(name: &str) -> Option<Level> {
        let name = name.trim().to_ascii_lowercase();
        LEVEL_ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .map(|(_, level)| *level)
    }

    /// The highest level among several role names (e.g. all Twitch badges of a chatter).
    pub fn highest<'a>(names: impl IntoIterator<Item = &'a str>) -> Level {
        names
            .into_iter()
            .filter_map(Level::from_alias)
            .max()
            .unwrap_or(Level::Everyone)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Everyone => "everyone",
            Level::Subscriber => "subscriber",
            Level::Vip => "vip",
            Level::Moderator => "moderator",
            Level::Broadcaster => "broadcaster",
        }
    }

    pub fn parse(s: &str) -> Level {
        Level::from_alias(s).unwrap_or(Level::Everyone)
    }
}

/// Where a request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Twitch,
    Youtube,
    /// The generic chat-bot HTTP API (Nightbot, StreamElements, Fossabot, Streamer.bot…)
    Bot,
    /// Someone signed in to the web page
    Web,
    /// Picked from the default playlist because the queue was empty
    Autoplay,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Twitch => "twitch",
            Source::Youtube => "youtube",
            Source::Bot => "bot",
            Source::Web => "web",
            Source::Autoplay => "autoplay",
        }
    }
}

/// The person behind a chat command or a web action.
#[derive(Debug, Clone)]
pub struct Requester {
    /// Name to show, e.g. "NatLee"
    pub name: String,
    /// Stable lower-case identity for limits and ownership, e.g. "natlee" or "yt:UC…"
    pub key: String,
    pub level: Level,
    pub source: Source,
}

impl Requester {
    pub fn new(name: &str, key: Option<&str>, level: Level, source: Source) -> Self {
        let name = name.trim();
        let key = key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .unwrap_or(name)
            .to_lowercase();
        Self {
            name: name.to_string(),
            key,
            level,
            source,
        }
    }

    pub fn privileged(&self) -> bool {
        self.level >= Level::Moderator
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct Song {
    pub id: i64,
    /// `youtube` or `media`
    pub kind: String,
    pub media_key: String,
    /// YouTube video id, or the page URL for yt-dlp
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub duration_secs: Option<i64>,
    pub thumbnail: Option<String>,
    pub site: String,
    pub favorite: bool,
    pub blocked: bool,
    pub missing: bool,
    pub fail_count: i64,
    pub created_at: i64,
    pub last_played_at: Option<i64>,
}

pub const KIND_YOUTUBE: &str = "youtube";
pub const KIND_MEDIA: &str = "media";

/// One request joined with its song.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct Entry {
    #[sqlx(rename = "request_id")]
    pub id: i64,
    pub requester: String,
    pub requester_key: String,
    pub source: String,
    pub status: String,
    pub autoplay: bool,
    pub requested_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    #[sqlx(flatten)]
    pub song: Song,
}

/// Columns for `Entry`; use with `FROM requests r JOIN songs s ON s.id = r.song_id`.
pub const ENTRY_COLUMNS: &str = "r.id AS request_id, r.requester, r.requester_key, r.source, \
     r.status, r.autoplay, r.requested_at, r.started_at, r.ended_at, \
     s.id, s.kind, s.media_key, s.source_id, s.url, s.title, s.duration_secs, s.thumbnail, \
     s.site, s.favorite, s.blocked, s.missing, s.fail_count, s.created_at, s.last_played_at";

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_PLAYING: &str = "playing";

/// How a request left the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EndReason {
    Played,
    Skipped,
    Removed,
    Failed,
}

impl EndReason {
    pub fn as_status(self) -> &'static str {
        match self {
            EndReason::Played => "played",
            EndReason::Skipped => "skipped",
            EndReason::Removed => "removed",
            EndReason::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered() {
        assert!(Level::Broadcaster > Level::Moderator);
        assert!(Level::Moderator > Level::Vip);
        assert!(Level::Vip > Level::Subscriber);
        assert!(Level::Subscriber > Level::Everyone);
    }

    #[test]
    fn highest_badge_wins() {
        assert_eq!(
            Level::highest(["subscriber", "moderator"]),
            Level::Moderator
        );
        assert_eq!(Level::highest(["sub-gifter", "premium"]), Level::Everyone);
        assert_eq!(
            Level::highest(["broadcaster", "subscriber"]),
            Level::Broadcaster
        );
        assert_eq!(Level::highest(Vec::<&str>::new()), Level::Everyone);
    }

    #[test]
    fn bot_level_names_map() {
        // Nightbot userLevel values
        assert_eq!(Level::parse("owner"), Level::Broadcaster);
        assert_eq!(Level::parse("twitch_vip"), Level::Vip);
        assert_eq!(Level::parse("regular"), Level::Vip);
        assert_eq!(Level::parse("Moderator"), Level::Moderator);
        assert_eq!(Level::parse("nonsense"), Level::Everyone);
    }

    #[test]
    fn requester_key_defaults_to_lowercase_name() {
        let r = Requester::new(" NatLee ", None, Level::Everyone, Source::Bot);
        assert_eq!(r.name, "NatLee");
        assert_eq!(r.key, "natlee");
        let r = Requester::new("Nat", Some("yt:UCabc"), Level::Everyone, Source::Youtube);
        assert_eq!(r.key, "yt:ucabc");
    }
}
