# OBS setup

Two browser sources: the **player** (video and sound) and the **overlay** (a transparent "now playing" card with request pop-ups). Both URLs are on the admin page under **設定 → OBS**; they carry the OBS key (`?key=…`), so keep them off screen.

## 1. Player

1. In OBS: **Sources → + → Browser**, name it e.g. `Song player`.
2. **URL**: the player link, e.g. `http://localhost:7878/obs/player?key=…`
3. **Width / Height**: `1920` × `1080` (or the size you show it at; the video keeps its aspect ratio).
4. Tick **Control audio via OBS**. The sound then shows up in the OBS audio mixer as its own source, so you set its volume (and monitoring) there like any other source.
5. Untick **Shutdown source when not visible** if you hide the source but still want music. When OBS shuts the source down, that player disconnects and the queue waits.
6. Leave **Refresh browser when scene becomes active** off.

OBS's browser plays with sound without a click, so autoplay works there. A normal browser tab blocks autoplay with sound; the page then shows **點一下開始播放** ("click to start"), which never appears in OBS.

Options (add to the URL):

| Option | Effect |
|---|---|
| `&transparent=1` | No black background and no "no song" text: handy for audio-only scenes |
| `&debug=1` | Small corner label: connected or not, leader or follower |

## 2. Overlay

1. **Sources → + → Browser**, e.g. `Now playing`.
2. **URL**: the overlay link, e.g. `http://localhost:7878/obs/overlay?key=…`
3. **Width / Height**: about `640` × `260`. The background is transparent; the card sits at the bottom and new requests pop up above it.
4. No audio settings needed; the overlay makes no sound.

Options: `&next=0` hides the "next song" line, `&toasts=0` hides the request pop-ups.

## Several devices, one queue

Every open player page (OBS on the stream PC, a laptop, a phone) shows the same song at the same position. They stay in sync over a WebSocket:

- The **oldest connected player page leads**. It reports when a song starts, where it is every 5 seconds, and when it ends. The server moves the queue only on the leader's reports, and each report names the song, so two pages can never skip two songs.
- The other pages **follow**: when they drift more than 3 seconds from the leader, they seek.
- Close the leader and the next oldest page takes over within a second.
- With no player page open, nothing advances. The dashboard warns about this ("沒有播放頁連線").

Pause, resume, skip and volume on the admin page (or from chat moderators) reach every player page at once.

## Volume

There are two volume controls on purpose:

- **OBS mixer**: what your stream hears. Use this one while live.
- **Admin page slider**: the player's own volume (0–100), shared by all player pages. Leave it at 100 for OBS and use it for the other devices, or to mute every player at once.

## Troubleshooting

- **"YouTube 錯誤 150/101"**: the uploader doesn't allow embedding. With **嵌入失敗時改用 yt-dlp 直接連結** on (default), the player switches to the direct link by itself.
- **"YouTube 錯誤 153"**: the browser sent no Referer. The server sends `Referrer-Policy: strict-origin-when-cross-origin` so this doesn't happen; a reverse proxy that sets `no-referrer` brings it back.
- **The song never moves on**: open the player URL with `&debug=1` and check it says `leader`. The dashboard shows how many player pages are connected.
- **Bilibili / SoundCloud plays no sound or stops**: update yt-dlp on **設定 → 系統 → 更新 yt-dlp**. Sites change their pages often.
