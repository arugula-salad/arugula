-- 0016: runs (M210, platform step 7). A project's runs, each one S37
-- operation on a workspace, with its append-only history of state changes
-- and what it left behind. Times are milliseconds since the epoch. JSON is
-- text. `db::runs` has the rules.
-- SQLite's file in Postgres's words: `BIGINT` for its integers,
-- `COLLATE pg_c_utf8` text, and the append-only trigger as a function.

-- A run. `id` is the platform's (`run_` and 32 hex characters).
-- `workspace_id` is the workspace it runs on (step 5's hosted ones), not
-- checked here. `trigger_kind` is `schedule`, `event` or `manual`, and
-- `trigger_source` a small JSON object saying which (the schedule, the
-- forge event). `op` is the S37 operation's bytes (`arugula op v1`), and
-- `op_hash` their SHA-256 in hex: what an approval or a grant names.
-- `daemon` is the device the operation names (its `daemon` line): the
-- only one whose reports move the run.
-- `state` is one of S37's eight, moved only with an event in
-- `run_events` (`last_seq` is the latest one's). `started_by` is the
-- account that started it (by hand; an agent's run keeps its owner's
-- account and names the machine in `started_by_agent`), or the schedule's
-- or event's id. `grant_id` is the standing grant it ran under (M211).
-- `started_at` is when it began running, `ended_at` when it reached a
-- terminal state, `exit_code` its process's. `chant_ledger` links the
-- chant ledger entry a release run wrote.
CREATE TABLE runs (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    workspace_id TEXT COLLATE pg_c_utf8,
    trigger_kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (trigger_kind IN ('schedule', 'event', 'manual')),
    trigger_source TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '{}',
    op TEXT COLLATE pg_c_utf8 NOT NULL,
    op_hash TEXT COLLATE pg_c_utf8 NOT NULL,
    daemon TEXT COLLATE pg_c_utf8 NOT NULL,
    state TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'approved', 'denied', 'canceled', 'expired', 'running', 'succeeded', 'failed')
    ),
    started_by TEXT COLLATE pg_c_utf8 NOT NULL,
    started_by_agent TEXT COLLATE pg_c_utf8,
    grant_id TEXT COLLATE pg_c_utf8,
    last_seq BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    started_at BIGINT,
    ended_at BIGINT,
    exit_code BIGINT,
    chant_ledger TEXT COLLATE pg_c_utf8
);
CREATE INDEX runs_project ON runs (project_id, created_at);
CREATE INDEX runs_state ON runs (state, created_at);
CREATE INDEX runs_started_by ON runs (started_by);

-- A run's history: each change of state, numbered from 1 in the run.
-- `from_state` is NULL for the first (the run made, as `pending`).
-- `detail` is a small JSON object (an exit code, why it failed), kept as
-- the reporter wrote it. A change a daemon reported names it in `daemon`
-- with its signature (`arugula run event v1`, `arugula_e2e::run`, hex) in
-- `sig`; one control recorded has neither. Rows are never changed, and
-- never deleted while their run is there: only deleting the run (with its
-- project) takes them.
CREATE TABLE run_events (
    run_id TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    from_state TEXT COLLATE pg_c_utf8,
    to_state TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL,
    detail TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '{}',
    daemon TEXT COLLATE pg_c_utf8,
    sig TEXT COLLATE pg_c_utf8,
    PRIMARY KEY (run_id, seq),
    CHECK ((daemon IS NULL) = (sig IS NULL))
);
CREATE FUNCTION run_events_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'run_events are append-only';
END;
$$;
CREATE TRIGGER run_events_append_only BEFORE UPDATE ON run_events
    FOR EACH ROW EXECUTE FUNCTION run_events_append_only();
CREATE FUNCTION run_events_kept() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM runs WHERE id = OLD.run_id) THEN
        RAISE EXCEPTION 'run_events are append-only: delete the run first';
    END IF;
    RETURN OLD;
END;
$$;
CREATE TRIGGER run_events_kept BEFORE DELETE ON run_events
    FOR EACH ROW EXECUTE FUNCTION run_events_kept();

-- What a run left behind: a file by name, its size in bytes, its digest
-- (`sha256:` and hex) and where it is in the run bucket (M213).
CREATE TABLE run_artifacts (
    run_id TEXT COLLATE pg_c_utf8 NOT NULL,
    name TEXT COLLATE pg_c_utf8 NOT NULL,
    size BIGINT NOT NULL,
    digest TEXT COLLATE pg_c_utf8 NOT NULL,
    bucket_key TEXT COLLATE pg_c_utf8 NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (run_id, name)
);
