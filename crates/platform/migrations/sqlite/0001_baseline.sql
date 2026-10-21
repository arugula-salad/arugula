-- 0001: the baseline. The platform's tables as production has them, made
-- by control v0.53.2 inside the hub's file before the platform had
-- migrations (each feature's `CREATE TABLE IF NOT EXISTS` and its column
-- probes). On a database that already has them, this isn't run: the runner
-- checks that the tables are exactly these, and marks it applied
-- (`db::migrate`, "Adopting a database made before migrations").
--
-- Never edit a migration that has shipped: its checksum is recorded, and a
-- changed one stops the platform at start. Add the next number instead.

-- ---- billing (M22; the platform's since M139)
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

-- ---- chat (M122; the platform's since M138)
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
-- A DM's two people (and an archive's readers, M124, each from `from_ms`).
-- A channel's people are its team's roster, and a pane's are its reader
-- list: not rows here.
CREATE TABLE chat_members (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    from_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX chat_members_account ON chat_members (account);
-- `id` is the FTS index's rowid: an INTEGER PRIMARY KEY, so VACUUM keeps it.
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
-- A person's read mark in a room (it only moves forward), and whether
-- they turned its pushes off.
CREATE TABLE chat_reads (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    seq INTEGER NOT NULL DEFAULT 0,
    muted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX chat_reads_account ON chat_reads (account);
-- Who may read the thread on a pane or session, as its machine last said
-- (S41 decision 2): `readers` is a JSON list of `ChatReader`.
CREATE TABLE chat_readers (
    machine TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    readers TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (machine, kind, target)
);

-- ---- Cloud (M114-M120; the platform's since M137)
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
-- One per run, one per Cloud workspace and one per tab, each for its whole
-- life: `id` is the run's, the Cloud workspace's or the tab's, and `kind`
-- says which ('run', 'cloud_workspace' or 'cloud_tab'). `cancel` on a Cloud
-- workspace's job is its delete, and on a tab's its close.
CREATE TABLE cloud_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'run',
    body TEXT NOT NULL,
    cancel INTEGER NOT NULL DEFAULT 0,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0
);
-- A conversation's approvals, push deliveries and events, keyed on the
-- conversation: a run (`run_…`) or a Cloud workspace's tab (`cwc_…`).
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
-- `saved_spec` is what the user saved and `saved_revision` the environment
-- revision it became; `built_spec` is the saved spec as of the revision that
-- last reached `ready`. The rest mirrors the Managoat workspace's last poll.
-- `deleted` is set once its Managoat resources are gone. `settings_applied`
-- is 0 from a settings save until Managoat has them (M117), and
-- `vault_secrets` names the secrets in the vault once a save has changed
-- them (NULL: the ones it was created with). `rebuild` is a rebuild or
-- retry Managoat had no machine for yet, which the worker sends again
-- (M120); `rebuild_error` says why the last one queued didn't happen, and
-- `rebuilds` counts the ones Managoat took. `setup_step` is the step a
-- setup is running, while it's `provisioning` (Managoat v0.39.0).
-- `project_id` is its owner's personal project (M129), as for a secret.
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
-- The saved spec each environment revision was saved as, so `built_spec`
-- is right when a setup built an older revision than the one saved last.
-- The newest few are kept.
CREATE TABLE cloud_workspace_specs (
    cloud_workspace TEXT NOT NULL,
    account TEXT NOT NULL,
    revision TEXT NOT NULL,
    spec TEXT NOT NULL,
    saved INTEGER NOT NULL,
    PRIMARY KEY(cloud_workspace, revision)
);
-- A tab: one conversation in a Cloud workspace, on a Managoat agent of its
-- own. `spec` is what it was opened with. `provider_id` is the conversation
-- and `started` whether a turn has begun. `closed` is set once its
-- conversation and agent are gone. `relaunch` is a prompt sent to a tab
-- whose conversation failed: the worker starts a new conversation with it.
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

-- ---- projects, team members and grants (M129, M130; the platform's since #1187)
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
-- The members of each team's latest roster that still have an account.
CREATE TABLE team_members (
    team_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    roster_version INTEGER NOT NULL,
    PRIMARY KEY (team_id, account_id)
);
CREATE INDEX team_members_account ON team_members (account_id);
-- `subject_kind` is 'daemon' for now, `power` 'cloud.drive', and the scope
-- a project or an account. `granted_by` is the account that granted it.
-- One live grant per subject, power and scope; a revoked one keeps its row.
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

-- What a project hangs off (M129): a Cloud workspace and a cloud secret
-- belong to their owner's personal project, a team channel to its team's.
CREATE INDEX cloud_workspaces_project ON cloud_workspaces (project_id);
CREATE INDEX chat_rooms_project ON chat_rooms (project_id);
CREATE INDEX cloud_secrets_project ON cloud_secrets (project_id);
