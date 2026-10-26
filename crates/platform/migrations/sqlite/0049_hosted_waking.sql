-- 0049: a hosted workspace can be `waking` (M258, Arugula #124): its box resumed,
-- its daemon not yet dialed in again. It's `ready` only once it has; one
-- whose daemon doesn't within the platform's dial wait is `failed`, its
-- `error` saying it needs a rebuild.
--
-- Additive: it only lets `state` hold one more value. A build from before
-- this one claims a `waking` row like a `ready` one, reads its box's state
-- and calls it `ready`, as it did before. SQLite changes a `CHECK` by
-- making the table again, with every column it has now.
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
    purpose TEXT NOT NULL DEFAULT 'workspace' CHECK (purpose IN ('workspace', 'deploy'))
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
