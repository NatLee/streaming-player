import { useCallback, useEffect, useRef, useState } from "react";
import type { LiveState, Notice } from "../types";

export type Role = "player" | "overlay" | "admin" | "viewer";

export interface Live {
  state: LiveState | null;
  /** When the last state arrived (performance.now()), to advance the clock locally. */
  receivedAt: number;
  connected: boolean;
  /** This page leads playback reports (player pages only). */
  leader: boolean;
  notices: Notice[];
  send: (msg: unknown) => void;
}

const MAX_NOTICES = 5;

export function wsUrl(role: Role, key?: string | null): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({ role });
  if (key) params.set("key", key);
  return `${proto}//${window.location.host}/api/ws?${params}`;
}

/** Live state over WebSocket, reconnecting with backoff when the server goes away. */
export function useLive(role: Role, key?: string | null): Live {
  const [state, setState] = useState<LiveState | null>(null);
  const [receivedAt, setReceivedAt] = useState(0);
  const [connected, setConnected] = useState(false);
  const [leader, setLeader] = useState(false);
  const [notices, setNotices] = useState<Notice[]>([]);
  const socket = useRef<WebSocket | null>(null);

  useEffect(() => {
    let closed = false;
    let retry = 1000;
    let timer: number | undefined;

    const connect = () => {
      const ws = new WebSocket(wsUrl(role, key));
      socket.current = ws;
      ws.onopen = () => {
        setConnected(true);
        retry = 1000;
      };
      ws.onmessage = (ev) => {
        let msg: { type?: string; [k: string]: unknown };
        try {
          msg = JSON.parse(String(ev.data));
        } catch {
          return;
        }
        if (msg.type === "state") {
          setState(msg as unknown as LiveState);
          setReceivedAt(performance.now());
        } else if (msg.type === "hello" || msg.type === "leader") {
          setLeader(Boolean(msg.leader));
        } else if (msg.type === "notice") {
          // Time it by this page's clock, so toasts expire even if the server clock differs.
          const n = { ...(msg as unknown as Notice), at: Date.now() };
          setNotices((list) => [...list, n].slice(-MAX_NOTICES));
        }
      };
      ws.onclose = () => {
        setConnected(false);
        setLeader(false);
        if (!closed) {
          timer = window.setTimeout(connect, retry);
          retry = Math.min(retry * 2, 10_000);
        }
      };
    };
    connect();
    return () => {
      closed = true;
      window.clearTimeout(timer);
      socket.current?.close();
    };
  }, [role, key]);

  const send = useCallback((msg: unknown) => {
    const ws = socket.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  }, []);

  return { state, receivedAt, connected, leader, notices, send };
}

/** Milliseconds into the current song, ticking locally between server updates. */
export function useElapsed(live: Pick<Live, "state" | "receivedAt">, tickMs = 500): number {
  const [, force] = useState(0);
  const now = live.state?.now;
  const running = Boolean(now?.clock_started && !live.state?.player.paused);
  useEffect(() => {
    if (!running) return;
    const t = window.setInterval(() => force((n) => n + 1), tickMs);
    return () => window.clearInterval(t);
  }, [running, tickMs]);
  if (!now) return 0;
  return now.elapsed_ms + (running ? performance.now() - live.receivedAt : 0);
}

/** Read `?key=` from the page URL (OBS pages). */
export function pageKey(): string | null {
  return new URLSearchParams(window.location.search).get("key");
}
