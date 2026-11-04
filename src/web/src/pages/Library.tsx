import { Ban, Download, ListPlus, RotateCcw, Star, Trash2, Upload } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { del, errorText, get, patch, post } from "../api";
import { ConfirmButton, Empty, ErrorBox, Loading, Notice, PageHeader, Thumb } from "../components/ui";
import { clock, when } from "../lib/format";
import type { LibrarySong, Page } from "../types";

const FILTERS = [
  { value: "", label: "全部" },
  { value: "favorite", label: "預設播放清單" },
  { value: "blocked", label: "黑名單" },
  { value: "missing", label: "不見了" },
];

interface Record_ {
  url: string;
  title: string;
  duration_secs: number | null;
  favorite: boolean;
  blocked: boolean;
}

/** Parse the CSV this app exports (quoted fields allowed). */
export function parseCsv(text: string): Record_[] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"') quoted = true;
    else if (c === ",") {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else field += c;
  }
  if (field || row.length) {
    row.push(field);
    rows.push(row);
  }
  const [header, ...body] = rows;
  if (!header) return [];
  const col = (name: string) => header.indexOf(name);
  return body
    .filter((r) => r.some((f) => f.trim()))
    .map((r) => ({
      url: r[col("url")] ?? "",
      title: r[col("title")] ?? "",
      duration_secs: r[col("duration_secs")] ? Number(r[col("duration_secs")]) : null,
      favorite: r[col("favorite")] === "true",
      blocked: r[col("blocked")] === "true",
    }));
}

