-- 0040: emoji reactions on chat messages (#104). Times are milliseconds
-- since the epoch. `db::chat` has the rules.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

CREATE TABLE chat_reactions (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    emoji TEXT COLLATE pg_c_utf8 NOT NULL,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    agent TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    at BIGINT NOT NULL,
    PRIMARY KEY (room, seq, emoji, account, agent)
);
CREATE INDEX chat_reactions_account ON chat_reactions (account);
