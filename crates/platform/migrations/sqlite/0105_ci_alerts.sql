-- 0105: CI alerts as native issues (native #774, `db::ci_alerts`): chant's
-- signed `chant.notify/v1` events open, comment on and close a project's
-- "main is red" issue. Times are milliseconds since the epoch. Additive: the
-- build before this one never reads them.

-- A project's CI alerts: one row, made when its first key is made. On
-- while `key_made_at` is set; turning them off clears it and removes the
-- key, and keeps the rest (their people, their open issues), which carry
-- on when a new key is made. The key itself is sealed in
-- `project_secrets` (`ci_alerts/key`), never here. `assign`: 1 when the
-- people below are assigned a new red issue as well as mentioned on it.
-- `last_event_at` is when the last signed event arrived.
CREATE TABLE ci_alerts (
    project_id TEXT PRIMARY KEY,
    assign INTEGER NOT NULL DEFAULT 0,
    key_made_by TEXT NOT NULL DEFAULT '',
    key_made_at INTEGER,
    last_event_at INTEGER
);

-- Who a project's new red issue mentions (and assigns, with `assign`), in
-- the order they were named.
CREATE TABLE ci_alert_people (
    project_id TEXT NOT NULL,
    account TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (project_id, account)
);
CREATE INDEX ci_alert_people_account ON ci_alert_people (account);

-- Each red issue CI alerts opened for a project's branch: `red_commit` is
-- the commit its first event named and `red_at` that event's `at`;
-- `closed_at` is when green closed it (or when it was found closed by a
-- person). At most one row per branch is open: the unique index below
-- holds it while two of the platform's machines take events for the
-- branch at once (`Db::create_ci_issue`).
CREATE TABLE ci_alert_issues (
    issue_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    branch TEXT NOT NULL,
    red_commit TEXT NOT NULL,
    red_at INTEGER NOT NULL,
    opened_at INTEGER NOT NULL,
    closed_at INTEGER
);
CREATE UNIQUE INDEX ci_alert_issues_open ON ci_alert_issues (project_id, branch) WHERE closed_at IS NULL;

-- Each branch's newest green, by its event's `at`: a red, culprit or stale
-- event from before it is late, and changes nothing.
CREATE TABLE ci_alert_greens (
    project_id TEXT NOT NULL,
    branch TEXT NOT NULL,
    green_commit TEXT NOT NULL,
    green_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, branch)
);
