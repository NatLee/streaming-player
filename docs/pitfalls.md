# Pitfalls

Problems met while building v2. Each one: what we saw, why, the fix, and how to avoid it next time.

## Database

### "database is locked" under load

- **Seen**: an integration test failed now and then with HTTP 500. The log said `(code: 5) database is locked`, even though the busy timeout is 10 seconds.
- **Why**: a plain `BEGIN` starts as a reader. Our queue transactions read first (the next position), then write. If another connection writes in between (the activity log, say), SQLite can't upgrade the stale read and fails at once, without waiting for the busy timeout.
- **Fix**: every write transaction starts with `BEGIN IMMEDIATE` (`db::write_tx`), which takes the write lock up front and waits for it.
- **Avoid**: never use `pool.begin()` for a transaction that writes.

## YouTube

### The player shows error 153

- **Why**: since 2025 the embedded YouTube player refuses to run when the page sends no `Referer`. A `Referrer-Policy` of `no-referrer` or `same-origin` (which bahamut-automation uses) strips it for cross-origin requests.
- **Fix**: the server sends `strict-origin-when-cross-origin`, the browser default.
- **Avoid**: check what a reverse proxy adds; some "security header" presets set `no-referrer`.

### yt-dlp needs a JavaScript runtime for YouTube

- **Why**: since yt-dlp 2025.11.12, YouTube needs an external JavaScript runtime to solve its challenges. Without one, formats go missing.
- **Fix**: the image copies Deno from `denoland/deno:bin-…`. That image has no C library, so its binary only runs in the Debian runtime image, not on its own.

### A direct YouTube link answers 403 sometimes

- **Seen**: the stream fallback for one video gave 502; the same request a minute later worked.
- **Why**: some links yt-dlp returns are refused right away.
- **Fix**: on 403/404/410 the proxy asks yt-dlp for a new link once. YouTube embeds are the main path anyway.

### The first chat page is old chat

- **Why**: the first `liveChatMessages.list` call returns recent messages from before we connected.
- **Fix**: the first page is skipped, so a restart doesn't run old `!sr` commands again.

## Other sites

### Bilibili plays without picture

- **Why**: Bilibili only offers separate video and audio tracks (DASH). A browser `<video>` can't join two files, and joining them on the server would need ffmpeg and a re-mux per song.
- **Behaviour**: the format selector falls back to the best audio-only file. The player shows the cover art and title (as for SoundCloud). The source endpoint says `audio_only: true`.
- **Note**: many Bilibili videos are geo-restricted; yt-dlp then says "may be deleted or geo-restricted".

### SoundCloud gives HLS

- SoundCloud serves HLS playlists with `Access-Control-Allow-Origin: *`, so the browser plays them with hls.js directly. HLS isn't proxied: its segments are separate requests, so one proxy URL couldn't cover them.

## Chat

### Bots differ in what they send

- Nightbot sends `Nightbot-User` with name and level. Fossabot sends login and display name but **no role**. StreamElements sends no headers at all, and shows its own error text for any status ≥ 400, so normal answers must be 200.
- **Fix**: the header formats are a table (`chat::bots::HEADER_PROFILES`); `user=` and `level=` query parameters always win.

### Invisible characters at the end of messages

- **Why**: 7TV appends `U+E0000` so a user can repeat a message; some clients add zero-width spaces.
- **Fix**: `commands::clean` strips them before parsing, otherwise `!sr <link>\u{E0000}` would fail to resolve.

### Twitch echoes our own replies

- Our reply comes back as a `channel.chat.message` event. We remember the ids returned by Send Chat Message and skip those events. Shared-chat messages from other channels are skipped too.

## Development (Windows + Git Bash + Docker)

- Git Bash rewrites `/opt/...` arguments into Windows paths: `docker exec sp /opt/yt-dlp/yt-dlp` fails. Prefix with `MSYS_NO_PATHCONV=1`.
- The official `rust` image has no rustfmt or clippy; the dev image adds them (`rustup component add rustfmt clippy`).
- A bare 11-character word made only of letters is treated as search words, not a YouTube id (`abcdefghijk`). Test ids contain a digit for that reason.
