-- 0027: deploy approvals (M218, platform step 7). The signed approvals and
-- denies a deploy got (`db::deploy_approvals`). Times are milliseconds
-- since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

-- An approval (or a deny) of a deploy: S37's work approval over the
-- deploy's operation (`arugula_e2e::deploy`), signed by a project owner's
-- device and checked before it's written. `id` is `apv_` and 32 hex
-- characters. `seq` is its `approved` or `denied` event's in the
-- environment's log, written in the same transaction; that event's detail
-- holds the same signed approval, so the log alone can be checked.
-- `decision` is `once` or `deny`; `op_hash` the operation's SHA-256;
-- `account` the approver's; `device` the device that signed (its id) and
-- `device_name` its name then; `expires` by when the apply must start;
-- `sig` the device's Ed25519 signature, hex. `passkey_*` are the passkey
-- assertion the hub checked over the approval's bytes (its credential id,
-- clientDataJSON, authenticatorData and signature, each base64url), which
-- anyone with the passkey's public key can check again. Never updated.
CREATE TABLE deploy_approvals (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    deploy_id TEXT COLLATE pg_c_utf8 NOT NULL,
    environment_id TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    decision TEXT COLLATE pg_c_utf8 NOT NULL CHECK (decision IN ('once', 'deny')),
    op_hash TEXT COLLATE pg_c_utf8 NOT NULL,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    device TEXT COLLATE pg_c_utf8 NOT NULL,
    device_name TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    expires BIGINT NOT NULL,
    sig TEXT COLLATE pg_c_utf8 NOT NULL,
    passkey_id TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    passkey_client_data TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    passkey_authenticator_data TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    passkey_signature TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    at BIGINT NOT NULL,
    UNIQUE (environment_id, seq)
);
CREATE INDEX deploy_approvals_deploy ON deploy_approvals (deploy_id, seq);
