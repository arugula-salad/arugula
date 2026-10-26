-- 0050: agent sessions (M254, thread = session, Arugula #129). A thread's
-- agent session on a project's hosted workspace: the session, its
-- append-only history, and the prompts that start its turns. Times are
-- milliseconds since the epoch. JSON is text. `db::sessions` has the rules,
-- and `arugula_e2e::session` the states and the signed event.

-- A session. `id` is the platform's (`ses_` and 32 hex characters).
-- `room_id` and `parent_seq` are its thread: the chat room and the number
-- of the message the thread hangs off. `asker` is the account whose
-- mention started it, and it runs as them, on the project's key.
-- `workspace_id` is the hosted workspace (`hw_`) it runs on, and `machine`
-- that workspace's hosted machine (its device id), bound by the first
-- report it takes: the only one whose reports move the session. `block` is
-- the agent block on the box, as the machine said. `repo` is the linked
-- repo (`owner/name`) it works in, and `branch` the one ref it may push
-- (`arugula/<id>-<slug>`). `pr` is the draft PR's link once the asker sent
-- it, and `ci_rounds` how many times CI's failure went back to it.
-- `state` moves only with an event in `session_events` (`last_seq` is the
-- latest one's); `daemon_n` is the number of the machine's latest report,
-- events and heartbeats both. `was_state` is the state an `unresponsive`
-- session goes back to. `heard_at` is when the platform last heard from the
-- machine (its own clock), `last_tool` and `last_error` what the machine
-- last said of the agent.
CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    room_id TEXT NOT NULL,
    parent_seq INTEGER NOT NULL,
    asker TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    machine TEXT,
    block TEXT,
    repo TEXT,
    branch TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (
        state IN ('queued', 'working', 'needs_input', 'done', 'failed', 'unresponsive', 'stopped')
    ),
    was_state TEXT,
    last_seq INTEGER NOT NULL DEFAULT 0,
    daemon_n INTEGER NOT NULL DEFAULT 0,
    heard_at INTEGER,
    last_tool TEXT,
    last_error TEXT,
    pr TEXT,
    ci_rounds INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX agent_sessions_project ON agent_sessions (project_id, created_at);
CREATE INDEX agent_sessions_workspace ON agent_sessions (workspace_id, state);
CREATE INDEX agent_sessions_asker ON agent_sessions (asker);
-- A thread has one session going at a time; one that ended leaves room for
-- the next.
CREATE UNIQUE INDEX agent_sessions_thread ON agent_sessions (room_id, parent_seq)
    WHERE state NOT IN ('failed', 'stopped');

-- A session's history, numbered from 1 in the session. `kind` is `state`
-- (a move from `from_state` to `to_state`), `error` or `turn` (the session
-- stays where it was: both states the same). The first is control's, the
-- session made, with `from_state` NULL. `detail` is a small JSON object,
-- kept as the reporter wrote it. An event the machine reported names it in
-- `daemon` with its report number (`daemon_n`) and its signature
-- (`arugula session event v1`, `arugula_e2e::session`, hex) in `sig`; one
-- control recorded has none of them. Rows are never changed, and never
-- deleted while their session is there: only deleting the session (with
-- its project) takes them.
CREATE TABLE session_events (
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('state', 'error', 'turn')),
    from_state TEXT,
    to_state TEXT NOT NULL,
    at INTEGER NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    daemon TEXT,
    daemon_n INTEGER,
    sig TEXT,
    PRIMARY KEY (session_id, seq),
    CHECK ((daemon IS NULL) = (sig IS NULL) AND (daemon IS NULL) = (daemon_n IS NULL))
);
CREATE UNIQUE INDEX session_events_daemon_n ON session_events (session_id, daemon_n) WHERE daemon_n IS NOT NULL;
CREATE TRIGGER session_events_append_only BEFORE UPDATE ON session_events BEGIN
    SELECT RAISE(ABORT, 'session_events are append-only');
END;
CREATE TRIGGER session_events_kept BEFORE DELETE ON session_events
    WHEN EXISTS (SELECT 1 FROM agent_sessions WHERE id = old.session_id) BEGIN
    SELECT RAISE(ABORT, 'session_events are append-only: delete the session first');
END;

-- What starts a session's turns: a prompt (`spr_` and 32 hex characters),
-- `queued` until the machine starts a turn with it (`sent`), or `dropped`
-- (its author took it back, or the session ended first). `author` is an
-- account, or `github:<login>` for text from GitHub; `source` says which
-- (`member`, `github`), and `association` is GitHub's `author_association`
-- for GitHub's. `text` is the prompt as written: the machine fences
-- GitHub's as quoted data.
CREATE TABLE session_prompts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    author TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'member' CHECK (source IN ('member', 'github')),
    association TEXT,
    text TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'sent', 'dropped')),
    created_at INTEGER NOT NULL,
    sent_at INTEGER,
    dropped_at INTEGER
);
CREATE INDEX session_prompts_session ON session_prompts (session_id, created_at);

-- The project's setting "agents run on <hw_>": the hosted workspace its
-- sessions go to. NULL: the project's only workspace, or, with several,
-- the thread is asked (`db::sessions::agents_run_on`).
ALTER TABLE projects ADD COLUMN agents_workspace TEXT;
