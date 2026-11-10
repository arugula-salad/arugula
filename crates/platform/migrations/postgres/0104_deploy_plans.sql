-- 0104: a chant→Fly deploy's plan, run before anyone approves it (#1985,
-- `db::deploy_plans`). Times are milliseconds since the epoch.

-- One per deploy, made when its approval is first asked for. `asked_by`
-- and `asked_at` are who asked and when (an actor as `db::deploys` writes
-- one), recorded as its `requested` event once chant planned it.
-- `run_id` is its plan run (`runs`, M210, `kind deploy` with `plan`) and
-- `workspace_id` that run's box, both once made. The run gets two GitHub
-- tokens, each minted for it and given once while it runs: `read_at` is
-- when its machine was given the one that reads, to fetch the commit
-- before anything of the commit's runs, and `secrets_at` the one that
-- writes, to push chant's ledger once everything the plan ran is stopped
-- (never before the first). `gate` is the chant gate the plan
-- stopped at (JSON, `arugula_e2e::deploy::chant::Gate`) once it planned;
-- `failed` why it didn't. `ended_at` is when either was set. Additive: the
-- build before this one never reads it.
CREATE TABLE deploy_plans (
    deploy_id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    asked_by TEXT COLLATE pg_c_utf8 NOT NULL,
    asked_at BIGINT NOT NULL,
    run_id TEXT COLLATE pg_c_utf8 UNIQUE,
    workspace_id TEXT COLLATE pg_c_utf8,
    read_at BIGINT,
    secrets_at BIGINT,
    gate TEXT COLLATE pg_c_utf8,
    failed TEXT COLLATE pg_c_utf8,
    ended_at BIGINT
);

-- A v3 approval (#1985) is chant's gate statement for the gate its deploy's
-- plan stopped at: `chant` keeps that gate (JSON) and `chant_at` the
-- statement's time, as chant writes one. Both empty for a v1 or a v2.
ALTER TABLE deploy_approvals ADD COLUMN chant TEXT COLLATE pg_c_utf8;
ALTER TABLE deploy_approvals ADD COLUMN chant_at TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '';
