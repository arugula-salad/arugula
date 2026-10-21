-- 0008: what bodies on read and content mirroring need (M166, #1212).
--
-- `forge_items.content_rev` goes up with each webhook that may change an
-- item's body or comments: the read cache, kept in memory on each
-- machine, serves an item's content only while its rev is the one it was
-- fetched at, so a webhook taken on one machine drops what every machine
-- kept.
ALTER TABLE forge_items ADD COLUMN content_rev BIGINT NOT NULL DEFAULT 0;

-- `forge_content` (nothing wrote it before this) also keeps a review's
-- text (`review`), and a review comment's thread: its file (`path`), its
-- `line`, and the comment it answers (`reply_to`, GitHub's id).
-- `created_at` is GitHub's.
ALTER TABLE forge_content DROP CONSTRAINT forge_content_kind_check;
ALTER TABLE forge_content ADD CONSTRAINT forge_content_kind_check
    CHECK (kind IN ('body', 'comment', 'review', 'review_comment'));
ALTER TABLE forge_content ADD COLUMN path TEXT COLLATE pg_c_utf8;
ALTER TABLE forge_content ADD COLUMN line BIGINT;
ALTER TABLE forge_content ADD COLUMN reply_to BIGINT;
ALTER TABLE forge_content ADD COLUMN created_at BIGINT NOT NULL DEFAULT 0;
