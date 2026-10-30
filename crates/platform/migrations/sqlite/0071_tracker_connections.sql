-- 0071: a Plane project connected to an Arugula project (Arugula #356, P3-1).
-- The tracker's work items are mirrored into `forge_items`, beside GitHub's.
--
-- A connection is the counterpart of `linked_repos`: `mirror_id` stands where
-- a repo's id does, as `forge_items.repo_id` of its rows. It is always
-- negative, so it can't be a GitHub repo's id, and nothing that joins
-- `linked_repos` on `repo_id` sees a tracker's rows. Its rows' `number` is
-- the tracker's own sequence id. The API key and the webhook secret are
-- sealed in `project_secrets` as `tracker/plane/<id>/key` and `/webhook`,
-- never here. `lease`, `lease_until`, `backfilled_at`, `poll_cursor` and
-- `poll_failures` are the backfill's, as `linked_repos`'.
--
-- Additive: the build before this one doesn't read the columns.
CREATE TABLE tracker_connections (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    provider TEXT NOT NULL CHECK (provider IN ('plane')),
    workspace_slug TEXT NOT NULL,
    source_project_id TEXT NOT NULL,
    identifier TEXT NOT NULL,
    name TEXT NOT NULL,
    mirror_id INTEGER NOT NULL UNIQUE CHECK (mirror_id < 0),
    agent_writes TEXT NOT NULL DEFAULT 'approve' CHECK (agent_writes IN ('approve', 'allow')),
    linked_by TEXT,
    created_at INTEGER NOT NULL,
    unlinked_at INTEGER,
    purge_after INTEGER,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0,
    backfilled_at INTEGER,
    poll_cursor TEXT,
    poll_failures INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX tracker_connections_project ON tracker_connections (project_id);
CREATE INDEX tracker_connections_purge ON tracker_connections (purge_after) WHERE purge_after IS NOT NULL;
-- A Plane project is connected to one Arugula project at a time.
CREATE UNIQUE INDEX tracker_connections_source ON tracker_connections (provider, workspace_slug, source_project_id)
    WHERE unlinked_at IS NULL;

ALTER TABLE forge_items ADD COLUMN provider TEXT NOT NULL DEFAULT 'github';
ALTER TABLE forge_items ADD COLUMN connection_id TEXT;
ALTER TABLE forge_items ADD COLUMN source_item_id TEXT;
ALTER TABLE forge_items ADD COLUMN item_type TEXT;
ALTER TABLE forge_items ADD COLUMN state_name TEXT;
ALTER TABLE forge_items ADD COLUMN state_group TEXT;
ALTER TABLE forge_items ADD COLUMN parent_source_id TEXT;
CREATE UNIQUE INDEX forge_items_source ON forge_items (connection_id, source_item_id);
