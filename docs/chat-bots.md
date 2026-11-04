# Chat bots (Nightbot, StreamElements, Fossabot, Streamer.bot, …)

Any chat bot that can fetch a URL can take song requests. The bot calls

```
GET {SP_PUBLIC_URL}/api/bot/{command}?key={bot key}&user={chatter}&args={what they typed}
```

and posts the plain-text answer in chat. `{command}` is one of the command words on **聊天指令** without the prefix (`sr`, `song`, `點歌`, …), so the same commands, permissions, cooldowns and reply texts apply as for the built-in Twitch and YouTube connections.

The admin page (**聊天室連線 → 聊天機器人**) shows the bot key and ready-to-paste texts for every bot below, with your address and key already filled in.

## Parameters

| Parameter | Needed | Meaning |
|---|---|---|
| `key` | yes | The bot key from the admin page. It works like a password: anyone with it can request songs as anyone. **重新產生** makes a new one. |
| `user` | unless the bot sends it | The chatter's name. Nightbot and Fossabot send it in headers by themselves. |
| `level` | no | `everyone`, `subscriber`, `vip`, `moderator` or `broadcaster`. Bot-style names work too: `owner`, `regular`, `twitch_vip`, `mod`, `member`. Default: `everyone` (Nightbot sends it in a header). |
| `args` | no | Everything after the command. `q` and `query` are accepted as well. |

Answers are always HTTP 200 with the reply text, except a wrong key (401), a missing `user` (400) and an unknown command (404). StreamElements shows its own error message for any status from 400 up.

Replies are cut at 400 characters (the Nightbot and StreamElements limit).

## Nightbot

**Commands → Custom → Add Command**, command `!sr`, message:

```
$(urlfetch https://your.host/api/bot/sr?key=KEY&args=$(querystring))
```

Nightbot sends a `Nightbot-User` header with the chatter's name and level (owner, moderator, twitch_vip, regular, subscriber, everyone), so `user` and `level` aren't needed.

## StreamElements

**Chat commands → Custom commands → Add new command**, command `sr`, response:

```
$(customapi https://your.host/api/bot/sr?key=KEY&user=$(queryescape $(sender))&args=$(queryescape $(1:)))
```

StreamElements sends no headers, so the chatter comes from `$(sender)`. To limit a command to moderators, set its permission in StreamElements.

## Fossabot

New command, response:

```
$(customapi https://your.host/api/bot/sr?key=KEY&args=$(querystring))
```

Fossabot sends the chatter's login and display name (`x-fossabot-message-userlogin`, `x-fossabot-message-userdisplayname`) but not their role. Limit moderator commands in Fossabot's own permission setting, or add a fixed `&level=moderator` to a command that only moderators can run in Fossabot.

## Streamer.bot

In an action triggered by the chat command, add a **Fetch URL** sub-action with

```
https://your.host/api/bot/sr?key=KEY&user=%user%&args=%rawInputUrlEncoded%
```

store the result in a variable, and post it with a **Send Message** sub-action.

## Making the server reachable

Bots call from the internet, so `SP_PUBLIC_URL` must be an address they can reach: a domain behind a reverse proxy (Caddy, nginx, Cloudflare Tunnel, …). The built-in Twitch and YouTube connections don't need this; they connect out from the server.
