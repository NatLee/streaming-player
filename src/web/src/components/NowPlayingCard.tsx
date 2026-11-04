import { Music2, Radio, User } from "lucide-react";
import { clock, SOURCE_LABEL } from "../lib/format";
import type { NowPlaying, QueueItem } from "../types";
import { Equalizer } from "./ui";

export function progressOf(elapsedMs: number, duration: number | null): number {
  if (!duration || duration <= 0) return 0;
  return Math.min(1, Math.max(0, elapsedMs / 1000 / duration));
}

/** The song on air: thumbnail, title, who asked for it, and a progress bar. */
export function NowPlayingCard({
  now,
  elapsedMs,
  next,
  paused,
  size = "md",
}: {
  now: NowPlaying | null;
  elapsedMs: number;
  next?: QueueItem | null;
  paused?: boolean;
  size?: "md" | "lg";
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
  const pct = progressOf(elapsedMs, d);
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
        <div className="h-1.5 overflow-hidden rounded-full bg-white/8">
          <div className="h-full rounded-full bg-gradient-to-r from-gold-400 to-orange-400" style={{ width: `${pct * 100}%` }} />
        </div>
        <div className="mt-1.5 flex justify-between text-xs text-zinc-500 tabular-nums">
          <span>{clock(elapsedMs / 1000)}</span>
          {next ? (
            <span className="truncate px-3 text-zinc-400">下一首：{next.song.title}</span>
          ) : (
            <span />
          )}
          <span>{clock(d)}</span>
        </div>
      </div>
    </div>
  );
}
