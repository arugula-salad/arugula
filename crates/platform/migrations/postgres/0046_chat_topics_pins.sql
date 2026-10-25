-- 0046: a channel's topic and description, pinned messages, and system
-- lines in a room (#119). Additive: three new tables and one new column
-- with a default; nothing existing changes. Times are milliseconds since
-- the epoch. `db::chat_pins` has the rules.

-- What people wrote about a channel. No row, no topic: the page shows the
-- computed line.
CREATE TABLE chat_topics (
    room TEXT COLLATE pg_c_utf8 NOT NULL PRIMARY KEY,
    topic TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    description TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    author TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL
);

-- Message `seq` of `room` is pinned, author `author`. Dropped with the message's
-- content when it's deleted, and with the room.
CREATE TABLE chat_pins (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    author TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL,
    PRIMARY KEY (room, seq)
);

-- Message `seq` of `room` is not something said but something that
-- happened: kind 'topic', 'description', 'pin' or 'unpin' (the last two
-- with the pinned message's number in `target`). Its author is who did it.
-- Never counted as unread, searched, pushed, edited or reacted to.
CREATE TABLE chat_system (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    kind TEXT COLLATE pg_c_utf8 NOT NULL,
    target BIGINT,
    PRIMARY KEY (room, seq)
);

-- Whether setting a channel's topic is for the team's owners only.
ALTER TABLE chat_team_settings ADD COLUMN topic_owners BOOLEAN NOT NULL DEFAULT FALSE;
