-- 0048: chat channels get members and a private flag (#118), and a DM may
-- have up to eight people (#117). Times are milliseconds since the epoch.
-- `chat` has the rules ("Who sees a room").
--
-- NOT additive, though it only adds columns and rows: a channel used to be
-- seen by its whole team, and a build from before this one would show a
-- private channel to all of it. A rollback past this version is refused.

-- A private channel is invite-only and hidden from the browser.
ALTER TABLE chat_rooms ADD COLUMN private INTEGER NOT NULL DEFAULT 0;
-- A public channel everyone who joins the team joins too (`db::projects`'s
-- roster write adds them), as Slack's #general.
ALTER TABLE chat_rooms ADD COLUMN auto_join INTEGER NOT NULL DEFAULT 0;
-- A DM's people, sorted and joined by `,`: opening a DM with the same set
-- finds the one conversation, even after one of them left it. NULL for the
-- DMs from before this (always two people).
ALTER TABLE chat_rooms ADD COLUMN dm_key TEXT;
CREATE UNIQUE INDEX chat_rooms_dm_key ON chat_rooms (scope, dm_key) WHERE kind = 'dm' AND dm_key IS NOT NULL;

-- Every channel there is becomes public, with every current member of its
-- team a member, so nobody loses a room. (`team_members` is the latest
-- roster the platform holds, `db::projects::project_roster`.)
INSERT INTO chat_members (room, account, from_ms)
    SELECT r.id, m.account_id, 0
    FROM chat_rooms r JOIN team_members m ON 'team:' || m.team_id = r.scope
    WHERE r.kind = 'channel'
    ON CONFLICT DO NOTHING;

UPDATE chat_rooms SET auto_join = 1 WHERE kind = 'channel' AND name = 'general';
