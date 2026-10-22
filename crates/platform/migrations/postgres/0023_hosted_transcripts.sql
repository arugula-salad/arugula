-- 0023: hosted agent transcripts (M184, platform step 5, #1235). An agent
-- block's transcript lives on its hosted workspace's box while the block is
-- open; when it closes, the box's daemon sends it here, and it's kept read
-- only under the workspace's project, after the box is gone too.
--
-- Times are milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers, `BOOLEAN`
-- for its flags and `COLLATE pg_c_utf8` text.

-- One closed agent block's transcript: `htx_` and 32 hex characters. The
-- machine that sent it (its hosted machine's id) and its block's id there;
-- the same block sent again (a lost answer) is the same row. `body` is the
-- block's text (`capture --text`), cut by the daemon to at most 768 KiB:
-- `cut` says its middle was dropped. `bytes` is its length.
CREATE TABLE hosted_transcripts (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    workspace_id TEXT COLLATE pg_c_utf8 NOT NULL,
    machine TEXT COLLATE pg_c_utf8 NOT NULL,
    block BIGINT NOT NULL,
    agent TEXT COLLATE pg_c_utf8 NOT NULL,
    title TEXT COLLATE pg_c_utf8 NOT NULL,
    cwd TEXT COLLATE pg_c_utf8,
    started_at BIGINT NOT NULL,
    closed_at BIGINT NOT NULL,
    cut BOOLEAN NOT NULL DEFAULT FALSE,
    bytes BIGINT NOT NULL,
    body TEXT COLLATE pg_c_utf8 NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (machine, block)
);
CREATE INDEX hosted_transcripts_workspace ON hosted_transcripts (workspace_id, closed_at);
CREATE INDEX hosted_transcripts_project ON hosted_transcripts (project_id, created_at);
