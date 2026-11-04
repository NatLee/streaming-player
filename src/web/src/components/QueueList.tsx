import { ArrowDown, ArrowUp, Ban, ChevronsUp, Copy, Star, Trash2, UserX, Zap } from "lucide-react";
import { clock, SOURCE_LABEL } from "../lib/format";
import type { QueueItem } from "../types";
import { Empty, Thumb } from "./ui";

export interface QueueActions {
  move: (item: QueueItem, index: number) => void;
  playNow: (item: QueueItem) => void;
  remove: (item: QueueItem, block?: "song" | "user") => void;
  favorite: (item: QueueItem) => void;
}

/** The waiting list. With `actions` it shows the admin buttons. */
export function QueueList({
  items,
  actions,
  highlight,
}: {
  items: QueueItem[];
  actions?: QueueActions;
  /** Requester names (lower case) to highlight, e.g. "my songs" on the viewer page. */
  highlight?: string | null;
}) {
  if (items.length === 0) return <Empty>佇列是空的</Empty>;
  return (
    <ol className="divide-y divide-white/6" aria-label="佇列">
      {items.map((item, i) => {
        const mine =
          highlight != null &&
          (item.requester.toLowerCase() === highlight || item.requester_key === highlight);
        return (
          <li
            key={item.id}
            className={`flex items-center gap-3 px-2 py-2.5 ${mine ? "rounded-lg bg-gold-400/10 ring-1 ring-gold-400/40" : ""}`}
          >
            <span className="w-8 shrink-0 text-center font-mono text-sm text-zinc-500 tabular-nums">#{item.position}</span>
            <Thumb src={item.song.thumbnail} />
            <div className="min-w-0 flex-1">
              <a
                href={item.song.url}
                target="_blank"
                rel="noreferrer"
                className="line-clamp-1 font-medium text-zinc-100 hover:underline"
                title={item.song.title}
              >
                {item.song.title}
              </a>
              <div className="mt-0.5 flex flex-wrap gap-x-3 text-xs text-zinc-500">
                <span className="text-zinc-300">{item.requester}</span>
                <span>{SOURCE_LABEL[item.source] ?? item.source}</span>
                <span>長度 {clock(item.song.duration_secs)}</span>
                <span>約 {clock(item.wait_secs)} 後</span>
                <span>ID {item.id}</span>
              </div>
            </div>
            {actions && (
              <div className="flex shrink-0 items-center">
                <button className="icon-btn" title="上移" disabled={i === 0} onClick={() => actions.move(item, i - 1)}>
                  <ArrowUp className="size-4" />
                </button>
                <button className="icon-btn" title="下移" disabled={i === items.length - 1} onClick={() => actions.move(item, i + 1)}>
                  <ArrowDown className="size-4" />
                </button>
                <button className="icon-btn" title="移到下一首" disabled={i === 0} onClick={() => actions.move(item, 0)}>
                  <ChevronsUp className="size-4" />
                </button>
                <button className="icon-btn" title="立即插播" onClick={() => actions.playNow(item)}>
                  <Zap className="size-4" />
                </button>
                <button
                  className={`icon-btn ${item.song.favorite ? "text-gold-400" : ""}`}
                  title={item.song.favorite ? "從預設播放清單移除" : "加入預設播放清單"}
                  onClick={() => actions.favorite(item)}
                >
                  <Star className={`size-4 ${item.song.favorite ? "fill-current" : ""}`} />
                </button>
                <button className="icon-btn" title="複製網址" onClick={() => void navigator.clipboard?.writeText(item.song.url)}>
                  <Copy className="size-4" />
                </button>
                <button className="icon-btn hover:text-red-400" title="刪除" onClick={() => actions.remove(item)}>
                  <Trash2 className="size-4" />
                </button>
                <button className="icon-btn hover:text-red-400" title="刪除並封鎖這首歌" onClick={() => actions.remove(item, "song")}>
                  <Ban className="size-4" />
                </button>
                <button className="icon-btn hover:text-red-400" title="刪除並封鎖點歌者" onClick={() => actions.remove(item, "user")}>
                  <UserX className="size-4" />
                </button>
              </div>
            )}
          </li>
        );
      })}
    </ol>
  );
}
