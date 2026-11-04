# Moving from v1 (Django) to v2

v2 is a rewrite: one container instead of Django + Redis + nginx, a new admin page, and chat connections built in. Your Nightbot commands keep working, and your song library and request history can come along.

## 1. Stop v1

In the v1 folder:

```bash
docker compose down
```

Keep the folder: v1's database is `backend/db.sqlite3` in it.

## 2. Start v2

```bash
git pull            # or a fresh clone
docker compose up -d --build
```

It listens on **7878**, the same port as v1, so existing Nightbot URLs keep their address. Open <http://localhost:7878/>. On first run you create the admin account. v1 accounts aren't copied: Django's password hashes can't be reused, and v1 let anyone register.

## 3. Bring your songs and history (optional)

`import-v1` copies v1's song library, including favorites (the default playlist), blocked and missing marks, plus the whole request history. Favorites keep playing when the queue is empty, and `!songstats` keeps counting. It runs once per database. v1's pending queue isn't copied.

```bash
docker compose stop streaming-player
docker compose run --rm \
  -v /path/to/v1/backend/db.sqlite3:/v1.sqlite3:ro \
  streaming-player streaming-player import-v1 /v1.sqlite3
docker compose start streaming-player
```

## What moved where

| v1 | v2 |
|---|---|
| `/player/` (the player page) | `/admin` (control) and `/obs/player?key=…` (playback in OBS); the old address redirects |
| `/player/login`, first-login page | `/login` (first run creates the admin) |
| `/api/__hidden_admin` (Django admin) | `/admin` pages: library, history, block list, users; the old address redirects |
| `/api/__hidden_swagger`, `/api/__hidden_redoc` | `/api/docs/` (Swagger UI, OpenAPI at `/api/openapi.json`); the old addresses redirect |
| `/player/nightbot/current`, `current/poll`, `order`, `order/{user}/count`, `{id}/delete`, `{id}/insert` | **unchanged**, see below |
| `/player/playlist/queue` (public JSON) | unchanged, same fields |
| `/player/ping` | unchanged (`/api/health` is the new one) |
| WebSocket `/ws/player/` | `/api/ws` |
| Django admin import/export | **歌曲庫 → 匯出／匯入** (CSV or JSON) |

## Nightbot URLs

The six `/player/nightbot/…` URLs answer exactly where they did, through the same command engine as everything else. Their texts, limits and permissions come from **聊天指令** and **設定**. Defaults match v1: 30 songs in the queue, 15 minutes per video, 3 votes to skip, 2 votes to bump.

Small differences:

- Replies are plain text. v1 returned a JSON string, so Nightbot showed it in quotes.
- `{id}/delete` now also lets a moderator delete anyone's song. In v1, only the requester could.
- They have no key. Once your commands use the new `/api/bot/…` URLs (see [chat bots](chat-bots.md)), turn them off with **聊天室連線 → 接受 v1 的 Nightbot 網址**. The page shows when they were last used. They will be removed in v3.

## Removed (they only served v1's own page)

- JWT sign-in: `/api/auth/token`, `/token/refresh`, `/token/verify`, and the Google token endpoint `/api/auth/google/token`. The web page now uses a session cookie. Google sign-in is `/api/auth/google/start`.
- `/player/register`. v1 let anyone create an account at any time; now only the first-run setup does, and admins add other users.
- The JWT-only player endpoints `/player/playlist/get`, `…/mark`, `…/insert`, `…/delete`, `/player/playlist/order` and `/player/youtube-video-info`. The admin API under `/api/queue`, `/api/player` and `/api/library` covers all of them.

## Behaviour changes

- **Google sign-in**: v1 let in any gmail.com account. Now only addresses an admin entered on **使用者** can sign in. Set `SP_GOOGLE_CLIENT_ID` and `SP_GOOGLE_CLIENT_SECRET`, with redirect URI `{SP_PUBLIC_URL}/api/auth/google/callback`.
- **Missing videos**: v1 marked a video missing the first time it failed. Now it takes 3 failures in a row (a setting). This was issue #2.
- **Volume**: v1 kept the volume per browser, starting at 0.3%. Now there is one shared volume (default 100); control the stream's level in the OBS mixer.
- **YouTube playback**: v1 played everything from yt-dlp's direct link. v2 uses YouTube's own embedded player and falls back to the direct link when a video can't be embedded. To get v1's way back, use **設定 → 播放 → yt-dlp 直接連結**.
