//! The platform plane's nouns (#1122), held by the platform since they
//! moved out of the hub (platform step 2, #1150): projects ("The project is
//! the unit of platform sharing"), the projection of each team's roster, and
//! grants ("Grants, not impersonation"). Nothing here joins a hub table:
//! which accounts, teams and machines there are comes through `HubPort`, in
//! [`crate::projects`] and [`crate::grants`].
//!
//! - A project belongs to one team or one account, never both. Each has at
//!   most one default: a team's channels go to the team's, and a Cloud
//!   workspace or cloud secret to its owner's personal one, made the first
//!   time it's needed. DMs and pane or session threads belong to none.
//! - `team_members` is the latest signed roster of each team, as
//!   `HubPort::teams` gives it, rewritten when the hub's change feed says the
//!   roster changed ([`crate::projects::follow`]). It's never read for
//!   machine access: the signed roster stays the authority.
//! - A grant is a subject's power over a scope: a daemon's `cloud.drive`
//!   over its owner's account, for now (M130: aru-047 as grants). Every
//!   machine joined to an account gets one, and control checks it on the
//!   Cloud workspace routes ([`crate::grants::can_drive_cloud`]). A revoked
//!   grant keeps its row; giving it back is a new one. A machine that leaves
//!   loses its grants, so one that joins again starts with a new one.
//! - `hub_feed` is how far the platform has applied the hub's change feed
//!   to these tables.
//!
//! Nothing reads `project_id` yet (M129).

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::Db;
use crate::{Deleted, hub::Team};

pub(super) const SCHEMA: &str = "
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
);
";

/// What a project's id looks like: `prj_` and 32 hex characters, beside
/// `cw_` for Cloud workspaces.
const NEW_ID_SQL: &str = "'prj_' || lower(hex(randomblob(16)))";

/// A grant's id: `grt_` and 32 hex characters.
const NEW_GRANT_SQL: &str = "'grt_' || lower(hex(randomblob(16)))";

/// A machine's power to drive its owner's Cloud workspaces (aru-047), over
/// the owner's account.
pub const CLOUD_DRIVE: &str = "cloud.drive";

/// The tables that carry a `project_id`.
const ATTACHED: [&str; 3] = ["cloud_workspaces", "chat_rooms", "cloud_secrets"];

/// The reader in `hub_feed` that keeps these tables.
const READER: &str = "projects";

/// `project_id` on the tables that hang off a project, and the part of the
/// backfill (M129) the platform can do on its own: every account with Cloud
/// workspaces or cloud secrets gets its personal project, and those rows
/// get it. Runs on every start, after Cloud's and chat's tables are made;
/// with nothing to do, it changes nothing.
///
/// Team channels need their team's name from the hub, so the platform
/// attaches those once it's running ([`crate::projects::start`]), and a
/// team's default project is made when the feed says its roster changed or
/// the first time it's needed.
pub(super) fn migrate(conn: &Connection, now: u64) -> rusqlite::Result<()> {
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
    let tx = conn.unchecked_transaction()?;
    tx.execute(
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
        tx.execute(
            &format!(
                "UPDATE {table} SET project_id = (SELECT p.id FROM projects p
                     WHERE p.account_id = {table}.account AND p.is_default = 1)
                 WHERE project_id IS NULL"
            ),
            [],
        )?;
    }
    tx.commit()
}

/// A team's default project, made now (named `name`) if it has none.
fn team_project(c: &Connection, team: &str, name: &str, now: u64) -> rusqlite::Result<String> {
    c.execute(
        &format!(
            "INSERT INTO projects (id, team_id, name, is_default, created_at)
             SELECT {NEW_ID_SQL}, ?1, ?2, 1, ?3
             WHERE NOT EXISTS (SELECT 1 FROM projects WHERE team_id = ?1 AND is_default = 1)"
        ),
        params![team, name, now],
    )?;
    c.query_row("SELECT id FROM projects WHERE team_id = ?1 AND is_default = 1", [team], |r| r.get(0))
}

