-- The platform's tables in production: what control v0.53.2 made inside the
-- hub's file. v0.53.2's schema code (crates/control/src/db.rs and db/*.rs at
-- the tag) makes exactly what main-0cd3465c.sql holds except `hub_feed`
-- (#1187, unreleased then), so this is that file without it. Production's
-- own file went through each column probe and table rebuild on the way
-- here, so its text differs; the match test ages a copy the same way.
-- Checked in; never regenerated.
CREATE TABLE billing (
    owner TEXT PRIMARY KEY,
    customer TEXT,
    subscription TEXT,
    status TEXT NOT NULL,
    seat_item TEXT,
    updated INTEGER NOT NULL
);
CREATE TABLE reported (
    account TEXT PRIMARY KEY,
    minutes INTEGER NOT NULL
);
CREATE TABLE chat_rooms (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    machine TEXT,
    target TEXT,
    created_by TEXT NOT NULL,
    created INTEGER NOT NULL,
    archived INTEGER,
    last_seq INTEGER NOT NULL DEFAULT 0,
    -- The text and quotes it holds, for the team's cap.
    bytes INTEGER NOT NULL DEFAULT 0,
    -- A team channel's project (the team's default); NULL for the rest.
    project_id TEXT
);
CREATE INDEX chat_rooms_scope ON chat_rooms (scope);
CREATE UNIQUE INDEX chat_rooms_target ON chat_rooms (kind, machine, target) WHERE machine IS NOT NULL;
CREATE UNIQUE INDEX chat_rooms_channel ON chat_rooms (scope, name) WHERE kind = 'channel';
CREATE TABLE chat_members (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    from_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX chat_members_account ON chat_members (account);
CREATE TABLE chat_messages (
    id INTEGER PRIMARY KEY,
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    author TEXT NOT NULL,
    at INTEGER NOT NULL,
    text TEXT NOT NULL,
    quote TEXT,
    ref_machine TEXT,
    ref_pane TEXT,
    mentions TEXT NOT NULL DEFAULT '[]',
    edited INTEGER,
    deleted INTEGER,
    imported TEXT,
    -- Its number on the machine it was brought in from (M124).
    imported_id INTEGER,
    UNIQUE (room, seq)
);
CREATE INDEX chat_messages_author ON chat_messages (author);
CREATE VIRTUAL TABLE chat_fts USING fts5 (text, quote, content='chat_messages', content_rowid='id');
CREATE TRIGGER chat_fts_insert AFTER INSERT ON chat_messages BEGIN
    INSERT INTO chat_fts (rowid, text, quote) VALUES (new.id, new.text, coalesce(new.quote, ''));
END;
CREATE TRIGGER chat_fts_delete AFTER DELETE ON chat_messages BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text, quote) VALUES ('delete', old.id, old.text, coalesce(old.quote, ''));
END;
CREATE TRIGGER chat_fts_update AFTER UPDATE OF text, quote ON chat_messages BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text, quote) VALUES ('delete', old.id, old.text, coalesce(old.quote, ''));
    INSERT INTO chat_fts (rowid, text, quote) VALUES (new.id, new.text, coalesce(new.quote, ''));
END;
CREATE TABLE chat_reads (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    seq INTEGER NOT NULL DEFAULT 0,
    muted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX chat_reads_account ON chat_reads (account);
CREATE TABLE chat_readers (
    machine TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    readers TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (machine, kind, target)
);
CREATE TABLE cloud_agents (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    version INTEGER NOT NULL,
    definition TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX cloud_agents_owner ON cloud_agents(account);
CREATE TABLE cloud_runs (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    request_key TEXT NOT NULL,
    spec TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued',
    provider_id TEXT,
    created INTEGER NOT NULL,
    deadline INTEGER NOT NULL,
    result TEXT,
    UNIQUE(account, request_key)
);
CREATE INDEX cloud_runs_owner ON cloud_runs(account, created);
CREATE TABLE cloud_secrets (
    account TEXT NOT NULL,
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    project_id TEXT,
    PRIMARY KEY(account,name)
);
CREATE TABLE cloud_run_secrets (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    PRIMARY KEY(run,name)
);
CREATE TABLE cloud_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'run',
    body TEXT NOT NULL,
    cancel INTEGER NOT NULL DEFAULT 0,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE cloud_requests (
    conversation TEXT NOT NULL,
    account TEXT NOT NULL,
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    pending INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(conversation,id)
);
CREATE TABLE cloud_notifications (
    conversation TEXT NOT NULL,
    account TEXT NOT NULL,
    request TEXT NOT NULL,
    delivered INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(conversation,request)
);
CREATE TABLE cloud_events (
    conversation TEXT NOT NULL,
    account TEXT NOT NULL,
    cursor INTEGER NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY(conversation,cursor)
);
CREATE TABLE cloud_workspaces (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    request_key TEXT NOT NULL,
    name TEXT NOT NULL,
    saved_spec TEXT NOT NULL,
    saved_revision TEXT,
    built_spec TEXT,
    status TEXT NOT NULL DEFAULT 'provisioning',
    sandbox_status TEXT,
    error TEXT,
    revision TEXT,
    built_from_revision TEXT,
    stale INTEGER NOT NULL DEFAULT 0,
    changed_fields TEXT NOT NULL DEFAULT '[]',
    environment TEXT,
    vault TEXT,
    provider_id TEXT,
    created INTEGER NOT NULL,
    deleted INTEGER,
    settings_applied INTEGER NOT NULL DEFAULT 1,
    vault_secrets TEXT,
    rebuild TEXT,
    rebuild_error TEXT,
    rebuilds INTEGER NOT NULL DEFAULT 0,
    setup_step TEXT,
    project_id TEXT,
    UNIQUE(account, request_key)
);
CREATE INDEX cloud_workspaces_owner ON cloud_workspaces(account, created);
CREATE TABLE cloud_workspace_specs (
    cloud_workspace TEXT NOT NULL,
    account TEXT NOT NULL,
    revision TEXT NOT NULL,
    spec TEXT NOT NULL,
    saved INTEGER NOT NULL,
    PRIMARY KEY(cloud_workspace, revision)
);
CREATE TABLE cloud_tabs (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    cloud_workspace TEXT NOT NULL REFERENCES cloud_workspaces(id),
    request_key TEXT NOT NULL,
    spec TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'opening',
    error TEXT,
    started INTEGER NOT NULL DEFAULT 0,
    provider_id TEXT,
    created INTEGER NOT NULL,
    closed INTEGER,
    relaunch TEXT,
    UNIQUE(account, cloud_workspace, request_key)
);
CREATE INDEX cloud_tabs_owner ON cloud_tabs(cloud_workspace, created);
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    team_id TEXT,
    account_id TEXT,
    name TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    CHECK ((team_id IS NULL) != (account_id IS NULL))
);
CREATE UNIQUE INDEX projects_team_default ON projects (team_id)
    WHERE is_default = 1 AND team_id IS NOT NULL;
CREATE UNIQUE INDEX projects_account_default ON projects (account_id)
    WHERE is_default = 1 AND account_id IS NOT NULL;
CREATE TABLE team_members (
    team_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    roster_version INTEGER NOT NULL,
    PRIMARY KEY (team_id, account_id)
);
CREATE INDEX team_members_account ON team_members (account_id);
CREATE TABLE grants (
    id TEXT PRIMARY KEY,
    subject_kind TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    power TEXT NOT NULL,
    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('project', 'account')),
    scope_id TEXT NOT NULL,
    granted_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE UNIQUE INDEX grants_live ON grants (subject_kind, subject_id, power, scope_kind, scope_id)
    WHERE revoked_at IS NULL;
CREATE INDEX grants_scope ON grants (scope_kind, scope_id);
CREATE INDEX cloud_workspaces_project ON cloud_workspaces (project_id);
CREATE INDEX chat_rooms_project ON chat_rooms (project_id);
CREATE INDEX cloud_secrets_project ON cloud_secrets (project_id);
