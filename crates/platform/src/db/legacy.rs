//! The platform's schema code from before migrations (main at 0cd3465c),
//! kept only to adopt an old database. **It never changes.**
//!
//! A self-hosted control may upgrade straight from any older release, so
//! when a database made before migrations matches no version, adoption
//! ([`super::migrate`]) runs this first: every feature's
//! `CREATE TABLE IF NOT EXISTS` and column probes, as they were the last
//! time they ran, and then matches again. Nothing else calls it. A new
//! table or column goes in a numbered migration, never here.
//!
//! Copied as it was, with two changes so it runs inside adoption's one
//! transaction: Cloud's table rebuilds lost their own `BEGIN`/`COMMIT`, and
//! the projects backfill its own transaction.

use rusqlite::Connection;

/// Brings the platform's tables from any older release to the schema main
/// had at 0cd3465c (migration 0002's).
pub(super) fn upgrade(conn: &Connection, now: u64) -> rusqlite::Result<()> {
    for schema in [BILLING, CHAT, CLOUD, PROJECTS] {
        conn.execute_batch(schema)?;
    }
    chat_probes(conn)?;
    cloud_probes(conn)?;
    projects_probes(conn, now)
}

const BILLING: &str = "
CREATE TABLE IF NOT EXISTS billing (
    owner TEXT PRIMARY KEY,
    customer TEXT,
    subscription TEXT,
    status TEXT NOT NULL,
    seat_item TEXT,
    updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reported (
    account TEXT PRIMARY KEY,
    minutes INTEGER NOT NULL
);";

const CHAT: &str = "
CREATE TABLE IF NOT EXISTS chat_rooms (
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
CREATE INDEX IF NOT EXISTS chat_rooms_scope ON chat_rooms (scope);
CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_target ON chat_rooms (kind, machine, target) WHERE machine IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_channel ON chat_rooms (scope, name) WHERE kind = 'channel';
-- A DM's two people (and an archive's readers, M124, each from `from_ms`).
-- A channel's people are its team's roster, and a pane's are its reader
-- list: not rows here.
CREATE TABLE IF NOT EXISTS chat_members (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    from_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX IF NOT EXISTS chat_members_account ON chat_members (account);
-- `id` is the FTS index's rowid: an INTEGER PRIMARY KEY, so VACUUM keeps it.
CREATE TABLE IF NOT EXISTS chat_messages (
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
CREATE INDEX IF NOT EXISTS chat_messages_author ON chat_messages (author);
CREATE VIRTUAL TABLE IF NOT EXISTS chat_fts USING fts5 (text, quote, content='chat_messages', content_rowid='id');
CREATE TRIGGER IF NOT EXISTS chat_fts_insert AFTER INSERT ON chat_messages BEGIN
    INSERT INTO chat_fts (rowid, text, quote) VALUES (new.id, new.text, coalesce(new.quote, ''));
END;
CREATE TRIGGER IF NOT EXISTS chat_fts_delete AFTER DELETE ON chat_messages BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text, quote) VALUES ('delete', old.id, old.text, coalesce(old.quote, ''));
END;
CREATE TRIGGER IF NOT EXISTS chat_fts_update AFTER UPDATE OF text, quote ON chat_messages BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text, quote) VALUES ('delete', old.id, old.text, coalesce(old.quote, ''));
    INSERT INTO chat_fts (rowid, text, quote) VALUES (new.id, new.text, coalesce(new.quote, ''));
END;
-- A person's read mark in a room (it only moves forward), and whether
-- they turned its pushes off.
CREATE TABLE IF NOT EXISTS chat_reads (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    seq INTEGER NOT NULL DEFAULT 0,
    muted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room, account)
);
CREATE INDEX IF NOT EXISTS chat_reads_account ON chat_reads (account);
-- Who may read the thread on a pane or session, as its machine last said
-- (S41 decision 2): `readers` is a JSON list of `ChatReader`.
CREATE TABLE IF NOT EXISTS chat_readers (
    machine TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    readers TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (machine, kind, target)
);";

