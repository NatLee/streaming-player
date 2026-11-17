import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AdminLayout } from "../components/AdminLayout";
import type { Live } from "../lib/live";
import { Dashboard } from "../pages/Dashboard";
import type { Perm } from "../types";
import { liveState, nowPlaying } from "./fixtures";

// The dashboard's live state comes from the WebSocket; tests hand it one directly.
let live: Live;
vi.mock("../lib/live", async (original) => ({
  ...(await original<typeof import("../lib/live")>()),
  useLive: () => live,
}));

afterEach(() => vi.unstubAllGlobals());

function renderDashboard(permissions: Perm[], state = liveState()) {
  live = { state, receivedAt: 0, connected: true, leader: false, notices: [], send: vi.fn() };
  const user = {
    id: 1,
    username: "mod",
    role: "moderator",
    role_label: "協助管理",
    email: null,
    has_password: true,
    google_linked: false,
    disabled: false,
    created_at: 0,
    last_login_at: null,
    permissions,
  };
  const fetchMock = vi.fn<(path: string, init?: RequestInit) => Promise<Response>>(async (path) =>
    path === "/api/auth/state"
      ? new Response(JSON.stringify({ needs_setup: false, user, google_enabled: false, registration_open: true }))
      : new Response(null, { status: 204 }),
  );
  vi.stubGlobal("fetch", fetchMock);
  render(
    <MemoryRouter initialEntries={["/admin"]}>
      <Routes>
        <Route path="/admin" element={<AdminLayout />}>
          <Route index element={<Dashboard />} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
  /** Bodies sent to the seek endpoint. */
  const seeks = () =>
    fetchMock.mock.calls
      .filter(([path]) => path === "/api/player/seek")
      .map(([, init]) => JSON.parse(String(init?.body)));
  return { seeks };
}

describe("Dashboard seeking", () => {
  it("moves the song back and forward 5 seconds, or to where the bar is let go", async () => {
    const { seeks } = renderDashboard(["queue.view", "player.control"]);
    fireEvent.click(await screen.findByRole("button", { name: "倒退 5 秒" }));
    fireEvent.click(screen.getByRole("button", { name: "快轉 5 秒" }));
    fireEvent.change(screen.getByRole("slider", { name: "播放進度" }), { target: { value: "150" } });
    await waitFor(() =>
      expect(seeks()).toEqual([{ offset_ms: -5000 }, { offset_ms: 5000 }, { position_ms: 150_000 }]),
    );
  });

  it("waits for a player page to start the song", async () => {
    renderDashboard(["queue.view", "player.control"], liveState({ now: nowPlaying({ clock_started: false }) }));
    expect(await screen.findByRole("button", { name: "倒退 5 秒" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "快轉 5 秒" })).toBeDisabled();
    expect(screen.queryByRole("slider", { name: "播放進度" })).toBeNull();
  });

  it("shows no seek controls without the player.control permission", async () => {
    renderDashboard(["queue.view"]);
    expect(await screen.findByText("小幸運")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "快轉 5 秒" })).toBeNull();
    expect(screen.queryByRole("slider", { name: "播放進度" })).toBeNull();
  });
});
