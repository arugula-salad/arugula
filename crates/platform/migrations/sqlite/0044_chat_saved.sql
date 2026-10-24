-- 0044: chat's "save for later" (#116). Additive: one new table, nothing
-- existing changes. (Mark as unread needs none: it moves `chat_reads.seq`.)

-- A message an account saved: private to it. `done_at` is when it marked
-- the item done (it stays, in a Done section); `remind_at` is when to push
-- it once, `reminded` that it was.
CREATE TABLE chat_saved (
    account TEXT NOT NULL,
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    saved_at INTEGER NOT NULL,
    done_at INTEGER,
    remind_at INTEGER,
    reminded INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account, room, seq)
);
CREATE INDEX chat_saved_due ON chat_saved (remind_at) WHERE remind_at IS NOT NULL AND reminded = 0;
