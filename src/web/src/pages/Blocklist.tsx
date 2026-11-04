import { Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, patch, post } from "../api";
import { Empty, ErrorBox, PageHeader, Section, Thumb } from "../components/ui";
import { when } from "../lib/format";
import type { BlockedUser, Song } from "../types";

export function BlocklistPage() {
  const [users, setUsers] = useState<BlockedUser[]>([]);
  const [songs, setSongs] = useState<Song[]>([]);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    get<BlockedUser[]>("/api/blocklist").then(setUsers).catch((e) => setError(errorText(e)));
    get<Song[]>("/api/blocklist/songs")
      .then(setSongs)
      .catch((e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const act = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="mx-auto max-w-4xl space-y-5">
      <PageHeader title="黑名單" description="被封鎖的人不能點歌；被封鎖的歌誰都不能點。從佇列刪歌時也可以順便封鎖。" />
      {error && <ErrorBox message={error} />}
      <Section title="點歌者" description="填聊天室名稱；Twitch 用登入名稱，YouTube 可以填顯示名稱或 yt:頻道 ID。">
        <form
          className="mb-4 flex flex-wrap gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) act(() => post("/api/blocklist", { name, note })).then(() => { setName(""); setNote(""); });
          }}
        >
          <input className="input w-48" placeholder="名稱" value={name} onChange={(e) => setName(e.target.value)} />
          <input className="input min-w-48 flex-1" placeholder="原因（選填）" value={note} onChange={(e) => setNote(e.target.value)} />
          <button className="btn btn-primary">封鎖</button>
        </form>
        {users.length === 0 ? (
          <Empty>沒有被封鎖的人</Empty>
        ) : (
          <ul className="divide-y divide-white/5">
            {users.map((u) => (
              <li key={u.id} className="flex items-center gap-3 py-2">
                <span className="font-mono text-sm">{u.name_key}</span>
                <span className="flex-1 text-sm text-zinc-500">{u.note}</span>
                <span className="text-xs text-zinc-500">{when(u.created_at)}</span>
                <button className="icon-btn hover:text-red-400" title="解除封鎖" onClick={() => act(() => del(`/api/blocklist/${u.id}`))}>
                  <Trash2 className="size-4" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
      <Section title="歌曲">
        {songs.length === 0 ? (
          <Empty>沒有被封鎖的歌</Empty>
        ) : (
          <ul className="divide-y divide-white/5">
            {songs.map((s) => (
              <li key={s.id} className="flex items-center gap-3 py-2">
                <Thumb src={s.thumbnail} className="h-9 w-14" />
                <a href={s.url} target="_blank" rel="noreferrer" className="line-clamp-1 flex-1 text-sm hover:underline">
                  {s.title}
                </a>
                <button className="btn btn-secondary" onClick={() => act(() => patch(`/api/library/${s.id}`, { blocked: false }))}>
                  解除封鎖
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
