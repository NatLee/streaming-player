//! Playing yt-dlp links in the browser.
//!
//! Direct links are often tied to the server's IP, need the site's Referer, or lack CORS
//! headers, so plain files are streamed through this server (with Range support for
//! seeking). HLS playlists are handed to the browser as they are, because their segments
//! are separate requests that a single proxy URL can't cover.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use anyhow::{bail, Result};
use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use serde::Serialize;
use utoipa::ToSchema;

use super::{source, ytdlp};
use crate::{
    models::{Song, KIND_YOUTUBE},
    state::Inner,
    util::now,
};

/// yt-dlp links usually last a few hours; refresh well before that.
const LINK_TTL_SECS: i64 = 20 * 60;
/// Response headers worth passing on to the browser.
const PASS_HEADERS: &[header::HeaderName] = &[
    header::CONTENT_TYPE,
    header::CONTENT_LENGTH,
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::LAST_MODIFIED,
    header::ETAG,
];

#[derive(Debug, Clone)]
pub struct Playable {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub hls: bool,
    pub audio_only: bool,
}

pub struct StreamCache {
    links: Mutex<HashMap<i64, (Playable, i64)>>,
    /// No overall timeout: a song streams for minutes.
    client: reqwest::Client,
}

impl Default for StreamCache {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            links: Mutex::new(HashMap::new()),
            client,
        }
    }
}

impl StreamCache {
    fn get(&self, song_id: i64) -> Option<Playable> {
        let links = self.links.lock().unwrap();
        links
            .get(&song_id)
            .filter(|(_, expires)| *expires > now())
            .map(|(p, _)| p.clone())
    }

    fn put(&self, song_id: i64, p: Playable) {
        let mut links = self.links.lock().unwrap();
        let t = now();
        links.retain(|_, (_, expires)| *expires > t);
        links.insert(song_id, (p, t + LINK_TTL_SECS));
    }

    pub fn forget(&self, song_id: i64) {
        self.links.lock().unwrap().remove(&song_id);
    }
}

fn target(song: &Song) -> String {
    if song.kind == KIND_YOUTUBE {
        source::youtube_watch_url(&song.source_id)
    } else {
        song.source_id.clone()
    }
}

pub async fn playable(state: &Inner, song: &Song) -> Result<Playable> {
    if let Some(p) = state.streams.get(song.id) {
        return Ok(p);
    }
    let info = ytdlp::probe(&state.config.ytdlp, &target(song)).await?;
    let Some(url) = info.url.clone() else {
        bail!("yt-dlp 沒有給出可以直接播放的連結");
    };
    let p = Playable {
        hls: info.is_hls(),
        audio_only: info.is_audio_only(),
        headers: info.http_headers.into_iter().collect(),
        url,
    };
    state.streams.put(song.id, p.clone());
    Ok(p)
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlaySource {
    /// `file` (play `url` in a video element) or `hls` (play with hls.js)
    pub kind: String,
    pub url: String,
    /// No picture: the player shows the thumbnail instead.
    pub audio_only: bool,
}

pub async fn play_source(state: &Inner, song: &Song) -> Result<PlaySource> {
    let p = playable(state, song).await?;
    Ok(if p.hls {
        PlaySource {
            kind: "hls".into(),
            url: p.url,
            audio_only: p.audio_only,
        }
    } else {
        PlaySource {
            kind: "file".into(),
            url: format!("/api/media/{}/stream", song.id),
            audio_only: p.audio_only,
        }
    })
}

async fn fetch(
    state: &Inner,
    p: &Playable,
    range: Option<&HeaderValue>,
) -> Result<reqwest::Response> {
    let mut req = state.streams.client.get(&p.url);
    for (k, v) in &p.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if let Some(r) = range {
        req = req.header(header::RANGE, r.clone());
    }
    Ok(req.send().await?)
}

/// Stream the media file through this server.
pub async fn proxy(state: &Inner, song: &Song, headers: &HeaderMap) -> Result<Response> {
    let range = headers.get(header::RANGE);
    let mut p = playable(state, song).await?;
    let mut resp = fetch(state, &p, range).await?;
    if matches!(resp.status().as_u16(), 403 | 404 | 410) {
        // The link expired early; ask yt-dlp for a fresh one once.
        state.streams.forget(song.id);
        p = playable(state, song).await?;
        resp = fetch(state, &p, range).await?;
    }
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        bail!("影片來源回應 HTTP {status}");
    }
    let mut builder = Response::builder().status(status);
    for name in PASS_HEADERS {
        if let Some(v) = resp.headers().get(name.as_str()) {
            if let Ok(v) = HeaderValue::from_bytes(v.as_bytes()) {
                builder = builder.header(name, v);
            }
        }
    }
    Ok(builder
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(resp.bytes_stream()))?)
}
