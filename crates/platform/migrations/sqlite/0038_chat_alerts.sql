-- 0038: chat's @channel guard and keyword alerts (#114). Additive: two new
-- tables, nothing existing changes.

-- The words that notify an account as a mention does: a JSON list of
-- lowercase strings. No row, no words.
CREATE TABLE chat_keywords (
    account TEXT NOT NULL PRIMARY KEY,
    words TEXT NOT NULL DEFAULT '[]'
);

-- What a team's owners decided about its chat: whether `@channel` and
-- `@here` are for owners only. No row, everyone may.
CREATE TABLE chat_team_settings (
    team TEXT NOT NULL PRIMARY KEY,
    broadcast_owners INTEGER NOT NULL DEFAULT 0
);
