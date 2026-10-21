-- 0007: the forge mirror's poller's own state (M164, #1210).
--
-- `poll_failures` is how many of a linked repo's turns failed in a row,
-- for a back-off that grows (to an hour). `poll_cursor` is where a turn
-- that ran out of pages stopped (JSON: which list, and the next page's
-- URL), so the next turn carries on rather than dropping items.
ALTER TABLE linked_repos ADD COLUMN poll_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE linked_repos ADD COLUMN poll_cursor TEXT;

-- What the poller reads again at its repo's next turn, first: an item
-- (`pr` or `issue`, `key` its number) or a head's checks (`head`, `key`
-- the SHA) that two answers gave in the same second and disagreed on.
CREATE TABLE forge_refetch (
    repo_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('pr', 'issue', 'head')),
    key TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (repo_id, kind, key)
);
