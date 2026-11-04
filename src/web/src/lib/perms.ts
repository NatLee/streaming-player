import type { Perm } from "../types";

/** Short labels for permission points (the server sends the long ones with /api/roles). */
export const PERM_LABEL: Record<Perm, string> = {
  "player.control": "控制播放",
  "queue.view": "看佇列",
  "queue.request": "從網頁點歌",
  "queue.manage": "管理佇列",
  "library.manage": "管理歌曲庫",
  "blocklist.manage": "管理黑名單",
  "history.view": "看歷史和動態",
  "commands.manage": "管理指令和聊天室",
  "settings.view": "看設定",
  "settings.manage": "改設定",
  "users.manage": "管理使用者",
};
