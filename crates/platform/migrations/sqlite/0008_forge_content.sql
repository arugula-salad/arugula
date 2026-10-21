-- 0008: what bodies on read and content mirroring need (M166, #1212).
--
-- `forge_items.content_rev` goes up with each webhook that may change an
-- item's body or comments: the read cache, kept in memory on each
-- machine, serves an item's content only while its rev is the one it was
-- fetched at, so a webhook taken on one machine drops what every machine
-- kept.
ALTER TABLE forge_items ADD COLUMN content_rev INTEGER NOT NULL DEFAULT 0;

-- `forge_content` (nothing wrote it before this) also keeps a review's
-- text (`review`), and a review comment's thread: its file (`path`), its
-- `line`, and the comment it answers (`reply_to`, GitHub's id).
-- `created_at` is GitHub's. SQLite changes a `CHECK` by making the table
-- again.
CREATE TABLE forge_content_new (
    repo_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('body', 'comment', 'review', 'review_comment')),
    github_id INTEGER NOT NULL,
    author_login TEXT,
    author_id INTEGER,
    body TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    path TEXT,
    line INTEGER,
    reply_to INTEGER,
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (repo_id, kind, github_id)
);
INSERT INTO forge_content_new (repo_id, number, kind, github_id, author_login, author_id, body, updated_at)
    SELECT repo_id, number, kind, github_id, author_login, author_id, body, updated_at FROM forge_content;
DROP TABLE forge_content;
ALTER TABLE forge_content_new RENAME TO forge_content;
CREATE INDEX forge_content_item ON forge_content (repo_id, number);
