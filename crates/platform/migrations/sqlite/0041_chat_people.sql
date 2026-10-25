-- 0041: chat's people (additive): avatars and presence. Times are
-- milliseconds since the epoch. `db::chat` has the rules.

-- A GitHub login's picture, as control fetched it and serves it, so readers
-- never load githubusercontent themselves. `fetched` is when it was read;
-- a row with no `body` says GitHub had none (or refused) then, and holds
-- off asking again for a while. Rows are by login (lowercase), not account,
-- so a renamed account asks again under its new one.
CREATE TABLE chat_avatars (
    login TEXT PRIMARY KEY,
    ctype TEXT NOT NULL DEFAULT '',
    body BLOB,
    fetched INTEGER NOT NULL
);

-- Presence: the last a person's pages were heard from, never more.
-- `seen` is their latest heartbeat; `active` the latest one from a page
-- that had been used in the last minutes (so "active" and "away" come
-- from it and the clock, and nothing is kept of where or how long).
-- `mode` is what they chose: `auto`, `away` (shown away) or `offline`
-- (shown offline however they are). Dropped with the account.
CREATE TABLE chat_presence (
    account TEXT PRIMARY KEY,
    seen INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 0,
    mode TEXT NOT NULL DEFAULT 'auto' CHECK (mode IN ('auto', 'away', 'offline'))
);
