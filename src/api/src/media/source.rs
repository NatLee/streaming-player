//! What did the viewer type after `!sr`? A YouTube link or id, another site's link,
//! or search words (v1 searched YouTube for anything that wasn't a URL).

use std::net::IpAddr;

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    YouTube(String),
    Url(Url),
    Search(String),
}

/// Hosts that serve YouTube videos, and how each one carries the video id.
const YOUTUBE_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtube-nocookie.com",
    "www.youtube-nocookie.com",
];
/// Path prefixes followed by the id: `/shorts/<id>`, `/live/<id>`, `/embed/<id>`, `/v/<id>`.
const YOUTUBE_ID_PATHS: &[&str] = &["shorts", "live", "embed", "v"];
const SHORT_HOST: &str = "youtu.be";

pub fn is_youtube_id(s: &str) -> bool {
    s.len() == 11
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The YouTube video id in a URL, if it is a YouTube video URL.
pub fn youtube_id(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    let candidate = if host == SHORT_HOST {
        url.path_segments()?.next().map(str::to_string)
    } else if YOUTUBE_HOSTS.contains(&host.as_str()) {
        let mut segs = url.path_segments()?;
        match segs.next() {
            Some("watch") => url
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned()),
            Some(first) if YOUTUBE_ID_PATHS.contains(&first) => segs.next().map(str::to_string),
            _ => None,
        }
    } else {
        None
    };
    candidate.filter(|id| is_youtube_id(id))
}

pub fn parse_input(raw: &str) -> Option<Input> {
    // Chat clients wrap links in <…> or add trailing punctuation; strip both.
    let text = raw
        .trim()
        .trim_start_matches('<')
        .trim_end_matches(['>', ',', '，', '。']);
    if text.is_empty() {
        return None;
    }
    if is_youtube_id(text) && !text.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(Input::YouTube(text.to_string()));
    }
    let candidate = if text.contains("://") {
        Url::parse(text).ok()
    } else if !text.contains(char::is_whitespace) && text.contains('.') && text.contains('/') {
        // "youtu.be/xxxx" or "www.youtube.com/watch?v=…" without the scheme
        Url::parse(&format!("https://{text}")).ok()
    } else {
        None
    };
    match candidate {
        Some(url) if matches!(url.scheme(), "http" | "https") => Some(match youtube_id(&url) {
            Some(id) => Input::YouTube(id),
            None => Input::Url(url),
        }),
        Some(_) => None,
        None => Some(Input::Search(text.to_string())),
    }
}

/// Refuse URLs that point inside the server's own network. Chat can type any URL and
/// yt-dlp would fetch it from the server, so loopback and private addresses are off limits.
pub fn is_public_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast())
        }
        Ok(IpAddr::V6(ip)) => {
            let seg0 = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00 // unique local
                || (seg0 & 0xffc0) == 0xfe80) // link local
        }
        Err(_) => true,
    }
}

pub fn youtube_watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

pub fn youtube_thumbnail(id: &str) -> String {
    format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yt(s: &str) -> Option<String> {
        match parse_input(s) {
            Some(Input::YouTube(id)) => Some(id),
            _ => None,
        }
    }

    #[test]
    fn youtube_links_in_every_shape() {
        let id = Some("JJo-zUi9E5U".to_string());
        assert_eq!(yt("https://www.youtube.com/watch?v=JJo-zUi9E5U"), id);
        assert_eq!(
            yt("https://www.youtube.com/watch?list=x&v=JJo-zUi9E5U&t=30"),
            id
        );
        assert_eq!(yt("https://youtu.be/JJo-zUi9E5U?si=abc"), id);
        assert_eq!(yt("youtu.be/JJo-zUi9E5U"), id);
        assert_eq!(yt("https://m.youtube.com/shorts/JJo-zUi9E5U"), id);
        assert_eq!(yt("https://music.youtube.com/watch?v=JJo-zUi9E5U"), id);
        assert_eq!(yt("https://www.youtube.com/live/JJo-zUi9E5U"), id);
        assert_eq!(yt("<https://youtu.be/JJo-zUi9E5U>"), id);
        assert_eq!(yt("JJo-zUi9E5U"), id, "a bare id works like v1's ?v=");
    }

    #[test]
    fn other_sites_and_search_words() {
        assert!(matches!(
            parse_input("https://www.bilibili.com/video/BV1GJ411x7h7"),
            Some(Input::Url(_))
        ));
        assert!(matches!(
            parse_input("https://example.com/video.mp4"),
            Some(Input::Url(_))
        ));
        assert_eq!(
            parse_input("  never gonna give you up "),
            Some(Input::Search("never gonna give you up".into()))
        );
        assert_eq!(
            parse_input("abcdefghijk"),
            Some(Input::Search("abcdefghijk".into())),
            "an 11-letter word is a search, not an id"
        );
        assert_eq!(parse_input("ftp://example.com/a.mp3"), None);
        assert_eq!(parse_input("   "), None);
    }

    #[test]
    fn private_addresses_are_refused() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(!is_public_url(&u("http://localhost:7878/x")));
        assert!(!is_public_url(&u("http://127.0.0.1/x")));
        assert!(!is_public_url(&u("http://192.168.1.5/x")));
        assert!(!is_public_url(&u("http://169.254.169.254/latest")));
        assert!(!is_public_url(&u("http://[::1]/x")));
        assert!(is_public_url(&u("https://www.bilibili.com/video/x")));
        assert!(is_public_url(&u("http://8.8.8.8/x")));
    }
}
