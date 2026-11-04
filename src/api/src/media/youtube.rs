//! YouTube Data API v3 (needs an API key) and oEmbed (no key, title only).
//! `videos.list` costs 1 quota unit per call.

use anyhow::{bail, Result};
use serde_json::Value;

use crate::config::Endpoints;

#[derive(Debug, Clone, PartialEq)]
pub struct VideoDetails {
    pub title: String,
    pub duration_secs: Option<i64>,
    pub thumbnail: Option<String>,
    pub live: bool,
    pub embeddable: bool,
}

/// `PT1H2M3S` / `P1DT2H` → seconds. YouTube reports live streams as `P0D`.
pub fn parse_iso8601_duration(s: &str) -> Option<i64> {
    let rest = s.strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    fn sum(part: &str, units: &[(char, i64)]) -> Option<i64> {
        let mut total = 0;
        let mut num = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                let mult = units.iter().find(|(u, _)| *u == c)?.1;
                total += num.parse::<i64>().ok()? * mult;
                num.clear();
            }
        }
        num.is_empty().then_some(total)
    }
    let days = sum(date, &[('W', 7 * 86400), ('D', 86400)])?;
    let time = sum(time, &[('H', 3600), ('M', 60), ('S', 1)])?;
    Some(days + time)
}

pub async fn video_details(
    http: &reqwest::Client,
    ep: &Endpoints,
    key: &str,
    video_id: &str,
) -> Result<Option<VideoDetails>> {
    let resp = http
        .get(format!("{}/videos", ep.youtube_api))
        .query(&[
            ("part", "snippet,contentDetails,status"),
            ("id", video_id),
            ("key", key),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await?;
    if !status.is_success() {
        bail!("YouTube API HTTP {status}: {}", api_error(&body));
    }
    let Some(item) = body["items"].get(0) else {
        return Ok(None);
    };
    let snippet = &item["snippet"];
    let thumbs = &snippet["thumbnails"];
    let thumbnail = ["medium", "high", "default"]
        .iter()
        .find_map(|k| thumbs[k]["url"].as_str())
        .map(str::to_string);
    let live = snippet["liveBroadcastContent"].as_str() == Some("live");
    Ok(Some(VideoDetails {
        title: snippet["title"].as_str().unwrap_or_default().to_string(),
        duration_secs: item["contentDetails"]["duration"]
            .as_str()
            .and_then(parse_iso8601_duration)
            .filter(|d| *d > 0),
        thumbnail,
        live,
        embeddable: item["status"]["embeddable"].as_bool().unwrap_or(true),
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OEmbed {
    Found {
        title: String,
        thumbnail: Option<String>,
    },
    /// 401/403: the uploader turned embedding off.
    NotEmbeddable,
    /// 400/404: no such public video.
    NotFound,
}

/// Title and thumbnail without an API key, from YouTube's official oEmbed endpoint.
/// The length isn't included; the player reports it once the video plays.
pub async fn oembed(http: &reqwest::Client, ep: &Endpoints, watch_url: &str) -> Result<OEmbed> {
    let resp = http
        .get(&ep.youtube_oembed)
        .query(&[("url", watch_url), ("format", "json")])
        .send()
        .await?;
    match resp.status().as_u16() {
        401 | 403 => return Ok(OEmbed::NotEmbeddable),
        400 | 404 => return Ok(OEmbed::NotFound),
        _ => {}
    }
    let body: Value = resp.error_for_status()?.json().await?;
    Ok(OEmbed::Found {
        title: body["title"].as_str().unwrap_or_default().to_string(),
        thumbnail: body["thumbnail_url"].as_str().map(str::to_string),
    })
}

/// The chat id of a live broadcast (1 quota unit). None when the video isn't live.
pub async fn active_live_chat_id(
    http: &reqwest::Client,
    ep: &Endpoints,
    key: &str,
    video_id: &str,
) -> Result<Option<String>> {
    let resp = http
        .get(format!("{}/videos", ep.youtube_api))
        .query(&[
            ("part", "liveStreamingDetails"),
            ("id", video_id),
            ("key", key),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body: Value = resp.json().await?;
    if !status.is_success() {
        bail!("YouTube API HTTP {status}: {}", api_error(&body));
    }
    Ok(body["items"][0]["liveStreamingDetails"]["activeLiveChatId"]
        .as_str()
        .map(str::to_string))
}

pub fn api_error(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .or_else(|| body["error"]["errors"][0]["reason"].as_str())
        .unwrap_or("unknown error")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_durations() {
        assert_eq!(parse_iso8601_duration("PT4M13S"), Some(253));
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3723));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45));
        assert_eq!(parse_iso8601_duration("P1DT1S"), Some(86401));
        assert_eq!(parse_iso8601_duration("P0D"), Some(0));
        assert_eq!(parse_iso8601_duration("4M"), None);
        assert_eq!(parse_iso8601_duration("PT4X"), None);
    }
}
