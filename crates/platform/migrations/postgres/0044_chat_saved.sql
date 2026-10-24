-- 0044: chat's "save for later" (#116). Additive: one new table, nothing
-- existing changes. (Mark as unread needs none: it moves `chat_reads.seq`.)
--
-- SQLite's file in Postgres's words: `BIGINT` times and `COLLATE
-- pg_c_utf8` text.

-- A message an account saved: private to it. `done_at` is when it marked
-- the item done (it stays, in a Done section); `remind_at` is when to push
-- it once, `reminded` that it was.
CREATE TABLE chat_saved (
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    saved_at BIGINT NOT NULL,
    done_at BIGINT,
    remind_at BIGINT,
    reminded BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (account, room, seq)
);
CREATE INDEX chat_saved_due ON chat_saved (remind_at) WHERE remind_at IS NOT NULL AND NOT reminded;
