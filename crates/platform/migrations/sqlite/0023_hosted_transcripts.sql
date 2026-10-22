-- 0023: hosted agent transcripts (M184, platform step 5, #1235). An agent
-- block's transcript lives on its hosted workspace's box while the block is
-- open; when it closes, the box's daemon sends it here, and it's kept read
-- only under the workspace's project, after the box is gone too.
--
-- Times are milliseconds since the epoch.

-- One closed agent block's transcript: `htx_` and 32 hex characters. The
-- machine that sent it (its hosted machine's id) and its block's id there;
-- the same block sent again (a lost answer) is the same row. `body` is the
-- block's text (`capture --text`), cut by the daemon to at most 768 KiB:
-- `cut` says its middle was dropped. `bytes` is its length.
CREATE TABLE hosted_transcripts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    machine TEXT NOT NULL,
    block INTEGER NOT NULL,
    agent TEXT NOT NULL,
    title TEXT NOT NULL,
    cwd TEXT,
    started_at INTEGER NOT NULL,
    closed_at INTEGER NOT NULL,
    cut INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (machine, block)
);
CREATE INDEX hosted_transcripts_workspace ON hosted_transcripts (workspace_id, closed_at);
CREATE INDEX hosted_transcripts_project ON hosted_transcripts (project_id, created_at);
