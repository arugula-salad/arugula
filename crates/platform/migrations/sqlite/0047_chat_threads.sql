-- 0047: reply threads on a message (#106). Additive: two columns an older
-- build never reads (it would list a reply in the room, which is all) and a
-- new table. Times are milliseconds since the epoch. `db::chat_threads`
-- has the rules.

-- A reply names its thread's first message: `parent` is that message's
-- `seq` in the same room (never a reply's: threads are one deep). `also`:
-- sent to the channel too, so the room's list shows it as well.
ALTER TABLE chat_messages ADD COLUMN parent INTEGER;
ALTER TABLE chat_messages ADD COLUMN also INTEGER NOT NULL DEFAULT 0;
CREATE INDEX chat_messages_parent ON chat_messages (room, parent) WHERE parent IS NOT NULL;

-- A person's place in a thread: whether they follow it, and their read
-- mark in it (a room `seq`, only forward). A row is made when they follow,
-- reply or are mentioned, or read it; no row is "not following, nothing
-- read". Unfollowing keeps the row, so a thread that follows you back in
-- (your own reply) is not told apart from one you never had.
CREATE TABLE chat_thread_reads (
    room TEXT NOT NULL,
    parent INTEGER NOT NULL,
    account TEXT NOT NULL,
    following INTEGER NOT NULL DEFAULT 1,
    seq INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, parent, account)
);
CREATE INDEX chat_thread_reads_account ON chat_thread_reads (account) WHERE following = 1;
