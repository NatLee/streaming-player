//! Integration tests: a real server with a temporary database and a fake yt-dlp.

mod common;

use std::collections::HashSet;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode as AxumStatus},
    response::IntoResponse,
    routing::get,
    Router,
};
use common::{start, start_with, yt};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use tokio::net::TcpListener;

const A: &str = "AAAAAAAAAA1";
const B: &str = "BBBBBBBBBB2";
const C: &str = "CCCCCCCCCC3";
const LONG: &str = "longAAAAAA4";

#[tokio::test]
async fn first_run_setup_login_and_roles() {
    let mut t = start().await;
    let (_, st) = t.call(Method::GET, "/api/auth/state", None).await;
    assert_eq!(st["needs_setup"], false);
    assert_eq!(st["user"]["username"], "admin");

    // Setup only works once (v1 let anyone register at any time).
    let (status, _) = t
        .call(
            Method::POST,
            "/api/auth/setup",
            Some(json!({"username": "evil", "password": "password123"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Without a session the admin API is closed.
    let (status, _, _) = t.raw(Method::GET, "/api/queue", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A moderator can run the queue but not change settings.
    let (status, _) = t
        .call(
            Method::POST,
            "/api/users",
            Some(json!({"username": "mod", "password": "password456", "role": "moderator"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, headers, _) = t
        .raw(
            Method::POST,
            "/api/auth/login",
            Some(json!({"username": "mod", "password": "password456"})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let mod_cookie = headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (status, _, _) = t
        .raw(Method::GET, "/api/queue", None, Some(&mod_cookie))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = t
        .raw(Method::GET, "/api/settings", None, Some(&mod_cookie))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Five wrong passwords lock the account name for a while.
    for _ in 0..5 {
        let (status, _, _) = t
            .raw(
                Method::POST,
                "/api/auth/login",
                Some(json!({"username": "mod", "password": "nope-nope"})),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _, _) = t
        .raw(
            Method::POST,
            "/api/auth/login",
            Some(json!({"username": "mod", "password": "password456"})),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn bot_api_requests_songs_with_v1_replies() {
    let mut t = start().await;

    let (status, _) = {
        let url = format!("{}/api/bot/sr?key=wrong&user=x&args=y", t.base);
        let r = t.http.get(url).send().await.unwrap();
        (r.status(), r.text().await.unwrap())
    };
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, reply) = t.bot("sr", "Alice", &yt(A)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        reply.contains("Alice 點播了『Video AAAAAAAAAA1』，馬上開始播放"),
        "{reply}"
    );

    // Second song waits for the first (200 s, not started yet).
    let (_, reply) = t.bot("!sr", "Bob", &format!("https://youtu.be/{B}")).await;
    assert!(reply.contains("播放順位是#2"), "{reply}");
    assert!(reply.contains("還要再等3分20秒"), "{reply}");

    // Same song again: v1's duplicate message.
    let (_, reply) = t.bot("點歌", "Carol", B).await;
    assert_eq!(reply, "這首歌被 Bob 點過，在佇列還沒播放！目前順位 2！");
    let (_, reply) = t.bot("sr", "Carol", &yt(A)).await;
    assert_eq!(reply, "這首歌被 Alice 點過，順位 1，正在放送中！");

    // Too long, search words, private network, live, missing.
    let (_, reply) = t.bot("sr", "Carol", &yt(LONG)).await;
    assert_eq!(reply, "這歌怎麼能超過15分鐘！");
    let (_, reply) = t.bot("sr", "Carol", "some song name").await;
    assert!(reply.contains("『Search hit』"), "{reply}");
    let (_, reply) = t.bot("sr", "Carol", "http://192.168.0.1/video.mp4").await;
    assert!(reply.contains("內部網路"), "{reply}");
    let (_, reply) = t.bot("sr", "Carol", "https://media.example.com/live").await;
    assert_eq!(reply, "Carol，直播沒辦法點喔！");
    let (_, reply) = t
        .bot("sr", "Carol", "https://media.example.com/missing")
        .await;
    assert!(
        reply.contains("找不到這首歌") && reply.contains("HTTP Error 404"),
        "{reply}"
    );
    let (_, reply) = t.bot("sr", "Carol", "").await;
    assert_eq!(reply, "哪有人點歌不輸入網址的！");

    // Known songs are not looked up again.
    let calls_before = t.ytdlp_calls().len();
    t.bot("sr", "Dave", &yt(A)).await;
    assert_eq!(t.ytdlp_calls().len(), calls_before);

    // Other commands.
    let (_, reply) = t.bot("song", "Dave", "").await;
    assert_eq!(
        reply,
        format!("現在播放的是 Alice 點的『Video {A}』，傳送門 -> {}", yt(A))
    );
    let (_, reply) = t.bot("mysongs", "bob", "").await;
    assert!(reply.contains("#2『Video BBBBBBBBBB2』"), "{reply}");
    let (_, reply) = t.bot("queue", "Dave", "").await;
    assert!(
        reply.starts_with("佇列共 2 首，下一首是『Video BBBBBBBBBB2』"),
        "{reply}"
    );
    assert!(reply.contains("/queue"), "{reply}");
    let (_, reply) = t.bot("songhelp", "Dave", "").await;
    assert!(
        reply.contains("!sr") && !reply.contains("!skip"),
        "viewers don't see mod commands: {reply}"
    );
    let (status, _) = t.bot("nosuchcommand", "Dave", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Moderator-only command, with the level from Nightbot's header.
    let (_, reply) = t.bot("skip", "Dave", "").await;
    assert_eq!(reply, "Dave，這個指令只有管理員可以用。");
    let key = t.bot_key().await;
    let r = t
        .http
        .get(format!("{}/api/bot/skip?key={key}", t.base))
        .header(
            "Nightbot-User",
            "name=modguy&displayName=ModGuy&provider=twitch&providerId=1&userLevel=moderator",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.text().await.unwrap(),
        format!("ModGuy 跳過了『Video {A}』")
    );
    let q = t.queue().await;
    assert_eq!(q["now"]["song"]["source_id"], B);
}

#[tokio::test]
async fn queue_rules_limits_blocks_and_cooldowns() {
    let mut t = start().await;
    t.put_settings(|s| {
        s["queue"]["max_per_user"] = json!(1);
        s["queue"]["max_queue"] = json!(2);
    })
    .await;
    t.bot("sr", "Alice", &yt(A)).await; // on air
    let (_, reply) = t.bot("sr", "Alice", &yt(B)).await;
    assert_eq!(reply, "Alice，你已經有 1 首在排了，每人最多 1 首！");
    t.bot("sr", "Bob", &yt(B)).await;
    t.bot("sr", "Carol", &yt(C)).await;
    let (_, reply) = t.bot("sr", "Dave", "some song").await;
    assert_eq!(reply, "要播的歌太多了！再點我要罷工了！");

    // Block a chatter.
    let (status, _) = t
        .call(Method::POST, "/api/blocklist", Some(json!({"name": "Eve"})))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, reply) = t.bot("sr", "eve", &yt(C)).await;
    assert_eq!(reply, "eve，你目前不能點歌。");

    // Close requests: viewers are refused, a moderator still gets through the gate.
    t.put_settings(|s| {
        s["queue"]["requests_open"] = json!(false);
        s["queue"]["max_queue"] = json!(0);
    })
    .await;
    let (_, reply) = t.bot("sr", "Frank", &yt(C)).await;
    assert_eq!(reply, "現在沒有開放點歌喔！");

    // A cooldown on a command.
    let (_, cmds) = t.call(Method::GET, "/api/commands", None).await;
    let mut current = cmds["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["action"] == "current")
        .unwrap()
        .clone();
    current["cooldown_secs"] = json!(60);
    current["replies"] = json!({"ok": "正在播：{title}（{requester}）"});
    let id = current["id"].as_i64().unwrap();
    let (status, body) = t
        .call(Method::PUT, &format!("/api/commands/{id}"), Some(current))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, reply) = t.bot("song", "Gina", "").await;
    assert_eq!(reply, format!("正在播：Video {A}（Alice）"));
    let (_, reply) = t.bot("song", "Gina", "").await;
    assert!(reply.starts_with("Gina，請等 "), "{reply}");
    let (_, reply) = t.bot("song", "Hank", "").await;
    assert!(
        reply.starts_with("正在播"),
        "cooldown is per person: {reply}"
    );

    // Triggers must be unique.
    let (status, body) = t
        .call(
            Method::POST,
            "/api/commands",
            Some(json!({"action": "request", "triggers": ["!SR"], "min_level": "everyone"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn votes_skip_and_bump_like_v1() {
    let mut t = start().await;
    t.bot("sr", "Alice", &yt(A)).await;
    t.bot("sr", "Bob", &yt(B)).await;
    t.bot("sr", "Carol", &yt(C)).await;

    let (_, r) = t.bot("voteskip", "v1", "").await;
    assert_eq!(
        r,
        format!("[ v1 ] 投票要卡掉當前的歌 [Video {A}]，達成人數 [1/3]")
    );
    let (_, r) = t.bot("voteskip", "v1", "").await;
    assert_eq!(r, "v1，天啊！不要重複投票好嗎！");
    t.bot("voteskip", "v2", "").await;
    let (_, r) = t.bot("卡歌", "v3", "").await;
    assert_eq!(
        r,
        format!("當前的歌 [Video {A}] 已經被 [ v1 , v2 , v3 ] 投票卡掉！")
    );
    let q = t.queue().await;
    assert_eq!(q["now"]["song"]["source_id"], B);

    // Add D so C isn't next, then bump C… C is next already: refuse.
    let c_id = q["queue"][0]["id"].as_i64().unwrap();
    let (_, r) = t.bot("bump", "v1", &c_id.to_string()).await;
    assert_eq!(r, "無法插歌！因爲這首已經是下一首了！");
    t.bot("sr", "Dave", "some song").await;
    let q = t.queue().await;
    let d_id = q["queue"][1]["id"].as_i64().unwrap();
    let (_, r) = t.bot("bump", "v1", &d_id.to_string()).await;
    assert!(r.contains("[1/2]"), "{r}");
    let (_, r) = t.bot("bump", "v2", &format!("#{d_id}")).await;
    assert_eq!(r, "[Search hit] 由 [ v1 , v2 ] 投票成功插歌！");
    let q = t.queue().await;
    assert_eq!(q["queue"][0]["id"].as_i64(), Some(d_id));
    let (_, r) = t.bot("bump", "v1", "abc").await;
    assert_eq!(r, "ID是點歌時會給的一組數字ㄛ！");
}

#[tokio::test]
async fn admin_queue_controls() {
    let mut t = start().await;
    for (input, placement) in [
        (yt(A), "end"),
        (yt(B), "end"),
        (yt(C), "end"),
        ("some song".to_string(), "next"),
    ] {
        let (status, body) = t
            .call(
                Method::POST,
                "/api/queue",
                Some(json!({"input": input, "placement": placement})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let titles = |q: &Value| -> Vec<String> {
        q["queue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["song"]["title"].as_str().unwrap().to_string())
            .collect()
    };
    let q = t.queue().await;
    assert_eq!(q["now"]["requester"], "admin");
    assert_eq!(
        titles(&q),
        vec![
            "Search hit".to_string(),
            format!("Video {B}"),
            format!("Video {C}")
        ]
    );

    // Reorder and move.
    let ids: Vec<i64> = q["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    let (s, _) = t
        .call(
            Method::PUT,
            "/api/queue/order",
            Some(json!({"ids": [ids[2], ids[0]]})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let q = t.queue().await;
    assert_eq!(
        titles(&q),
        vec![
            format!("Video {C}"),
            "Search hit".to_string(),
            format!("Video {B}")
        ],
        "left-out ids keep their place at the end"
    );
    t.call(
        Method::POST,
        &format!("/api/queue/{}/move", ids[1]),
        Some(json!({"index": 0})),
    )
    .await;
    assert_eq!(titles(&t.queue().await)[0], format!("Video {B}"));

    // Interrupt: C plays now, A goes back to the front.
    let (s, _) = t
        .call(
            Method::POST,
            &format!("/api/queue/{}/play-now", ids[2]),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let q = t.queue().await;
    assert_eq!(q["now"]["song"]["source_id"], C);
    assert_eq!(q["queue"][0]["song"]["source_id"], A);

    // Skip and block; the blocked song can't be requested from chat.
    let (s, _) = t
        .call(
            Method::POST,
            "/api/player/skip",
            Some(json!({"block": true})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, r) = t.bot("sr", "Zed", &yt(C)).await;
    assert_eq!(r, "Sorry! This song has been blocked ;/");

    // Remove and block the requester.
    let q = t.queue().await;
    let first = q["queue"][0]["id"].as_i64().unwrap();
    let (s, _) = t
        .call(
            Method::DELETE,
            &format!("/api/queue/{first}?block=user"),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = t.call(Method::GET, "/api/blocklist", None).await;
    assert_eq!(list[0]["name_key"], "admin");

    // Pause, volume.
    t.call(Method::POST, "/api/player/pause", None).await;
    t.call(
        Method::PUT,
        "/api/player/volume",
        Some(json!({"volume": 35, "muted": true})),
    )
    .await;
    let q = t.queue().await;
    assert_eq!(
        q["player"],
        json!({"paused": true, "volume": 35, "muted": true})
    );

    // History and activity log.
    let (_, h) = t.call(Method::GET, "/api/history", None).await;
    let statuses: HashSet<&str> = h["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["status"].as_str().unwrap())
        .collect();
    assert!(
        statuses.contains("skipped") && statuses.contains("removed"),
        "{statuses:?}"
    );
    let (_, ev) = t.call(Method::GET, "/api/events", None).await;
    assert!(ev.as_array().unwrap().iter().any(|e| e["kind"] == "skip"));

    let (s, _) = t.call(Method::POST, "/api/queue/clear", None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(t.queue().await["queue"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn players_sync_over_websocket_with_one_leader() {
    let mut t = start().await;
    let mut lead = t.ws("player").await;
    let hello = lead.expect("hello", |_| true).await;
    assert_eq!(hello["leader"], true);
    let mut follow = t.ws("player").await;
    assert_eq!(follow.expect("hello", |_| true).await["leader"], false);
    let mut overlay = t.ws("overlay").await;
    overlay.expect("hello", |_| true).await;

    // A request reaches every page.
    t.bot("sr", "Alice", &yt(A)).await;
    t.bot("sr", "Bob", &yt(B)).await;
    let n = overlay
        .expect("notice", |v| v["text"].as_str().unwrap().contains("Bob"))
        .await;
    assert_eq!(n["kind"], "request");
    let st = follow
        .expect("state", |v| {
            v["queue"].as_array().is_some_and(|q| q.len() == 1)
        })
        .await;
    let a_id = st["now"]["id"].as_i64().unwrap();
    assert_eq!(st["players_connected"], 2);
    assert_eq!(st["now"]["clock_started"], false);

    // The leader starts the clock; followers see it.
    lead.send(json!({"type": "started", "request_id": a_id, "position_ms": 0}))
        .await;
    follow
        .expect("state", |v| v["now"]["clock_started"] == true)
        .await;

    // A follower's "ended" is ignored, the leader's moves the queue once.
    follow
        .send(json!({"type": "ended", "request_id": a_id}))
        .await;
    lead.send(json!({"type": "ended", "request_id": a_id}))
        .await;
    lead.send(json!({"type": "ended", "request_id": a_id}))
        .await;
    let st = follow
        .expect("state", |v| v["now"]["song"]["source_id"] == B)
        .await;
    assert!(st["queue"].as_array().unwrap().is_empty());
    let (_, h) = t.call(Method::GET, "/api/history", None).await;
    assert_eq!(h["items"][0]["status"], "played");
    assert_eq!(h["total"], 1, "the duplicate ended report did nothing");

    // The leader leaves; the follower takes over.
    lead.close().await;
    let l = follow.expect("leader", |_| true).await;
    assert_eq!(l["leader"], true);

    // Viewer sockets work while the public page is on, and are refused when it is off.
    let url = format!("{}/api/ws?role=viewer", t.base.replace("http", "ws"));
    assert!(tokio_tungstenite::connect_async(url.clone()).await.is_ok());
    t.put_settings(|s| s["viewer"]["public_queue"] = json!(false))
        .await;
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
    let bad = format!(
        "{}/api/ws?role=player&key=nope",
        t.base.replace("http", "ws")
    );
    assert!(tokio_tungstenite::connect_async(bad).await.is_err());
}

#[tokio::test]
async fn failures_mark_a_song_missing_and_autoplay_uses_favorites() {
    let mut t = start().await;
    let mut player = t.ws("player").await;
    player.expect("hello", |_| true).await;
    for _ in 0..3 {
        let (s, body) = t
            .call(Method::POST, "/api/queue", Some(json!({"input": yt(A)})))
            .await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let id = body["id"].as_i64().unwrap();
        player
            .send(json!({"type": "error", "request_id": id, "reason": "embed 150"}))
            .await;
        t.wait_until(|q| q["now"].is_null()).await;
    }
    let (_, lib) = t
        .call(Method::GET, "/api/library?filter=missing", None)
        .await;
    assert_eq!(lib["total"], 1, "{lib}");
    assert_eq!(lib["items"][0]["fail_count"], 3);
    let (_, r) = t.bot("sr", "Alice", &yt(A)).await;
    assert_eq!(r, "Sad! This song has lost :(");

    // Mark B as a favorite: with the queue empty it plays by itself.
    let (_, song) = t
        .call(
            Method::POST,
            "/api/library",
            Some(json!({"input": yt(B), "favorite": true})),
        )
        .await;
    assert_eq!(song["favorite"], true);
    let (s, _) = t
        .call(Method::POST, "/api/library", Some(json!({"input": yt(C)})))
        .await;
    assert_eq!(s, StatusCode::OK);
    // Turning autoplay off and on starts it.
    t.put_settings(|s| s["playback"]["autoplay_favorites"] = json!(false))
        .await;
    t.put_settings(|s| s["playback"]["autoplay_favorites"] = json!(true))
        .await;
    let st = player
        .expect("state", |v| v["now"]["autoplay"] == true)
        .await;
    assert_eq!(st["now"]["song"]["source_id"], B);
    assert_eq!(st["now"]["requester"], "自動播放");
    let (_, r) = t.bot("song", "x", "").await;
    assert!(r.starts_with("現在播放的歌由自動點播提供"), "{r}");

    // Clearing "missing" lets the song be requested again.
    let id = lib["items"][0]["id"].as_i64().unwrap();
    let (_, s) = t
        .call(
            Method::PATCH,
            &format!("/api/library/{id}"),
            Some(json!({"missing": false})),
        )
        .await;
    assert_eq!(
        (s["missing"].clone(), s["fail_count"].clone()),
        (json!(false), json!(0))
    );
}

#[tokio::test]
async fn unknown_lengths_are_checked_when_the_player_learns_them() {
    let mut t = start().await;
    let mut player = t.ws("player").await;
    player.expect("hello", |_| true).await;
    t.bot("sr", "Alice", "https://media.example.com/clip").await;
    // The fake says 30 s; pretend the site didn't say by clearing it.
    sqlx::query("UPDATE songs SET duration_secs = NULL")
        .execute(&t.state.db)
        .await
        .unwrap();
    let st = t.queue().await;
    let id = st["now"]["id"].as_i64().unwrap();
    player
        .send(json!({"type": "duration", "request_id": id, "secs": 5000}))
        .await;
    t.wait_until(|q| q["now"].is_null()).await;
    let (_, h) = t.call(Method::GET, "/api/history", None).await;
    assert_eq!(h["items"][0]["status"], "skipped");
}

async fn media_server(
    State(seen): State<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    seen.lock().unwrap().push(
        headers
            .get("referer")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default(),
    );
    let body = b"0123456789";
    match headers.get("range").and_then(|v| v.to_str().ok()) {
        Some("bytes=0-3") => (
            AxumStatus::PARTIAL_CONTENT,
            [
                ("content-type", "video/mp4"),
                ("content-range", "bytes 0-3/10"),
                ("accept-ranges", "bytes"),
            ],
            body[..4].to_vec(),
        ),
        _ => (
            AxumStatus::OK,
            [
                ("content-type", "video/mp4"),
                ("accept-ranges", "bytes"),
                ("x-whole-file", "1"),
            ],
            body.to_vec(),
        ),
    }
}

#[tokio::test]
async fn media_is_streamed_through_the_server_with_ranges() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/file.mp4", get(media_server))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let mut t = start().await;
    t.set_media_url(&format!("http://{addr}/file.mp4"));
    let (_, song) = t
        .call(
            Method::POST,
            "/api/library",
            Some(json!({"input": "https://media.example.com/clip"})),
        )
        .await;
    assert_eq!(song["kind"], "media");
    assert_eq!(song["site"], "media.example.com");
    let id = song["id"].as_i64().unwrap();

    // OBS pages pass the key in the URL.
    let (s, _) = t.get_text(&format!("/api/media/{id}/source")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let key = t.obs_key().await;
    let (s, body) = t
        .get_text(&format!("/api/media/{id}/source?key={key}"))
        .await;
    assert_eq!(s, StatusCode::OK);
    let src: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        src,
        json!({"kind": "file", "url": format!("/api/media/{id}/stream"), "audio_only": false})
    );

    let resp = t
        .http
        .get(format!("{}/api/media/{id}/stream?key={key}", t.base))
        .header("range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.headers().get("content-range").unwrap(), "bytes 0-3/10");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), b"0123");
    assert_eq!(
        seen.lock().unwrap()[0],
        "https://media.example.com/",
        "the site's Referer is sent"
    );
}

#[tokio::test]
async fn library_export_and_import() {
    let mut t = start().await;
    t.call(
        Method::POST,
        "/api/library",
        Some(json!({"input": yt(A), "favorite": true})),
    )
    .await;
    let (s, h, body) = t
        .raw(
            Method::GET,
            "/api/library/export?format=csv",
            None,
            Some(&t.cookie.clone()),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(h
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains(".csv"));
    assert!(body.starts_with("url,title,duration_secs,favorite,blocked\n"));
    assert!(body.contains(&format!("{},Video {A},200,true,false", yt(A))));

    let (_, json_export) = t.call(Method::GET, "/api/library/export", None).await;
    let mut fresh = start().await;
    let mut records = json_export.as_array().unwrap().clone();
    records.push(json!({"url": "not a url at all", "title": "x"}));
    records.push(json!({"url": "https://soundcloud.com/a/b", "title": "Track", "blocked": true}));
    let (_, r) = fresh
        .call(
            Method::POST,
            "/api/library/import",
            Some(Value::Array(records)),
        )
        .await;
    assert_eq!(r, json!({"imported": 2, "skipped": 1}));
    let (_, lib) = fresh
        .call(Method::GET, "/api/library?filter=favorite", None)
        .await;
    assert_eq!(lib["items"][0]["source_id"], A);
    assert!(
        fresh.ytdlp_calls().is_empty(),
        "import does not look songs up"
    );
}

#[tokio::test]
async fn viewer_page_api() {
    let mut t = start().await;
    t.bot("sr", "Alice", &yt(A)).await;
    t.bot("sr", "Bob", &yt(B)).await;
    let (s, body) = t.get_text("/api/public/state").await;
    assert_eq!(s, StatusCode::OK);
    let st: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(st["queue"][0]["position"], 2);
    let (_, mine) = t.get_text("/api/public/requests?user=bob").await;
    let mine: Value = serde_json::from_str(&mine).unwrap();
    assert_eq!(mine["pending"][0]["song"]["source_id"], B);
    t.put_settings(|s| s["viewer"]["public_queue"] = json!(false))
        .await;
    let (s, _) = t.get_text("/api/public/state").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_document_is_complete() {
    let t = start_with(|_| {}).await;
    let (s, body) = t.get_text("/api/openapi.json").await;
    assert_eq!(s, StatusCode::OK);
    let doc: Value = serde_json::from_str(&body).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    for p in [
        "/api/bot/{command}",
        "/api/auth/token",
        "/api/queue",
        "/api/ws",
        "/api/settings",
    ] {
        assert!(paths.contains_key(p), "missing {p}");
    }
    // Every $ref points at a schema that exists.
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let mut refs = Vec::new();
    collect_refs(&doc, &mut refs);
    for r in refs {
        let name = r.trim_start_matches("#/components/schemas/");
        assert!(schemas.contains_key(name), "dangling $ref {r}");
    }
    let (s, _) = t.get_text("/api/docs/").await;
    assert_eq!(s, StatusCode::OK);
    let (s, body) = t.get_text("/api/nope").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(body.contains("error"));
}

fn collect_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if k == "$ref" {
                    out.push(v.as_str().unwrap().to_string());
                } else {
                    collect_refs(v, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

async fn fake_oembed(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let url = q.get("url").cloned().unwrap_or_default();
    if url.ends_with("noembed0001") {
        return (AxumStatus::UNAUTHORIZED, "Unauthorized").into_response();
    }
    if url.ends_with("gone0000001") {
        return (AxumStatus::NOT_FOUND, "Not Found").into_response();
    }
    axum::Json(
        json!({"title": "oEmbed title", "thumbnail_url": "https://i.ytimg.com/vi/x/hqdefault.jpg"}),
    )
    .into_response()
}

#[tokio::test]
async fn youtube_titles_come_from_oembed_and_yt_dlp_is_the_last_resort() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().route("/oembed", get(fake_oembed));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut t = start_with(|c| c.endpoints.youtube_oembed = format!("http://{addr}/oembed")).await;

    let (_, r) = t.bot("sr", "Alice", &yt(A)).await;
    assert!(r.contains("『oEmbed title』"), "{r}");
    assert!(
        t.ytdlp_calls().is_empty(),
        "no scraping for a normal YouTube request"
    );
    let q = t.queue().await;
    assert_eq!(
        q["now"]["song"]["duration_secs"],
        Value::Null,
        "the player reports the length later"
    );

    let (_, r) = t.bot("sr", "Bob", &yt("gone0000001")).await;
    assert_eq!(r, "Bob，找不到這首歌：找不到這部影片");
    // Not embeddable: with the stream fallback on, yt-dlp gets a chance…
    let (_, r) = t.bot("sr", "Bob", &yt("noembed0001")).await;
    assert!(r.contains("『Video noembed0001』"), "{r}");
    assert_eq!(t.ytdlp_calls().len(), 1);
    // …and with it off, the request is refused without scraping.
    t.put_settings(|s| s["playback"]["stream_fallback"] = json!(false))
        .await;
    sqlx::query("DELETE FROM songs WHERE source_id = 'noembed0001'")
        .execute(&t.state.db)
        .await
        .ok();
    let (_, r) = t.bot("sr", "Carol", &yt("noembed0001")).await;
    assert_eq!(r, "Carol，找不到這首歌：作者不允許嵌入播放");
    assert_eq!(t.ytdlp_calls().len(), 1);
}

#[tokio::test]
async fn links_split_at_ampersands_are_put_back_together() {
    let mut t = start().await;
    let key = t.bot_key().await;
    // A bot that doesn't encode its arguments: the link's "&v=" arrives as its own parameter.
    let url = format!(
        "{}/api/bot/sr?key={key}&user=Zed&args=https://www.youtube.com/watch?list=PL1&v={A}&t=30",
        t.base
    );
    let r = t.http.get(url).send().await.unwrap().text().await.unwrap();
    assert!(r.contains(&format!("『Video {A}』")), "{r}");
    // The same through the "q" name.
    let url = format!(
        "{}/api/bot/sr?key={key}&user=Yan&q=https://www.youtube.com/watch?list=PL2&v={B}",
        t.base
    );
    let r = t.http.get(url).send().await.unwrap().text().await.unwrap();
    assert!(r.contains(&format!("『Video {B}』")), "{r}");
}
