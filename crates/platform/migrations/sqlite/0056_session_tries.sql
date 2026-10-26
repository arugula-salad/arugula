-- 0056: a session's siblings and reviews (M264, thread = session, Arugula
-- #129). Times are milliseconds since the epoch. `db::sessions` has the
-- rules, and `sessions::tries` and `sessions::review` the routes.
--
-- `parent_session` is the session a Try 3's sibling runs beside (it
-- re-runs that one's first prompt, on the same thread, box and repo), or
-- the one a review reviews; NULL for a thread's own session. `role` says
-- which: `thread`, `try` or `review`. `harness` is the agent it asked for
-- (`claude`, `codex`): its box runs Claude Code when it hasn't the other,
-- and says so on taking it. On a Try 3's parent, `picked` is the session
-- kept (the parent or one of its siblings), with who picked it and when.
ALTER TABLE agent_sessions ADD COLUMN parent_session TEXT;
ALTER TABLE agent_sessions ADD COLUMN role TEXT NOT NULL DEFAULT 'thread' CHECK (role IN ('thread', 'try', 'review'));
ALTER TABLE agent_sessions ADD COLUMN harness TEXT NOT NULL DEFAULT 'claude' CHECK (harness IN ('claude', 'codex'));
ALTER TABLE agent_sessions ADD COLUMN picked TEXT;
ALTER TABLE agent_sessions ADD COLUMN picked_by TEXT;
ALTER TABLE agent_sessions ADD COLUMN picked_at INTEGER;
CREATE INDEX agent_sessions_parent ON agent_sessions (parent_session) WHERE parent_session IS NOT NULL;
-- A thread still has one session of its own going at a time; its siblings
-- and reviews run beside it.
DROP INDEX agent_sessions_thread;
CREATE UNIQUE INDEX agent_sessions_thread ON agent_sessions (room_id, parent_seq)
    WHERE state NOT IN ('failed', 'stopped') AND parent_session IS NULL;
