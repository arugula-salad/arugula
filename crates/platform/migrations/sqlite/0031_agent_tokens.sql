-- 0031: agent grant tokens (M230, platform step 8): the platform's MCP
-- endpoint for agents with no machine of their own. Times are milliseconds
-- since the epoch. `db::agent_tokens` has the rules.

-- A bearer token an owner minted for an agent with no machine (`id` is
-- `agt_` and 32 hex characters). Its agent acts as `account_id`'s, labelled
-- with `name`, and never past what that account reads. What it may do is a
-- live grant per power and project in `grants` (`subject_kind` 'agent',
-- `subject_id` this id), given when it's minted. Only the token's SHA-256
-- (hex) is kept: the token is shown once. Revoking it revokes its grants
-- too; a revoked one keeps its row, for the record. It's minted only from
-- a browser's session on the platform page; `minted_from` is that
-- browser's User-Agent (the hub ties no session to a device).
CREATE TABLE agent_tokens (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER,
    minted_from TEXT NOT NULL DEFAULT ''
);
CREATE INDEX agent_tokens_account ON agent_tokens (account_id, created_at);

-- Every tool call a token's agent made (`id` is `atc_` and 32 hex
-- characters), as its owner reads them: which tool, in which project
-- when it named one, how it went (`ok`, `refused` or `failed`), and a line
-- of what happened (the object it made, or why not). Never an argument's
-- value: a secret's name, never the secret. Never updated.
CREATE TABLE agent_token_calls (
    id TEXT PRIMARY KEY,
    token_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    project_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('ok', 'refused', 'failed')),
    detail TEXT NOT NULL DEFAULT '',
    at INTEGER NOT NULL
);
CREATE INDEX agent_token_calls_token ON agent_token_calls (token_id, at);
CREATE INDEX agent_token_calls_account ON agent_token_calls (account_id);

-- Who last set each of a project's own secrets (`project/<NAME>`, M230):
-- an account, and the agent grant token its agent set it with, if one did.
-- Shown to the project's owners by name, never the value.
ALTER TABLE project_secrets ADD COLUMN updated_by TEXT;
ALTER TABLE project_secrets ADD COLUMN updated_by_agent TEXT;
