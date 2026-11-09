-- 0099: the order of a machine's reader-list posts. See SQLite's file.
ALTER TABLE chat_readers ADD COLUMN seq BIGINT;
