//! Process configuration from environment variables (`SP_*`).
//! Things a streamer changes while live (queue rules, commands, chat) live in the
//! database instead, see `settings.rs`.

use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result};

/// v1 listened on 7878; keeping it means old Nightbot commands keep their URL.
pub const DEFAULT_BIND: &str = "0.0.0.0:7878";
pub const DEFAULT_PUBLIC_URL: &str = "http://localhost:7878";

/// Base URLs of the outside services. Tests point these at local fake servers.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub twitch_id: String,
    pub twitch_api: String,
    pub twitch_eventsub_ws: String,
    pub youtube_api: String,
    pub youtube_oembed: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            twitch_id: "https://id.twitch.tv".into(),
            twitch_api: "https://api.twitch.tv/helix".into(),
            twitch_eventsub_ws: "wss://eventsub.wss.twitch.tv/ws".into(),
            youtube_api: "https://www.googleapis.com/youtube/v3".into(),
            youtube_oembed: "https://www.youtube.com/oembed".into(),
        }
    }
}

/// yt-dlp, used for every site except YouTube embeds (and as YouTube's fallback).
#[derive(Clone, Debug)]
pub struct YtDlpConfig {
    /// Executable; the Docker image ships one that the admin page can update.
    pub path: String,
    /// Format selector. Prefer progressive HTTP files the browser can play directly.
    pub format: String,
    pub timeout: Duration,
}

pub const DEFAULT_YTDLP_FORMAT: &str =
    "b[protocol^=http][vcodec!=none][acodec!=none]/b[protocol^=http]/ba[protocol^=http]/b/ba";

/// Google sign-in (OAuth 2.0 web client). Redirect URI: `{public_url}/api/auth/google/callback`.
#[derive(Clone, Debug)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
}

impl GoogleConfig {
    pub fn from_env() -> Result<Option<Self>> {
        match (var("SP_GOOGLE_CLIENT_ID"), var("SP_GOOGLE_CLIENT_SECRET")) {
            (Some(client_id), Some(client_secret)) => Ok(Some(Self {
                client_id,
                client_secret,
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
            })),
            (None, None) => Ok(None),
            _ => anyhow::bail!(
                "SP_GOOGLE_CLIENT_ID and SP_GOOGLE_CLIENT_SECRET must be set together"
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub static_dir: PathBuf,
    /// The address people (and bots) use to reach this server, e.g. `https://player.example.com`.
    pub public_url: String,
    pub cookie_secure: bool,
    pub ytdlp: YtDlpConfig,
    pub google: Option<GoogleConfig>,
    /// Default Twitch application client id; the admin page can override it.
    pub twitch_client_id: Option<String>,
    pub endpoints: Endpoints,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let data_dir = PathBuf::from(var("SP_DATA_DIR").unwrap_or_else(|| "data".into()));
        let database_path = var("SP_DATABASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("streaming-player.db"));
        Ok(Self {
            bind: var("SP_BIND").unwrap_or_else(|| DEFAULT_BIND.into()),
            database_path,
            static_dir: PathBuf::from(
                var("SP_STATIC_DIR").unwrap_or_else(|| "src/web/dist".into()),
            ),
            public_url: var("SP_PUBLIC_URL")
                .unwrap_or_else(|| DEFAULT_PUBLIC_URL.into())
                .trim_end_matches('/')
                .to_string(),
            cookie_secure: var("SP_COOKIE_SECURE").map(|v| truthy(&v)).unwrap_or(false),
            ytdlp: YtDlpConfig {
                path: var("SP_YTDLP").unwrap_or_else(|| "yt-dlp".into()),
                format: var("SP_YTDLP_FORMAT").unwrap_or_else(|| DEFAULT_YTDLP_FORMAT.into()),
                timeout: Duration::from_secs(parse_var("SP_YTDLP_TIMEOUT_SECS", 60)?),
            },
            google: GoogleConfig::from_env()?,
            twitch_client_id: var("SP_TWITCH_CLIENT_ID"),
            endpoints: Endpoints::default(),
            data_dir,
        })
    }

    /// A config for tests and tools: everything under `data_dir`, services at their real URLs.
    pub fn for_data_dir(data_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            database_path: data_dir.join("streaming-player.db"),
            static_dir: data_dir.join("web"),
            public_url: DEFAULT_PUBLIC_URL.into(),
            cookie_secure: false,
            ytdlp: YtDlpConfig {
                path: "yt-dlp".into(),
                format: DEFAULT_YTDLP_FORMAT.into(),
                timeout: Duration::from_secs(60),
            },
            google: None,
            twitch_client_id: None,
            endpoints: Endpoints::default(),
            data_dir,
        }
    }
}

fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_var<T: std::str::FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match var(name) {
        Some(v) => v
            .trim()
            .parse()
            .with_context(|| format!("{name} has an invalid value: {v}")),
        None => Ok(default),
    }
}
