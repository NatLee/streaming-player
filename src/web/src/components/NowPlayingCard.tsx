import { Music2, Radio, User } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { clock, SOURCE_LABEL } from "../lib/format";
import type { NowPlaying, QueueItem } from "../types";
import { Equalizer } from "./ui";

export function progressOf(elapsedMs: number, duration: number | null): number {
  if (!duration || duration <= 0) return 0;
  return Math.min(1, Math.max(0, elapsedMs / 1000 / duration));
}

/**
 * The song on air: thumbnail, title, who asked for it, and a progress bar.
 * With `onSeek` the bar can be dragged (or moved with the arrow keys) once the song
 * has started and its length is known.
 */
export function NowPlayingCard({
  now,
  elapsedMs,
  next,
  paused,
  size = "md",
  onSeek,
}: {
  now: NowPlaying | null;
  elapsedMs: number;
  next?: QueueItem | null;
  paused?: boolean;
  size?: "md" | "lg";
  /** Move the song to this position (ms); the bar holds the new spot until it settles. */
  onSeek?: (positionMs: number) => Promise<unknown> | void;
}) {
  if (!now) {
    return (
      <div className="flex items-center gap-4 rounded-2xl border border-white/8 bg-ink-900/90 p-4 text-zinc-400">
        <div className="flex size-14 items-center justify-center rounded-xl bg-ink-800">
          <Music2 className="size-6" />
        </div>
        <div>
          <div className="font-medium text-zinc-200">目前沒有歌曲</div>
          <div className="text-sm">在聊天室點歌，或從管理頁加入</div>
        </div>
      </div>
    );
  }
  const d = now.song.duration_secs;
  const big = size === "lg";
  return (
    <div className="overflow-hidden rounded-2xl border border-white/8 bg-ink-900/90 shadow-xl shadow-black/30">
      <div className={`flex gap-4 ${big ? "p-5" : "p-4"}`}>
        {now.song.thumbnail ? (
          <img
            src={now.song.thumbnail}
            alt=""
            referrerPolicy="no-referrer"
            className={`${big ? "h-28 w-48" : "h-20 w-32"} shrink-0 rounded-xl bg-ink-800 object-cover`}
          />
        ) : (
          <div
            className={`${big ? "h-28 w-48" : "h-20 w-32"} flex shrink-0 items-center justify-center rounded-xl bg-ink-800`}
          >
            <Music2 className="size-8 text-zinc-500" />
          </div>
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-xs font-medium tracking-wide text-gold-400">
            <Equalizer active={!paused && now.clock_started} />
            {paused ? "已暫停" : "正在播放"}
            <span className="text-zinc-500">· {now.song.site}</span>
          </div>
          <a
            href={now.song.url}
            target="_blank"
            rel="noreferrer"
            className={`mt-1 line-clamp-2 font-semibold text-zinc-50 hover:underline ${big ? "text-xl" : "text-base"}`}
            title={now.song.title}
          >
            {now.song.title}
          </a>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-zinc-400">
            <span className="inline-flex items-center gap-1">
              {now.autoplay ? <Radio className="size-3.5" /> : <User className="size-3.5" />}
              {now.autoplay ? "預設播放清單" : now.requester}
            </span>
            {!now.autoplay && <span className="text-zinc-500">{SOURCE_LABEL[now.source] ?? now.source}</span>}
            <span className="text-zinc-500">ID {now.id}</span>
          </div>
        </div>
      </div>
      <div className={`${big ? "px-5 pb-4" : "px-4 pb-3"}`}>
        <Progress
          key={now.id}
          elapsedMs={elapsedMs}
          duration={d}
          next={next}
          onSeek={now.clock_started && d && d > 0 ? onSeek : undefined}
        />
      </div>
    </div>
  );
}

const FILL = "h-full rounded-full bg-gradient-to-r from-gold-400 to-orange-400";

function Progress({
  elapsedMs,
  duration,
  next,
  onSeek,
}: {
  elapsedMs: number;
  duration: number | null;
  next?: QueueItem | null;
  onSeek?: (positionMs: number) => Promise<unknown> | void;
}) {
  // While the bar is held (and until the seek is through) it shows where it was let go.
  const [held, setHeld] = useState<number | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const seekRef = useRef(onSeek);
  seekRef.current = onSeek;
  const seekable = Boolean(onSeek);

  // The native "change" event fires once, when the thumb is let go or a key moves it;
  // React's onChange fires on every step of a drag.
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    const commit = () => {
      const secs = Number(el.value);
      setHeld(secs);
      Promise.resolve(seekRef.current?.(secs * 1000)).finally(() => setHeld(null));
    };
    el.addEventListener("change", commit);
    return () => el.removeEventListener("change", commit);
  }, [seekable]);

  const secs = held ?? elapsedMs / 1000;
  const pct = progressOf(secs * 1000, duration) * 100;
  return (
    <>
      {seekable && duration ? (
        <div className="group relative flex h-3 items-center">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-white/8 transition-[height] group-hover:h-2">
            <div className={FILL} style={{ width: `${pct}%` }} />
          </div>
          <div
            className="pointer-events-none absolute size-3 -translate-x-1/2 rounded-full bg-gold-300 opacity-0 shadow shadow-black/50 transition-opacity group-hover:opacity-100 group-has-[:focus-visible]:opacity-100"
            style={{ left: `${pct}%` }}
          />
          <input
            ref={input}
            type="range"
            min={0}
            max={duration}
            step={1}
            value={Math.round(secs)}
            aria-label="播放進度"
            aria-valuetext={`${clock(secs)} / ${clock(duration)}`}
            className="absolute inset-0 size-full cursor-pointer opacity-0"
            onChange={(e) => setHeld(Number(e.target.value))}
          />
        </div>
      ) : (
        <div className="h-1.5 overflow-hidden rounded-full bg-white/8">
          <div className={FILL} style={{ width: `${pct}%` }} />
        </div>
      )}
      <div className="mt-1.5 flex justify-between text-xs text-zinc-500 tabular-nums">
        <span>{clock(secs)}</span>
        {next ? <span className="truncate px-3 text-zinc-400">下一首：{next.song.title}</span> : <span />}
        <span>{clock(duration)}</span>
      </div>
    </>
  );
}
