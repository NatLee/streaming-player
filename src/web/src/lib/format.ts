import type { Level } from "../types";

/** 75 → "1:15", 3725 → "1:02:05" */
export function clock(secs: number | null | undefined): string {
  if (secs == null || !Number.isFinite(secs)) return "--:--";
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  return `${h > 0 ? `${h}:` : ""}${mm}:${String(r).padStart(2, "0")}`;
}

/** Unix seconds → "11/04 21:30" in the viewer's time zone. */
export function when(ts: number | null | undefined): string {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleString("zh-TW", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function ago(ts: number): string {
  const d = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (d < 60) return `${d} 秒前`;
  if (d < 3600) return `${Math.floor(d / 60)} 分鐘前`;
  if (d < 86400) return `${Math.floor(d / 3600)} 小時前`;
  return `${Math.floor(d / 86400)} 天前`;
}

export const LEVELS: { value: Level; label: string }[] = [
  { value: "everyone", label: "所有人" },
  { value: "subscriber", label: "訂閱者／會員" },
  { value: "vip", label: "VIP" },
  { value: "moderator", label: "管理員" },
  { value: "broadcaster", label: "實況主" },
];

export const SOURCE_LABEL: Record<string, string> = {
  twitch: "Twitch",
  youtube: "YouTube",
  bot: "聊天機器人",
  legacy: "Nightbot（舊網址）",
  web: "管理頁",
  autoplay: "自動播放",
};

export const STATUS_LABEL: Record<string, string> = {
  queued: "排隊中",
  playing: "播放中",
  played: "播完",
  skipped: "跳過",
  removed: "刪除",
  failed: "播放失敗",
};
