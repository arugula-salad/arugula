-- 0048: chat channels get members and a private flag (#118), and a DM may
-- have up to eight people (#117). Times are milliseconds since the epoch.
-- `chat` has the rules ("Who sees a room").
--
-- NOT additive, though it only adds columns and rows: a channel used to be
-- seen by its whole team, and a build from before this one would show a
-- private channel to all of it. A rollback past this version is refused.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

ALTER TABLE chat_rooms ADD COLUMN private BIGINT NOT NULL DEFAULT 0;
ALTER TABLE chat_rooms ADD COLUMN auto_join BIGINT NOT NULL DEFAULT 0;
ALTER TABLE chat_rooms ADD COLUMN dm_key TEXT COLLATE pg_c_utf8;
CREATE UNIQUE INDEX chat_rooms_dm_key ON chat_rooms (scope, dm_key) WHERE kind = 'dm' AND dm_key IS NOT NULL;

INSERT INTO chat_members (room, account, from_ms)
    SELECT r.id, m.account_id, 0
    FROM chat_rooms r JOIN team_members m ON 'team:' || m.team_id = r.scope
    WHERE r.kind = 'channel'
    ON CONFLICT DO NOTHING;

UPDATE chat_rooms SET auto_join = 1 WHERE kind = 'channel' AND name = 'general';
