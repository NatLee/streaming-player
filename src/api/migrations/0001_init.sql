-- People who can sign in to the web admin. The first one is created on first run.
CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    -- argon2 hash; empty when the user only signs in with Google
    password_hash TEXT    NOT NULL DEFAULT '',
    role          TEXT    NOT NULL CHECK (role IN ('admin', 'moderator')),
    -- Google sign-in is allowed only for an email an admin put here
    email         TEXT    UNIQUE COLLATE NOCASE,
    google_sub    TEXT    UNIQUE,
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER
);

-- Sign-in sessions; only the SHA-256 of the cookie token is stored
CREATE TABLE sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

-- Song library: every video anyone ever requested (v1 "Playlist").
-- favorite = part of the default playlist that plays when the queue is empty.
CREATE TABLE songs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    -- youtube: played with the YouTube IFrame player; media: played from a yt-dlp link
    kind           TEXT    NOT NULL CHECK (kind IN ('youtube', 'media')),
    -- unique identity: "youtube:<id>" or "<extractor>:<id>" from yt-dlp
    media_key      TEXT    NOT NULL UNIQUE,
    -- YouTube video id, or the page URL handed to yt-dlp
    source_id      TEXT    NOT NULL,
    url            TEXT    NOT NULL,
    title          TEXT    NOT NULL,
    -- NULL when the site does not tell (direct links); the player reports it later
    duration_secs  INTEGER,
    thumbnail      TEXT,
    site           TEXT    NOT NULL DEFAULT '',
    favorite       INTEGER NOT NULL DEFAULT 0,
    blocked        INTEGER NOT NULL DEFAULT 0,
    -- the video could not be played several times in a row (issue #2)
    missing        INTEGER NOT NULL DEFAULT 0,
    fail_count     INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    last_played_at INTEGER
);

-- Every request (v1 "PlaylistOrderHistory" + "PlaylistOrderQueue").
-- The queue is status = 'queued' ordered by position; at most one row is 'playing'.
CREATE TABLE requests (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id       INTEGER NOT NULL REFERENCES songs (id) ON DELETE CASCADE,
    requester     TEXT    NOT NULL,
    -- lower-case identity used for limits and ownership, e.g. "natlee" or "yt:UC..."
    requester_key TEXT    NOT NULL,
    source        TEXT    NOT NULL,
    status        TEXT    NOT NULL CHECK (status IN ('queued', 'playing', 'played', 'skipped', 'removed', 'failed')),
    position      INTEGER NOT NULL DEFAULT 0,
    autoplay      INTEGER NOT NULL DEFAULT 0,
    requested_at  INTEGER NOT NULL,
    started_at    INTEGER,
    ended_at      INTEGER
);
CREATE INDEX requests_status_position ON requests (status, position);
CREATE INDEX requests_requester ON requests (requester_key, requested_at);
CREATE UNIQUE INDEX requests_one_playing ON requests (status) WHERE status = 'playing';

-- Chat votes: skip the current song, or bump a queued song to play next
CREATE TABLE votes (
    request_id INTEGER NOT NULL REFERENCES requests (id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('skip', 'bump')),
    voter_key  TEXT    NOT NULL,
    voter_name TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (request_id, kind, voter_key)
);

-- Chatters who may not request songs
CREATE TABLE blocked_users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name_key   TEXT    NOT NULL UNIQUE,
    note       TEXT    NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);

-- Chat commands. triggers: JSON array without the prefix, replies: JSON object outcome -> template
CREATE TABLE commands (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    action        TEXT    NOT NULL,
    triggers      TEXT    NOT NULL,
    min_level     TEXT    NOT NULL,
    cooldown_secs INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 1,
    replies       TEXT    NOT NULL DEFAULT '{}'
);

-- Key/value store: the settings document, keys for OBS and bots, chat tokens
CREATE TABLE kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Activity log shown on the dashboard (who requested, skipped, blocked, changed settings)
CREATE TABLE events (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    at     INTEGER NOT NULL,
    actor  TEXT    NOT NULL,
    kind   TEXT    NOT NULL,
    detail TEXT    NOT NULL
);
CREATE INDEX events_at ON events (at);
