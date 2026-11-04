//! Turn what a viewer typed into a song in the library.
//!
//! YouTube: the Data API when an API key is set (1 quota unit, gives the length),
//! otherwise oEmbed (title only; the player reports the length). yt-dlp only as a last
//! resort, because YouTube blocks frequent scraping. Everything else: yt-dlp.

use anyhow::Result;
use sqlx::SqlitePool;
use url::Url;

use super::{
    source::{self, Input},
    youtube::{self, OEmbed},
    ytdlp,
};
use crate::{
    models::{Song, KIND_MEDIA, KIND_YOUTUBE},
    settings,
    state::Inner,
    util::now,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub kind: &'static str,
    pub media_key: String,
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub duration_secs: Option<i64>,
    pub thumbnail: Option<String>,
    pub site: String,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    Empty,
    /// A link into the server's own network.
    NotAllowed,
    /// A live stream never ends, so it can't be queued.
    Live,
    NotFound(String),
}

impl ResolveError {
    pub fn reason(&self) -> String {
        match self {
            ResolveError::Empty => "沒有網址".into(),
            ResolveError::NotAllowed => "不能點內部網路的網址".into(),
            ResolveError::Live => "直播不能點".into(),
            ResolveError::NotFound(r) => r.clone(),
        }
    }
}

pub const YOUTUBE_SITE: &str = "YouTube";
const YOUTUBE_EXTRACTOR: &str = "Youtube";

fn youtube_key(id: &str) -> String {
    format!("youtube:{id}")
}

fn from_ytdlp(info: ytdlp::Info, fallback_url: &str) -> Resolved {
    let live = info.is_live();
    let duration_secs = info.duration_secs();
    if info.extractor_key == YOUTUBE_EXTRACTOR && source::is_youtube_id(&info.id) {
        return Resolved {
            kind: KIND_YOUTUBE,
            media_key: youtube_key(&info.id),
            url: source::youtube_watch_url(&info.id),
            thumbnail: Some(source::youtube_thumbnail(&info.id)),
            source_id: info.id,
            duration_secs,
            live,
            title: info.title,
            site: YOUTUBE_SITE.into(),
        };
    }
    let page = info
        .webpage_url
        .clone()
        .unwrap_or_else(|| fallback_url.to_string());
    let site = match info.extractor_key.as_str() {
        "" | "Generic" => Url::parse(&page)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "link".into()),
        other => other.to_string(),
    };
    let media_key = if info.id.is_empty() {
        page.clone()
    } else {
        format!("{}:{}", info.extractor_key.to_lowercase(), info.id)
    };
    Resolved {
        kind: KIND_MEDIA,
        media_key,
        source_id: page.clone(),
        url: page,
        duration_secs,
        live,
        title: if info.title.is_empty() {
            "(untitled)".into()
        } else {
            info.title
        },
        thumbnail: info.thumbnail,
        site,
    }
}

async fn resolve_youtube(state: &Inner, id: &str) -> Result<Resolved, ResolveError> {
    let watch = source::youtube_watch_url(id);
    let base =
        |title: String, duration_secs: Option<i64>, thumbnail: Option<String>, live: bool| {
            Resolved {
                kind: KIND_YOUTUBE,
                media_key: youtube_key(id),
                source_id: id.to_string(),
                url: watch.clone(),
                title,
                duration_secs,
                thumbnail: thumbnail.or_else(|| Some(source::youtube_thumbnail(id))),
                site: YOUTUBE_SITE.into(),
                live,
            }
        };

    let ep = &state.config.endpoints;
    if let Ok(Some(key)) = settings::youtube_api_key(&state.db).await {
        match youtube::video_details(&state.http, ep, &key, id).await {
            Ok(Some(d)) => return Ok(base(d.title, d.duration_secs, d.thumbnail, d.live)),
            Ok(None) => return Err(ResolveError::NotFound("找不到這部影片".into())),
            Err(err) => tracing::warn!("YouTube Data API failed, trying oEmbed: {err:#}"),
        }
    }
    // YouTube rate-limits scrapers (it asks to "confirm you're not a bot"), so yt-dlp is
    // the last resort: only when oEmbed can't answer, or for a video that can't be
    // embedded and might still play from its direct link.
    let fallback = state.settings().playback.stream_fallback;
    match youtube::oembed(&state.http, ep, &watch).await {
        Ok(OEmbed::Found { title, thumbnail }) => return Ok(base(title, None, thumbnail, false)),
        Ok(OEmbed::NotFound) => return Err(ResolveError::NotFound("找不到這部影片".into())),
        Ok(OEmbed::NotEmbeddable) if !fallback => {
            return Err(ResolveError::NotFound("作者不允許嵌入播放".into()))
        }
        Ok(OEmbed::NotEmbeddable) => {}
        Err(err) => tracing::warn!("YouTube oEmbed failed, trying yt-dlp: {err:#}"),
    }
    match ytdlp::probe(&state.config.ytdlp, &watch).await {
        Ok(info) => Ok(from_ytdlp(info, &watch)),
        Err(err) => Err(ResolveError::NotFound(err.to_string())),
    }
}

