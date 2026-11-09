-- 0099: the place in its machine's order of the post a reader list came in
-- (`ChatReaders::seq`), so a post that arrives late doesn't undo a newer
-- one. NULL for a list from a daemon that doesn't number its posts.
ALTER TABLE chat_readers ADD COLUMN seq INTEGER;
