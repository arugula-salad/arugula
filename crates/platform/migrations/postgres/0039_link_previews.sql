-- 0039: link previews in chat (#108). Additive: three new tables, nothing
-- existing is touched. Times are milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers, `BYTEA`
-- for its blob and `COLLATE pg_c_utf8` text.

-- See SQLite's file.
CREATE TABLE chat_link_previews (
    url TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    fetched_at BIGINT NOT NULL,
    ok BIGINT NOT NULL,
    site TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    title TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    description TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    image_url TEXT COLLATE pg_c_utf8,
    image_type TEXT COLLATE pg_c_utf8,
    image BYTEA
);
CREATE INDEX chat_link_previews_fetched ON chat_link_previews (fetched_at);

CREATE TABLE chat_preview_hidden (
    room_id TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    at BIGINT NOT NULL,
    PRIMARY KEY (room_id, seq)
);

CREATE TABLE chat_team_prefs (
    team_id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    web_previews BIGINT NOT NULL DEFAULT 1
);
