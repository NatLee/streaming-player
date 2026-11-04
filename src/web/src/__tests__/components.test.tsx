import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NowPlayingCard, progressOf } from "../components/NowPlayingCard";
import { Player } from "../components/Player";
import { QueueList } from "../components/QueueList";
import type { Live } from "../lib/live";
import { liveState, nowPlaying, queueItem, song } from "./fixtures";

describe("NowPlayingCard", () => {
  it("shows the song, the requester, the clock and the next song", () => {
    render(<NowPlayingCard now={nowPlaying()} elapsedMs={50_000} next={queueItem(8, "晴天", "Bob", 2)} />);
    expect(screen.getByText("小幸運")).toHaveAttribute("href", "https://www.youtube.com/watch?v=AAAAAAAAAA1");
    expect(screen.getByText("Alice")).toBeInTheDocument();
    expect(screen.getByText("0:50")).toBeInTheDocument();
    expect(screen.getByText("3:20")).toBeInTheDocument();
    expect(screen.getByText("下一首：晴天")).toBeInTheDocument();
  });

  it("says so when the default playlist is playing, or nothing is", () => {
    render(<NowPlayingCard now={nowPlaying({ autoplay: true })} elapsedMs={0} />);
    expect(screen.getByText("預設播放清單")).toBeInTheDocument();
    render(<NowPlayingCard now={null} elapsedMs={0} />);
    expect(screen.getByText("目前沒有歌曲")).toBeInTheDocument();
  });

  it("clamps progress", () => {
    expect(progressOf(50_000, 200)).toBeCloseTo(0.25);
    expect(progressOf(999_000, 200)).toBe(1);
    expect(progressOf(1000, null)).toBe(0);
  });
});

describe("QueueList", () => {
  const items = [queueItem(8, "晴天", "Bob", 2), queueItem(9, "稻香", "Carol", 3)];

  it("lists songs with position, requester and waiting time", () => {
    render(<QueueList items={items} highlight="carol" />);
    expect(screen.getByText("#2")).toBeInTheDocument();
    expect(screen.getByText("稻香")).toBeInTheDocument();
    expect(screen.getAllByText("約 2:30 後")).toHaveLength(2);
    // The viewer's own song is highlighted
    expect(screen.getByText("Carol").closest("li")?.className).toContain("ring-gold");
    expect(screen.queryByTitle("刪除")).toBeNull();
  });

  it("calls the admin actions", () => {
    const actions = { move: vi.fn(), playNow: vi.fn(), remove: vi.fn(), favorite: vi.fn() };
    render(<QueueList items={items} actions={actions} />);
    fireEvent.click(screen.getAllByTitle("移到下一首")[1]);
    expect(actions.move).toHaveBeenCalledWith(items[1], 0);
    fireEvent.click(screen.getAllByTitle("刪除並封鎖點歌者")[0]);
    expect(actions.remove).toHaveBeenCalledWith(items[0], "user");
    fireEvent.click(screen.getAllByTitle("立即插播")[0]);
    expect(actions.playNow).toHaveBeenCalledWith(items[0]);
    expect(screen.getAllByTitle("上移")[0]).toBeDisabled();
  });

  it("shows an empty state", () => {
    render(<QueueList items={[]} />);
    expect(screen.getByText("佇列是空的")).toBeInTheDocument();
  });
});

function liveWith(over: Partial<Live> = {}): Live {
  return { state: liveState(), receivedAt: 0, connected: true, leader: true, notices: [], send: vi.fn(), ...over };
}

describe("Player", () => {
  it("plays other sites through the media endpoint with the OBS key and reports as leader", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ kind: "file", url: "/api/media/101/stream" })));
    vi.stubGlobal("fetch", fetchMock);
    const state = liveState({
      now: nowPlaying({ id: 7, clock_started: false, elapsed_ms: 0, song: song({ id: 101, kind: "media", site: "Soundcloud" }) }),
    });
    const live = liveWith({ state });
    render(<Player live={live} mediaKey="k1" report />);
    expect(fetchMock).toHaveBeenCalledWith("/api/media/101/source?key=k1", expect.anything());
    const video = (await screen.findByTestId("stream-player")).querySelector("video")!;
    await waitFor(() => expect(video.getAttribute("src")).toBe("/api/media/101/stream?key=k1"));
    fireEvent.playing(video);
    expect(live.send).toHaveBeenCalledWith({ type: "started", request_id: 7, position_ms: 0 });
    fireEvent.ended(video);
    expect(live.send).toHaveBeenCalledWith({ type: "ended", request_id: 7 });
    vi.unstubAllGlobals();
  });

  it("does not report when it is not the leader", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ kind: "file", url: "/x" }))));
    const live = liveWith({ leader: false, state: liveState({ now: nowPlaying({ song: song({ kind: "media" }) }) }) });
    render(<Player live={live} report />);
    const video = (await screen.findByTestId("stream-player")).querySelector("video")!;
    fireEvent.ended(video);
    expect(live.send).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("uses the YouTube player for YouTube songs and the idle view when nothing plays", () => {
    const live = liveWith();
    const { rerender } = render(<Player live={live} idle={<p>idle</p>} />);
    expect(screen.getByTestId("youtube-player")).toBeInTheDocument();
    rerender(<Player live={liveWith({ state: liveState({ now: null }) })} idle={<p>idle</p>} />);
    expect(screen.getByText("idle")).toBeInTheDocument();
  });
});
