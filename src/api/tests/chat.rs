//! Chat platforms against fake servers: Twitch (device sign-in, EventSub WebSocket,
//! Helix) and YouTube (Data API + live chat polling). No real accounts are involved.

mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Form, Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use common::{start_with, yt};
use reqwest::Method;
use serde_json::{json, Value};
use tokio::net::TcpListener;

#[derive(Default)]
struct Fake {
    token_calls: Mutex<u32>,
    subscriptions: Mutex<Vec<Value>>,
    sent: Mutex<Vec<Value>>,
    chat_calls: Mutex<Vec<HashMap<String, String>>>,
}

type Shared = Arc<Fake>;

async fn serve(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("127.0.0.1:{}", addr.port())
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    for _ in 0..150 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

// ───────────────────────────── fake Twitch ─────────────────────────────

async fn device() -> Json<Value> {
    Json(
        json!({"device_code": "dc", "user_code": "ABCD-1234", "verification_uri": "https://www.twitch.tv/activate", "expires_in": 60, "interval": 1}),
    )
}

async fn token(
    State(f): State<Shared>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    assert_eq!(form.get("client_id").map(String::as_str), Some("cid"));
    match form.get("grant_type").map(String::as_str) {
        Some("urn:ietf:params:oauth:grant-type:device_code") => {
            assert_eq!(
                form.get("scopes").map(String::as_str),
                Some("user:read:chat user:write:chat")
            );
            let mut calls = f.token_calls.lock().unwrap();
            *calls += 1;
            if *calls == 1 {
                // The streamer hasn't approved yet.
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"status": 400, "message": "authorization_pending"})),
                );
            }
            (
                StatusCode::OK,
                Json(
                    json!({"access_token": "at1", "refresh_token": "rt1", "expires_in": 14400, "scope": ["user:read:chat", "user:write:chat"], "token_type": "bearer"}),
                ),
            )
        }
        other => panic!("unexpected grant {other:?}"),
    }
}

async fn validate(headers: HeaderMap) -> impl IntoResponse {
    if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("OAuth at1") {
        (
            StatusCode::OK,
            Json(
                json!({"client_id": "cid", "login": "streamer", "user_id": "100", "scopes": [], "expires_in": 14000}),
            ),
        )
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": 401, "message": "invalid access token"})),
        )
    }
}

fn helix_auth(headers: &HeaderMap) {
    assert_eq!(headers.get("client-id").unwrap(), "cid");
    assert_eq!(headers.get("authorization").unwrap(), "Bearer at1");
}

async fn subscribe(
    State(f): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    helix_auth(&headers);
    f.subscriptions.lock().unwrap().push(body);
    (
        StatusCode::ACCEPTED,
        Json(json!({"data": [{"id": "sub1", "status": "enabled"}]})),
    )
}

async fn send_chat(
    State(f): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    helix_auth(&headers);
    let n = {
        let mut sent = f.sent.lock().unwrap();
        sent.push(body);
        sent.len()
    };
    Json(json!({"data": [{"message_id": format!("sent-{n}"), "is_sent": true}]}))
}

fn notification(event: Value) -> String {
    json!({
        "metadata": {"message_type": "notification", "subscription_type": "channel.chat.message"},
        "payload": {"subscription": {"type": "channel.chat.message"}, "event": event}
    })
    .to_string()
}

fn chat(id: &str, login: &str, name: &str, text: &str, badges: &[&str]) -> Value {
    json!({
        "broadcaster_user_id": "100", "broadcaster_user_login": "streamer",
        "chatter_user_id": format!("u-{login}"), "chatter_user_login": login, "chatter_user_name": name,
        "message_id": id, "message": {"text": text, "fragments": []},
        "badges": badges.iter().map(|b| json!({"set_id": b, "id": "1", "info": ""})).collect::<Vec<_>>(),
        "source_broadcaster_user_id": null
    })
}

async fn eventsub(State(f): State<Shared>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(move |ws| eventsub_session(f, ws))
}

