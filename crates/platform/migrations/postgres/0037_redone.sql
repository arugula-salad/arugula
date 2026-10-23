-- 0037: absorbing redone (M235, platform step 8): what `arugula-platform
-- import redone` carried over, the redone tokens that now work on the
-- platform's MCP endpoint, and every write of a project's secret into
-- Infisical (the projection). Times are milliseconds since the epoch.
-- `db::redone` has the rules.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

-- What the import did, one row per redone object it carried over: a
-- tenant (`redone_id` its id) and the project it became, a token (`tok_…`)
-- and the agent grant token it became (`platform_id`, `agt_…`, owned by
-- `account_id`), or an app (its name) and what it became. Never updated,
-- never deleted by the import: a second run finds these and changes
-- nothing, and a token revoked since is never imported again.
CREATE TABLE redone_imports (
    kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (kind IN ('tenant', 'token', 'app')),
    redone_id TEXT COLLATE pg_c_utf8 NOT NULL,
    tenant TEXT COLLATE pg_c_utf8 NOT NULL,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    account_id TEXT COLLATE pg_c_utf8,
    platform_id TEXT COLLATE pg_c_utf8,
    at BIGINT NOT NULL,
    PRIMARY KEY (kind, redone_id)
);
CREATE INDEX redone_imports_project ON redone_imports (project_id);

-- An agent grant token carried over from another product rather than
-- minted on the platform page: 'redone' for a redone `rdn_` token, whose
-- SHA-256 redone kept and the import copied (the token itself was never
-- seen). Its agent gets redone's tool names besides the platform's (the
-- shim) until it's revoked.
ALTER TABLE agent_tokens ADD COLUMN imported_from TEXT COLLATE pg_c_utf8;

-- Each write of a project's secret into Infisical, as the platform
-- projects it (aru-107: the platform's copy is the truth): the project,
-- the app (Infisical's folder `/<app>/`) and the secret's name, never its
-- value. `mode` is how the projection was set (`dry_run` writes nothing);
-- `outcome` what became of it (`written`, `would_write`, `refused`, or
-- `failed`), with a line of why. Who set it: an account, and the agent
-- token if its agent did. Never updated.
CREATE TABLE secret_projections (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    app TEXT COLLATE pg_c_utf8 NOT NULL,
    name TEXT COLLATE pg_c_utf8 NOT NULL,
    mode TEXT COLLATE pg_c_utf8 NOT NULL CHECK (mode IN ('dry_run', 'write')),
    outcome TEXT COLLATE pg_c_utf8 NOT NULL CHECK (outcome IN ('written', 'would_write', 'refused', 'failed')),
    detail TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    by_account TEXT COLLATE pg_c_utf8,
    by_agent TEXT COLLATE pg_c_utf8,
    at BIGINT NOT NULL
);
CREATE INDEX secret_projections_app ON secret_projections (app, project_id, at);
CREATE INDEX secret_projections_project ON secret_projections (project_id, at);
