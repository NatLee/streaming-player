//! Accounts: tokens for programs, registration, roles and permission points.

mod common;

use common::{start, yt, TestApp};
use reqwest::{header, Method, StatusCode};
use serde_json::{json, Value};
use streaming_player::{
    google::{self, GoogleAccount},
    tokens,
    util::now,
};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const PW: &str = "password123";

/// Sign in and return the session cookie.
async fn login(t: &TestApp, user: &str, pw: &str) -> String {
    let (s, h, body) = t
        .raw(
            Method::POST,
            "/api/auth/login",
            Some(json!({"username": user, "password": pw})),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    cookie_of(&h)
}

fn cookie_of(h: &header::HeaderMap) -> String {
    h.get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn as_cookie(
    t: &TestApp,
    cookie: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (s, _, text) = t.raw(method, path, body, Some(cookie)).await;
    (
        s,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn as_bearer(
    t: &TestApp,
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> StatusCode {
    let mut req = t
        .http
        .request(method, format!("{}{path}", t.base))
        .bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    req.send().await.unwrap().status()
}

async fn add_user(t: &mut TestApp, name: &str, role: &str) -> i64 {
    let (s, b) = t
        .call(
            Method::POST,
            "/api/users",
            Some(json!({"username": name, "password": PW, "role": role})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    user_id(t, name).await
}

async fn user_id(t: &mut TestApp, name: &str) -> i64 {
    let (_, users) = t.call(Method::GET, "/api/users", None).await;
    users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == name)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn access_and_refresh_tokens() {
    let mut t = start().await;
    let (s, pair) = t
        .call(
            Method::POST,
            "/api/auth/token",
            Some(json!({"username": "admin", "password": PW})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{pair}");
    assert_eq!(pair["token_type"], "Bearer");
    assert_eq!(pair["expires_in"], 15 * 60, "default 15 minutes");
    let access = pair["access_token"].as_str().unwrap().to_string();
    let refresh = pair["refresh_token"].as_str().unwrap().to_string();
    assert_eq!(
        as_bearer(&t, &access, Method::GET, "/api/queue", None).await,
        StatusCode::OK
    );
    assert_eq!(
        as_bearer(&t, &access, Method::GET, "/api/settings", None).await,
        StatusCode::OK
    );
    assert_eq!(
        as_bearer(&t, "nonsense", Method::GET, "/api/queue", None).await,
        StatusCode::UNAUTHORIZED
    );
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/token",
            Some(json!({"username": "admin", "password": "wrong"})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // An expired token, signed with the server's own key, is refused.
    let expired = tokens::encode_claims(
        &t.state.jwt_key,
        &tokens::Claims {
            sub: 1,
            ver: 0,
            iat: now() - 100,
            exp: now() - 1,
        },
    )
    .unwrap();
    assert_eq!(
        as_bearer(&t, &expired, Method::GET, "/api/queue", None).await,
        StatusCode::UNAUTHORIZED
    );

    // Refresh tokens work once; the new pair works; a revoked one doesn't.
    let (s, pair2) = t
        .call(
            Method::POST,
            "/api/auth/refresh",
            Some(json!({"refresh_token": refresh})),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/refresh",
            Some(json!({"refresh_token": refresh})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "used refresh token");
    let refresh2 = pair2["refresh_token"].as_str().unwrap().to_string();
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/revoke",
            Some(json!({"refresh_token": refresh2})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/refresh",
            Some(json!({"refresh_token": refresh2})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "revoked refresh token");

    // Changing the password signs every token out.
    let (_, pair3) = t
        .call(
            Method::POST,
            "/api/auth/token",
            Some(json!({"username": "admin", "password": PW})),
        )
        .await;
    let access3 = pair3["access_token"].as_str().unwrap().to_string();
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/password",
            Some(json!({"current": PW, "new": "another-password"})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        as_bearer(&t, &access3, Method::GET, "/api/queue", None).await,
        StatusCode::UNAUTHORIZED
    );
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/refresh",
            Some(json!({"refresh_token": pair3["refresh_token"]})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = t.call(Method::GET, "/api/queue", None).await;
    assert_eq!(
        s,
        StatusCode::OK,
        "the page that changed the password stays signed in"
    );

    // Disabling a user stops their tokens too.
    let mod_id = add_user(&mut t, "helper", "moderator").await;
    let (_, mp) = t
        .call(
            Method::POST,
            "/api/auth/token",
            Some(json!({"username": "helper", "password": PW})),
        )
        .await;
    let mod_access = mp["access_token"].as_str().unwrap().to_string();
    assert_eq!(
        as_bearer(&t, &mod_access, Method::GET, "/api/queue", None).await,
        StatusCode::OK
    );
    t.call(
        Method::PUT,
        &format!("/api/users/{mod_id}"),
        Some(json!({"disabled": true})),
    )
    .await;
    assert_eq!(
        as_bearer(&t, &mod_access, Method::GET, "/api/queue", None).await,
        StatusCode::UNAUTHORIZED
    );
    let (s, _) = t
        .call(
            Method::POST,
            "/api/auth/refresh",
            Some(json!({"refresh_token": mp["refresh_token"]})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn personal_api_tokens_are_limited_to_their_permissions() {
    let mut t = start().await;
    t.bot("sr", "Alice", &yt("AAAAAAAAAA1")).await;
    let (s, made) = t
        .call(
            Method::POST,
            "/api/auth/api-tokens",
            Some(json!({"name": "stream deck", "permissions": ["player.control"]})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{made}");
    let token = made["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("spat_"));
    assert_eq!(
        as_bearer(&t, &token, Method::POST, "/api/player/pause", None).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        as_bearer(&t, &token, Method::GET, "/api/settings", None).await,
        StatusCode::FORBIDDEN,
        "outside its scope"
    );
    let (_, list) = t.call(Method::GET, "/api/auth/api-tokens", None).await;
    assert_eq!(list[0]["name"], "stream deck");
    assert_eq!(list[0]["permissions"], json!(["player.control"]));
    assert!(list[0]["last_used_at"].as_i64().is_some());

    // A moderator can't make a token with powers they don't have.
    add_user(&mut t, "helper", "moderator").await;
    let cookie = login(&t, "helper", PW).await;
    let (s, _) = as_cookie(
        &t,
        &cookie,
        Method::POST,
        "/api/auth/api-tokens",
        Some(json!({"name": "x", "permissions": ["users.manage"]})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let id = made["id"].as_i64().unwrap();
    let (s, _) = t
        .call(Method::DELETE, &format!("/api/auth/api-tokens/{id}"), None)
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        as_bearer(&t, &token, Method::POST, "/api/player/resume", None).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn registration_follows_the_settings() {
    let mut t = start().await;
    let (s, st) = t.call(Method::GET, "/api/auth/state", None).await;
    assert_eq!(
        (s, st["registration_open"].clone()),
        (StatusCode::OK, json!(true))
    );

    // Open by default; new accounts get the default role (viewer).
    let (s, h, body) = t
        .raw(
            Method::POST,
            "/api/auth/register",
            Some(json!({"username": "fan1", "password": PW})),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let user: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(user["role"], "viewer");
    assert_eq!(user["permissions"], json!(["queue.view", "queue.request"]));
    let fan = cookie_of(&h);

    // A viewer can request under the rules, but not jump the queue or control the player.
    let (s, b) = as_cookie(
        &t,
        &fan,
        Method::POST,
        "/api/queue",
        Some(json!({"input": yt("AAAAAAAAAA1")})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = as_cookie(
        &t,
        &fan,
        Method::POST,
        "/api/queue",
        Some(json!({"input": yt("longAAAAAA4")})),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "15-minute limit applies to viewers: {b}"
    );
    let (s, _) = as_cookie(
        &t,
        &fan,
        Method::POST,
        "/api/queue",
        Some(json!({"input": yt("BBBBBBBBBB2"), "placement": "now"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = as_cookie(&t, &fan, Method::POST, "/api/player/skip", Some(json!({}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = as_cookie(&t, &fan, Method::GET, "/api/users", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // The default role can't be one that manages users.
    let mut bad = t.settings().await;
    bad["auth"]["default_role"] = json!("admin");
    let (s, b) = t.call(Method::PUT, "/api/settings", Some(bad)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");

    // Rate limit per address.
    t.put_settings(|s| s["auth"]["register_per_ip_per_hour"] = json!(2))
        .await;
    let (s, _, _) = t
        .raw(
            Method::POST,
            "/api/auth/register",
            Some(json!({"username": "fan2", "password": PW})),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = t
        .raw(
            Method::POST,
            "/api/auth/register",
            Some(json!({"username": "fan3", "password": PW})),
            None,
        )
        .await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "fan1 and fan2 used the two this hour"
    );

    // Closed: no more accounts.
    t.put_settings(|s| {
        s["auth"]["registration_open"] = json!(false);
        s["auth"]["register_per_ip_per_hour"] = json!(100);
    })
    .await;
    let (s, _, _) = t
        .raw(
            Method::POST,
            "/api/auth/register",
            Some(json!({"username": "fan4", "password": PW})),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn google_sign_in_follows_the_registration_rules() {
    let mut t = start().await;
    // Open: an unknown Google account gets an account with the default role.
    let first = google::google_account(&t.state, "sub-1", "Nat.Lee@example.com", "10.0.0.1")
        .await
        .unwrap();
    let GoogleAccount::Found(id) = first else {
        panic!("{first:?}")
    };
    let again = google::google_account(&t.state, "sub-1", "nat.lee@example.com", "10.0.0.1")
        .await
        .unwrap();
    assert_eq!(
        again,
        GoogleAccount::Found(id),
        "the same Google account signs in again"
    );
    let (_, users) = t.call(Method::GET, "/api/users", None).await;
    let u = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(
        (
            u["username"].clone(),
            u["role"].clone(),
            u["google_linked"].clone()
        ),
        (json!("Nat.Lee"), json!("viewer"), json!(true))
    );
    // A second person with the same local part gets a free name.
    let GoogleAccount::Found(id2) =
        google::google_account(&t.state, "sub-2", "nat.lee@other.example", "10.0.0.2")
            .await
            .unwrap()
    else {
        panic!()
    };
    let (_, users) = t.call(Method::GET, "/api/users", None).await;
    assert!(users
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["id"] == id2 && u["username"] == "nat.lee-2"));

    // Closed: only emails an admin entered.
    t.put_settings(|s| s["auth"]["registration_open"] = json!(false))
        .await;
    assert_eq!(
        google::google_account(&t.state, "sub-3", "stranger@example.com", "10.0.0.3")
            .await
            .unwrap(),
        GoogleAccount::NotAllowed
    );
    t.call(
        Method::POST,
        "/api/users",
        Some(json!({"username": "invited", "role": "moderator", "email": "Invited@Example.com"})),
    )
    .await;
    let r = google::google_account(&t.state, "sub-4", "invited@example.com", "10.0.0.4")
        .await
        .unwrap();
    assert!(matches!(r, GoogleAccount::Found(_)), "{r:?}");
    // Disabled accounts stay out.
    t.call(
        Method::PUT,
        &format!("/api/users/{id}"),
        Some(json!({"disabled": true})),
    )
    .await;
    assert_eq!(
        google::google_account(&t.state, "sub-1", "nat.lee@example.com", "1")
            .await
            .unwrap(),
        GoogleAccount::Disabled
    );
}

/// No sign-in needed for these (everything else must check a permission).
const PUBLIC: &[&str] = &[
    "/api/health",
    "/api/meta",
    "/api/ws",
    "/api/bot/",
    "/api/public/",
    "/api/auth/",
];

#[tokio::test]
async fn every_api_operation_checks_a_permission() {
    let mut t = start().await;
    let (s, _) = t
        .call(
            Method::POST,
            "/api/roles",
            Some(json!({"name": "nothing", "label": "No rights", "permissions": []})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    add_user(&mut t, "nobody", "nothing").await;
    let nobody = login(&t, "nobody", PW).await;

    let (_, doc) = t.call(Method::GET, "/api/openapi.json", None).await;
    let mut checked = 0;
    for (path, ops) in doc["paths"].as_object().unwrap() {
        if PUBLIC.iter().any(|p| path.starts_with(p)) {
            continue;
        }
        let concrete = path
            .replace("/keys/{name}", "/keys/obs")
            .replace("{id}", "1")
            .replace("{name}", "viewer");
        for method in ops.as_object().unwrap().keys() {
            let m = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let body = matches!(m, Method::POST | Method::PUT | Method::PATCH).then(|| json!({}));
            let (s, _, text) = t
                .raw(m.clone(), &concrete, body.clone(), Some(&nobody))
                .await;
            assert_eq!(
                s,
                StatusCode::FORBIDDEN,
                "{method} {path} without permission: {text}"
            );
            let (s, _, _) = t.raw(m, &concrete, body, None).await;
            assert!(
                matches!(s, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
                "{method} {path} signed out: {s}"
            );
            checked += 1;
        }
    }
    assert!(checked > 40, "walked {checked} operations");
}

async fn ws_with_cookie(t: &TestApp, role: &str, cookie: &str) -> bool {
    let url = format!("{}/api/ws?role={role}", t.base.replace("http", "ws"));
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    tokio_tungstenite::connect_async(req).await.is_ok()
}

#[tokio::test]
async fn websocket_pages_need_their_permission() {
    let mut t = start().await;
    add_user(&mut t, "fan", "viewer").await;
    add_user(&mut t, "helper", "moderator").await;
    let fan = login(&t, "fan", PW).await;
    let helper = login(&t, "helper", PW).await;
    assert!(
        !ws_with_cookie(&t, "player", &fan).await,
        "a viewer can't act as a player and skip songs"
    );
    assert!(
        ws_with_cookie(&t, "admin", &fan).await,
        "but may watch the queue"
    );
    assert!(ws_with_cookie(&t, "player", &helper).await);
    // The OBS key is enough for the OBS pages.
    let mut p = t.ws("player").await;
    p.expect("hello", |_| true).await;
    // With the public page off, viewers need queue.view.
    t.put_settings(|s| s["viewer"]["public_queue"] = json!(false))
        .await;
    let url = format!("{}/api/ws?role=viewer", t.base.replace("http", "ws"));
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
    assert!(ws_with_cookie(&t, "viewer", &fan).await);
}

#[tokio::test]
async fn the_last_user_manager_is_protected() {
    let mut t = start().await;
    let (s, b) = t
        .call(Method::PUT, "/api/users/1", Some(json!({"disabled": true})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    let (s, _) = t
        .call(
            Method::PUT,
            "/api/users/1",
            Some(json!({"role": "moderator"})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = t.call(Method::DELETE, "/api/users/1", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = t
        .call(
            Method::PUT,
            "/api/roles/admin",
            Some(json!({"label": "x", "permissions": []})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "the admin role is locked");

    // A custom role that manages users counts too…
    t.call(
        Method::POST,
        "/api/roles",
        Some(
            json!({"name": "boss", "label": "Boss", "permissions": ["users.manage", "queue.view"]}),
        ),
    )
    .await;
    let boss = add_user(&mut t, "boss1", "boss").await;
    let (s, _) = t
        .call(
            Method::PUT,
            "/api/users/1",
            Some(json!({"role": "moderator"})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "boss1 can still manage users");
    // …so now boss1 is the last one: their role can't lose the permission, and they can't go.
    let cookie = login(&t, "boss1", PW).await;
    let (s, b) = as_cookie(
        &t,
        &cookie,
        Method::PUT,
        "/api/roles/boss",
        Some(json!({"label": "Boss", "permissions": ["queue.view"]})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    let (s, _) = as_cookie(
        &t,
        &cookie,
        Method::PUT,
        &format!("/api/users/{boss}"),
        Some(json!({"disabled": true})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Roles in use, built-in roles and the default role can't be deleted.
    let (s, _) = as_cookie(&t, &cookie, Method::DELETE, "/api/roles/boss", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = as_cookie(&t, &cookie, Method::DELETE, "/api/roles/viewer", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Every change is in the audit log.
    let (_, log) = as_cookie(&t, &cookie, Method::GET, "/api/users/audit", None).await;
    let text: Vec<String> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["detail"].as_str().unwrap().to_string())
        .collect();
    assert!(
        text.iter().any(|d| d.contains("admin：角色改成 moderator")),
        "{text:?}"
    );
    assert!(text.iter().any(|d| d.contains("新增角色 boss")), "{text:?}");
    assert!(
        text.iter().any(|d| d.contains("新增使用者 boss1")),
        "{text:?}"
    );
}

#[tokio::test]
async fn register_promote_and_moderate() {
    let mut t = start().await;
    t.bot("sr", "Alice", &yt("AAAAAAAAAA1")).await;
    let (s, h, _) = t
        .raw(
            Method::POST,
            "/api/auth/register",
            Some(json!({"username": "newmod", "password": PW})),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let newmod = cookie_of(&h);
    let (s, _) = as_cookie(
        &t,
        &newmod,
        Method::POST,
        "/api/player/skip",
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "a viewer can't skip");

    let id = user_id(&mut t, "newmod").await;
    let (s, _) = t
        .call(
            Method::PUT,
            &format!("/api/users/{id}"),
            Some(json!({"role": "moderator"})),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // Same session, new powers right away.
    let (s, _) = as_cookie(
        &t,
        &newmod,
        Method::POST,
        "/api/player/skip",
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, st) = as_cookie(&t, &newmod, Method::GET, "/api/auth/state", None).await;
    assert!(st["user"]["permissions"]
        .as_array()
        .unwrap()
        .contains(&json!("player.control")));
    let (s, _) = as_cookie(&t, &newmod, Method::GET, "/api/users", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "moderators can't manage users");
    let (s, _) = as_cookie(&t, &newmod, Method::GET, "/api/settings", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "or see settings");
    let (s, _) = as_cookie(
        &t,
        &newmod,
        Method::POST,
        "/api/blocklist",
        Some(json!({"name": "troll"})),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "but may block chatters");

    // The dashboard's activity log shows the block, but sign-ups and role changes stay in
    // the audit log, which only user managers read.
    let (s, activity) = as_cookie(&t, &newmod, Method::GET, "/api/events?limit=100", None).await;
    assert_eq!(s, StatusCode::OK);
    let kinds: Vec<&str> = activity
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"block"), "{kinds:?}");
    for k in streaming_player::events::AUDIT_KINDS {
        assert!(
            !kinds.contains(k),
            "no {k} events in the activity log: {kinds:?}"
        );
    }
    let (_, audit) = t.call(Method::GET, "/api/users/audit", None).await;
    let details: Vec<&str> = audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["detail"].as_str().unwrap())
        .collect();
    assert!(
        details.iter().any(|d| d.contains("moderator")),
        "{details:?}"
    );
    assert!(details.iter().any(|d| d.contains("註冊")), "{details:?}");
}

#[tokio::test]
async fn settings_backup_round_trip() {
    let mut a = start().await;
    a.put_settings(|s| {
        s["queue"]["max_queue"] = json!(12);
        s["chat"]["prefix"] = json!("?");
    })
    .await;
    let (_, cmds) = a.call(Method::GET, "/api/commands", None).await;
    let mut first = cmds["commands"][0].clone();
    first["replies"] = json!({"ok": "收到 {title}"});
    let id = first["id"].as_i64().unwrap();
    a.call(Method::PUT, &format!("/api/commands/{id}"), Some(first))
        .await;
    a.call(
        Method::POST,
        "/api/blocklist",
        Some(json!({"name": "troll", "note": "spam"})),
    )
    .await;
    let (s, backup) = a.call(Method::GET, "/api/backup", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(backup["format"], "streaming-player-backup");
    assert!(
        backup.to_string().find("bot_key").is_none(),
        "keys are not in the backup"
    );

    let mut b = start().await;
    let (s, r) = b
        .call(Method::POST, "/api/backup", Some(backup.clone()))
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["blocked_added"], 1);
    assert_eq!(b.settings().await["queue"]["max_queue"], 12);
    let (_, cmds) = b.call(Method::GET, "/api/commands", None).await;
    assert_eq!(cmds["prefix"], "?");
    assert_eq!(cmds["commands"][0]["replies"]["ok"], "收到 {title}");
    let (_, bl) = b.call(Method::GET, "/api/blocklist", None).await;
    assert_eq!(bl[0]["name_key"], "troll");
    // Restoring needs the permissions for each part.
    add_user(&mut b, "helper", "moderator").await;
    let helper = login(&b, "helper", PW).await;
    let (s, _) = as_cookie(&b, &helper, Method::POST, "/api/backup", Some(backup)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
