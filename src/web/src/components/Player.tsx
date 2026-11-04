import { MousePointerClick, Music2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { errorText, get, withKey } from "../api";
import type { Live } from "../lib/live";
import { loadYouTube, YT_EMBED_ERRORS, YT_STATE, type YTPlayer } from "../lib/youtube";
import type { NowPlaying } from "../types";

/** Followers further than this from the shared clock seek to catch up (same as the server). */
const DRIFT_MS = 3000;
const PROGRESS_EVERY_MS = 5000;
/** No playback this long after loading usually means the browser blocked autoplay. */
const AUTOPLAY_GRACE_MS = 4000;

type Emit = (type: string, extra?: Record<string, unknown>) => void;

interface Common {
  entry: NowPlaying;
  /** Where to start, from the shared clock. */
  startMs: number;
  /** Where the shared clock says we should be now (followers only). */
  expectedMs: () => number | null;
  /** Changes every time a new state arrives. */
  syncTick: number;
  paused: boolean;
  volume: number;
  muted: boolean;
  emit: Emit;
}

export interface PlayerProps {
  live: Live;
  /** OBS key for media links (the page's `?key=`). */
  mediaKey?: string | null;
  /** Send start/progress/end reports when this page leads (OBS player pages). */
  report?: boolean;
  /** Always muted (admin preview). */
  forceMuted?: boolean;
  idle?: ReactNode;
}

/**
 * Plays the song on air: YouTube through the official IFrame player, everything else
 * (and YouTube in stream mode or after an embed error) from the yt-dlp link.
 */
export function Player({ live, mediaKey, report = false, forceMuted = false, idle }: PlayerProps) {
  const now = live.state?.now ?? null;
  const playback = live.state?.playback;
  const player = live.state?.player;
  const [fallbackFor, setFallbackFor] = useState<number | null>(null);

  const liveRef = useRef(live);
  liveRef.current = live;
  const nowRef = useRef(now);
  nowRef.current = now;

  const emit = useCallback<Emit>(
    (type, extra = {}) => {
      const l = liveRef.current;
      const n = nowRef.current;
      if (!report || !l.leader || !n) return;
      l.send({ type, request_id: n.id, ...extra });
    },
    [report],
  );

  const expectedMs = useCallback(() => {
    const l = liveRef.current;
    const n = l.state?.now;
    if (!n || !n.clock_started || l.leader) return null;
    const running = !l.state?.player.paused;
    return n.elapsed_ms + (running ? performance.now() - l.receivedAt : 0);
  }, []);

  if (!now || !playback || !player) return <>{idle}</>;

  const startMs = now.clock_started ? (expectedMs() ?? now.elapsed_ms) : 0;
  const common: Common = {
    entry: now,
    startMs,
    expectedMs,
    syncTick: live.receivedAt,
    paused: player.paused,
    volume: player.volume,
    muted: forceMuted || player.muted,
    emit,
  };
  const stream =
    now.song.kind !== "youtube" || playback.youtube_mode === "stream" || fallbackFor === now.id;
  if (stream) {
    return <StreamPlayer key={`s${now.id}`} {...common} mediaKey={mediaKey} />;
  }
  return (
    <YouTubePlayer
      key={`y${now.id}`}
      {...common}
      onEmbedError={(code) => {
        const reason = `YouTube 錯誤 ${code}：${YT_EMBED_ERRORS[code] ?? "未知錯誤"}`;
        if (playback.stream_fallback) {
          setFallbackFor(now.id);
        } else {
          emit("error", { reason });
        }
      }}
    />
  );
}

function ClickToPlay({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-black/70 text-zinc-100"
    >
      <MousePointerClick className="size-10 text-gold-400" />
      <span className="text-lg font-semibold">點一下開始播放</span>
      <span className="text-sm text-zinc-400">瀏覽器擋住了自動播放；OBS 裡不會出現這個畫面</span>
    </button>
  );
}

function useAutoplayWatch(entryId: number, paused: boolean) {
  const [blocked, setBlocked] = useState(false);
  const playing = useRef(false);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  useEffect(() => {
    const t = window.setTimeout(() => {
      if (!playing.current && !pausedRef.current) setBlocked(true);
    }, AUTOPLAY_GRACE_MS);
    return () => window.clearTimeout(t);
  }, [entryId]);
  const markPlaying = () => {
    playing.current = true;
    setBlocked(false);
  };
  return { blocked, setBlocked, markPlaying, playing };
}

function YouTubePlayer(props: Common & { onEmbedError: (code: number) => void }) {
  const { entry, startMs, expectedMs, syncTick, paused, volume, muted, emit, onEmbedError } = props;
  const host = useRef<HTMLDivElement>(null);
  const yt = useRef<YTPlayer | null>(null);
  const started = useRef(false);
  const watch = useAutoplayWatch(entry.id, paused);
  const latest = useRef({ paused, volume, muted, onEmbedError });
  latest.current = { paused, volume, muted, onEmbedError };

  useEffect(() => {
    let cancelled = false;
    let instance: YTPlayer | null = null;
    loadYouTube()
      .then((YT) => {
        if (cancelled || !host.current) return;
        // The API replaces this element with its iframe.
        const el = document.createElement("div");
        host.current.appendChild(el);
        instance = new YT.Player(el, {
          videoId: entry.song.source_id,
          width: "100%",
          height: "100%",
          playerVars: {
            autoplay: 1,
            controls: 0,
            disablekb: 1,
            fs: 0,
            iv_load_policy: 3,
            playsinline: 1,
            rel: 0,
            start: Math.floor(startMs / 1000),
            origin: window.location.origin,
          },
          events: {
            onReady: (e) => {
              yt.current = e.target;
              const l = latest.current;
              e.target.setVolume(l.volume);
              if (l.muted) e.target.mute();
              else e.target.unMute();
              if (!l.paused) e.target.playVideo();
            },
            onStateChange: (e) => {
              if (e.data === YT_STATE.PLAYING) {
                watch.markPlaying();
                if (!started.current) {
                  started.current = true;
                  emit("started", { position_ms: Math.round(e.target.getCurrentTime() * 1000) });
                  const d = e.target.getDuration();
                  if (entry.song.duration_secs == null && d > 0) {
                    emit("duration", { secs: Math.round(d) });
                  }
                }
              } else if (e.data === YT_STATE.ENDED) {
                emit("ended");
              }
            },
            onError: (e) => latest.current.onEmbedError(e.data),
          },
        });
      })
      .catch((err) => emit("error", { reason: errorText(err) }));
    return () => {
      cancelled = true;
      instance?.destroy();
      yt.current = null;
    };
    // A new player per song; everything else is applied by the effects below.
  }, [entry.id]);

  useEffect(() => {
    const p = yt.current;
    if (!p) return;
    p.setVolume(volume);
    if (muted) p.mute();
    else p.unMute();
  }, [volume, muted]);

  useEffect(() => {
    const p = yt.current;
    if (!p) return;
    if (paused) p.pauseVideo();
    else p.playVideo();
  }, [paused]);

  useEffect(() => {
    const t = window.setInterval(() => {
      const p = yt.current;
      if (p && p.getPlayerState() === YT_STATE.PLAYING) {
        emit("progress", { position_ms: Math.round(p.getCurrentTime() * 1000) });
      }
    }, PROGRESS_EVERY_MS);
    return () => window.clearInterval(t);
  }, [emit]);

  useEffect(() => {
    const p = yt.current;
    const expected = expectedMs();
    if (!p || expected == null) return;
    if (Math.abs(p.getCurrentTime() * 1000 - expected) > DRIFT_MS) p.seekTo(expected / 1000, true);
  }, [syncTick, expectedMs]);

  return (
    <div className="relative size-full bg-black" data-testid="youtube-player">
      <div ref={host} className="size-full [&>iframe]:size-full" />
      {watch.blocked && (
        <ClickToPlay
          onClick={() => {
            yt.current?.playVideo();
            watch.setBlocked(false);
          }}
        />
      )}
    </div>
  );
}

interface PlaySource {
  kind: "file" | "hls";
  url: string;
  /** No picture (SoundCloud, or sites with separate video/audio tracks such as Bilibili). */
  audio_only: boolean;
}

function StreamPlayer(props: Common & { mediaKey?: string | null }) {
  const { entry, startMs, expectedMs, syncTick, paused, volume, muted, emit, mediaKey } = props;
  const video = useRef<HTMLVideoElement>(null);
  const started = useRef(false);
  const failed = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [audioOnly, setAudioOnly] = useState(false);
  const watch = useAutoplayWatch(entry.id, paused);

  const fail = useCallback(
    (reason: string) => {
      if (failed.current) return;
      failed.current = true;
      setError(reason);
      emit("error", { reason });
    },
    [emit],
  );

  useEffect(() => {
    let cancelled = false;
    let hls: { destroy(): void } | null = null;
    const v = video.current;
    get<PlaySource>(withKey(`/api/media/${entry.song.id}/source`, mediaKey))
      .then(async (src) => {
        if (cancelled || !v) return;
        setAudioOnly(src.audio_only);
        if (src.kind === "hls" && !v.canPlayType("application/vnd.apple.mpegurl")) {
          const { default: Hls } = await import("hls.js");
          if (cancelled) return;
          const h = new Hls();
          h.on(Hls.Events.ERROR, (_e, data) => {
            if (data.fatal) fail(`HLS：${data.details}`);
          });
          h.loadSource(src.url);
          h.attachMedia(v);
          hls = h;
        } else {
          v.src = src.kind === "file" ? withKey(src.url, mediaKey) : src.url;
        }
      })
      .catch((err) => fail(errorText(err)));
    return () => {
      cancelled = true;
      hls?.destroy();
      if (v) {
        v.removeAttribute("src");
        v.load();
      }
    };
  }, [entry.id]);

  useEffect(() => {
    const v = video.current;
    if (!v) return;
    v.volume = Math.min(1, Math.max(0, volume / 100));
    v.muted = muted;
  }, [volume, muted]);

  useEffect(() => {
    const v = video.current;
    if (!v || !v.src) return;
    if (paused) v.pause();
    else v.play().catch(() => watch.setBlocked(true));
  }, [paused]);

  useEffect(() => {
    const t = window.setInterval(() => {
      const v = video.current;
      if (v && !v.paused && !v.ended) emit("progress", { position_ms: Math.round(v.currentTime * 1000) });
    }, PROGRESS_EVERY_MS);
    return () => window.clearInterval(t);
  }, [emit]);

  useEffect(() => {
    const v = video.current;
    const expected = expectedMs();
    if (!v || expected == null || v.readyState < 1) return;
    if (Math.abs(v.currentTime * 1000 - expected) > DRIFT_MS) v.currentTime = expected / 1000;
  }, [syncTick, expectedMs]);

  return (
    <div className="relative size-full bg-black" data-testid="stream-player">
      <video
        ref={video}
        className="size-full object-contain"
        autoPlay={!paused}
        playsInline
        onLoadedMetadata={(e) => {
          const v = e.currentTarget;
          if (startMs > 1000) v.currentTime = startMs / 1000;
          v.volume = Math.min(1, Math.max(0, volume / 100));
          v.muted = muted;
          if (entry.song.duration_secs == null && Number.isFinite(v.duration) && v.duration > 0) {
            emit("duration", { secs: Math.round(v.duration) });
          }
          if (!paused) v.play().catch(() => watch.setBlocked(true));
        }}
        onPlaying={(e) => {
          watch.markPlaying();
          if (!started.current) {
            started.current = true;
            emit("started", { position_ms: Math.round(e.currentTarget.currentTime * 1000) });
          }
        }}
        onEnded={() => emit("ended")}
        onError={(e) => {
          const code = e.currentTarget.error?.code;
          if (e.currentTarget.getAttribute("src")) fail(`影片載入失敗（代碼 ${code ?? "?"}）`);
        }}
      />
      {audioOnly && (
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-[4vh] bg-gradient-to-br from-ink-850 to-black p-[6vh]">
          {entry.song.thumbnail ? (
            <img src={entry.song.thumbnail} alt="" referrerPolicy="no-referrer" className="max-h-[55%] max-w-[70%] rounded-2xl object-contain shadow-2xl shadow-black" />
          ) : (
            <Music2 className="size-[20vh] text-gold-400" />
          )}
          <div className="line-clamp-2 max-w-[80%] text-center text-[4vh] font-semibold text-zinc-100">{entry.song.title}</div>
        </div>
      )}
      {error && (
        <div className="absolute inset-x-0 bottom-0 bg-red-950/80 px-4 py-2 text-sm text-red-100">{error}</div>
      )}
      {watch.blocked && !error && (
        <ClickToPlay
          onClick={() => {
            video.current?.play().catch(() => undefined);
            watch.setBlocked(false);
          }}
        />
      )}
    </div>
  );
}