async fn eventsub_session(f: Shared, mut ws: WebSocket) {
    let welcome = json!({
        "metadata": {"message_type": "session_welcome"},
        "payload": {"session": {"id": "sess1", "status": "connected", "keepalive_timeout_seconds": 10}}
    });
    ws.send(Message::Text(welcome.to_string().into()))
        .await
        .unwrap();
    // Twitch only delivers events after the subscription exists.
    for _ in 0..100 {
        if !f.subscriptions.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let ka = json!({"metadata": {"message_type": "session_keepalive"}, "payload": {}});
    ws.send(Message::Text(ka.to_string().into())).await.unwrap();
    let request = notification(chat(
        "m1",
        "viewer",
        "Viewer",
        &format!("!sr {}", yt("AAAAAAAAAA1")),
        &["subscriber"],
    ));
    ws.send(Message::Text(request.into())).await.unwrap();
    // Hello from a chatter who isn't using a command: no reply.
    ws.send(Message::Text(
        notification(chat("m2", "other", "Other", "hello!", &[])).into(),
    ))
    .await
    .unwrap();
    // Our own reply comes back as a chat message; it must not be answered again.
    for _ in 0..100 {
        if !f.sent.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let echo = chat(
        "sent-1",
        "streamer",
        "Streamer",
        "!sr echo",
        &["broadcaster"],
    );
    ws.send(Message::Text(notification(echo).into()))
        .await
        .unwrap();
    // A moderator skips.
    ws.send(Message::Text(
        notification(chat("m3", "mod", "Mod", "!skip", &["moderator"])).into(),
    ))
    .await
    .unwrap();
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if ws.send(Message::Text(ka.to_string().into())).await.is_err() {
            return;
        }
    }
}

#[tokio::test]
async fn twitch_device_sign_in_eventsub_and_replies() {
    let fake: Shared = Arc::default();
    let router = Router::new()
        .route("/oauth2/device", post(device))
        .route("/oauth2/token", post(token))
        .route("/oauth2/validate", get(validate))
        .route("/helix/eventsub/subscriptions", post(subscribe))
        .route("/helix/chat/messages", post(send_chat))
        .route("/ws", get(eventsub))
        .with_state(fake.clone());
    let host = serve(router).await;

    let mut t = start_with(|c| {
        c.endpoints.twitch_id = format!("http://{host}");
        c.endpoints.twitch_api = format!("http://{host}/helix");
        c.endpoints.twitch_eventsub_ws = format!("ws://{host}/ws");
    })
    .await;
    t.put_settings(|s| {
        s["chat"]["twitch"]["enabled"] = json!(true);
        s["chat"]["twitch"]["client_id"] = json!("cid");
    })
    .await;
    let (_, info) = t.call(Method::GET, "/api/chat", None).await;
    assert_eq!(info["twitch"]["status"]["state"], "needs_auth", "{info}");

    let (status, dev) = t.call(Method::POST, "/api/chat/twitch/device", None).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{dev}");
    assert_eq!(dev["user_code"], "ABCD-1234");

    eventually("a chat reply", || !fake.sent.lock().unwrap().is_empty()).await;
    let reply = fake.sent.lock().unwrap()[0].clone();
    assert_eq!(reply["broadcaster_id"], "100");
    assert_eq!(reply["sender_id"], "100");
    assert_eq!(reply["reply_parent_message_id"], "m1");
    assert!(
        reply["message"]
            .as_str()
            .unwrap()
            .contains("Viewer 點播了『Video AAAAAAAAAA1』"),
        "{reply}"
    );

    let sub = fake.subscriptions.lock().unwrap()[0].clone();
    assert_eq!(sub["type"], "channel.chat.message");
    assert_eq!(
        sub["condition"],
        json!({"broadcaster_user_id": "100", "user_id": "100"})
    );
    assert_eq!(
        sub["transport"],
        json!({"method": "websocket", "session_id": "sess1"})
    );

    // The moderator's !skip goes through; the echo of our own message was ignored.
    eventually("the skip reply", || fake.sent.lock().unwrap().len() >= 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[1]["message"], "Mod 跳過了『Video AAAAAAAAAA1』");

    let (_, info) = t.call(Method::GET, "/api/chat", None).await;
    assert_eq!(info["twitch"]["status"]["state"], "connected", "{info}");
    assert_eq!(info["twitch"]["login"], "streamer");
    let (_, hist) = t.call(Method::GET, "/api/history", None).await;
    assert_eq!(hist["items"][0]["source"], "twitch");
    assert_eq!(hist["items"][0]["requester"], "Viewer");
    assert_eq!(hist["items"][0]["requester_key"], "viewer");

    // Signing out stops the connection.
    let (s, _) = t.call(Method::DELETE, "/api/chat/twitch", None).await;
    assert_eq!(s, reqwest::StatusCode::NO_CONTENT);
    eventually("needs_auth after sign-out", || {
        t.state.chat.get("twitch").state == "needs_auth"
    })
    .await;
}

// ───────────────────────────── fake YouTube ─────────────────────────────

async fn yt_videos(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    assert_eq!(q.get("key").map(String::as_str), Some("k"));
    if q.get("part").map(String::as_str) == Some("liveStreamingDetails") {
        assert_eq!(q.get("id").map(String::as_str), Some("LIVEVIDEO01"));
        return Json(json!({"items": [{"liveStreamingDetails": {"activeLiveChatId": "chat1"}}]}));
    }
    // Video details for a request (Data API path, 1 quota unit).
    let id = q.get("id").cloned().unwrap_or_default();
    Json(json!({"items": [{
        "id": id,
        "snippet": {"title": format!("YT {id}"), "liveBroadcastContent": "none", "thumbnails": {"medium": {"url": "https://i.ytimg.com/x.jpg"}}},
        "contentDetails": {"duration": "PT3M"},
        "status": {"embeddable": true}
    }]}))
}

fn yt_message(text: &str, name: &str, channel: &str, moderator: bool) -> Value {
    json!({
        "snippet": {"type": "textMessageEvent", "displayMessage": text},
        "authorDetails": {"displayName": name, "channelId": channel, "isChatOwner": false, "isChatModerator": moderator, "isChatSponsor": false}
    })
}

async fn yt_chat(State(f): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    assert_eq!(q.get("liveChatId").map(String::as_str), Some("chat1"));
    f.chat_calls.lock().unwrap().push(q.clone());
    let items = match q.get("pageToken").map(String::as_str) {
        // Chat from before we connected: must not run.
        None => vec![yt_message(
            &format!("!sr {}", yt("BBBBBBBBBB2")),
            "Early",
            "UCearly",
            false,
        )],
        Some("p1") => vec![
            yt_message(
                &format!("!sr {}", yt("CCCCCCCCCC3")),
                "ModYT",
                "UCmod",
                true,
            ),
            json!({"snippet": {"type": "superChatEvent"}, "authorDetails": {}}),
        ],
        _ => vec![],
    };
    let next = format!("p{}", f.chat_calls.lock().unwrap().len());
    Json(json!({"items": items, "nextPageToken": next, "pollingIntervalMillis": 1000}))
}

#[tokio::test]
async fn youtube_live_chat_polling() {
    let fake: Shared = Arc::default();
    let router = Router::new()
        .route("/youtube/videos", get(yt_videos))
        .route("/youtube/liveChat/messages", get(yt_chat))
        .with_state(fake.clone());
    let host = serve(router).await;
    let mut t = start_with(|c| c.endpoints.youtube_api = format!("http://{host}/youtube")).await;

    let (s, _) = t
        .call(
            Method::PUT,
            "/api/settings/youtube-key",
            Some(json!({"api_key": "k"})),
        )
        .await;
    assert_eq!(s, reqwest::StatusCode::NO_CONTENT);
    t.put_settings(|s| {
        s["chat"]["youtube"]["enabled"] = json!(true);
        s["chat"]["youtube"]["video"] = json!("https://www.youtube.com/live/LIVEVIDEO01");
        s["chat"]["youtube"]["poll_secs"] = json!(5);
    })
    .await;

    let q = t
        .wait_until(|q| q["now"]["song"]["source_id"] == "CCCCCCCCCC3")
        .await;
    // Metadata came from the Data API, not yt-dlp.
    assert_eq!(q["now"]["song"]["title"], "YT CCCCCCCCCC3");
    assert_eq!(q["now"]["song"]["duration_secs"], 180);
    assert_eq!(q["now"]["requester"], "ModYT");
    assert_eq!(q["now"]["requester_key"], "yt:ucmod");
    assert!(t.ytdlp_calls().is_empty());

    let (_, hist) = t.call(Method::GET, "/api/history?user=early", None).await;
    assert_eq!(
        hist["total"], 0,
        "backlog from before connecting is ignored"
    );
    let (_, info) = t.call(Method::GET, "/api/chat", None).await;
    assert_eq!(info["youtube"]["status"]["state"], "connected", "{info}");
    assert_eq!(info["youtube"]["units_per_hour"], 3600);
    // Replies can't be posted with an API key, so they go to the activity log.
    let mut logged = false;
    for _ in 0..50 {
        let (_, ev) = t.call(Method::GET, "/api/events", None).await;
        logged = ev
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "youtube" && e["detail"].as_str().unwrap().contains("ModYT"));
        if logged {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(logged, "the reply was logged");
    let calls = fake.chat_calls.lock().unwrap().clone();
    assert!(calls
        .iter()
        .all(|c| c.get("key").map(String::as_str) == Some("k")));
}
