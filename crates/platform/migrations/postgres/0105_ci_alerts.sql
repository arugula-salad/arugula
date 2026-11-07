-- 0105: CI alerts as native issues (native #774, `db::ci_alerts`): chant's
-- signed `chant.notify/v1` events open, comment on and close a project's
-- "main is red" issue. Times are milliseconds since the epoch. Additive: the
-- build before this one never reads them.

-- A project with CI alerts on: one row, made when its key is made, gone
-- when they're turned off. The key itself is sealed in `project_secrets`
-- (`ci_alerts/key`), never here. `assign`: 1 when the people below are
-- assigned a new red issue as well as mentioned on it. `last_event_at` is
-- when the last signed event arrived.
CREATE TABLE ci_alerts (
    project_id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    assign BIGINT NOT NULL DEFAULT 0,
    key_made_by TEXT COLLATE pg_c_utf8 NOT NULL,
    key_made_at BIGINT NOT NULL,
    last_event_at BIGINT
);

-- Who a project's new red issue mentions (and assigns, with `assign`), in
-- the order they were named.
CREATE TABLE ci_alert_people (
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    position BIGINT NOT NULL,
    PRIMARY KEY (project_id, account)
);
CREATE INDEX ci_alert_people_account ON ci_alert_people (account);

-- Each red issue CI alerts opened for a project's branch: `red_commit` is
-- the commit its first event named, `closed_at` when green closed it (or
-- when it was found closed by a person). At most one row per branch is
-- open: the unique index below holds it while two of the platform's
-- machines take events for the branch at once (`Db::create_ci_issue`).
CREATE TABLE ci_alert_issues (
    issue_id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    branch TEXT COLLATE pg_c_utf8 NOT NULL,
    red_commit TEXT COLLATE pg_c_utf8 NOT NULL,
    opened_at BIGINT NOT NULL,
    closed_at BIGINT
);
CREATE UNIQUE INDEX ci_alert_issues_open ON ci_alert_issues (project_id, branch) WHERE closed_at IS NULL;
