-- 0018: hosted inference (M185, platform step 5, #1235). Agents on a
-- hosted workspace's box reach their model through the compute
-- provider's egress proxy, which fills in the key: a project's own key
-- (strangers bring theirs), or the platform's for accounts on the beta
-- allowlist, under caps ("Hosted inference: strangers bring their own
-- key", aru-096). No key is ever in a box, and none is here in the
-- clear.
--
-- Times are milliseconds since the epoch.

-- A project's own inference key, one per provider (`anthropic`,
-- `openai`), sealed with ARUGULA_CLOUD_SECRET_KEY (bound to the project
-- and the provider, as cloud secrets are): nothing answers it back. The
-- platform opens it only to write it into a box's egress rules, which the
-- compute provider's proxy holds (never the box). `set_by` is the account
-- that set it, NULL once that account is deleted. A key whose project is
-- gone is dropped.
CREATE TABLE project_inference_keys (
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    provider TEXT COLLATE pg_c_utf8 NOT NULL CHECK (provider IN ('anthropic', 'openai')),
    sealed BYTEA NOT NULL,
    set_by TEXT COLLATE pg_c_utf8,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (project_id, provider)
);

-- An agent turn on a hosted workspace's box, from its daemon asking to
-- start it to its end (`ended_at`, NULL while it runs). `pays` is whose
-- key: `project` or `platform`; a platform turn counts against `account`'s
-- caps (the workspace's maker, on the allowlist), NULL for a project's
-- key or once that account is deleted. Kept 30 days.
CREATE TABLE hosted_turns (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    workspace_id TEXT COLLATE pg_c_utf8 NOT NULL,
    provider TEXT COLLATE pg_c_utf8 NOT NULL CHECK (provider IN ('anthropic', 'openai')),
    pays TEXT COLLATE pg_c_utf8 NOT NULL CHECK (pays IN ('project', 'platform')),
    account TEXT COLLATE pg_c_utf8,
    started_at BIGINT NOT NULL,
    ended_at BIGINT
);
CREATE INDEX hosted_turns_account ON hosted_turns (account, started_at);
CREATE INDEX hosted_turns_workspace ON hosted_turns (workspace_id, started_at);

-- The SHA-256 of the egress rules a workspace's box was last given (they
-- can hold a project's key, so never the rules themselves): what a turn's
-- start and a wake compare with what they should be now.
ALTER TABLE hosted_workspaces ADD COLUMN rules TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '';
