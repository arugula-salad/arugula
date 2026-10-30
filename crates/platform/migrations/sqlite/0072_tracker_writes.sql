-- 0072: an agent's write to a tracker that waits for a person (Arugula #359,
-- P3-4). On a connection whose `agent_writes` is `approve`, an agent's
-- create, comment, close, reopen or edit is kept here as `waiting` until an
-- owner or editor of the project approves it (it is sent then, as that
-- agent's), edits its text first, or drops it. `request` is the write as
-- JSON, without the "via Arugula" footer: the footer is added when it is
-- sent. `agent_machine` is the machine whose agent asked, `for_account` the
-- person it acts for. `item_id` is the `fi_` id of the row the sent write
-- reached or made. Never a key.
--
-- Additive: the build before this one doesn't read the table.
CREATE TABLE tracker_writes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('create', 'comment', 'close', 'reopen', 'edit')),
    request TEXT NOT NULL,
    agent_machine TEXT NOT NULL,
    for_account TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting', 'sent', 'dropped', 'failed')),
    error TEXT,
    item_id TEXT,
    created_at INTEGER NOT NULL,
    decided_at INTEGER,
    decided_by TEXT
);
CREATE INDEX tracker_writes_project ON tracker_writes (project_id, state);
CREATE INDEX tracker_writes_connection ON tracker_writes (connection_id, state);
