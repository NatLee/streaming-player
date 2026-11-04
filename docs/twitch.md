# Twitch chat

The server reads your Twitch chat and replies in it by itself, with no third-party bot. It uses what Twitch documents as "the preferred method of viewing and sending chats": **EventSub** over WebSocket for incoming messages (`channel.chat.message`) and the **Send Chat Message** API for replies. IRC is not used.

Sign-in uses Twitch's **Device Code Grant** flow: you type a short code on twitch.tv, and no redirect URL or public address is needed. This works for a server on `localhost`.

## 1. Create a Twitch application (once)

1. Go to the [Twitch developer console](https://dev.twitch.tv/console/apps) and choose **Register Your Application**.
2. **Name**: anything, e.g. `my-song-requests`.
3. **OAuth Redirect URLs**: `http://localhost` (the device flow doesn't use it, but the form needs one).
4. **Category**: `Chat Bot`.
5. **Client Type**: **Public**. A public client signs in and refreshes tokens without a client secret, so no secret ever sits on the server.
6. Create it and copy the **Client ID**.

## 2. Connect

1. On the admin page open **聊天室連線 → Twitch**.
2. Paste the Client ID. (Or set `SP_TWITCH_CLIENT_ID` for the container.)
3. **頻道**: leave it empty to read the signed-in account's own channel. To use a separate bot account, sign in as the bot and put your channel's login here.
4. Turn on **啟用 Twitch 聊天室**, press **儲存設定**, then **登入 Twitch**.
5. A code appears. Open <https://www.twitch.tv/activate>, enter it, and approve the two permissions:
   - `user:read:chat`: read chat messages
   - `user:write:chat`: send the replies
6. Within a few seconds the status turns **已連線**.

Type `!sr https://youtu.be/…` in your chat to try it.

## Roles

Each chatter's role comes from their Twitch badges and maps to the permission levels used by commands:

| Twitch badge | Level |
|---|---|
| broadcaster (or the channel owner) | broadcaster |
| moderator | moderator |
| vip | vip |
| subscriber, founder | subscriber |
| anything else | everyone |

Set who may use each command on **聊天指令**.

## Good to know

- **Bot badge**: replies are sent with a user token, so Twitch doesn't show the "Chat Bot" badge. Twitch only gives that badge to apps using an app access token with the broadcaster's `channel:bot` grant, which needs a client secret. That's not supported here on purpose.
- **Tokens**: the access token lasts about 4 hours and refreshes itself. A public client's refresh token stops working after 30 days without use; then the status shows **需要設定**, and you sign in again.
- **Shared chat**: messages from the other channels in a shared chat session are ignored.
- **Your own messages**: if you sign in as yourself, your own commands work. The server skips its own replies, so they can't trigger commands.
- Tokens are stored in the SQLite database in the data volume. **登出** deletes them.
