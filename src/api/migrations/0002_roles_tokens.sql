-- Roles with permission points. `locked` roles always hold every permission and can't
-- be edited (the built-in admin); `builtin` roles can be edited but not deleted.
CREATE TABLE roles (
    name        TEXT    PRIMARY KEY,
    label       TEXT    NOT NULL,
    permissions TEXT    NOT NULL DEFAULT '[]', -- JSON array of permission ids
    builtin     INTEGER NOT NULL DEFAULT 0,
    locked      INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);
INSERT INTO roles (name, label, permissions, builtin, locked, created_at) VALUES
    ('admin', '管理員', '[]', 1, 1, strftime('%s', 'now')),
    ('moderator', '協助管理', '["player.control","queue.view","queue.request","queue.manage","library.manage","blocklist.manage","history.view"]', 1, 0, strftime('%s', 'now')),
    ('viewer', '觀眾', '["queue.view","queue.request"]', 1, 0, strftime('%s', 'now'));

-- Users now point at a role, and carry a token version: bumping it (disable, password
-- change) invalidates every access token issued before.
CREATE TABLE users_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL DEFAULT '',
    role          TEXT    NOT NULL REFERENCES roles (name) ON UPDATE CASCADE,
    email         TEXT    UNIQUE COLLATE NOCASE,
    google_sub    TEXT    UNIQUE,
    disabled      INTEGER NOT NULL DEFAULT 0,
    token_version INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER
);
INSERT INTO users_new (id, username, password_hash, role, email, google_sub, disabled, created_at, last_login_at)
    SELECT id, username, password_hash, role, email, google_sub, disabled, created_at, last_login_at FROM users;
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

-- Refresh tokens for API clients (only the SHA-256 is stored, one row per token)
CREATE TABLE refresh_tokens (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

-- Personal API tokens: long-lived, limited to the permissions picked when created
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    token_hash   TEXT    NOT NULL UNIQUE,
    permissions  TEXT    NOT NULL DEFAULT '[]',
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER,
    last_used_at INTEGER
);
