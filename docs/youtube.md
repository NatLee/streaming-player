# YouTube live chat

The server can read the chat of your YouTube live stream through the **YouTube Live Streaming API**, with an **API key** only. No Google sign-in is needed.

The same key also lets the server look up title and length of requested YouTube videos (`videos.list`, 1 quota unit each) instead of asking yt-dlp.

## Limits to know first

- **Read-only.** An API key can read a public chat but can't post in it. Replies to chat commands appear in the dashboard's activity log (**最近動態**), and accepted requests pop up on the OBS overlay, but nothing is written back to YouTube chat.
- **Quota.** A Google Cloud project gets **10,000 units a day** by default, and every chat poll (`liveChatMessages.list`) costs **5 units**. The poll interval is a setting:

  | Poll every | Units per hour | Hours per day of quota |
  |---|---|---|
  | 10 s | 1,800 | 5.5 |
  | 20 s (default) | 900 | 11 |
  | 30 s | 600 | 16 |

  The server never polls faster than YouTube's own suggested interval. When the quota runs out it waits an hour and says so on the status.
- **Only the live chat.** The video has to be live. Before the stream starts, the server checks every 2 minutes (1 unit each).

## 1. Get an API key (once)

1. Open the [Google Cloud console](https://console.cloud.google.com/) and create a project (or pick one).
2. **APIs & Services → Library**: enable **YouTube Data API v3**.
3. **APIs & Services → Credentials → Create credentials → API key**.
4. Recommended: **Edit API key → API restrictions → Restrict key → YouTube Data API v3**.

## 2. Connect

1. On the admin page open **聊天室連線 → YouTube 直播聊天室**.
2. Paste the API key and press its **儲存** button. The page never shows the key again, only "已設定".
3. **直播網址**: the live video's URL or id, e.g. `https://www.youtube.com/live/xxxxxxxxxxx`. Change it for each new stream.
4. Pick the poll interval, turn on **啟用 YouTube 聊天室**, press **儲存設定**.
5. The status shows **已連線** while the stream is live and **等待中** before it starts or after it ends.

Chat from before the connection is ignored, so old commands don't run twice after a restart.

## Roles

| YouTube | Level |
|---|---|
| Channel owner | broadcaster |
| Moderator | moderator |
| Member (sponsor) | subscriber |
| Everyone else | everyone |

A YouTube chatter's identity is their channel id (`yt:UC…`), so two people with the same display name don't share limits. Their display name is what shows in the queue.

## Not done

YouTube now also offers `liveChatMessages.streamList`, a long-lived streaming connection that would avoid most of the polling cost. Its REST framing isn't documented clearly enough to build on without a live stream to test against, so the server uses the documented `list` call for now.