const CLOUD: &str = "
CREATE TABLE IF NOT EXISTS cloud_agents (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    version INTEGER NOT NULL,
    definition TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS cloud_agents_owner ON cloud_agents(account);
CREATE TABLE IF NOT EXISTS cloud_runs (
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
CREATE INDEX IF NOT EXISTS cloud_runs_owner ON cloud_runs(account, created);
CREATE TABLE IF NOT EXISTS cloud_secrets (
    account TEXT NOT NULL,
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    project_id TEXT,
    PRIMARY KEY(account,name)
);
CREATE TABLE IF NOT EXISTS cloud_run_secrets (
    run TEXT NOT NULL REFERENCES cloud_runs(id),
    name TEXT NOT NULL,
    sealed BLOB NOT NULL,
    PRIMARY KEY(run,name)
);
-- One per run, one per Cloud workspace and one per tab, each for its whole
-- life: `id` is the run's, the Cloud workspace's or the tab's, and `kind`
-- says which ('run', 'cloud_workspace' or 'cloud_tab'). `cancel` on a Cloud
-- workspace's job is its delete, and on a tab's its close.
CREATE TABLE IF NOT EXISTS cloud_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'run',
    body TEXT NOT NULL,
    cancel INTEGER NOT NULL DEFAULT 0,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0
);
-- A conversation's approvals, push deliveries and events, keyed on the
-- conversation: a run (`run_…`) or a Cloud workspace's tab (`cwc_…`).
CREATE TABLE IF NOT EXISTS cloud_requests (
    conversation TEXT NOT NULL,
    account TEXT NOT NULL,
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    pending INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(conversation,id)
);
CREATE TABLE IF NOT EXISTS cloud_notifications (
    conversation TEXT NOT NULL,
    account TEXT NOT NULL,
    request TEXT NOT NULL,
    delivered INTEGER NOT NULL DEFAULT 0,
    next_attempt INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(conversation,request)
);
CREATE TABLE IF NOT EXISTS cloud_events (
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
CREATE TABLE IF NOT EXISTS cloud_workspaces (
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
CREATE INDEX IF NOT EXISTS cloud_workspaces_owner ON cloud_workspaces(account, created);
-- The saved spec each environment revision was saved as, so `built_spec`
-- is right when a setup built an older revision than the one saved last.
-- The newest few are kept.
CREATE TABLE IF NOT EXISTS cloud_workspace_specs (
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
CREATE TABLE IF NOT EXISTS cloud_tabs (
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
CREATE INDEX IF NOT EXISTS cloud_tabs_owner ON cloud_tabs(cloud_workspace, created);";

const PROJECTS: &str = "
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    team_id TEXT,
    account_id TEXT,
    name TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    CHECK ((team_id IS NULL) != (account_id IS NULL))
);
CREATE UNIQUE INDEX IF NOT EXISTS projects_team_default ON projects (team_id)
    WHERE is_default = 1 AND team_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS projects_account_default ON projects (account_id)
    WHERE is_default = 1 AND account_id IS NOT NULL;
-- The members of each team's latest roster that still have an account.
CREATE TABLE IF NOT EXISTS team_members (
    team_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    roster_version INTEGER NOT NULL,
    PRIMARY KEY (team_id, account_id)
);
CREATE INDEX IF NOT EXISTS team_members_account ON team_members (account_id);
-- `subject_kind` is 'daemon' for now, `power` 'cloud.drive', and the scope
-- a project or an account. `granted_by` is the account that granted it.
-- One live grant per subject, power and scope; a revoked one keeps its row.
CREATE TABLE IF NOT EXISTS grants (
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
CREATE UNIQUE INDEX IF NOT EXISTS grants_live ON grants (subject_kind, subject_id, power, scope_kind, scope_id)
    WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS grants_scope ON grants (scope_kind, scope_id);
-- The last change of the hub's feed (`HubPort::changes`) applied to the
-- tables above, by the reader that applied it.
CREATE TABLE IF NOT EXISTS hub_feed (
    reader TEXT PRIMARY KEY,
    seq INTEGER NOT NULL
);";

/// Columns M124 added to M122's chat tables.
fn chat_probes(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let has = |table: &str, col: &str| -> rusqlite::Result<bool> {
        let mut q = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names: Vec<String> = q.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
        Ok(names.iter().any(|n| n == col))
    };
    if !has("chat_members", "from_ms")? {
        conn.execute_batch("ALTER TABLE chat_members ADD COLUMN from_ms INTEGER NOT NULL DEFAULT 0")?;
    }
    if !has("chat_messages", "imported_id")? {
        conn.execute_batch("ALTER TABLE chat_messages ADD COLUMN imported_id INTEGER")?;
    }
    Ok(())
}

/// Cloud's tables keyed on a run before Cloud workspaces, rebuilt (M114,
/// M115), and the columns M117, M120 and later added.
fn cloud_probes(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let keyed_on_run: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('cloud_jobs') WHERE name='run')", [], |r| {
            r.get(0)
        })?;
    if keyed_on_run {
        conn.execute_batch(
            "CREATE TABLE cloud_jobs_v2 (
                 id TEXT PRIMARY KEY,
                 kind TEXT NOT NULL DEFAULT 'run',
                 body TEXT NOT NULL,
                 cancel INTEGER NOT NULL DEFAULT 0,
                 lease TEXT,
                 lease_until INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO cloud_jobs_v2(id,kind,body,cancel,lease,lease_until)
                 SELECT run,'run',body,cancel,lease,lease_until FROM cloud_jobs;
             DROP TABLE cloud_jobs;
             ALTER TABLE cloud_jobs_v2 RENAME TO cloud_jobs;",
        )?;
    }
    // Settings saves (M117).
    let unsaved: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_workspaces')
             AND NOT EXISTS(SELECT 1 FROM pragma_table_info('cloud_workspaces') WHERE name='settings_applied')",
        [],
        |r| r.get(0),
    )?;
    if unsaved {
        conn.execute_batch(
            "ALTER TABLE cloud_workspaces ADD COLUMN settings_applied INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE cloud_workspaces ADD COLUMN vault_secrets TEXT;",
        )?;
    }
    // Rebuild and Retry (M120).
    let unrebuilt: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_workspaces')
             AND NOT EXISTS(SELECT 1 FROM pragma_table_info('cloud_workspaces') WHERE name='rebuilds')",
        [],
        |r| r.get(0),
    )?;
    if unrebuilt {
        conn.execute_batch(
            "ALTER TABLE cloud_workspaces ADD COLUMN rebuild TEXT;
             ALTER TABLE cloud_workspaces ADD COLUMN rebuild_error TEXT;
             ALTER TABLE cloud_workspaces ADD COLUMN rebuilds INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // The setup's step (Managoat v0.39.0).
    let stepless: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_workspaces')
             AND NOT EXISTS(SELECT 1 FROM pragma_table_info('cloud_workspaces') WHERE name='setup_step')",
        [],
        |r| r.get(0),
    )?;
    if stepless {
        conn.execute_batch("ALTER TABLE cloud_workspaces ADD COLUMN setup_step TEXT;")?;
    }
    // A failed tab's new conversation.
    let unrelaunched: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_tabs')
             AND NOT EXISTS(SELECT 1 FROM pragma_table_info('cloud_tabs') WHERE name='relaunch')",
        [],
        |r| r.get(0),
    )?;
    if unrelaunched {
        conn.execute_batch("ALTER TABLE cloud_tabs ADD COLUMN relaunch TEXT;")?;
    }
    // Requests, notifications and events keyed on `run` (a foreign key to
    // `cloud_runs`) before tabs (M115): rekeyed on the conversation, with
    // its account.
    let events_keyed_on_run: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('cloud_events') WHERE name='run')", [], |r| {
            r.get(0)
        })?;
    if events_keyed_on_run {
        conn.execute_batch(
            "CREATE TABLE cloud_requests_v2 (
                 conversation TEXT NOT NULL,
                 account TEXT NOT NULL,
                 id TEXT NOT NULL,
                 body TEXT NOT NULL,
                 pending INTEGER NOT NULL DEFAULT 1,
                 PRIMARY KEY(conversation,id)
             );
             INSERT INTO cloud_requests_v2(conversation,account,id,body,pending)
                 SELECT q.run,r.account,q.id,q.body,q.pending FROM cloud_requests q JOIN cloud_runs r ON r.id=q.run;
             DROP TABLE cloud_requests;
             ALTER TABLE cloud_requests_v2 RENAME TO cloud_requests;
             CREATE TABLE cloud_notifications_v2 (
                 conversation TEXT NOT NULL,
                 account TEXT NOT NULL,
                 request TEXT NOT NULL,
                 delivered INTEGER NOT NULL DEFAULT 0,
                 next_attempt INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY(conversation,request)
             );
             INSERT INTO cloud_notifications_v2(conversation,account,request,delivered,next_attempt)
                 SELECT n.run,r.account,n.request,n.delivered,n.next_attempt FROM cloud_notifications n JOIN cloud_runs r ON r.id=n.run;
             DROP TABLE cloud_notifications;
             ALTER TABLE cloud_notifications_v2 RENAME TO cloud_notifications;
             CREATE TABLE cloud_events_v2 (
                 conversation TEXT NOT NULL,
                 account TEXT NOT NULL,
                 cursor INTEGER NOT NULL,
                 body TEXT NOT NULL,
                 PRIMARY KEY(conversation,cursor)
             );
             INSERT INTO cloud_events_v2(conversation,account,cursor,body)
                 SELECT e.run,r.account,e.cursor,e.body FROM cloud_events e JOIN cloud_runs r ON r.id=e.run;
             DROP TABLE cloud_events;
             ALTER TABLE cloud_events_v2 RENAME TO cloud_events;",
        )?;
    }
    Ok(())
}

