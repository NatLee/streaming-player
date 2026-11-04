//! Calls the yt-dlp executable. One JSON dump gives everything: title, length, page URL
//! and a direct media link (with the HTTP headers the site wants for it).

use std::{collections::HashMap, process::Stdio};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use tokio::process::Command;

use crate::config::YtDlpConfig;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Info {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub duration: Option<f64>,
    pub webpage_url: Option<String>,
    #[serde(default)]
    pub extractor_key: String,
    #[serde(default)]
    pub is_live: Option<bool>,
    pub live_status: Option<String>,
    pub thumbnail: Option<String>,
    /// Direct link of the selected format.
    pub url: Option<String>,
    pub protocol: Option<String>,
    /// `none` for audio-only formats.
    pub vcodec: Option<String>,
    #[serde(default)]
    pub http_headers: HashMap<String, String>,
    /// Search results come back as a playlist.
    #[serde(default)]
    pub entries: Vec<Info>,
}

impl Info {
    pub fn is_live(&self) -> bool {
        self.is_live == Some(true) || self.live_status.as_deref() == Some("is_live")
    }

    pub fn duration_secs(&self) -> Option<i64> {
        self.duration.filter(|d| *d > 0.0).map(|d| d.round() as i64)
    }

    /// HLS playlists need hls.js in the browser; everything else is a plain file.
    /// Sites that only offer separate video and audio tracks (Bilibili) or only audio
    /// (SoundCloud) end up with an audio-only format: browsers can't join two tracks.
    pub fn is_audio_only(&self) -> bool {
        self.vcodec.as_deref() == Some("none")
    }

    pub fn is_hls(&self) -> bool {
        self.protocol
            .as_deref()
            .is_some_and(|p| p.starts_with("m3u8"))
            || self.url.as_deref().is_some_and(|u| u.contains(".m3u8"))
    }
}

/// Parse yt-dlp's `--dump-single-json` output. A search returns its first entry.
pub fn parse(json: &str) -> Result<Info> {
    let mut info: Info =
        serde_json::from_str(json).context("yt-dlp printed something that isn't JSON")?;
    if !info.entries.is_empty() {
        info = info.entries.remove(0);
    } else if info.id.is_empty() && info.title.is_empty() {
        bail!("找不到符合的影片");
    }
    Ok(info)
}

/// Look up a URL (or `ytsearch1:words`).
pub async fn probe(cfg: &YtDlpConfig, target: &str) -> Result<Info> {
    let mut cmd = Command::new(&cfg.path);
    cmd.args([
        "--dump-single-json",
        "--no-playlist",
        "--skip-download",
        "--no-warnings",
        "--no-progress",
        "-f",
        &cfg.format,
        // "--" ends the options: whatever chat typed is only ever a URL, never a flag.
        "--",
        target,
    ]);
    let out = run(cmd, cfg).await?;
    parse(&out)
}

pub async fn version(cfg: &YtDlpConfig) -> Result<String> {
    let mut cmd = Command::new(&cfg.path);
    cmd.arg("--version");
    Ok(run(cmd, cfg).await?.trim().to_string())
}

/// Self-update to the newest stable release. The executable must be writable.
pub async fn update(cfg: &YtDlpConfig) -> Result<String> {
    let mut cmd = Command::new(&cfg.path);
    cmd.args(["--update-to", "stable"]);
    run(cmd, cfg).await
}

async fn run(mut cmd: Command, cfg: &YtDlpConfig) -> Result<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd
        .spawn()
        .with_context(|| format!("找不到 yt-dlp（{}）", cfg.path))?;
    let out = tokio::time::timeout(cfg.timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow!("yt-dlp 超過 {} 秒沒有回應", cfg.timeout.as_secs()))??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| err.lines().last())
            .unwrap_or("yt-dlp failed");
        bail!("{}", line.trim().trim_start_matches("ERROR:").trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_video() {
        let json = r#"{"id":"BV1x","title":"測試","duration":215.4,"webpage_url":"https://www.bilibili.com/video/BV1x",
            "extractor_key":"BiliBili","url":"https://cdn.example/v.mp4","protocol":"https",
            "http_headers":{"Referer":"https://www.bilibili.com/"},"thumbnail":"https://i/x.jpg"}"#;
        let info = parse(json).unwrap();
        assert_eq!(info.title, "測試");
        assert_eq!(info.duration_secs(), Some(215));
        assert_eq!(
            info.http_headers.get("Referer").unwrap(),
            "https://www.bilibili.com/"
        );
        assert!(!info.is_hls());
        assert!(!info.is_live());
        assert!(!info.is_audio_only());
        let audio =
            parse(r#"{"id":"x","title":"t","vcodec":"none","acodec":"mp4a.40.2"}"#).unwrap();
        assert!(audio.is_audio_only());
    }

    #[test]
    fn search_returns_the_first_entry() {
        let json = r#"{"_type":"playlist","entries":[{"id":"abcDEF12345","title":"first","extractor_key":"Youtube"},{"id":"x","title":"second"}]}"#;
        assert_eq!(parse(json).unwrap().title, "first");
        assert!(parse(r#"{"_type":"playlist","entries":[]}"#).is_err());
    }

    #[test]
    fn detects_hls_and_live() {
        let info =
            parse(r#"{"id":"x","title":"t","protocol":"m3u8_native","live_status":"is_live"}"#)
                .unwrap();
        assert!(info.is_hls());
        assert!(info.is_live());
    }
}
