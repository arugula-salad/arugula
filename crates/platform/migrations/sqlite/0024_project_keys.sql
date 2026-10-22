-- 0024: per-project envelope keys for secrets (M209, platform step 7).
-- Each project has a random data key, wrapped (sealed) by the platform's
-- master key, ARUGULA_CLOUD_SECRET_KEY; the data key seals that project's
-- secrets (`keys`). Rotating the master key re-wraps the data keys and
-- touches no secret. Times are milliseconds since the epoch.

-- A project's data key, version `version` (1 until a data key is
-- rotated, which nothing does yet). `wrapped` is it sealed with the
-- master key whose id is `master_id` (a hash of the key, never the key),
-- bound to the project and the version. `rotated_at` is when it was last
-- re-wrapped by a new master key. A project's keys go with it.
CREATE TABLE project_keys (
    project_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    wrapped BLOB NOT NULL,
    master_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    PRIMARY KEY (project_id, version)
);

-- A project's secret that has no table of its own: an environment's
-- (M217), by `name` (`environment/<env id>/<NAME>`). Sealed with
-- version `key_version` of the project's data key, bound to the project
-- and the name.
CREATE TABLE project_secrets (
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    key_version INTEGER NOT NULL,
    sealed BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, name)
);

-- Which key sealed each stored secret: 0 is the master key itself (before
-- M209: bound to the owner and the name), a version is the project's data
-- key. The platform re-seals 0s with the project's key, one row at a time
-- (`keys::reseal`), and opens either until none is left.
ALTER TABLE cloud_secrets ADD COLUMN key_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE hosted_workspace_secrets ADD COLUMN key_version INTEGER NOT NULL DEFAULT 0;
