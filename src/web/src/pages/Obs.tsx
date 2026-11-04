import { Music2 } from "lucide-react";
import { useEffect, useState } from "react";
import { NowPlayingCard } from "../components/NowPlayingCard";
import { Player } from "../components/Player";
import { pageKey, useElapsed, useLive } from "../lib/live";

const TOAST_MS = 8000;

function useTransparentBody() {
  useEffect(() => {
    document.body.classList.add("transparent");
    return () => document.body.classList.remove("transparent");
  }, []);
}

function flag(name: string, fallback: boolean): boolean {
  const v = new URLSearchParams(window.location.search).get(name);
  return v == null ? fallback : !["0", "false", "no", "off"].includes(v.toLowerCase());
}

/**
 * `/obs/player?key=…` — the video for an OBS browser source. The oldest open player page
 * leads: it reports when songs start and end, so the queue moves exactly once.
 */
export function ObsPlayer() {
  const key = pageKey();
  const live = useLive("player", key);
  const transparent = flag("transparent", false);
  useTransparentBody();
  return (
    <div className={`fixed inset-0 overflow-hidden ${transparent ? "" : "bg-black"}`}>
      <Player
        live={live}
        mediaKey={key}
        report
        idle={
          transparent ? null : (
            <div className="flex size-full flex-col items-center justify-center gap-3 text-zinc-500">
              <Music2 className="size-12" />
              <div className="text-xl">{live.connected ? "目前沒有歌曲" : "連線中…"}</div>
            </div>
          )
        }
      />
      {flag("debug", false) && (
        <div className="absolute top-2 left-2 rounded bg-black/70 px-2 py-1 font-mono text-xs text-zinc-300">
          {live.connected ? "connected" : "offline"} · {live.leader ? "leader" : "follower"}
        </div>
      )}
    </div>
  );
}

/**
 * `/obs/overlay?key=…` — transparent "now playing" card and request toasts.
 * Options: `next=0` hides the next song, `toasts=0` hides the request pop-ups.
 */
export function ObsOverlay() {
  const live = useLive("overlay", pageKey());
  const elapsed = useElapsed(live);
  useTransparentBody();
  const showNext = flag("next", true);
  const showToasts = flag("toasts", true);
  const now = live.state?.now ?? null;
  // Tick once a second so toasts disappear after TOAST_MS.
  const [, tick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, []);
  const recent = live.notices.filter((n) => Date.now() - n.at < TOAST_MS);

  return (
    <div className="flex min-h-screen flex-col justify-end gap-3 p-4">
      {showToasts && (
        <div className="flex flex-col items-start gap-2">
          {recent.map((n) => (
            <div key={n.at} className="toast-in rounded-xl border border-gold-400/30 bg-ink-900/90 px-4 py-2 text-sm text-zinc-100 shadow-lg">
              {n.text}
            </div>
          ))}
        </div>
      )}
      {now && (
        <div className="w-full max-w-xl">
          <NowPlayingCard now={now} elapsedMs={elapsed} next={showNext ? live.state?.queue[0] : null} paused={live.state?.player.paused} />
        </div>
      )}
    </div>
  );
}
