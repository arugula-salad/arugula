-- 0014: hosted workspaces' usage (M188, platform step 5, #1235). What the
-- compute provider bills for (E2B: vCPU-seconds and GiB-seconds while a
-- box runs; nothing while it's paused), kept from the platform's own
-- lifecycle, and what the provider says it billed, to check that against.
--
-- Times are milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

-- A box's running intervals: from its create or a wake to its pause, its
-- delete or the worker seeing it stopped. `ended_at` is NULL while it
-- runs; a workspace has at most one such interval, and its intervals never
-- overlap (`hosted::usage`). `cpus` and `memory_mb` are the box's size,
-- once the provider has said it.
CREATE TABLE hosted_usage (
    workspace_id TEXT COLLATE pg_c_utf8 NOT NULL,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    sandbox_id TEXT COLLATE pg_c_utf8 NOT NULL,
    cpus BIGINT,
    memory_mb BIGINT,
    started_at BIGINT NOT NULL,
    ended_at BIGINT,
    CHECK (ended_at IS NULL OR ended_at >= started_at),
    PRIMARY KEY (workspace_id, started_at)
);
CREATE UNIQUE INDEX hosted_usage_open ON hosted_usage (workspace_id) WHERE ended_at IS NULL;
CREATE INDEX hosted_usage_project ON hosted_usage (project_id, started_at);
CREATE INDEX hosted_usage_sandbox ON hosted_usage (sandbox_id);

-- What the provider says a box ran (E2B's lifecycle events), summed over
-- its runs, as last read at `checked_at`. `runs` is NULL when the provider
-- keeps no such record for it (loam).
CREATE TABLE hosted_usage_provider (
    sandbox_id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    workspace_id TEXT COLLATE pg_c_utf8 NOT NULL,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    runs BIGINT,
    ms BIGINT NOT NULL DEFAULT 0,
    cpu_ms BIGINT NOT NULL DEFAULT 0,
    mib_ms BIGINT NOT NULL DEFAULT 0,
    checked_at BIGINT NOT NULL
);
