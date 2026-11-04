# Moving from v1 (Django) to v2

v2 is a new program and **not compatible with v1**. The old Nightbot URLs (`/player/nightbot/…`), the JWT endpoints of v1 and its database don't carry over. Moving takes about ten minutes by hand: start v2, switch your bot commands, and bring your songs if you want them.

## 1. Stop v1, start v2

```bash
# in the v1 folder
docker compose down

# v2
git clone https://github.com/NatLee/streaming-player.git
cd streaming-player
docker compose up -d --build
```

v2 also listens on port **7878**. Open <http://localhost:7878/>; the first visit creates the admin account. v1 accounts aren't copied: make new ones on **使用者與權限**, or let people register at `/register`.

## 2. Replace the bot commands

Your Nightbot `!sr` was something like `$(urlfetch https://your.host/player/nightbot/order?user=$(user)&url=$(querystring))`. That address no longer answers.

1. Open **聊天室連線 → 聊天機器人** on the admin page.
2. Copy the text for your bot (Nightbot, StreamElements, Fossabot, Streamer.bot). It already contains your address and the bot key.
3. In the bot, edit `!sr` and paste it in place of the old message.
4. Do the same for the other commands you used. **其他指令的網址** on the same page lists all of them.

| v1 URL | v2 command (default words) |
|---|---|
| `order?user=&url=` | `sr` (also `點歌`, `songrequest`) |
| `current` | `song` (also `現在`, `nowplaying`) |
| `current/poll` | `voteskip` (also `卡歌`) |
| `order/{user}/count` | `songstats` (also `點歌統計`) |
| `{id}/delete` | `wrongsong` (also `刪歌`) |
| `{id}/insert` | `bump` (also `插歌`) |

Or skip the bot: v2 reads Twitch chat and YouTube live chat by itself ([Twitch](twitch.md), [YouTube](youtube.md)).

The reply texts use the same wording as v1 by default, and every text can be changed on **聊天指令**.

## 3. Bring your songs (optional)

v2 doesn't read the v1 database. Two ways to get your starred songs (the default playlist) back:

- **Add them again**: on **歌曲庫**, paste a link, keep **加入預設播放清單** ticked, press **加入**.
- **Import a CSV**: while v1 still runs, open its Django admin (`/api/__hidden_admin`), go to 歌曲清單 and export it as CSV. In the first row, rename `song_name` to `title`, `duration` to `duration_secs` and `block` to `blocked` (`url` and `favorite` stay). Then import the file on **歌曲庫 → 匯入**. Yes/no cells may be `1`/`0` or `true`/`false`; other columns are ignored. Nothing is looked up online during the import.

Request history and statistics start fresh.

## Where things are now

| v1 | v2 |
|---|---|
| `/player/` (the player page) | `/admin` to control, `/obs/player?key=…` to play in OBS |
| `/player/login`, `/player/register` | `/login`, `/register` |
| `/api/__hidden_admin` (Django admin) | `/admin` pages: library, history, block list, users and permissions |
| `/api/__hidden_swagger`, `/api/__hidden_redoc` | `/api/docs/` (Swagger UI), `/api/openapi.json` |
| `/player/nightbot/…` | `/api/bot/{command}?key=…` ([chat bots](chat-bots.md)) |
| `/player/playlist/queue` | `/api/public/state` (if the viewer page is on) |
| `/player/ping` | `/api/health` |
| WebSocket `/ws/player/` | `/api/ws` |
| Django admin import/export | **歌曲庫 → 匯出／匯入** (CSV or JSON) |

None of the old addresses redirect; they answer 404.

## What changed

- **One container** instead of Django, Redis, nginx and supervisord.
- **Chat**: Twitch and YouTube chat are read directly; any bot can use `/api/bot/{command}` with a key. Commands, levels, cooldowns and replies are editable.
- **Playback**: YouTube plays in YouTube's own player; other sites go through yt-dlp. The OBS player and overlay are separate pages, synced over WebSocket.
- **Accounts**: roles with permission points instead of "logged in or not"; registration is a setting (open by default, new accounts are viewers); Google sign-in follows the same setting instead of letting in any gmail.com address. Set `SP_GOOGLE_CLIENT_ID` and `SP_GOOGLE_CLIENT_SECRET`, with redirect URI `{SP_PUBLIC_URL}/api/auth/google/callback`. See [users and permissions](users-and-permissions.md).
- **API access**: `/api/auth/token`, `/api/auth/refresh` and `/api/auth/revoke` for access tokens, plus personal API tokens, instead of v1's Simple JWT endpoints.
- **Missing videos**: marked after 3 failed plays in a row (a setting), not on the first failure (v1 issue #2).
- **Volume**: v1 kept it per browser, starting at 0.3%. Now there is one shared volume (default 100); set the stream's level in the OBS mixer.
- **YouTube playback**: v1 played everything from yt-dlp's direct link. v2 uses YouTube's own embedded player and falls back to the direct link when a video can't be embedded. **設定 → 播放 → yt-dlp 直接連結** brings v1's way back.
- **Limits**: the defaults match v1: 30 songs in the queue, 15 minutes per video, 3 votes to skip, 2 votes to bump.

## Backups from now on

**設定 → 備份和搬家** saves settings, chat commands and the block list as one JSON file and restores them on another server. **歌曲庫 → 匯出** saves the library as CSV or JSON.