export function LibraryPage() {
  const [filter, setFilter] = useState("");
  const [q, setQ] = useState("");
  const [page, setPage] = useState<Page<LibrarySong> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [favorite, setFavorite] = useState(true);
  const file = useRef<HTMLInputElement>(null);

  const load = useCallback(() => {
    const params = new URLSearchParams({ limit: "200" });
    if (filter) params.set("filter", filter);
    if (q.trim()) params.set("q", q.trim());
    get<Page<LibrarySong>>(`/api/library?${params}`)
      .then(setPage)
      .catch((e) => setError(errorText(e)));
  }, [filter, q]);
  useEffect(load, [load]);

  const act = async (fn: () => Promise<unknown>, done?: string) => {
    setError(null);
    try {
      await fn();
      if (done) setNote(done);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const importFile = async (f: File) => {
    const text = await f.text();
    const records = f.name.endsWith(".csv") ? parseCsv(text) : JSON.parse(text);
    const r = await post<{ imported: number; skipped: number }>("/api/library/import", records);
    setNote(`匯入 ${r.imported} 首${r.skipped ? `，略過 ${r.skipped} 筆看不懂的網址` : ""}`);
    load();
  };

  return (
    <div className="mx-auto max-w-6xl">
      <PageHeader
        title="歌曲庫"
        description="所有點過的歌。打星號的歌組成「預設播放清單」：佇列空了就從這裡隨機播放。黑名單的歌不能點，「不見了」是連續播放失敗太多次的歌。"
        actions={
          <>
            <a className="btn btn-secondary" href="/api/library/export?format=csv">
              <Download className="size-4" /> 匯出 CSV
            </a>
            <a className="btn btn-secondary" href="/api/library/export">
              <Download className="size-4" /> 匯出 JSON
            </a>
            <button className="btn btn-secondary" onClick={() => file.current?.click()}>
              <Upload className="size-4" /> 匯入
            </button>
            <input
              ref={file}
              type="file"
              accept=".json,.csv"
              hidden
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) importFile(f).catch((err) => setError(errorText(err)));
                e.target.value = "";
              }}
            />
          </>
        }
      />
      <form
        className="card mb-4 flex flex-wrap items-center gap-3 p-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (input.trim()) act(() => post("/api/library", { input, favorite }), "已加入歌曲庫").then(() => setInput(""));
        }}
      >
        <input className="input min-w-64 flex-1" placeholder="加入歌曲：網址或搜尋文字" value={input} onChange={(e) => setInput(e.target.value)} />
        <label className="flex items-center gap-2 text-sm text-zinc-300">
          <input type="checkbox" checked={favorite} onChange={(e) => setFavorite(e.target.checked)} className="accent-gold-400" />
          加入預設播放清單
        </label>
        <button className="btn btn-primary">加入</button>
      </form>
      {error && <ErrorBox message={error} />}
      {note && <Notice tone="ok">{note}</Notice>}
      <div className="my-4 flex flex-wrap items-center gap-2">
        {FILTERS.map((f) => (
          <button key={f.value} className={`btn ${filter === f.value ? "btn-primary" : "btn-secondary"}`} onClick={() => setFilter(f.value)}>
            {f.label}
          </button>
        ))}
        <input className="input ml-auto w-64" placeholder="搜尋標題或網址" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      {!page ? (
        <Loading />
      ) : page.items.length === 0 ? (
        <Empty>沒有符合的歌</Empty>
      ) : (
        <div className="card overflow-x-auto">
          <table className="w-full">
            <thead className="border-b border-white/6">
              <tr>
                <th className="th">歌曲</th>
                <th className="th">長度</th>
                <th className="th">播完／點播</th>
                <th className="th">最後播放</th>
                <th className="th text-right">動作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {page.items.map((s) => (
                <tr key={s.id} className={s.blocked || s.missing ? "opacity-60" : ""}>
                  <td className="td">
                    <div className="flex items-center gap-3">
                      <Thumb src={s.thumbnail} className="h-10 w-16" />
                      <div className="min-w-0">
                        <a href={s.url} target="_blank" rel="noreferrer" className="line-clamp-1 font-medium hover:underline">
                          {s.title}
                        </a>
                        <div className="flex gap-2 text-xs text-zinc-500">
                          <span>{s.site}</span>
                          {s.blocked && <span className="text-red-400">黑名單</span>}
                          {s.missing && <span className="text-amber-400">不見了（失敗 {s.fail_count} 次）</span>}
                        </div>
                      </div>
                    </div>
                  </td>
                  <td className="td tabular-nums text-zinc-400">{clock(s.duration_secs)}</td>
                  <td className="td tabular-nums text-zinc-400">
                    {s.plays}／{s.requests}
                  </td>
                  <td className="td text-zinc-400">{when(s.last_played_at)}</td>
                  <td className="td">
                    <div className="flex justify-end">
                      <button
                        className={`icon-btn ${s.favorite ? "text-gold-400" : ""}`}
                        title={s.favorite ? "移出預設播放清單" : "加入預設播放清單"}
                        onClick={() => act(() => patch(`/api/library/${s.id}`, { favorite: !s.favorite }))}
                      >
                        <Star className={`size-4 ${s.favorite ? "fill-current" : ""}`} />
                      </button>
                      <button
                        className={`icon-btn ${s.blocked ? "text-red-400" : ""}`}
                        title={s.blocked ? "解除封鎖" : "加入黑名單"}
                        onClick={() => act(() => patch(`/api/library/${s.id}`, { blocked: !s.blocked }))}
                      >
                        <Ban className="size-4" />
                      </button>
                      {s.missing && (
                        <button className="icon-btn" title="再給它一次機會" onClick={() => act(() => patch(`/api/library/${s.id}`, { missing: false }))}>
                          <RotateCcw className="size-4" />
                        </button>
                      )}
                      <button
                        className="icon-btn"
                        title="排入佇列"
                        onClick={() => act(() => post(`/api/library/${s.id}/enqueue`, { placement: "end" }), `『${s.title}』已排入佇列`)}
                      >
                        <ListPlus className="size-4" />
                      </button>
                      <ConfirmButton className="icon-btn hover:text-red-400" title="從歌曲庫刪除" confirmLabel="刪？" onConfirm={() => act(() => del(`/api/library/${s.id}`))}>
                        <Trash2 className="size-4" />
                      </ConfirmButton>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {page.total > page.items.length && (
            <p className="px-3 py-2 text-xs text-zinc-500">只顯示前 {page.items.length} 首（共 {page.total} 首），用搜尋找其他的歌。</p>
          )}
        </div>
      )}
    </div>
  );
}
