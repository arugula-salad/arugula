-- 0057: a thread is done when its PR merges (M265, thread = session,
-- Arugula #129). Additive: one column an older build never reads. Times are
-- milliseconds since the epoch. `db::session_merged` has the rules.
--
-- `merged_at` is when the mirror saw the session's PR merged: a PR from the
-- session's own branch (`branch`) on its repo, whoever opened it. The
-- session is stopped then, saying `merged`, which frees its slot on the
-- box; its card says "merged ✓", and its thread (the newest session on it
-- merged, none going) leaves the Threads view. NULL until then, and for
-- every session made before it.
ALTER TABLE agent_sessions ADD COLUMN merged_at INTEGER;
-- A merged PR finds its session by its project and branch.
CREATE INDEX agent_sessions_branch ON agent_sessions (project_id, branch);
