-- 0047: reply threads on a message (#106). Additive, as SQLite's file, in
-- Postgres's words: `BIGINT` for its integers, `BOOLEAN` for its flags and
-- `COLLATE pg_c_utf8` text.

ALTER TABLE chat_messages ADD COLUMN parent BIGINT;
ALTER TABLE chat_messages ADD COLUMN also BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX chat_messages_parent ON chat_messages (room, parent) WHERE parent IS NOT NULL;

CREATE TABLE chat_thread_reads (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    parent BIGINT NOT NULL,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    following BOOLEAN NOT NULL DEFAULT TRUE,
    seq BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (room, parent, account)
);
CREATE INDEX chat_thread_reads_account ON chat_thread_reads (account) WHERE following;
