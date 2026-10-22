-- 0021: environments and deploys (M217, platform step 7). A project's
-- environments, the deploys planned to them, and each environment's
-- append-only event log, hash-chained so anyone can verify it
-- (`db::deploys`). Times are milliseconds since the epoch. JSON lists are
-- text.

-- An environment of a project. `id` is `env_` and 32 hex characters;
-- `name` is unique in its project. `driver` is what applies its deploys
-- (`chant-fly`, `apps-platform`), text so a new driver needs no
-- migration. `secret_names` is a JSON list of the names of the secrets a
-- driver run gets; their values are sealed per project elsewhere (M209),
-- never here.
CREATE TABLE environments (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    secret_names TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (project_id, name)
);

-- A deploy to an environment. `id` is `dep_` and 32 hex characters.
-- `source_kind` is `commit` (`source_ref` a commit's sha) or `artifact`
-- (an artifact's digest), and `source_where` where it is (a repo, a
-- registry). `plan_digest` is the SHA-256 of the plan the driver made.
-- `state` is S37's lifecycle (q4) with `planned` before it and `stale`
-- beside it: `planned`, `pending` (approval asked for), `approved`,
-- `denied`, `canceled`, `expired`, `running` (being applied),
-- `succeeded`, `failed` or `stale`. `planned_seq` is the seq of its
-- `planned` event in its environment's log: the stale rule compares it
-- with the environment's later applies. `run_id` is the run that applied
-- it (`run_`, M210), set when it starts. `planned_by` is the account that
-- planned it; an agent's plan keeps its owner's account there and names
-- its machine in `planned_by_agent`. Its state is also its events', which
-- are the record; this row is what's quick to read.
CREATE TABLE deploys (
    id TEXT PRIMARY KEY,
    environment_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('commit', 'artifact')),
    source_ref TEXT NOT NULL,
    source_where TEXT NOT NULL,
    plan_digest TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned', 'pending', 'approved', 'denied', 'canceled', 'expired',
        'running', 'succeeded', 'failed', 'stale')),
    planned_seq INTEGER NOT NULL,
    run_id TEXT,
    planned_by TEXT NOT NULL,
    planned_by_agent TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX deploys_environment ON deploys (environment_id, state);
CREATE INDEX deploys_planned_by ON deploys (planned_by);

-- An environment's event log: append-only, never updated or deleted in
-- place. Triggers refuse an update, and a delete while the event's
-- environment exists: a log goes only whole, after its environment (an
-- account's deletion; a window's `--replace` deletes environments first). `seq` counts from 1 in each environment.
-- `kind` is what happened to `deploy_id`: `planned`, `requested`,
-- `approved`, `denied`, `canceled`, `expired`, `started`, `succeeded`,
-- `failed` or `staled`. `actor` is who: `person <account>`, `agent
-- <account> <machine>`, `run <run id>` or `platform`. `detail` is a small
-- JSON object, kept as the exact text that was hashed. `hash` is SHA-256
-- over the event's canonical lines (`db::deploys::Event::canonical`),
-- which start with `prev_hash`, the hash of the event before it in the
-- environment (64 zeros for the first): a chain per environment.
CREATE TABLE deploy_events (
    id INTEGER PRIMARY KEY,
    environment_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    deploy_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    actor TEXT NOT NULL,
    at INTEGER NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    prev_hash TEXT NOT NULL,
    hash TEXT NOT NULL,
    UNIQUE (environment_id, seq)
);
CREATE INDEX deploy_events_deploy ON deploy_events (deploy_id, seq);
CREATE TRIGGER deploy_events_append_only BEFORE UPDATE ON deploy_events BEGIN
    SELECT RAISE(ABORT, 'deploy_events are append-only');
END;
CREATE TRIGGER deploy_events_kept BEFORE DELETE ON deploy_events
    WHEN EXISTS (SELECT 1 FROM environments WHERE id = old.environment_id)
BEGIN
    SELECT RAISE(ABORT, 'deploy_events are append-only: only a deleted environment''s go');
END;
