//! The generic chat-bot API: any bot that can fetch a URL (Nightbot `$(urlfetch)`,
//! StreamElements and Fossabot `$(customapi)`, Streamer.bot "Fetch URL")
//! calls `/api/bot/<command>?key=…` and posts the plain-text answer in chat.
//!
//! Some bots also send who is asking in headers. Those are listed in `HEADER_PROFILES`
//! (data, not one branch per bot); query parameters `user` and `level` always win.

use axum::http::HeaderMap;

use crate::models::{Level, Requester, Source};

/// How a bot describes the chatter in request headers.
enum Shape {
    /// One header holding a query string, like Nightbot's
    /// `Nightbot-User: name=natlee&displayName=NatLee&provider=twitch&userLevel=owner`.
    QueryString {
        header: &'static str,
        name: &'static str,
        login: &'static str,
        level: &'static str,
    },
    /// A header per field, like Fossabot's `x-fossabot-message-userlogin`.
    /// Fossabot sends no role header: set the permission in Fossabot, or pass `level=`.
    Headers {
        name: &'static str,
        login: &'static str,
        level: Option<&'static str>,
    },
}

const HEADER_PROFILES: &[Shape] = &[
    Shape::QueryString {
        header: "nightbot-user",
        name: "displayName",
        login: "name",
        level: "userLevel",
    },
    Shape::Headers {
        name: "x-fossabot-message-userdisplayname",
        login: "x-fossabot-message-userlogin",
        level: None,
    },
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Caller {
    pub name: Option<String>,
    pub login: Option<String>,
    pub level: Option<String>,
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Who the bot says is asking, from its headers.
pub fn from_headers(headers: &HeaderMap) -> Caller {
    for shape in HEADER_PROFILES {
        let found = match shape {
            Shape::QueryString {
                header: h,
                name,
                login,
                level,
            } => header(headers, h).map(|raw| {
                let pairs: Vec<(String, String)> = url::form_urlencoded::parse(raw.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                let get = |k: &str| pairs.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone());
                Caller {
                    name: get(name),
                    login: get(login),
                    level: get(level),
                }
            }),
            Shape::Headers { name, login, level } => {
                let c = Caller {
                    name: header(headers, name),
                    login: header(headers, login),
                    level: level.and_then(|l| header(headers, l)),
                };
                (c != Caller::default()).then_some(c)
            }
        };
        if let Some(c) = found {
            return c;
        }
    }
    Caller::default()
}

/// Build the requester: query parameters first, then headers. None without any name.
pub fn requester(
    headers: &HeaderMap,
    user: Option<&str>,
    level: Option<&str>,
) -> Option<Requester> {
    let from = from_headers(headers);
    let user = user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string);
    let name = user.clone().or(from.name).or(from.login.clone())?;
    // An explicit ?user= is its own identity; otherwise use the bot's login when given.
    let key = if user.is_some() { None } else { from.login };
    let level = level
        .map(str::to_string)
        .or(from.level)
        .map(|l| Level::parse(&l))
        .unwrap_or(Level::Everyone);
    Some(Requester::new(&name, key.as_deref(), level, Source::Bot))
}

/// Bots that don't URL-encode the arguments split a pasted link at its `&`
/// (`args=https://www.youtube.com/watch?list=PL1&v=ID`). Parameters after the value that
/// aren't ours belong to that link, so they are glued back on, in order.
pub fn rejoin(raw_query: &str, value_params: &[&str], known: &[&str]) -> Option<String> {
    let mut out: Option<String> = None;
    for pair in raw_query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let decode = |s: &str| {
            url::form_urlencoded::parse(format!("x={s}").as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        };
        let key = decode(k);
        match &mut out {
            None if value_params.contains(&key.as_str()) => out = Some(decode(v)),
            Some(value) if !known.contains(&key.as_str()) => {
                value.push('&');
                value.push_str(&decode(pair));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn rejoins_links_split_at_ampersands() {
        let known = ["key", "user", "args"];
        let q = "key=k&user=a&args=https://www.youtube.com/watch?list=PL1&v=JJo-zUi9E5U&t=30";
        assert_eq!(
            rejoin(q, &["args"], &known).as_deref(),
            Some("https://www.youtube.com/watch?list=PL1&v=JJo-zUi9E5U&t=30")
        );
        let encoded = "args=https%3A%2F%2Fyoutu.be%2Fx%3Fsi%3D1&user=a";
        assert_eq!(
            rejoin(encoded, &["args"], &known).as_deref(),
            Some("https://youtu.be/x?si=1")
        );
        assert_eq!(
            rejoin("args=%E5%B0%8F+%E5%B9%B8", &["args"], &known).as_deref(),
            Some("小 幸")
        );
        assert_eq!(rejoin("key=k&user=a", &["args"], &known), None);
    }

    #[test]
    fn nightbot_header() {
        let mut h = HeaderMap::new();
        h.insert(
            "Nightbot-User",
            HeaderValue::from_static(
                "name=natlee&displayName=NatLee&provider=twitch&providerId=1&userLevel=moderator",
            ),
        );
        let r = requester(&h, None, None).unwrap();
        assert_eq!(
            (r.name.as_str(), r.key.as_str(), r.level),
            ("NatLee", "natlee", Level::Moderator)
        );
    }

    #[test]
    fn fossabot_headers() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-fossabot-message-userlogin",
            HeaderValue::from_static("viewer1"),
        );
        h.insert(
            "x-fossabot-message-userdisplayname",
            HeaderValue::from_static("Viewer1"),
        );
        let r = requester(&h, None, None).unwrap();
        assert_eq!(
            (r.name.as_str(), r.key.as_str(), r.level),
            ("Viewer1", "viewer1", Level::Everyone)
        );
        let r = requester(&h, None, Some("vip")).unwrap();
        assert_eq!(
            r.level,
            Level::Vip,
            "level= adds the role Fossabot doesn't send"
        );
    }

    #[test]
    fn query_parameters_win_and_default_to_everyone() {
        let mut h = HeaderMap::new();
        h.insert(
            "Nightbot-User",
            HeaderValue::from_static("name=a&displayName=A&userLevel=owner"),
        );
        let r = requester(&h, Some("Someone"), Some("subscriber")).unwrap();
        assert_eq!(
            (r.name.as_str(), r.key.as_str(), r.level),
            ("Someone", "someone", Level::Subscriber)
        );
        let r = requester(&HeaderMap::new(), Some("StreamElementsUser"), None).unwrap();
        assert_eq!(r.level, Level::Everyone);
        assert!(requester(&HeaderMap::new(), None, None).is_none());
    }
}
