-- 0042: how loud chat is (#113) and do not disturb (#115). Additive: a
-- new column and a new table, nothing read by an older build changes.
--
-- A person's level for one room: 'all', 'mentions' or 'nothing'. NULL
-- is "whatever my default says" (`notify_prefs`). `muted` stays, kept in
-- step with 'nothing' by `db::chat::set_chat_level`, so a build from
-- before this one still reads a muted room as muted.
ALTER TABLE chat_reads ADD COLUMN level TEXT COLLATE pg_c_utf8 CHECK (level IN ('all', 'mentions', 'nothing'));
UPDATE chat_reads SET level = 'nothing' WHERE muted;

-- One row per account that changed anything; none means every default.
-- `channels`, `dms` and `threads` are the levels for rooms of that kind
-- with no level of their own (threads: the followed ones; they come with
-- following). `snooze_until` is ms since the epoch. `tz` is an IANA name
-- (the schedule is read in it); `quiet` is a JSON list of
-- `{days, from, to}` (ISO weekdays 1..7, minutes after midnight), see
-- `notify::Quiet`. `all_notices`: do not disturb holds back every push,
-- not only chat's.
CREATE TABLE notify_prefs (
    account TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    channels TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'mentions' CHECK (channels IN ('all', 'mentions', 'nothing')),
    dms TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'all' CHECK (dms IN ('all', 'mentions', 'nothing')),
    threads TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'all' CHECK (threads IN ('all', 'mentions', 'nothing')),
    snooze_until BIGINT,
    tz TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'UTC',
    quiet TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '[]',
    all_notices BOOLEAN NOT NULL DEFAULT FALSE
);