/// An account's personal project, made now if it has none.
fn personal_project(c: &Connection, account: &str, now: u64) -> rusqlite::Result<String> {
    c.execute(
        &format!(
            "INSERT INTO projects (id, account_id, name, is_default, created_at)
             SELECT {NEW_ID_SQL}, ?1, 'Personal', 1, ?2
             WHERE NOT EXISTS (SELECT 1 FROM projects WHERE account_id = ?1 AND is_default = 1)"
        ),
        params![account, now],
    )?;
    c.query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = 1", [account], |r| r.get(0))
}

/// A grant, with its machine's name for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    /// The machine's name in the hub's directory, when it's there (the
    /// caller fills it in from `HubPort::daemons`).
    pub subject_name: Option<String>,
    pub power: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub granted_by: String,
    pub created_at: u64,
    pub revoked_at: Option<u64>,
}

const GRANT_COLUMNS: &str =
    "id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at, revoked_at";

fn grant_row(r: &rusqlite::Row) -> rusqlite::Result<Grant> {
    Ok(Grant {
        id: r.get(0)?,
        subject_kind: r.get(1)?,
        subject_id: r.get(2)?,
        subject_name: None,
        power: r.get(3)?,
        scope_kind: r.get(4)?,
        scope_id: r.get(5)?,
        granted_by: r.get(6)?,
        created_at: r.get(7)?,
        revoked_at: r.get(8)?,
    })
}

impl Db {
    /// The account's personal project, made now if it has none: what its
    /// Cloud workspaces and cloud secrets belong to. The caller has checked
    /// that the account is there ([`crate::projects::personal_project`]).
    pub fn personal_project(&self, account: &str, now: u64) -> anyhow::Result<String> {
        Ok(personal_project(&self.c(), account, now)?)
    }

