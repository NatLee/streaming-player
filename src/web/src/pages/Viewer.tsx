import { Search } from "lucide-react";
import { useEffect, useState } from "react";
import { errorText, get } from "../api";
import { NowPlayingCard } from "../components/NowPlayingCard";
import { QueueList } from "../components/QueueList";
import { ErrorBox, Loading, Thumb } from "../components/ui";
import { STATUS_LABEL, when } from "../lib/format";
import { useElapsed, useLive } from "../lib/live";
import type { Entry, Meta, Page } from "../types";

interface Mine {
  pending: Entry[];
  history: Page<Entry>;
}

/** `/queue` — public page: what's playing, what's next, and "my songs". */
export function Viewer() {
  const [meta, setMeta] = useState<Meta | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    get<Meta>("/api/meta").then(setMeta).catch((e) => setError(errorText(e)));
  }, []);
  if (error) return <div className="p-8"><ErrorBox message={error} /></div>;
  if (!meta) return <Loading />;
  if (!meta.public_queue) {
    return <div className="flex min-h-screen items-center justify-center text-zinc-400">實況主沒有開放觀眾頁</div>;
  }
  return <ViewerBody />;
}

function ViewerBody() {
  const live = useLive("viewer");
  const elapsed = useElapsed(live);
  const [name, setName] = useState(() => localStorage.getItem("sp-viewer-name") ?? "");
  const [query, setQuery] = useState(name);
  const [mine, setMine] = useState<Mine | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Look again when the queue changes, not on every progress update.
  const mineSignature = [live.state?.now?.id, ...(live.state?.queue ?? []).map((q) => q.id)].join(",");

  useEffect(() => {
    if (!query) {
      setMine(null);
      return;
    }
    get<Mine>(`/api/public/requests?user=${encodeURIComponent(query)}`)
      .then(setMine)
      .catch((e) => setError(errorText(e)));
  }, [query, mineSignature]);

  const state = live.state;
  return (
    <div className="mx-auto max-w-3xl px-4 py-8">
      <header className="mb-6 flex items-center gap-3">
        <img src="/favicon.svg" alt="" className="size-10" />
        <div>
          <h1 className="text-xl font-semibold">點歌佇列</h1>
          <p className="text-sm text-zinc-400">
            {state ? (state.requests_open ? "開放點歌中" : "目前沒有開放點歌") : "連線中…"}
          </p>
        </div>
      </header>
      {state ? (
        <div className="space-y-5">
          <NowPlayingCard now={state.now} elapsedMs={elapsed} paused={state.player.paused} size="lg" />
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              localStorage.setItem("sp-viewer-name", name.trim());
              setQuery(name.trim().toLowerCase());
            }}
          >
            <input className="input" placeholder="輸入你在聊天室的名字，查自己點的歌" value={name} onChange={(e) => setName(e.target.value)} />
            <button className="btn btn-primary">
              <Search className="size-4" /> 查詢
            </button>
          </form>
          {error && <ErrorBox message={error} />}
          {mine && (
            <section className="card p-4">
              <h2 className="mb-2 font-semibold">我點的歌</h2>
              {mine.pending.length === 0 && mine.history.items.length === 0 && (
                <p className="text-sm text-zinc-500">找不到這個名字的點歌紀錄</p>
              )}
              <ul className="divide-y divide-white/5">
                {[...mine.pending, ...mine.history.items].map((e) => (
                  <li key={e.id} className="flex items-center gap-3 py-2">
                    <Thumb src={e.song.thumbnail} className="h-9 w-14" />
                    <span className="line-clamp-1 flex-1 text-sm">{e.song.title}</span>
                    <span className="text-xs text-zinc-500">{when(e.ended_at ?? e.requested_at)}</span>
                    <span className={`text-xs ${e.status === "queued" || e.status === "playing" ? "text-gold-300" : "text-zinc-400"}`}>
                      {STATUS_LABEL[e.status]}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
          <section className="card p-3">
            <h2 className="mb-1 px-2 font-semibold">
              接下來 <span className="text-sm font-normal text-zinc-500">{state.queue.length} 首</span>
            </h2>
            <QueueList items={state.queue} highlight={query || null} />
          </section>
        </div>
      ) : (
        <Loading label="連線中…" />
      )}
    </div>
  );
}
