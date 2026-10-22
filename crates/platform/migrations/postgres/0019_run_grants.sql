-- 0019: the standing project grant (M211, platform step 7). An owner's
-- device signs, once per project, that the platform may start and
-- instruct runs on the project's hosted workspaces ("One standing grant
-- per project starts runs", aru-099; `arugula_e2e::project_grant`). The
-- platform keeps what was signed, exactly, and relays it with each start;
-- hosted `arugulad` checks it. Nothing here can make or widen one.
--
-- Times are milliseconds since the epoch.
-- SQLite's file in Postgres's words: `BIGINT` for its integers, `BOOLEAN`
-- for its flags, and `COLLATE pg_c_utf8` text.

-- A grant, by its own id (32 hex characters, signed into it), in a
-- project. `body` is the signed bytes (`arugula project grant v1`) and
-- `sig` the device's signature over them, hex: the grant is read back from
-- them, never from the other columns, which say the same for queries
-- (`delegation`, `by_device`, `made_at`, `expires_at`, NULL for never).
-- `posted_by` is the account that sent it, NULL once that account is
-- deleted. A revoke (`arugula project grant revoke v2`) takes back every
-- grant of the project under its delegation made by its time: each such
-- row keeps its signed bytes (`revoke_body`, `revoke_sig`), its signed time
-- (`revoke_at`), when the platform took it (`revoked_at`), and whether it
-- asks for the runs going to stop (`stop_running`). A grant that expired,
-- or whose delegation isn't the project's live one any more, is
-- `superseded_at` when a new one is signed. One live grant per project.
CREATE TABLE run_grants (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    delegation TEXT COLLATE pg_c_utf8 NOT NULL,
    body TEXT COLLATE pg_c_utf8 NOT NULL,
    sig TEXT COLLATE pg_c_utf8 NOT NULL,
    by_device TEXT COLLATE pg_c_utf8 NOT NULL,
    made_at BIGINT NOT NULL,
    expires_at BIGINT,
    posted_by TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL,
    revoke_body TEXT COLLATE pg_c_utf8,
    revoke_sig TEXT COLLATE pg_c_utf8,
    revoke_at BIGINT,
    revoked_at BIGINT,
    superseded_at BIGINT,
    stop_running BOOLEAN NOT NULL DEFAULT FALSE,
    CHECK ((revoked_at IS NULL) = (revoke_body IS NULL) AND (revoke_body IS NULL) = (revoke_sig IS NULL)
        AND (revoke_body IS NULL) = (revoke_at IS NULL))
);
CREATE INDEX run_grants_project ON run_grants (project_id, created_at);
CREATE UNIQUE INDEX run_grants_live ON run_grants (project_id) WHERE revoked_at IS NULL AND superseded_at IS NULL;