/// A song from a known link and title without asking any website (library imports).
/// None for text that isn't a link, or links into the private network.
pub fn offline(url: &str, title: &str, duration_secs: Option<i64>) -> Option<Resolved> {
    let title = title.trim().to_string();
    match source::parse_input(url)? {
        Input::YouTube(id) => Some(Resolved {
            kind: KIND_YOUTUBE,
            media_key: youtube_key(&id),
            url: source::youtube_watch_url(&id),
            thumbnail: Some(source::youtube_thumbnail(&id)),
            source_id: id,
            title,
            duration_secs,
            site: YOUTUBE_SITE.into(),
            live: false,
        }),
        Input::Url(u) if source::is_public_url(&u) => Some(Resolved {
            kind: KIND_MEDIA,
            media_key: u.to_string(),
            source_id: u.to_string(),
            url: u.to_string(),
            title,
            duration_secs,
            thumbnail: None,
            site: u.host_str().unwrap_or("link").to_string(),
            live: false,
        }),
        _ => None,
    }
}

pub async fn resolve(state: &Inner, raw: &str) -> Result<Resolved, ResolveError> {
    let r = resolve_any(state, raw).await?;
    if r.live {
        return Err(ResolveError::Live);
    }
    Ok(r)
}

async fn resolve_any(state: &Inner, raw: &str) -> Result<Resolved, ResolveError> {
    match source::parse_input(raw).ok_or(ResolveError::Empty)? {
        Input::YouTube(id) => resolve_youtube(state, &id).await,
        Input::Url(url) => {
            if !source::is_public_url(&url) {
                return Err(ResolveError::NotAllowed);
            }
            let info = ytdlp::probe(&state.config.ytdlp, url.as_str())
                .await
                .map_err(|e| ResolveError::NotFound(e.to_string()))?;
            Ok(from_ytdlp(info, url.as_str()))
        }
        Input::Search(words) => {
            let info = ytdlp::probe(&state.config.ytdlp, &format!("ytsearch1:{words}"))
                .await
                .map_err(|e| ResolveError::NotFound(e.to_string()))?;
            Ok(from_ytdlp(info, ""))
        }
    }
}

/// Library lookup that skips the network for songs we already know.
pub async fn known_song(db: &SqlitePool, raw: &str) -> Result<Option<Song>> {
    let key = match source::parse_input(raw) {
        Some(Input::YouTube(id)) => youtube_key(&id),
        Some(Input::Url(url)) => url.to_string(),
        _ => return Ok(None),
    };
    Ok(sqlx::query_as(
        "SELECT * FROM songs WHERE (media_key = ?1 OR url = ?1) AND title <> '' LIMIT 1",
    )
    .bind(key)
    .fetch_optional(db)
    .await?)
}

/// Insert or refresh a song; library flags (favorite, blocked, missing) are kept.
pub async fn upsert(db: &SqlitePool, r: &Resolved) -> Result<Song> {
    Ok(sqlx::query_as(
        "INSERT INTO songs (kind, media_key, source_id, url, title, duration_secs, thumbnail, site, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(media_key) DO UPDATE SET title = excluded.title, url = excluded.url, \
           source_id = excluded.source_id, \
           duration_secs = COALESCE(excluded.duration_secs, songs.duration_secs), \
           thumbnail = COALESCE(excluded.thumbnail, songs.thumbnail), site = excluded.site \
         RETURNING *",
    )
    .bind(r.kind)
    .bind(&r.media_key)
    .bind(&r.source_id)
    .bind(&r.url)
    .bind(&r.title)
    .bind(r.duration_secs)
    .bind(&r.thumbnail)
    .bind(&r.site)
    .bind(now())
    .fetch_one(db)
    .await?)
}

/// The library song for what a viewer typed, looking it up online only when it's new.
pub async fn song_for(state: &Inner, raw: &str) -> Result<Result<Song, ResolveError>> {
    if let Some(song) = known_song(&state.db, raw).await? {
        return Ok(Ok(song));
    }
    match resolve(state, raw).await {
        Ok(r) => Ok(Ok(upsert(&state.db, &r).await?)),
        Err(e) => Ok(Err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_from_ytdlp_uses_the_embed_kind() {
        let info = ytdlp::parse(r#"{"id":"JJo-zUi9E5U","title":"t","duration":61,"extractor_key":"Youtube","webpage_url":"https://www.youtube.com/watch?v=JJo-zUi9E5U"}"#).unwrap();
        let r = from_ytdlp(info, "");
        assert_eq!(r.kind, KIND_YOUTUBE);
        assert_eq!(r.media_key, "youtube:JJo-zUi9E5U");
        assert_eq!(r.duration_secs, Some(61));
    }

    #[test]
    fn other_sites_keep_their_page() {
        let info = ytdlp::parse(r#"{"id":"123","title":"track","duration":30,"extractor_key":"Soundcloud","webpage_url":"https://soundcloud.com/a/b"}"#).unwrap();
        let r = from_ytdlp(info, "https://soundcloud.com/a/b?x=1");
        assert_eq!(r.kind, KIND_MEDIA);
        assert_eq!(r.media_key, "soundcloud:123");
        assert_eq!(r.source_id, "https://soundcloud.com/a/b");
        assert_eq!(r.site, "Soundcloud");

        let info =
            ytdlp::parse(r#"{"id":"video","title":"video","extractor_key":"Generic"}"#).unwrap();
        let r = from_ytdlp(info, "https://cdn.example.com/video.mp4");
        assert_eq!(r.site, "cdn.example.com");
        assert_eq!(r.duration_secs, None);
    }
}
