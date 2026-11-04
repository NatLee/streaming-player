//! Test harness: a real server on a random port, a temporary database, and a fake yt-dlp.
#![allow(dead_code)]

use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use futures_util::{SinkExt, StreamExt};
use reqwest::{header, Method, StatusCode};
use serde_json::{json, Value};
use streaming_player::{build_state, chat, config::Config, service, AppState};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Stands in for yt-dlp. It answers by the shape of the last argument:
/// YouTube watch URLs (ids starting with "long" are 2000 s, others 200 s), `ytsearch1:`,
/// "missing" (fails like a 404), "live" (a live stream), anything else (a 30 s clip).
/// The direct media URL it reports is read from `media_url` next to it.
const FAKE_YTDLP: &str = r#"#!/bin/sh
for last; do :; done
case "$1" in
  --version) echo 2026.10.26; exit 0;;
  --update-to) echo "yt-dlp is up to date (stable@2026.10.26)"; exit 0;;
esac
dir=$(dirname "$0")
echo "$last" >> "$dir/calls.log"
media=$(cat "$dir/media_url" 2>/dev/null)
case "$last" in
  *missing*) echo "ERROR: [generic] Unable to download webpage: HTTP Error 404: Not Found" >&2; exit 1;;
  ytsearch1:*) echo '{"_type":"playlist","entries":[{"id":"srchAAAAAAA","title":"Search hit","duration":180,"extractor_key":"Youtube","webpage_url":"https://www.youtube.com/watch?v=srchAAAAAAA"}]}';;
  *youtube.com/watch\?v=*)
    id=${last##*v=}; dur=200
    case "$id" in long*) dur=2000;; esac
    printf '{"id":"%s","title":"Video %s","duration":%s,"extractor_key":"Youtube","webpage_url":"https://www.youtube.com/watch?v=%s","url":"%s","protocol":"https"}' "$id" "$id" "$dur" "$id" "$media";;
  *live*) echo '{"id":"live1","title":"Live now","extractor_key":"Generic","is_live":true}';;
  *) printf '{"id":"clip1","title":"Clip","duration":30,"extractor_key":"Generic","webpage_url":"%s","url":"%s","protocol":"https","http_headers":{"Referer":"https://media.example.com/"}}' "$last" "$media";;
esac
"#;

pub struct TestApp {
    pub base: String,
    pub state: AppState,
    pub dir: TempDir,
    pub http: reqwest::Client,
    /// Session cookie of the signed-in admin.
    pub cookie: String,
}

pub fn yt(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

pub async fn start() -> TestApp {
    start_with(|_| {}).await
}

pub async fn start_with(edit: impl FnOnce(&mut Config)) -> TestApp {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn,streaming_player=info")
        .with_test_writer()
        .try_init();
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("yt-dlp");
    std::fs::write(&fake, FAKE_YTDLP).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = Config::for_data_dir(dir.path().join("data"));
    config.ytdlp.path = fake.display().to_string();
    config.ytdlp.timeout = Duration::from_secs(10);
    // Nothing in the tests may reach the real YouTube.
    config.endpoints.youtube_oembed = "http://127.0.0.1:9/oembed".into();
    config.endpoints.youtube_api = "http://127.0.0.1:9/youtube".into();
    edit(&mut config);
    let state = build_state(config).await.unwrap();
    chat::restart(&state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let svc = service(state.clone());
    tokio::spawn(async move { axum::serve(listener, svc).await.unwrap() });
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut t = TestApp {
        base,
        state,
        dir,
        http,
        cookie: String::new(),
    };
    let (status, _) = t
        .call(
            Method::POST,
            "/api/auth/setup",
            Some(json!({"username": "admin", "password": "password123"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    t
}

impl TestApp {
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn set_media_url(&self, url: &str) {
        std::fs::write(self.path("media_url"), url).unwrap();
    }

    pub fn ytdlp_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// JSON call as the admin. Remembers the session cookie from setup/login.
    pub async fn call(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let cookie = self.cookie.clone();
        let (status, headers, text) = self.raw(method, path, body, Some(&cookie)).await;
        if let Some(c) = headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(pair) = c.split(';').next() {
                if pair.starts_with("sp_session=") {
                    self.cookie = pair.to_string();
                }
            }
        }
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    pub async fn raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> (StatusCode, header::HeaderMap, String) {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(c) = cookie.filter(|c| !c.is_empty()) {
            req = req.header(header::COOKIE, c);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        (status, headers, resp.text().await.unwrap())
    }

    pub async fn get_text(&self, path: &str) -> (StatusCode, String) {
        let (s, _, t) = self.raw(Method::GET, path, None, None).await;
        (s, t)
    }

    pub async fn bot_key(&mut self) -> String {
        let (_, s) = self.call(Method::GET, "/api/settings", None).await;
        s["bot_key"].as_str().unwrap().to_string()
    }

    pub async fn obs_key(&mut self) -> String {
        let (_, s) = self.call(Method::GET, "/api/settings", None).await;
        s["obs_key"].as_str().unwrap().to_string()
    }

    /// `/api/bot/<cmd>` as chatter `user`, returns (status, reply text).
    pub async fn bot(&mut self, cmd: &str, user: &str, args: &str) -> (StatusCode, String) {
        let key = self.bot_key().await;
        let url = reqwest::Url::parse_with_params(
            &format!("{}/api/bot/{cmd}", self.base),
            &[("key", key.as_str()), ("user", user), ("args", args)],
        )
        .unwrap();
        let resp = self.http.get(url).send().await.unwrap();
        (resp.status(), resp.text().await.unwrap())
    }

    pub async fn settings(&mut self) -> Value {
        self.call(Method::GET, "/api/settings", None).await.1["settings"].clone()
    }

    pub async fn put_settings(&mut self, edit: impl FnOnce(&mut Value)) {
        let mut s = self.settings().await;
        edit(&mut s);
        let (status, body) = self.call(Method::PUT, "/api/settings", Some(s)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    pub async fn queue(&mut self) -> Value {
        self.call(Method::GET, "/api/queue", None).await.1
    }

    /// Poll the queue until `pred` holds (10 s at most).
    pub async fn wait_until(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let q = self.queue().await;
            if pred(&q) {
                return q;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "queue never reached the expected state: {}",
            self.queue().await
        );
    }

    pub async fn ws(&mut self, role: &str) -> Ws {
        let key = self.obs_key().await;
        let url = format!(
            "{}/api/ws?role={role}&key={key}",
            self.base.replace("http", "ws")
        );
        let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        Ws { ws }
    }
}

pub struct Ws {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Ws {
    /// Next JSON message whose `type` matches and that passes `pred`.
    pub async fn expect(&mut self, kind: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let msg = tokio::time::timeout_at(deadline, self.ws.next())
                .await
                .unwrap_or_else(|_| panic!("no {kind} message in time"))
                .expect("socket closed")
                .unwrap();
            if let Message::Text(t) = msg {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["type"] == kind && pred(&v) {
                    return v;
                }
            }
        }
    }

    pub async fn send(&mut self, v: Value) {
        self.ws
            .send(Message::Text(v.to_string().into()))
            .await
            .unwrap();
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
