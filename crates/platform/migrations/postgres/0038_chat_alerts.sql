-- 0038: chat's @channel guard and keyword alerts (#114). Additive: two new
-- tables, nothing existing changes.
--
-- SQLite's file in Postgres's words: `BOOLEAN` for its flags and
-- `COLLATE pg_c_utf8` text.

-- The words that notify an account as a mention does: a JSON list of
-- lowercase strings. No row, no words.
CREATE TABLE chat_keywords (
    account TEXT COLLATE pg_c_utf8 NOT NULL PRIMARY KEY,
    words TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '[]'
);

-- What a team's owners decided about its chat: whether `@channel` and
-- `@here` are for owners only. No row, everyone may.
CREATE TABLE chat_team_settings (
    team TEXT COLLATE pg_c_utf8 NOT NULL PRIMARY KEY,
    broadcast_owners BOOLEAN NOT NULL DEFAULT FALSE
);
