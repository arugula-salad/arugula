-- 0111: a ticket marked Not finished (Arugula #841). `item_holds` has a row
-- for each native issue (`iss_`) or Plane item (`fi_`) a person or an agent
-- holds, with what is left. While a row is there, a merged PR that names
-- the ticket doesn't close it (`forge::closes`, `trackers::done`); taking
-- it off closes the ticket if such a PR has merged. On the ticket, not on a
-- board's row: one ticket on several boards is held on all of them.
-- `skipped` is JSON: the merged PR a hold kept from closing the ticket (its
-- id, `owner/repo#N`, the words that named it and who merged it), so a
-- release closes it even when the mirror keeps no PR bodies.
-- Additive: the build before this one never reads the table.
CREATE TABLE item_holds (
    project_id TEXT NOT NULL,
    item TEXT NOT NULL,
    reason TEXT NOT NULL,
    held_by TEXT NOT NULL,
    held_by_agent TEXT,
    held_at INTEGER NOT NULL,
    skipped TEXT,
    PRIMARY KEY (project_id, item)
);
