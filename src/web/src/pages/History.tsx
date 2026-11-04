import { useCallback, useEffect, useState } from "react";
import { errorText, get } from "../api";
import { Empty, ErrorBox, Loading, PageHeader, Thumb } from "../components/ui";
import { clock, SOURCE_LABEL, STATUS_LABEL, when } from "../lib/format";
import type { Entry, Page, UserStats } from "../types";

const STATUS_TONE: Record<string, string> = {
  played: "text-emerald-300",
  skipped: "text-amber-300",
  removed: "text-zinc-400",
  failed: "text-red-300",
};

export function StatsCard({ stats }: { stats: UserStats }) {
  const percent = stats.total ? Math.round((100 * stats.played) / stats.total) : 0;
  const items = [
    ["點歌次數", `${stats.total}`],
    ["播完比例", `${percent}%`],
    ["播完的總長度", clock(stats.played_secs)],
    ["不重複的歌", `${stats.unique_songs}`],
  ];
  return (
    <div className="card p-4">
      <div className="mb-3 text-sm text-zinc-400">
        <span className="font-semibold text-zinc-100">{stats.requester}</span> 最常點：
        <span className="text-gold-300">{stats.top_title}</span>（{stats.top_count} 次）
      </div>
      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {items.map(([k, v]) => (
          <div key={k} className="rounded-lg bg-white/4 px-3 py-2">
            <dt className="text-xs text-zinc-500">{k}</dt>
            <dd className="text-lg font-semibold tabular-nums">{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export function HistoryPage() {
  const [user, setUser] = useState("");
  const [applied, setApplied] = useState("");
  const [page, setPage] = useState<Page<Entry> | null>(null);
  const [stats, setStats] = useState<UserStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(50);

  const load = useCallback(() => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (applied) params.set("user", applied);
    get<Page<Entry>>(`/api/history?${params}`)
      .then(setPage)
      .catch((e) => setError(errorText(e)));
    setStats(null);
    if (applied) {
      get<UserStats>(`/api/history/stats?user=${encodeURIComponent(applied)}`)
        .then(setStats)
        .catch(() => setStats(null));
    }
  }, [applied, limit]);
  useEffect(load, [load]);

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader title="點歌歷史" description="播完、跳過、刪除和播放失敗的點歌紀錄。輸入名字可以看一個人的點歌統計（和聊天室的 !songstats 一樣）。" />
      <form
        className="mb-4 flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          setApplied(user.trim());
        }}
      >
        <input className="input max-w-xs" placeholder="點歌者名稱" value={user} onChange={(e) => setUser(e.target.value)} />
        <button className="btn btn-secondary">篩選</button>
        {applied && (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => {
              setUser("");
              setApplied("");
            }}
          >
            清除
          </button>
        )}
      </form>
      {error && <ErrorBox message={error} />}
      {stats && <div className="mb-4"><StatsCard stats={stats} /></div>}
      {!page ? (
        <Loading />
      ) : page.items.length === 0 ? (
        <Empty>還沒有紀錄</Empty>
      ) : (
        <div className="card divide-y divide-white/5">
          {page.items.map((e) => (
            <div key={e.id} className="flex items-center gap-3 px-4 py-2.5">
              <Thumb src={e.song.thumbnail} className="h-10 w-16" />
              <div className="min-w-0 flex-1">
                <a href={e.song.url} target="_blank" rel="noreferrer" className="line-clamp-1 font-medium hover:underline">
                  {e.song.title}
                </a>
                <div className="flex flex-wrap gap-x-3 text-xs text-zinc-500">
                  <button className="text-zinc-300 hover:underline" onClick={() => { setUser(e.requester); setApplied(e.requester); }}>
                    {e.autoplay ? "預設播放清單" : e.requester}
                  </button>
                  <span>{SOURCE_LABEL[e.source] ?? e.source}</span>
                  <span>{when(e.ended_at ?? e.requested_at)}</span>
                </div>
              </div>
              <span className={`text-sm ${STATUS_TONE[e.status] ?? ""}`}>{STATUS_LABEL[e.status] ?? e.status}</span>
            </div>
          ))}
        </div>
      )}
      {page && page.total > page.items.length && (
        <button className="btn btn-secondary mt-4" onClick={() => setLimit(limit + 50)}>
          載入更多（共 {page.total} 筆）
        </button>
      )}
    </div>
  );
}
