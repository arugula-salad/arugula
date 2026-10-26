-- 0052: what people ask of an agent session that only its box can do
-- (M257, thread = session, Arugula #129): answer its open question,
-- approve or deny its open permission, or cancel the turn under way. Times
-- are milliseconds since the epoch. `db::sessions` has the rules, and
-- `arugula_control_wire::work::SessionAction` the wire.
--
-- An action (`sac_` and 32 hex characters) is `pending` until a report of
-- the session's machine names it done (`done_n` is that report's number),
-- or `dropped` when the session ended first. `by_account` asked it; empty
-- once that account is deleted. `kind` is `answer`, `approve`, `deny` or
-- `cancel`; `request` the open request it answers (NULL: whichever is
-- open); `answer` an answer's JSON object; `option_id` an approval's
-- option; `reason` a denial's why.
CREATE TABLE session_actions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    by_account TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('answer', 'approve', 'deny', 'cancel')),
    request TEXT,
    answer TEXT,
    option_id TEXT,
    reason TEXT,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'done', 'dropped')),
    at INTEGER NOT NULL,
    done_at INTEGER,
    done_n INTEGER
);
CREATE INDEX session_actions_session ON session_actions (session_id, state, at);
CREATE INDEX session_actions_by ON session_actions (by_account);
