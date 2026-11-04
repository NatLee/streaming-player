// Shapes of the API responses (see /api/docs for the full OpenAPI description).

export type Level = "everyone" | "subscriber" | "vip" | "moderator" | "broadcaster";
export type Placement = "end" | "next" | "now";
export type YoutubeMode = "embed" | "stream";

export interface Song {
  id: number;
  kind: "youtube" | "media";
  media_key: string;
  source_id: string;
  url: string;
  title: string;
  duration_secs: number | null;
  thumbnail: string | null;
  site: string;
  favorite: boolean;
  blocked: boolean;
  missing: boolean;
  fail_count: number;
  created_at: number;
  last_played_at: number | null;
}

export interface Entry {
  id: number;
  requester: string;
  requester_key: string;
  source: string;
  status: "queued" | "playing" | "played" | "skipped" | "removed" | "failed";
  autoplay: boolean;
  requested_at: number;
  started_at: number | null;
  ended_at: number | null;
  song: Song;
}

export interface NowPlaying extends Entry {
  elapsed_ms: number;
  clock_started: boolean;
}

export interface QueueItem extends Entry {
  position: number;
  wait_secs: number;
}

export interface PlayerView {
  paused: boolean;
  volume: number;
  muted: boolean;
}

export interface Playback {
  autoplay_favorites: boolean;
  youtube_mode: YoutubeMode;
  stream_fallback: boolean;
}

export interface LiveState {
  now: NowPlaying | null;
  queue: QueueItem[];
  player: PlayerView;
  requests_open: boolean;
  playback: Playback;
  players_connected: number;
  server_time_ms: number;
}

export interface Notice {
  kind: string;
  text: string;
  at: number;
}

export interface User {
  id: number;
  username: string;
  role: "admin" | "moderator";
  email: string | null;
  has_password: boolean;
  google_linked: boolean;
}

export interface AuthState {
  needs_setup: boolean;
  user: User | null;
  google_enabled: boolean;
}

export interface QueueRules {
  requests_open: boolean;
  max_queue: number;
  max_per_user: number;
  max_duration_secs: number;
  allow_duplicates: boolean;
  skip_votes: number;
  bump_votes: number;
  vote_window_secs: number;
  missing_after_failures: number;
  privileged_bypass: boolean;
}

export interface Settings {
  queue: QueueRules;
  playback: Playback;
  viewer: { public_queue: boolean };
  chat: {
    prefix: string;
    twitch: { enabled: boolean; client_id: string; channel: string };
    youtube: { enabled: boolean; video: string; poll_secs: number };
  };
  legacy_api: boolean;
}

export interface SettingsView {
  settings: Settings;
  obs_key: string;
  bot_key: string;
  youtube_api_key_set: boolean;
  public_url: string;
  links: { player: string; overlay: string; viewer: string; bot_base: string };
}

export interface SystemInfo {
  version: string;
  ytdlp_path: string;
  ytdlp_version: string | null;
  ytdlp_error: string | null;
  legacy_last_hit: number | null;
  players_connected: number;
}

export type Action =
  | "request"
  | "current"
  | "queue"
  | "mine"
  | "stats"
  | "remove"
  | "vote_skip"
  | "skip"
  | "vote_bump"
  | "pause"
  | "resume"
  | "help";

export interface Command {
  id: number;
  action: Action;
  triggers: string[];
  min_level: Level;
  cooldown_secs: number;
  enabled: boolean;
  replies: Record<string, string>;
}

export interface ActionInfo {
  action: Action;
  outcomes: { key: string; default_text: string }[];
  variables: string[];
}

export interface CommandList {
  prefix: string;
  commands: Command[];
  actions: ActionInfo[];
}

export interface ChatStatus {
  state: "off" | "connecting" | "connected" | "waiting" | "needs_auth" | "error";
  detail: string;
  since: number;
}

export interface ChatInfo {
  twitch: {
    status: ChatStatus;
    client_id_set: boolean;
    login: string | null;
    device: { user_code: string; verification_uri: string; expires_at: number } | null;
    scopes: string;
  };
  youtube: { status: ChatStatus; api_key_set: boolean; units_per_hour: number };
}

export interface LibrarySong extends Song {
  plays: number;
  requests: number;
}

export interface Page<T> {
  items: T[];
  total: number;
}

export interface UserStats {
  requester: string;
  total: number;
  played: number;
  played_secs: number;
  unique_songs: number;
  top_title: string;
  top_count: number;
}

export interface BlockedUser {
  id: number;
  name_key: string;
  note: string;
  created_at: number;
}

export interface ActivityEvent {
  id: number;
  at: number;
  actor: string;
  kind: string;
  detail: string;
}

export interface Enqueued {
  id: number;
  position: number;
  wait_secs: number;
  started: boolean;
}

export interface Meta {
  version: string;
  public_queue: boolean;
  google_enabled: boolean;
}
