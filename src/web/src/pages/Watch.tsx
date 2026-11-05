import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { errorText, get, post } from "../api";
import { Player } from "../components/Player";
import { ErrorBox, Notice } from "../components/ui";
import type { Live } from "../lib/live";
import type { AuthState, LiveState, NowPlaying } from "../types";

/** A YouTube id from an id or any YouTube link (same rules as the server). */
export function youtubeId(input: string): string | null {
  const s = input.trim();
  if (/^[A-Za-z0-9_-]{11}$/.test(s)) return s;
  try {
    const u = new URL(s.includes("://") ? s : `https://${s}`);
    const host = u.hostname.replace(/^(www|m|music)\./, "");
    if (host === "youtu.be") return u.pathname.slice(1, 12) || null;
    if (host === "youtube.com" || host === "youtube-nocookie.com") {
      const v = u.searchParams.get("v");
      if (v) return v;
      const m = u.pathname.match(/^\/(shorts|live|embed|v)\/([A-Za-z0-9_-]{11})/);
      return m ? m[2] : null;
    }
  } catch {
    return null;
  }
  return null;
}

/**
 * `/watch?v=<id>` — play one YouTube video right away, without the queue.
 * Signed-in users can also add it to the queue.
 */
export function Watch() {
  const [params, setParams] = useSearchParams();
  const [input, setInput] = useState(params.get("v") ?? "");
  const [signedIn, setSignedIn] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const id = youtubeId(params.get("v") ?? "");

  useEffect(() => {
    get<AuthState>("/api/auth/state")
      .then((s) => setSignedIn(Boolean(s.user)))
      .catch(() => undefined);
  }, []);

  // A one-song "live state" for the shared player component; nothing is reported.
  const live = useMemo<Live | null>(() => {
    if (!id) return null;
    const now: NowPlaying = {
      id: 0,
      requester: "",
      requester_key: "",
      source: "web",
      status: "playing",
      autoplay: false,
      requested_at: 0,
      started_at: null,
      ended_at: null,
      elapsed_ms: 0,
      clock_started: false,
      song: {
        id: 0,
        kind: "youtube",
        media_key: `youtube:${id}`,
        source_id: id,
        url: `https://www.youtube.com/watch?v=${id}`,
        title: id,
        duration_secs: null,
        thumbnail: null,
        site: "YouTube",
        favorite: false,
        blocked: false,
        missing: false,
        fail_count: 0,
        created_at: 0,
        last_played_at: null,
      },
    };
    const state: LiveState = {
      now,
      queue: [],
      player: { paused: false, volume: 100, muted: false },
      requests_open: true,
      playback: { autoplay_favorites: false, youtube_mode: "embed", stream_fallback: false },
      players_connected: 0,
      server_time_ms: Date.now(),
    };
    return { state, receivedAt: 0, connected: true, leader: false, notices: [], send: () => undefined };
  }, [id]);

  return (
    <div className="mx-auto max-w-5xl space-y-4 px-4 py-8">
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          const v = youtubeId(input);
          if (v) setParams({ v });
          else setMessage({ ok: false, text: "看不懂這個 YouTube 網址" });
        }}
      >
        <input className="input" placeholder="YouTube 網址或影片 ID" value={input} onChange={(e) => setInput(e.target.value)} />
        <button className="btn btn-primary">播放</button>
        {signedIn && id && (
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() =>
              post("/api/queue", { input: id })
                .then(() => setMessage({ ok: true, text: "已加入佇列" }))
                .catch((err) => setMessage({ ok: false, text: errorText(err) }))
            }
          >
            加入佇列
          </button>
        )}
      </form>
      {message && (message.ok ? <Notice tone="ok">{message.text}</Notice> : <ErrorBox message={message.text} />)}
      <div className="card aspect-video overflow-hidden">
        {live ? <Player live={live} /> : <div className="flex size-full items-center justify-center text-zinc-500">貼上 YouTube 網址開始播放</div>}
      </div>
    </div>
  );
}