/// What a project's id looks like.
const NEW_ID_SQL: &str = "'prj_' || lower(hex(randomblob(16)))";

/// The tables that carry a `project_id`.
const ATTACHED: [&str; 3] = ["cloud_workspaces", "chat_rooms", "cloud_secrets"];

/// `project_id` where it's missing, and M129's personal-project backfill.
fn projects_probes(conn: &Connection, now: u64) -> rusqlite::Result<()> {
    for table in ATTACHED {
        let has: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='project_id')"),
            [],
            |r| r.get(0),
        )?;
        if !has {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN project_id TEXT"))?;
        }
        conn.execute_batch(&format!("CREATE INDEX IF NOT EXISTS {table}_project ON {table} (project_id)"))?;
    }
    conn.execute(
        &format!(
            "INSERT INTO projects (id, account_id, name, is_default, created_at)
             SELECT {NEW_ID_SQL}, o.account, 'Personal', 1, ?1
             FROM (SELECT account FROM cloud_workspaces WHERE project_id IS NULL
                   UNION SELECT account FROM cloud_secrets WHERE project_id IS NULL) o
             WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.account_id = o.account AND p.is_default = 1)"
        ),
        [now],
    )?;
    for table in ["cloud_workspaces", "cloud_secrets"] {
        conn.execute(
            &format!(
                "UPDATE {table} SET project_id = (SELECT p.id FROM projects p
                     WHERE p.account_id = {table}.account AND p.is_default = 1)
                 WHERE project_id IS NULL"
            ),
            [],
        )?;
    }
    Ok(())
}
