-- 0020: what starts runs (M214, platform step 7): a project's run specs,
-- their schedules and their event rules, and a key on a run that a
-- trigger made so the same tick or event never makes two. Times are
-- milliseconds since the epoch. JSON is text. `db::triggers` has the rules.

-- A run spec: a named operation a project runs on one of its hosted
-- workspaces. `id` is `rsp_` and 32 hex characters; `name` is unique in
-- its project. `workspace_id` is the hosted workspace (`hw_`) it runs
-- on. `kind` is `run` (a command: `argv`, a JSON list) or `agent` (an
-- agent block given `instruction`, from recipe `agent` or the default).
-- `directory` is where it runs, NULL for the workspace's home.
-- `created_by` and `updated_by` are accounts, NULL once deleted.
CREATE TABLE run_specs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('run', 'agent')),
    argv TEXT NOT NULL DEFAULT '[]',
    instruction TEXT,
    agent TEXT,
    directory TEXT,
    created_by TEXT,
    created_at INTEGER NOT NULL,
    updated_by TEXT,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

-- A spec's schedule: a cron expression (five fields) read in `timezone`
-- (IANA's name). `next_at` is its next tick; `last_fired_at` the tick it
-- last fired for, `last_run_id` the run that made, and `last_error` why
-- its last tick made none. Its `platform_jobs` row (kind `schedule`, the
-- same id) fires it. `id` is `rsc_` and 32 hex characters.
CREATE TABLE run_schedules (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    enabled INTEGER NOT NULL DEFAULT 1,
    next_at INTEGER NOT NULL,
    last_fired_at INTEGER,
    last_run_id TEXT,
    last_error TEXT,
    created_by TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX run_schedules_spec ON run_schedules (spec_id);

-- A spec's event rule: a linked repo's `event` (`pr_opened`,
-- `check_failed` or `push`) starts it, from `repo` (`owner/name`) or any
-- repo linked to the project when NULL, on `branch` (a name, `*` for any
-- run of characters) or any when NULL: a PR's base branch, a check's or
-- a push's branch. `outsiders`: it also fires on events from forks and
-- from people who aren't the repo's owners, members or collaborators
-- (off: those are refused). `max_per_hour`: the most runs it starts in
-- an hour. `id` is `rru_` and 32 hex characters.
CREATE TABLE run_rules (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    event TEXT NOT NULL CHECK (event IN ('pr_opened', 'check_failed', 'push')),
    repo TEXT,
    branch TEXT,
    outsiders INTEGER NOT NULL DEFAULT 0,
    max_per_hour INTEGER NOT NULL DEFAULT 20,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX run_rules_spec ON run_rules (spec_id);
CREATE INDEX run_rules_project ON run_rules (project_id, event);

-- What a trigger made a run for: a schedule's tick
-- (`schedule:<id>:<tick>`) or a rule's event (`rule:<id>:<event>`).
-- NULL for a run started by hand. Unique, so a tick or an event makes
-- one run however many machines see it.
ALTER TABLE runs ADD COLUMN trigger_key TEXT;
CREATE UNIQUE INDEX runs_trigger_key ON runs (trigger_key) WHERE trigger_key IS NOT NULL;
