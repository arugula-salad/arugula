-- 0080: one work item behind the issues that are the same work (Arugula
-- #405). A native issue, the GitHub issue it was exported to and a Plane item
-- are each an `iss_` or `fi_` of their own; a `wi_` is the work they share,
-- made on demand by `Db::work_item_for`: when a board takes the item, and by
-- the one-time move (`db::boards_work`) for the rows there already. Links
-- come from `issues.github_repo_id` and `github_number` (a native issue
-- exported to GitHub); a Plane item has no link to a native one, so it is a
-- work item of its own.
--   - `work_item_refs`: where the work lives. `system` is `native`, `github`
--     or `plane`; `target` is the `iss_` or `fi_` id, in one work item only;
--     `instance` is the project, the GitHub repo id or the tracker connection
--     id; `key` is `12`, `org/repo#12` or `ARUGU-12`; `synced_rev` is for a
--     later sync and stays NULL.
--   - `board_entries.work_item`: the work item a row's issue is, NULL for a
--     note and a PR row. A board has one row for a work item, so the same
--     work as a native issue and as its GitHub issue is one row. The row
--     keeps its pointer, which is what clients address it by.
-- Additive: the build before this one ignores the table and the column.
CREATE TABLE work_items (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX work_items_project ON work_items (project_id);
CREATE TABLE work_item_refs (
    work_item TEXT NOT NULL,
    system TEXT NOT NULL CHECK (system IN ('native', 'github', 'plane')),
    target TEXT NOT NULL UNIQUE,
    instance TEXT NOT NULL,
    key TEXT NOT NULL,
    url TEXT,
    synced_rev TEXT
);
CREATE INDEX work_item_refs_item ON work_item_refs (work_item);
ALTER TABLE board_entries ADD COLUMN work_item TEXT;
CREATE UNIQUE INDEX board_entries_work_item ON board_entries (board_id, work_item) WHERE work_item IS NOT NULL;
