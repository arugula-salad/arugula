-- 0027: deploy approvals (M218, platform step 7). The signed approvals and
-- denies a deploy got (`db::deploy_approvals`). Times are milliseconds
-- since the epoch.

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
    id TEXT PRIMARY KEY,
    deploy_id TEXT NOT NULL,
    environment_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('once', 'deny')),
    op_hash TEXT NOT NULL,
    account TEXT NOT NULL,
    device TEXT NOT NULL,
    device_name TEXT NOT NULL DEFAULT '',
    expires INTEGER NOT NULL,
    sig TEXT NOT NULL,
    passkey_id TEXT NOT NULL DEFAULT '',
    passkey_client_data TEXT NOT NULL DEFAULT '',
    passkey_authenticator_data TEXT NOT NULL DEFAULT '',
    passkey_signature TEXT NOT NULL DEFAULT '',
    at INTEGER NOT NULL,
    UNIQUE (environment_id, seq)
);
CREATE INDEX deploy_approvals_deploy ON deploy_approvals (deploy_id, seq);
