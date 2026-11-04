import {
  Ban,
  Eye,
  EyeOff,
  ListPlus,
  Pause,
  Play,
  SkipForward,
  Star,
  Trash2,
  Tv,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router";
import { del, errorText, get, patch, post, put } from "../api";
import { NowPlayingCard } from "../components/NowPlayingCard";
import { useCan } from "../components/AdminLayout";
import { Player } from "../components/Player";
import { QueueList, type QueueActions } from "../components/QueueList";
import { ConfirmButton, ErrorBox, Loading, Notice, PageHeader } from "../components/ui";
import { ago } from "../lib/format";
import { useElapsed, useLive } from "../lib/live";
import type { ActivityEvent, Enqueued, Placement, Settings } from "../types";

const PLACEMENTS: { value: Placement; label: string }[] = [
  { value: "end", label: "排到最後" },
  { value: "next", label: "下一首" },
  { value: "now", label: "立即插播" },
];

export function Dashboard() {
  const live = useLive("admin");
  const can = useCan();
  const elapsed = useElapsed(live);
  const [input, setInput] = useState("");
  const [placement, setPlacement] = useState<Placement>("end");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState(false);
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  // While the slider moves, show its value and send it once it settles.
  const [dragVolume, setDragVolume] = useState<number | null>(null);
  const volumeTimer = useRef<number | undefined>(undefined);
  const state = live.state;

  // Refresh the activity log when the queue or the song changes (not on every
  // progress tick, which arrives every few seconds while a song plays).
  const signature = state
    ? [state.now?.id, state.player.paused, state.requests_open, ...state.queue.map((q) => q.id)].join(",")
    : "";
  const seeHistory = can("history.view");
  useEffect(() => {
    if (seeHistory) get<ActivityEvent[]>("/api/events?limit=15").then(setEvents).catch(() => undefined);
  }, [signature, seeHistory]);

  const run = useCallback(async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      setMessage({ ok: false, text: errorText(e) });
    }
  }, []);

  if (!state) return <Loading label="連線中…" />;
  const now = state.now;

  const add = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      const r = await post<Enqueued>("/api/queue", { input, placement });
      setMessage({ ok: true, text: r.started ? "開始播放了" : `已加入，第 ${r.position} 位` });
      setInput("");
    } catch (err) {
      setMessage({ ok: false, text: errorText(err) });
    } finally {
      setBusy(false);
    }
  };

  // Only the buttons this user may use (the server checks again).
  const manage = can("queue.manage");
  const actions: QueueActions = {
    move: manage ? (item, index) => run(() => post(`/api/queue/${item.id}/move`, { index })) : undefined,
    playNow: manage ? (item) => run(() => post(`/api/queue/${item.id}/play-now`)) : undefined,
    remove: manage ? (item) => run(() => del(`/api/queue/${item.id}`)) : undefined,
    removeAndBlock:
      manage && can("blocklist.manage") ? (item, block) => run(() => del(`/api/queue/${item.id}?block=${block}`)) : undefined,
    favorite: can("library.manage")
      ? (item) => run(() => patch(`/api/library/${item.song.id}`, { favorite: !item.song.favorite }))
      : undefined,
  };
  const control = can("player.control");

  const toggleRequests = () =>
    run(async () => {
      const view = await get<{ settings: Settings }>("/api/settings");
      const s = view.settings;
      s.queue.requests_open = !s.queue.requests_open;
      await put("/api/settings", s);
    });

  return (
    <div className="mx-auto max-w-6xl">
      <PageHeader
        title="控制台"
        description="正在播放的歌、佇列和最近發生的事。所有開著的播放頁都會同步。"
        actions={
          <>
            <span className={`chip ${state.players_connected ? "" : "bg-amber-500/15 text-amber-300"}`}>
              <Tv className="size-3.5" />
              {state.players_connected ? `${state.players_connected} 個播放頁連線中` : "沒有播放頁連線"}
            </span>
            {can("settings.manage") ? (
              <button className={`btn ${state.requests_open ? "btn-secondary" : "btn-danger"}`} onClick={toggleRequests}>
                {state.requests_open ? "點歌開放中" : "點歌已關閉"}
              </button>
            ) : (
              <span className="chip">{state.requests_open ? "點歌開放中" : "點歌已關閉"}</span>
            )}
          </>
        }
      />
      {!state.players_connected && (
        <div className="mb-4">
          <Notice tone="warn">
            沒有播放頁在連線，歌曲不會前進。到 OBS 加一個瀏覽器來源，網址在 <Link className="underline" to="/admin/settings">設定 → OBS</Link>。
          </Notice>
        </div>
      )}
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
        <div className="space-y-5">
          <NowPlayingCard now={now} elapsedMs={elapsed} next={state.queue[0]} paused={state.player.paused} size="lg" />
          {control && (
          <div className="card flex flex-wrap items-center gap-2 p-3">
            {state.player.paused ? (
              <button className="btn btn-primary" onClick={() => run(() => post("/api/player/resume"))}>
                <Play className="size-4" /> 播放
              </button>
            ) : (
              <button className="btn btn-secondary" onClick={() => run(() => post("/api/player/pause"))}>
                <Pause className="size-4" /> 暫停
              </button>
            )}
            <button className="btn btn-secondary" disabled={!now} onClick={() => run(() => post("/api/player/skip", { block: false }))}>
              <SkipForward className="size-4" /> 跳過
            </button>
            {can("blocklist.manage") && (
              <ConfirmButton
                title="跳過並把這首歌加入黑名單"
                confirmLabel="確定封鎖？"
                onConfirm={() => run(() => post("/api/player/skip", { block: true }))}
              >
                <Ban className="size-4" /> 跳過並封鎖
              </ConfirmButton>
            )}
            {now && can("library.manage") && (
              <button
                className={`btn btn-secondary ${now.song.favorite ? "text-gold-300" : ""}`}
                onClick={() => run(() => patch(`/api/library/${now.song.id}`, { favorite: !now.song.favorite }))}
              >
                <Star className={`size-4 ${now.song.favorite ? "fill-current" : ""}`} />
                {now.song.favorite ? "已在預設播放清單" : "加入預設播放清單"}
              </button>
            )}
            <div className="ml-auto flex items-center gap-2">
              <button
                className="icon-btn"
                title={state.player.muted ? "取消靜音" : "靜音"}
                onClick={() => run(() => put("/api/player/volume", { muted: !state.player.muted }))}
              >
                {state.player.muted ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />}
              </button>
              <input
                type="range"
                min={0}
                max={100}
                aria-label="音量"
                value={dragVolume ?? state.player.volume}
                className="w-32 accent-gold-400"
                onChange={(e) => {
                  const volume = Number(e.target.value);
                  setDragVolume(volume);
                  window.clearTimeout(volumeTimer.current);
                  volumeTimer.current = window.setTimeout(
                    () => run(() => put("/api/player/volume", { volume })).finally(() => setDragVolume(null)),
                    200,
                  );
                }}
              />
              <span className="w-8 text-right text-xs text-zinc-400 tabular-nums">{dragVolume ?? state.player.volume}</span>
            </div>
          </div>
          )}

          {preview && (
            <div className="card overflow-hidden">
              <div className="aspect-video">
                <Player live={live} forceMuted idle={<div className="flex size-full items-center justify-center text-zinc-500">沒有歌曲</div>} />
              </div>
            </div>
          )}

          {can("queue.request") && (
          <form onSubmit={add} className="card flex flex-wrap items-center gap-2 p-3">
            <input
              className="input min-w-64 flex-1"
              placeholder="YouTube／Bilibili／SoundCloud 網址、影片連結，或搜尋文字"
              value={input}
              onChange={(e) => setInput(e.target.value)}
            />
            {manage && (
              <select className="input w-32" value={placement} onChange={(e) => setPlacement(e.target.value as Placement)}>
                {PLACEMENTS.map((p) => (
                  <option key={p.value} value={p.value}>
                    {p.label}
                  </option>
                ))}
              </select>
            )}
            <button className="btn btn-primary" disabled={busy}>
              <ListPlus className="size-4" /> 點歌
            </button>
          </form>
          )}
          {message &&
            (message.ok ? <Notice tone="ok">{message.text}</Notice> : <ErrorBox message={message.text} />)}

          <section className="card p-3">
            <div className="mb-2 flex items-center justify-between px-2">
              <h2 className="font-semibold">
                佇列 <span className="text-sm font-normal text-zinc-500">{state.queue.length} 首</span>
              </h2>
              <div className="flex gap-1">
                <button className="btn btn-ghost" onClick={() => setPreview(!preview)}>
                  {preview ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                  {preview ? "關閉預覽" : "預覽畫面"}
                </button>
                {manage && state.queue.length > 0 && (
                  <ConfirmButton className="btn btn-ghost" confirmLabel="確定清空？" onConfirm={() => run(() => post("/api/queue/clear"))}>
                    <Trash2 className="size-4" /> 清空
                  </ConfirmButton>
                )}
              </div>
            </div>
            <QueueList items={state.queue} actions={actions} />
          </section>
        </div>

        {seeHistory && (
        <aside className="card h-fit p-4">
          <h2 className="mb-3 font-semibold">最近動態</h2>
          <ul className="space-y-2.5">
            {events.map((e) => (
              <li key={e.id} className="text-sm">
                <div className="text-zinc-200">
                  <span className="font-medium text-gold-300">{e.actor}</span> {e.detail}
                </div>
                <div className="text-xs text-zinc-500">{ago(e.at)}</div>
              </li>
            ))}
            {events.length === 0 && <li className="text-sm text-zinc-500">還沒有任何動態</li>}
          </ul>
        </aside>
        )}
      </div>
    </div>
  );
}
