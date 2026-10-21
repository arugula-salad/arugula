-- 0009: native issues (M194, platform step 6, #1263). A project's own
-- issues, numbered per project, with a timeline and full-text search.
-- Their comments aren't here: they're the messages of the issue's object
-- thread (`chat_rooms` with `kind = 'object'`, M196). Times are
-- milliseconds since the epoch. JSON lists are text.

-- The last number a project gave an issue: bumped in the same transaction
-- as the issue's insert, so numbers have no gaps and no duplicates.
ALTER TABLE projects ADD COLUMN last_issue INTEGER NOT NULL DEFAULT 0;

-- An issue. `id` is the platform's (`iss_` and 32 hex characters); `number`
-- is the project's. `state` is `open` or `closed`, and a closed one has a
-- reason (`completed`, `not_planned` or `duplicate`) and a time. `author`
-- is the person's account; an agent's issue keeps its owner's account
-- there and names its machine in `author_agent`. `labels` and `assignees`
-- (accounts) are JSON lists. `github_repo` (`owner/name`), `github_repo_id`
-- and `github_number` say where it was exported to (M205). `search_id` is
-- `issues_fts`'s rowid: an INTEGER PRIMARY KEY, so VACUUM keeps it.
CREATE TABLE issues (
    search_id INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'closed')),
    close_reason TEXT CHECK (close_reason IN ('completed', 'not_planned', 'duplicate')),
    author TEXT NOT NULL,
    author_agent TEXT,
    labels TEXT NOT NULL DEFAULT '[]',
    assignees TEXT NOT NULL DEFAULT '[]',
    github_repo TEXT,
    github_repo_id INTEGER,
    github_number INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    closed_at INTEGER,
    UNIQUE (project_id, number),
    CHECK ((state = 'open') = (close_reason IS NULL AND closed_at IS NULL)),
    CHECK ((state = 'closed') = (close_reason IS NOT NULL AND closed_at IS NOT NULL))
);
CREATE INDEX issues_state ON issues (project_id, state, number);
CREATE INDEX issues_author ON issues (author);
CREATE VIRTUAL TABLE issues_fts USING fts5 (title, body, content='issues', content_rowid='search_id');
CREATE TRIGGER issues_fts_insert AFTER INSERT ON issues BEGIN
    INSERT INTO issues_fts (rowid, title, body) VALUES (new.search_id, new.title, new.body);
END;
CREATE TRIGGER issues_fts_delete AFTER DELETE ON issues BEGIN
    INSERT INTO issues_fts (issues_fts, rowid, title, body) VALUES ('delete', old.search_id, old.title, old.body);
END;
CREATE TRIGGER issues_fts_update AFTER UPDATE OF title, body ON issues BEGIN
    INSERT INTO issues_fts (issues_fts, rowid, title, body) VALUES ('delete', old.search_id, old.title, old.body);
    INSERT INTO issues_fts (rowid, title, body) VALUES (new.search_id, new.title, new.body);
END;

-- An issue's timeline: `kind` is `opened`, `edited`, `closed`, `reopened`,
-- `labelled`, `unlabelled`, `assigned`, `unassigned` or `exported`. `actor`
-- is who did it (an agent: its owner's account, and its machine in
-- `actor_agent`). `detail` is a small JSON object: an edit's old title, a
-- close's reason, the label or assignee, where it was exported to
-- (`db::issues::Event`). `id` orders them.
CREATE TABLE issue_events (
    id INTEGER PRIMARY KEY,
    issue_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    actor TEXT NOT NULL,
    actor_agent TEXT,
    at INTEGER NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX issue_events_issue ON issue_events (issue_id, id);
CREATE INDEX issue_events_actor ON issue_events (actor);
