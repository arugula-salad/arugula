-- 0018: the standing project grant (M211, platform step 7). An owner's
-- device signs, once per project, that the platform may start and
-- instruct runs on the project's hosted workspaces ("One standing grant
-- per project starts runs", aru-099; `arugula_e2e::project_grant`). The
-- platform keeps what was signed, exactly, and relays it with each start;
-- hosted `arugulad` checks it. Nothing here can make or widen one.
--
-- Times are milliseconds since the epoch.

-- A grant, by its own id (32 hex characters, signed into it), in a
-- project. `body` is the signed bytes (`arugula project grant v1`) and
-- `sig` the device's signature over them, hex: the grant is read back from
-- them, never from the other columns, which say the same for queries
-- (`delegation`, `by_device`, `made_at`, `expires_at`, NULL for never).
-- `posted_by` is the account that sent it, NULL once that account is
-- deleted. A revoke keeps its signed bytes too (`revoke_body`,
-- `revoke_sig`, `arugula project grant revoke v1`), when the platform took
-- it (`revoked_at`), and whether it asks for the runs going to stop
-- (`stop_running`). One live grant per project.
CREATE TABLE run_grants (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    delegation TEXT NOT NULL,
    body TEXT NOT NULL,
    sig TEXT NOT NULL,
    by_device TEXT NOT NULL,
    made_at INTEGER NOT NULL,
    expires_at INTEGER,
    posted_by TEXT,
    created_at INTEGER NOT NULL,
    revoke_body TEXT,
    revoke_sig TEXT,
    revoked_at INTEGER,
    stop_running INTEGER NOT NULL DEFAULT 0,
    CHECK ((revoked_at IS NULL) = (revoke_body IS NULL) AND (revoke_body IS NULL) = (revoke_sig IS NULL))
);
CREATE INDEX run_grants_project ON run_grants (project_id, created_at);
CREATE UNIQUE INDEX run_grants_live ON run_grants (project_id) WHERE revoked_at IS NULL;