    /// A team's default project, if it has one yet.
    pub fn team_default_project(&self, team: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .c()
            .query_row("SELECT id FROM projects WHERE team_id = ?1 AND is_default = 1", [team], |r| r.get(0))
            .optional()?)
    }

    /// A team's default project, made now if it has none (named `name`):
    /// what its chat channels belong to. The caller has checked that the
    /// team is there ([`crate::projects::team_project`]).
    pub fn team_project(&self, team: &str, name: &str, now: u64) -> anyhow::Result<String> {
        Ok(team_project(&self.c(), team, name, now)?)
    }

    /// Rewrite a team's `team_members` from its latest roster, as the hub
    /// gives it (members whose account is gone are already left out), and
    /// make its default project if it has none.
    pub fn project_roster(&self, team: &Team, now: u64) -> anyhow::Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM team_members WHERE team_id = ?1", [&team.id])?;
        for m in &team.members {
            tx.execute(
                "INSERT OR REPLACE INTO team_members (team_id, account_id, role, name, roster_version)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![team.id, m.account, m.role.as_str(), m.name, team.version],
            )?;
        }
        team_project(&tx, &team.id, &team.name, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Every team the platform holds something of: a projection, a project,
    /// or a team channel.
    pub fn known_teams(&self) -> anyhow::Result<Vec<String>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT team_id FROM team_members UNION SELECT team_id FROM projects WHERE team_id IS NOT NULL
             UNION SELECT substr(scope, 6) FROM chat_rooms WHERE kind = 'channel' AND scope LIKE 'team:%'
             ORDER BY 1",
        )?;
        Ok(q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// The teams whose channels have no project yet (a database from before
    /// M129).
    pub fn unattached_team_channels(&self) -> anyhow::Result<Vec<String>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT DISTINCT substr(scope, 6) FROM chat_rooms
             WHERE project_id IS NULL AND kind = 'channel' AND scope LIKE 'team:%' ORDER BY 1",
        )?;
        Ok(q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// A team's channels with no project get `project`.
    pub fn attach_team_channels(&self, team: &str, project: &str) -> anyhow::Result<()> {
        self.c().execute(
            "UPDATE chat_rooms SET project_id = ?2
             WHERE project_id IS NULL AND kind = 'channel' AND scope = 'team:' || ?1",
            params![team, project],
        )?;
        Ok(())
    }

    /// Whether this machine may drive `account`'s Cloud workspaces, as far
    /// as the platform's grants say: `Some(true)` for a live `cloud.drive`
    /// grant over it, `Some(false)` when every one it had was revoked, and
    /// `None` when it never had one (a machine that just joined).
    pub fn cloud_drive(&self, daemon: &str, account: &str) -> anyhow::Result<Option<bool>> {
        Ok(self.c().query_row(
            &format!(
                "SELECT max(revoked_at IS NULL) FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                     AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2"
            ),
            params![daemon, account],
            |r| r.get::<_, Option<bool>>(0),
        )?)
    }

    /// A machine joined to `account` gets `cloud.drive` over it, unless it
    /// has had one there before (a revoked one stays revoked; a machine that
    /// joins again lost its grants when it left). Whether it got one now.
    pub fn grant_on_join(&self, daemon: &str, account: &str, now: u64) -> anyhow::Result<bool> {
        Ok(self.c().execute(
            &format!(
                "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
                 SELECT {NEW_GRANT_SQL}, 'daemon', ?1, '{CLOUD_DRIVE}', 'account', ?2, ?2, ?3
                 WHERE NOT EXISTS (SELECT 1 FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                     AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2)"
            ),
            params![daemon, account, now],
        )? > 0)
    }

    /// A machine left (or was removed) at `at`: its grants from before then
    /// go, so one that joins again starts with a new one. A grant given
    /// after `at` is its new joining's, and stays.
    pub fn machine_left(&self, daemon: &str, at: u64) -> anyhow::Result<()> {
        self.c().execute(
            "DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1 AND created_at <= ?2",
            params![daemon, at],
        )?;
        Ok(())
    }

    /// Every machine that holds a grant, with the account it's over.
    pub fn granted_machines(&self) -> anyhow::Result<Vec<(String, String)>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT DISTINCT subject_id, scope_id FROM grants WHERE subject_kind = 'daemon' AND scope_kind = 'account'
             ORDER BY 1, 2",
        )?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Every grant of a machine over an account goes: it isn't that
    /// account's machine any more.
    pub fn drop_machine_grants(&self, daemon: &str, account: &str) -> anyhow::Result<()> {
        self.c().execute(
            "DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
             AND scope_kind = 'account' AND scope_id = ?2",
            params![daemon, account],
        )?;
        Ok(())
    }

    /// The grants over an account, live and revoked, oldest machine row
    /// first; [`crate::grants::list`] puts them in order by machine name.
    pub fn grants(&self, account: &str) -> anyhow::Result<Vec<Grant>> {
        let c = self.c();
        let mut q = c.prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM grants WHERE scope_kind = 'account' AND scope_id = ?1
             ORDER BY subject_id, created_at DESC, id"
        ))?;
        Ok(q.query_map([account], grant_row)?.collect::<rusqlite::Result<_>>()?)
    }

    fn grant(c: &Connection, account: &str, id: &str) -> rusqlite::Result<Option<Grant>> {
        c.query_row(
            &format!("SELECT {GRANT_COLUMNS} FROM grants WHERE id = ?1 AND scope_kind = 'account' AND scope_id = ?2"),
            params![id, account],
            grant_row,
        )
        .optional()
    }

    /// Revoke one of the grants over an account; revoking it again changes
    /// nothing. `None`: no such grant over it.
    pub fn revoke_grant(&self, account: &str, id: &str, now: u64) -> anyhow::Result<Option<Grant>> {
        let c = self.c();
        c.execute(
            "UPDATE grants SET revoked_at = ?3 WHERE id = ?1 AND scope_kind = 'account' AND scope_id = ?2
             AND revoked_at IS NULL",
            params![id, account, now],
        )?;
        Ok(Self::grant(&c, account, id)?)
    }

    /// Give one of the account's own machines `cloud.drive` over it again,
    /// as a new grant (the revoked one stays, for the record). One that's
    /// live already is the answer as it is. The caller has checked that it's
    /// the account's machine ([`crate::grants::give`]).
    pub fn give_cloud_drive(&self, account: &str, daemon: &str, by: &str, now: u64) -> anyhow::Result<Grant> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let live: Option<String> = tx
            .query_row(
                &format!(
                    "SELECT id FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1 AND power = '{CLOUD_DRIVE}'
                     AND scope_kind = 'account' AND scope_id = ?2 AND revoked_at IS NULL"
                ),
                params![daemon, account],
                |r| r.get(0),
            )
            .optional()?;
        let id = match live {
            Some(id) => id,
            None => tx.query_row(
                &format!(
                    "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
                     VALUES ({NEW_GRANT_SQL}, 'daemon', ?1, '{CLOUD_DRIVE}', 'account', ?2, ?3, ?4) RETURNING id"
                ),
                params![daemon, account, by, now],
                |r| r.get(0),
            )?,
        };
        let g = Self::grant(&tx, account, &id)?.ok_or_else(|| anyhow::anyhow!("the grant just made is gone"))?;
        tx.commit()?;
        Ok(g)
    }

    /// The last hub change applied here; `None` before the first read.
    pub fn feed_after(&self) -> anyhow::Result<Option<u64>> {
        Ok(self.c().query_row("SELECT seq FROM hub_feed WHERE reader = ?1", [READER], |r| r.get(0)).optional()?)
    }

    pub fn set_feed_after(&self, seq: u64) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO hub_feed (reader, seq) VALUES (?1, ?2) ON CONFLICT (reader) DO UPDATE SET seq = excluded.seq",
            params![READER, seq],
        )?;
        Ok(())
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction: its personal projects, its place in every
/// team's projection, the disbanded teams' projects and projections, and
/// grants to its machines, over it, or that it gave.
pub(crate) fn erase_account(c: &Connection, d: &Deleted) -> rusqlite::Result<()> {
    let account = &d.account;
    c.execute("DELETE FROM projects WHERE account_id = ?1", [account])?;
    c.execute("DELETE FROM team_members WHERE account_id = ?1", [account])?;
    for t in &d.disbanded {
        c.execute("DELETE FROM projects WHERE team_id = ?1", [t])?;
        c.execute("DELETE FROM team_members WHERE team_id = ?1", [t])?;
    }
    c.execute(
        "DELETE FROM grants WHERE granted_by = ?1 OR (scope_kind = 'account' AND scope_id = ?1)
         OR (scope_kind = 'project' AND scope_id NOT IN (SELECT id FROM projects))",
        [account],
    )?;
    for m in &d.machines {
        c.execute("DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1", [m])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Db;
    use crate::Deleted;
    use rusqlite::Connection;

    /// A database in the shape before M129: the tables a project hangs off
    /// with no `project_id`, and no projects.
    fn old_shape() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cloud_workspaces (id TEXT PRIMARY KEY, account TEXT NOT NULL, created INTEGER NOT NULL);
             CREATE TABLE cloud_secrets (account TEXT NOT NULL, name TEXT NOT NULL, sealed BLOB NOT NULL,
                 PRIMARY KEY(account,name));
             CREATE TABLE chat_rooms (id TEXT PRIMARY KEY, scope TEXT NOT NULL, kind TEXT NOT NULL);
             INSERT INTO cloud_workspaces VALUES ('cw_1', 'alice', 1), ('cw_2', 'alice', 2), ('cw_3', 'bob', 3);
             INSERT INTO cloud_secrets VALUES ('alice', 'A', x'00'), ('dave', 'D', x'00');
             INSERT INTO chat_rooms VALUES ('c1', 'team:t1', 'channel'), ('d1', 'team:t1', 'dm');",
        )
        .unwrap();
        conn.execute_batch(super::SCHEMA).unwrap();
        conn
    }

    fn dump(conn: &Connection, sql: &str) -> Vec<Vec<Option<String>>> {
        let mut q = conn.prepare(sql).unwrap();
        let n = q.column_count();
        let text = |v: rusqlite::types::Value| match v {
            rusqlite::types::Value::Null => None,
            rusqlite::types::Value::Integer(i) => Some(i.to_string()),
            rusqlite::types::Value::Text(t) => Some(t),
            v => Some(format!("{v:?}")),
        };
        q.query_map([], |r| (0..n).map(|i| r.get(i).map(text)).collect()).unwrap().map(Result::unwrap).collect()
    }

    fn count(conn: &Connection, sql: &str) -> u64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// What the platform backfills on its own (M129): every account with
    /// Cloud rows gets its personal project and those rows get it. Team
    /// channels wait for their team's name from the hub
    /// (`projects::start`).
    #[test]
    fn the_backfill_gives_every_cloud_row_its_personal_project() {
        let conn = old_shape();
        super::migrate(&conn, 5).unwrap();
        assert_eq!(
            dump(&conn, "SELECT account_id, name FROM projects WHERE is_default = 1 ORDER BY 1"),
            [["alice", "Personal"], ["bob", "Personal"], ["dave", "Personal"]]
                .map(|r| r.map(|s| Some(s.to_owned())).to_vec())
        );
        assert!(dump(&conn, "SELECT id FROM projects").iter().all(|r| {
            let id = r[0].as_deref().unwrap();
            id.len() == 36 && id.starts_with("prj_")
        }));
        let project = |owner: &str| -> String {
            conn.query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = 1", [owner], |r| r.get(0))
                .unwrap()
        };
        let of = |sql: &str| dump(&conn, sql).into_iter().map(|r| r[1].clone()).collect::<Vec<_>>();
        assert_eq!(
            of("SELECT id, project_id FROM cloud_workspaces ORDER BY id"),
            [Some(project("alice")), Some(project("alice")), Some(project("bob"))]
        );
        assert_eq!(
            of("SELECT name, project_id FROM cloud_secrets ORDER BY name"),
            [Some(project("alice")), Some(project("dave"))]
        );
        assert_eq!(
            of("SELECT id, project_id FROM chat_rooms ORDER BY id"),
            [None, None],
            "a column, not yet a project"
        );

        // Again: nothing changes.
        let all = "SELECT * FROM projects ORDER BY id";
        let before = dump(&conn, all);
        super::migrate(&conn, 9).unwrap();
        assert_eq!(dump(&conn, all), before);
    }

    #[test]
    fn a_project_belongs_to_one_owner_with_one_default() {
        let db = Db::memory();
        let c = db.c();
        let insert = |id: &str, team: Option<&str>, account: Option<&str>, default: bool| {
            c.execute(
                "INSERT INTO projects (id, team_id, account_id, name, is_default, created_at) VALUES (?1, ?2, ?3, 'x', ?4, 0)",
                rusqlite::params![id, team, account, default],
            )
        };
        assert!(insert("prj_both", Some("t"), Some("a"), false).is_err());
        assert!(insert("prj_neither", None, None, false).is_err());
        insert("prj_1", Some("t"), None, true).unwrap();
        assert!(insert("prj_2", Some("t"), None, true).is_err(), "a second default");
        insert("prj_3", Some("t"), None, false).unwrap();
        insert("prj_4", None, Some("t"), true).unwrap();
    }

    /// A default project is made once, the first time; a team's takes the
    /// name it's given then.
    #[test]
    fn a_default_project_is_made_once() {
        let db = Db::memory();
        let personal = db.personal_project("bob", 1).unwrap();
        assert_eq!(db.personal_project("bob", 2).unwrap(), personal);
        assert_eq!(db.team_default_project("t1").unwrap(), None);
        let team = db.team_project("t1", "one", 1).unwrap();
        assert_ne!(team, personal);
        assert_eq!(db.team_project("t1", "uno", 2).unwrap(), team);
        assert_eq!(db.team_default_project("t1").unwrap(), Some(team));
        assert_eq!(count(&db.c(), "SELECT count(*) FROM projects"), 2);
        assert_eq!(dump(&db.c(), "SELECT name FROM projects WHERE team_id = 't1'"), [[Some("one".to_owned())]]);
    }

    /// A machine's grant: none until it joins, one when it does, never a
    /// second while it has one (a revoked one stays revoked), and gone with
    /// what it had from before it left.
    #[test]
    fn a_machine_gets_one_grant_and_loses_it_when_it_leaves() {
        let db = Db::memory();
        assert_eq!(db.cloud_drive("geek", "alice").unwrap(), None, "never had one");
        assert!(db.grant_on_join("geek", "alice", 10).unwrap());
        assert!(!db.grant_on_join("geek", "alice", 11).unwrap(), "once");
        assert_eq!(db.cloud_drive("geek", "alice").unwrap(), Some(true));
        assert_eq!(db.cloud_drive("geek", "bob").unwrap(), None, "over its own account only");
        let g = &db.grants("alice").unwrap()[0];
        assert!(g.id.starts_with("grt_") && g.granted_by == "alice" && g.subject_name.is_none());
        db.revoke_grant("alice", &g.id, 20).unwrap();
        assert_eq!(db.cloud_drive("geek", "alice").unwrap(), Some(false));
        assert!(!db.grant_on_join("geek", "alice", 21).unwrap(), "a revoked one stays revoked");
        let again = db.give_cloud_drive("alice", "geek", "alice", 30).unwrap();
        assert_ne!(again.id, g.id);
        assert_eq!(db.give_cloud_drive("alice", "geek", "alice", 31).unwrap().id, again.id, "a live one is the answer");
        assert_eq!(db.cloud_drive("geek", "alice").unwrap(), Some(true));

        // It left at 25: what it had from before goes, what came after stays.
        db.machine_left("geek", 25).unwrap();
        let left: Vec<_> = db.grants("alice").unwrap().into_iter().map(|g| g.id).collect();
        assert_eq!(left, std::slice::from_ref(&again.id));
        db.machine_left("geek", 40).unwrap();
        assert_eq!(db.cloud_drive("geek", "alice").unwrap(), None);
    }

    /// Deleting an account takes its personal project, its place in teams'
    /// projections, the disbanded teams' projects and projections, and
    /// grants to its machines, over it, or that it gave.
    #[test]
    fn deleting_an_account_takes_its_projects_and_grants() {
        use crate::hub::{Member, Team};
        use arugula_e2e::team::TeamRole;
        let db = Db::memory();
        let team = |id: &str, members: &[(&str, TeamRole)]| Team {
            id: id.into(),
            name: id.into(),
            founder: "alice".into(),
            locked: false,
            version: 1,
            members: members
                .iter()
                .map(|(a, role)| Member { account: (*a).into(), role: *role, name: (*a).into() })
                .collect(),
        };
        db.project_roster(&team("t1", &[("alice", TeamRole::Owner)]), 1).unwrap();
        db.project_roster(&team("t2", &[("bob", TeamRole::Owner), ("alice", TeamRole::Editor)]), 1).unwrap();
        db.personal_project("alice", 1).unwrap();
        db.personal_project("bob", 1).unwrap();
        db.grant_on_join("geek", "alice", 1).unwrap();
        db.grant_on_join("box", "bob", 1).unwrap();
        let gone = Deleted { account: "alice".into(), machines: vec!["geek".into()], disbanded: vec!["t1".into()] };
        super::erase_account(&db.c(), &gone).unwrap();
        super::erase_account(&db.c(), &gone).unwrap();
        assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
        assert!(db.mentions("geek").is_empty());
        assert!(db.mentions("t1").is_empty());
        assert_eq!(
            dump(&db.c(), "SELECT team_id, account_id FROM team_members ORDER BY 1, 2"),
            [[Some("t2".to_owned()), Some("bob".to_owned())]]
        );
        assert_eq!(count(&db.c(), "SELECT count(*) FROM projects"), 2, "bob's and t2's stay");
        assert_eq!(db.cloud_drive("box", "bob").unwrap(), Some(true));
    }
}
