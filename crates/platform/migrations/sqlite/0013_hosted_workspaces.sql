-- 0013: hosted workspaces (M179, platform step 5, #1235). A hosted
-- workspace is `arugulad` in a box on the compute provider (E2B, M176),
-- in a project. The platform keeps what it is (its spec), where it runs
-- (its box), and what its leased worker is doing to it. Kept apart from
-- the Managoat `cloud_workspaces` (`cw_`), which go in M189.
--
-- Times are milliseconds since the epoch. JSON lists and objects are text.

-- A hosted workspace, by `hw_` and 32 hex characters, in a project.
-- - `spec` is JSON: its repos, packages, setup script and the names of the
--   env vars it takes from its secrets. `spec_hash` is its SHA-256; the box
--   was built from `built_hash` ("rebuild needed" when they differ, M183).
-- - `provider` is `e2b`. `sandbox_id` is its box, once made; `template` the
--   template it was made from.
-- - `state`: `creating`, `enrolling` (made, its daemon not yet enrolled),
--   `ready`, `paused`, `failed` (`error` says why), `deleting`, `deleted`.
-- - `asked` is what a person asked of it that the worker hasn't done yet:
--   `wake`, `pause`, `rebuild` or `delete`.
-- - `stage` is the worker's journal of a create: empty, `sent` (asked of
--   the provider, its answer not yet kept: found by its metadata, never
--   sent again) or `made`, since `stage_at`.
-- - `enroll_until` is when its one-time enrollment runs out, and `machine`
--   the hosted machine it enrolled as (the hub's id).
-- - `lease` and `lease_until` are the worker's, as `cloud_jobs`': a held
--   row has a lease, and a free one is looked at again from `lease_until`.
-- - `created_by` is the account that made it, NULL once that account is
--   deleted.
CREATE TABLE hosted_workspaces (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    spec TEXT NOT NULL,
    spec_hash TEXT NOT NULL,
    built_hash TEXT,
    provider TEXT NOT NULL DEFAULT 'e2b',
    template TEXT,
    sandbox_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('creating', 'enrolling', 'ready', 'paused', 'failed', 'deleting', 'deleted')),
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
    deleted_at INTEGER
);
CREATE INDEX hosted_workspaces_project ON hosted_workspaces (project_id);
CREATE UNIQUE INDEX hosted_workspaces_name ON hosted_workspaces (project_id, name) WHERE deleted_at IS NULL;
CREATE INDEX hosted_workspaces_due ON hosted_workspaces (lease_until) WHERE deleted_at IS NULL;

-- The secrets a hosted workspace takes, by name. A value is either sealed
-- here with ARUGULA_CLOUD_SECRET_KEY (`store` = `sealed`, bound to the
-- workspace and the name, as cloud secrets are), or held by the provider's
-- secrets broker (`broker`, M185), when `sealed` is NULL.
CREATE TABLE hosted_workspace_secrets (
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    store TEXT NOT NULL CHECK (store IN ('sealed', 'broker')),
    sealed BLOB,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, name)
);

-- A port a dev server listens on in a hosted workspace (M186), and
-- whether a member made it public (M187), who, and until when.
CREATE TABLE hosted_ports (
    workspace_id TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    label TEXT NOT NULL DEFAULT '',
    public_by TEXT,
    public_until INTEGER,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, port)
);
