-- 0054: snapshots per hosted workspace (M263, thread = session, Arugula #140).
-- A workspace whose box set up all right gets a snapshot of a box set up
-- from its spec, which its new boxes and rebuilds start from instead of
-- running setup again. One per workspace, not per project: setup runs with
-- the workspace's own secrets. `hosted::snapshot` has the rules.
--
-- - A snapshot is built by a box of its own, a row here of purpose
--   `snapshot`, whose `parent_id` is the workspace it builds for. It runs the
--   workspace's setup, wipes itself and is snapshotted, then deleted. People
--   never list or open it.
-- - On a workspace's row: `snapshot_id` is its current snapshot (an E2B
--   template id), built at `snapshot_built_at` from the spec whose hash is
--   `snapshot_hash`, on the release template `snapshot_template`.
--   `snapshot_status` is the latest build's: `building`, `ready` or
--   `failed` (`snapshot_error` says why; a failure keeps the current one).
--   `snapshot_try_hash` and `snapshot_try_at` are the latest build's spec
--   hash and start, which say when the next is due. `snapshot_retired` is a
--   JSON list of earlier snapshots still to delete, and `box_snapshot` the
--   snapshot its box was made from (one isn't deleted while its box lives).
--
-- Additive: it lets `purpose` hold one more value and adds columns that
-- may be NULL or have a default. A build from before this one ignores the
-- columns, and lists and opens only `workspace` rows; it would take a
-- builder's row like a workspace's and keep its box until it's deleted.
-- SQLite changes a `CHECK` by making the table again, with every column it
-- has now.
CREATE TABLE hosted_workspaces_new (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    spec TEXT NOT NULL,
    spec_hash TEXT NOT NULL,
    built_hash TEXT,
    provider TEXT NOT NULL DEFAULT 'e2b',
    template TEXT,
    sandbox_id TEXT,
    state TEXT NOT NULL
        CHECK (state IN ('creating', 'enrolling', 'ready', 'waking', 'paused', 'failed', 'deleting', 'deleted')),
    asked TEXT CHECK (asked IN ('wake', 'pause', 'rebuild', 'delete')),
    stage TEXT NOT NULL DEFAULT '',
    stage_at INTEGER,
    enroll_until INTEGER,
    machine TEXT,
    error TEXT,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0,
    created_by TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER,
    rules TEXT NOT NULL DEFAULT '',
    setup_status TEXT CHECK (setup_status IN ('setting_up', 'ready', 'failed')),
    setup_step TEXT,
    setup_reason TEXT,
    setup_pane INTEGER,
    setup_at INTEGER,
    purpose TEXT NOT NULL DEFAULT 'workspace' CHECK (purpose IN ('workspace', 'deploy', 'snapshot')),
    parent_id TEXT,
    snapshot_id TEXT,
    snapshot_hash TEXT,
    snapshot_template TEXT,
    snapshot_built_at INTEGER,
    snapshot_status TEXT CHECK (snapshot_status IN ('building', 'ready', 'failed')),
    snapshot_error TEXT,
    snapshot_try_hash TEXT,
    snapshot_try_at INTEGER,
    snapshot_retired TEXT NOT NULL DEFAULT '[]',
    box_snapshot TEXT
);
INSERT INTO hosted_workspaces_new (id, project_id, name, spec, spec_hash, built_hash, provider, template, sandbox_id,
        state, asked, stage, stage_at, enroll_until, machine, error, lease, lease_until, created_by, created_at,
        updated_at, deleted_at, rules, setup_status, setup_step, setup_reason, setup_pane, setup_at, purpose)
    SELECT id, project_id, name, spec, spec_hash, built_hash, provider, template, sandbox_id,
        state, asked, stage, stage_at, enroll_until, machine, error, lease, lease_until, created_by, created_at,
        updated_at, deleted_at, rules, setup_status, setup_step, setup_reason, setup_pane, setup_at, purpose
    FROM hosted_workspaces;
DROP TABLE hosted_workspaces;
ALTER TABLE hosted_workspaces_new RENAME TO hosted_workspaces;
CREATE INDEX hosted_workspaces_project ON hosted_workspaces (project_id);
CREATE UNIQUE INDEX hosted_workspaces_name ON hosted_workspaces (project_id, name) WHERE deleted_at IS NULL;
CREATE INDEX hosted_workspaces_due ON hosted_workspaces (lease_until) WHERE deleted_at IS NULL;
CREATE INDEX hosted_workspaces_parent ON hosted_workspaces (parent_id) WHERE parent_id IS NOT NULL;
