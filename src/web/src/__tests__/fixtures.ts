import type { LiveState, NowPlaying, QueueItem, Song } from "../types";

export function song(over: Partial<Song> = {}): Song {
  return {
    id: 1,
    kind: "youtube",
    media_key: "youtube:AAAAAAAAAA1",
    source_id: "AAAAAAAAAA1",
    url: "https://www.youtube.com/watch?v=AAAAAAAAAA1",
    title: "小幸運",
    duration_secs: 200,
    thumbnail: null,
    site: "YouTube",
    favorite: false,
    blocked: false,
    missing: false,
    fail_count: 0,
    created_at: 0,
    last_played_at: null,
    ...over,
  };
}

export function nowPlaying(over: Partial<NowPlaying> = {}): NowPlaying {
  return {
    id: 7,
    requester: "Alice",
    requester_key: "alice",
    source: "twitch",
    status: "playing",
    autoplay: false,
    requested_at: 0,
    started_at: 0,
    ended_at: null,
    elapsed_ms: 50_000,
    clock_started: true,
    song: song(),
    ...over,
  };
}

export function queueItem(id: number, title: string, requester: string, position: number): QueueItem {
  return {
    id,
    requester,
    requester_key: requester.toLowerCase(),
    source: "bot",
    status: "queued",
    autoplay: false,
    requested_at: 0,
    started_at: null,
    ended_at: null,
    position,
    wait_secs: 150,
    song: song({ id: id + 100, title, source_id: `id${id}` }),
  };
}

export function liveState(over: Partial<LiveState> = {}): LiveState {
  return {
    now: nowPlaying(),
    queue: [queueItem(8, "晴天", "Bob", 2), queueItem(9, "稻香", "Carol", 3)],
    player: { paused: false, volume: 80, muted: false },
    requests_open: true,
    playback: { autoplay_favorites: true, youtube_mode: "embed", stream_fallback: true },
    players_connected: 1,
    server_time_ms: 0,
    ...over,
  };
}
