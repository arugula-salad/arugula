-- 0033: studio becomes a client (M236, platform step 8, Arugula #34;
-- "studio is a full client: Sign in with Arugula, apps by project").
-- A registered client (studio's lobby, by its credential: M231's
-- `hosted::broker::clients`) keeps its apps in projects, reads the
-- projects of the people who let it, and reads the project secrets a
-- project's owner let it read. A link (M229) is never authorization:
-- nothing here is granted because an account is linked.
--
-- Times are milliseconds since the epoch.

-- An account letting a registered client do one more thing for it (M236),
-- beside `inference_consents` (M231, Claude): `projects`, see which
-- projects the account is in and its role in each, and keep the client's
-- apps in the ones it edits. Off until the account turns it on, on the
-- platform page; unlinking the product there ends it.
CREATE TABLE client_consents (
    account TEXT NOT NULL,
    client TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('projects')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account, client, scope)
);

-- A client's app, and the project it's in (M236): studio's lobby's apps
-- (`app-xxxxxxxx`), so an app's owners are its project's members. The
-- client says which app; an account it acts for that edits the project
-- (and the one it leaves, when it moves) put it there. `added_by` is that
-- account, NULL once it's deleted.
CREATE TABLE client_apps (
    client TEXT NOT NULL,
    app TEXT NOT NULL,
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    added_by TEXT,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (client, app)
);
CREATE INDEX client_apps_project ON client_apps (project_id);

-- A project's owner letting a registered client read one of the
-- project's own secrets (`project/<NAME>`, M230) for the project's apps
-- (M236: studio's egress secrets). Only by name, only for that client,
-- and only from the platform page. `reads` and `last_read_at` count the
-- client's reads, each also in `client_secret_events`.
CREATE TABLE client_secret_grants (
    project_id TEXT NOT NULL,
    client TEXT NOT NULL,
    name TEXT NOT NULL,
    granted_by TEXT,
    granted_at INTEGER NOT NULL,
    reads INTEGER NOT NULL DEFAULT 0,
    last_read_at INTEGER,
    PRIMARY KEY (project_id, client, name)
);

-- What happened to a client's reach into a project's secrets (M236):
-- `granted` and `revoked` by an owner (`by_account`), `read` by the client
-- for one of its apps (`app`). Never a value. Kept 90 days.
CREATE TABLE client_secret_events (
    id INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL,
    client TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('granted', 'revoked', 'read')),
    app TEXT,
    by_account TEXT,
    at INTEGER NOT NULL
);
CREATE INDEX client_secret_events_project ON client_secret_events (project_id, at);
