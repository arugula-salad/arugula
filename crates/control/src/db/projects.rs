//! The platform plane's nouns (#1122): projects ("The project is the unit
//! of platform sharing"), the projection of each team's roster, and grants
//! ("Grants, not impersonation").
//!
//! - A project belongs to one team or one account, never both. Each has at
//!   most one default: a team's channels go to the team's, and a Cloud
//!   workspace or cloud secret to its owner's personal one, made the first
//!   time it's needed. DMs and pane or session threads belong to none.
//! - `team_members` is the latest signed roster of each team, rewritten in
//!   the same transaction as every roster write, for platform queries. It's
//!   never read for machine access: the signed roster stays the authority.
//! - A grant is a subject's power over a scope: a daemon's `cloud.drive`
//!   over its owner's account, for now (M130: aru-047 as grants). Every
//!   machine joined to an account gets one, and control checks it on the
//!   Cloud workspace routes (`auth::CloudDriver`). A revoked grant keeps its
//!   row; giving it back is a new one.
//!
//! Nothing reads `project_id` yet (M129).

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::Db;

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

/// `project_id` on the tables that hang off a project, and the backfill
/// (M129): every team gets its default project, every account with Cloud
/// workspaces or cloud secrets its personal one, and those rows and team
/// channels the right one. Runs on every start; with nothing to do, it
/// changes nothing.
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
            "INSERT INTO projects (id, team_id, name, is_default, created_at)
             SELECT {NEW_ID_SQL}, t.id, t.name, 1, ?1 FROM teams t
             WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.team_id = t.id AND p.is_default = 1)"
        ),
        [now],
    )?;
    tx.execute(
        &format!(
            "INSERT INTO projects (id, account_id, name, is_default, created_at)
             SELECT {NEW_ID_SQL}, o.account, 'Personal', 1, ?1
             FROM (SELECT account FROM cloud_workspaces UNION SELECT account FROM cloud_secrets) o
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
    tx.execute(
        "UPDATE chat_rooms SET project_id = (SELECT p.id FROM projects p
             WHERE p.team_id = substr(chat_rooms.scope, 6) AND p.is_default = 1)
         WHERE project_id IS NULL AND kind = 'channel' AND scope LIKE 'team:%'",
        [],
    )?;
    // Teams from before the projection, and any that lost it.
    let unprojected: Vec<String> = {
        let mut q = tx.prepare(
            "SELECT id FROM teams WHERE NOT EXISTS (SELECT 1 FROM team_members m WHERE m.team_id = teams.id)",
        )?;
        q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for team in unprojected {
        project_roster(&tx, &team)?;
    }
    // aru-047 as grants (M130): every machine joined to an account drives
    // its Cloud workspaces, unless its owner took that back (a revoked
    // grant keeps its row, so it isn't given again here).
    tx.execute(
        &format!(
            "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
             SELECT {NEW_GRANT_SQL}, 'daemon', d.id, '{CLOUD_DRIVE}', 'account', d.account, d.account, ?1
             FROM devices d WHERE d.kind = 'daemon' AND d.approved = 1 AND NOT EXISTS (
                 SELECT 1 FROM grants g WHERE g.subject_kind = 'daemon' AND g.subject_id = d.id
                 AND g.power = '{CLOUD_DRIVE}' AND g.scope_kind = 'account' AND g.scope_id = d.account)"
        ),
        [now],
    )?;
    tx.commit()
}

/// A team's default project, made now if it has none (named after the team).
pub(super) fn team_project(c: &Connection, team: &str, now: u64) -> rusqlite::Result<String> {
    let found = c
        .query_row("SELECT id FROM projects WHERE team_id = ?1 AND is_default = 1", [team], |r| r.get(0))
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    c.query_row(
        &format!(
            "INSERT INTO projects (id, team_id, name, is_default, created_at)
             VALUES ({NEW_ID_SQL}, ?1, coalesce((SELECT name FROM teams WHERE id = ?1), ?1), 1, ?2)
             RETURNING id"
        ),
        params![team, now],
        |r| r.get(0),
    )
}

/// An account's personal project, made now if it has none.
pub(super) fn personal_project(c: &Connection, account: &str, now: u64) -> rusqlite::Result<String> {
    let found = c
        .query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = 1", [account], |r| r.get(0))
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    c.query_row(
        &format!(
            "INSERT INTO projects (id, account_id, name, is_default, created_at)
             VALUES ({NEW_ID_SQL}, ?1, 'Personal', 1, ?2) RETURNING id"
        ),
        params![account, now],
        |r| r.get(0),
    )
}

/// A machine that joined an account gets `cloud.drive` over it, inside the
/// transaction that approves it, unless it has had one there before (a
/// revoked one stays revoked; a machine that joins again was dropped first,
/// and its grants with it).
pub(super) fn grant_on_join(c: &Connection, daemon: &str, account: &str, now: u64) -> rusqlite::Result<()> {
    c.execute(
        &format!(
            "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
             SELECT {NEW_GRANT_SQL}, 'daemon', ?1, '{CLOUD_DRIVE}', 'account', ?2, ?2, ?3
             WHERE NOT EXISTS (SELECT 1 FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                 AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2)"
        ),
        params![daemon, account, now],
    )?;
    Ok(())
}

/// A grant, with its machine's name for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    /// The machine's name in the directory, when it's there.
    pub subject_name: Option<String>,
    pub power: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub granted_by: String,
    pub created_at: u64,
    pub revoked_at: Option<u64>,
}

const GRANT_COLUMNS: &str = "g.id, g.subject_kind, g.subject_id, d.name, g.power, g.scope_kind, g.scope_id,
    g.granted_by, g.created_at, g.revoked_at";

fn grant_row(r: &rusqlite::Row) -> rusqlite::Result<Grant> {
    Ok(Grant {
        id: r.get(0)?,
        subject_kind: r.get(1)?,
        subject_id: r.get(2)?,
        subject_name: r.get(3)?,
        power: r.get(4)?,
        scope_kind: r.get(5)?,
        scope_id: r.get(6)?,
        granted_by: r.get(7)?,
        created_at: r.get(8)?,
        revoked_at: r.get(9)?,
    })
}

impl Db {
    /// Whether this machine holds a live `cloud.drive` grant over its
    /// owner's account: what the Cloud workspace routes check (M130).
    pub fn can_drive_cloud(&self, daemon: &str, account: &str) -> anyhow::Result<bool> {
        Ok(self.c().query_row(
            &format!(
                "SELECT EXISTS (SELECT 1 FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                 AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2 AND revoked_at IS NULL)"
            ),
            params![daemon, account],
            |r| r.get(0),
        )?)
    }

    /// The grants over an account, live and revoked: by machine, newest
    /// first.
    pub fn grants(&self, account: &str) -> anyhow::Result<Vec<Grant>> {
        let c = self.c();
        let mut q = c.prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM grants g LEFT JOIN daemons d ON g.subject_kind = 'daemon' AND d.id = g.subject_id
             WHERE g.scope_kind = 'account' AND g.scope_id = ?1
             ORDER BY coalesce(d.name, g.subject_id), g.subject_id, g.created_at DESC, g.id"
        ))?;
        Ok(q.query_map([account], grant_row)?.collect::<rusqlite::Result<_>>()?)
    }

    fn grant(c: &Connection, account: &str, id: &str) -> rusqlite::Result<Option<Grant>> {
        c.query_row(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants g LEFT JOIN daemons d ON g.subject_kind = 'daemon' AND d.id = g.subject_id
                 WHERE g.id = ?1 AND g.scope_kind = 'account' AND g.scope_id = ?2"
            ),
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
    /// live already is the answer as it is. `None`: not the account's
    /// machine.
    pub fn give_cloud_drive(&self, account: &str, daemon: &str, by: &str, now: u64) -> anyhow::Result<Option<Grant>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let mine: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE account = ?1 AND id = ?2 AND kind = 'daemon' AND approved = 1)",
            params![account, daemon],
            |r| r.get(0),
        )?;
        if !mine {
            return Ok(None);
        }
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
        let g = Self::grant(&tx, account, &id)?;
        tx.commit()?;
        Ok(g)
    }
}

/// Rewrite a team's `team_members` from its latest roster, inside the
/// transaction that wrote the roster. Members whose account is gone are
/// left out: a roster can list a deleted account until an owner signs it
/// out.
pub(super) fn project_roster(c: &Connection, team: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM team_members WHERE team_id = ?1", [team])?;
    c.execute(
        "INSERT OR REPLACE INTO team_members (team_id, account_id, role, name, roster_version)
         SELECT r.team, json_extract(m.value, '$.account'), json_extract(m.value, '$.role'),
                coalesce(json_extract(m.value, '$.name'), ''), r.version
         FROM (SELECT team, version, body FROM rosters WHERE team = ?1 ORDER BY version DESC LIMIT 1) r,
              json_each(r.body, '$.members') m
         WHERE json_extract(m.value, '$.account') IN (SELECT id FROM accounts)",
        [team],
    )?;
    Ok(())
}

/// What deleting an account takes from here, inside `Db::delete_account`'s
/// transaction: its personal projects, its place in every team's
/// projection, the disbanded teams' projects and projections, and grants
/// to its machines, over it, or that it gave.
pub(super) fn erase_account(
    c: &Connection,
    account: &str,
    machines: &[String],
    disband: &[String],
) -> rusqlite::Result<()> {
    c.execute("DELETE FROM projects WHERE account_id = ?1", [account])?;
    c.execute("DELETE FROM team_members WHERE account_id = ?1", [account])?;
    for t in disband {
        c.execute("DELETE FROM projects WHERE team_id = ?1", [t])?;
        c.execute("DELETE FROM team_members WHERE team_id = ?1", [t])?;
    }
    c.execute(
        "DELETE FROM grants WHERE granted_by = ?1 OR (scope_kind = 'account' AND scope_id = ?1)
         OR (scope_kind = 'project' AND scope_id NOT IN (SELECT id FROM projects))",
        [account],
    )?;
    for m in machines {
        c.execute("DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1", [m])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Db;
    use rusqlite::Connection;

    /// A database in the shape before M129: the tables a project hangs off
    /// with no `project_id`, and no projects.
    fn old_shape() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::SCHEMA).unwrap();
        conn.execute_batch(
            "CREATE TABLE cloud_workspaces (id TEXT PRIMARY KEY, account TEXT NOT NULL, created INTEGER NOT NULL);
             CREATE TABLE cloud_secrets (account TEXT NOT NULL, name TEXT NOT NULL, sealed BLOB NOT NULL,
                 PRIMARY KEY(account,name));
             CREATE TABLE chat_rooms (id TEXT PRIMARY KEY, scope TEXT NOT NULL, kind TEXT NOT NULL);
             INSERT INTO accounts (id, created) VALUES ('alice', 1), ('bob', 1), ('carol', 1), ('dave', 1);
             INSERT INTO teams (id, name, founder, founder_root, created) VALUES
                 ('t1', 'one', 'alice', 'r', 1), ('t2', 'two', 'bob', 'r', 1), ('t3', 'quiet', 'carol', 'r', 1);
             INSERT INTO rosters (team, version, body) VALUES
                 ('t1', 1, '{\"members\":[{\"account\":\"alice\",\"role\":\"owner\",\"name\":\"alice\"}]}'),
                 ('t1', 2, '{\"members\":[{\"account\":\"alice\",\"role\":\"owner\",\"name\":\"alice\"},
                             {\"account\":\"bob\",\"role\":\"editor\",\"name\":\"bob\"},
                             {\"account\":\"gone\",\"role\":\"viewer\",\"name\":\"gone\"}]}'),
                 ('t2', 1, '{\"members\":[{\"account\":\"bob\",\"role\":\"owner\",\"name\":\"bob\"}]}');
             INSERT INTO cloud_workspaces VALUES ('cw_1', 'alice', 1), ('cw_2', 'alice', 2), ('cw_3', 'bob', 3);
             INSERT INTO cloud_secrets VALUES ('alice', 'A', x'00'), ('dave', 'D', x'00');
             INSERT INTO chat_rooms VALUES
                 ('c1', 'team:t1', 'channel'), ('c2', 'team:t1', 'channel'), ('c3', 'team:t2', 'channel'),
                 ('d1', 'team:t1', 'dm'), ('p1', 'machine:m1', 'pane'), ('a1', 'account:alice', 'archive');",
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

    #[test]
    fn the_backfill_gives_every_team_one_default_and_every_row_a_project() {
        let conn = old_shape();
        super::migrate(&conn, 5).unwrap();
        // One default per team, the quiet one too, and one per account with
        // Cloud rows: alice and bob (workspaces), dave (a secret), not carol.
        assert_eq!(
            dump(&conn, "SELECT coalesce(team_id, account_id), name FROM projects WHERE is_default = 1 ORDER BY 1"),
            [
                ["alice", "Personal"],
                ["bob", "Personal"],
                ["dave", "Personal"],
                ["t1", "one"],
                ["t2", "two"],
                ["t3", "quiet"]
            ]
            .map(|r| r.map(|s| Some(s.to_owned())).to_vec())
        );
        assert!(dump(&conn, "SELECT id FROM projects").iter().all(|r| {
            let id = r[0].as_deref().unwrap();
            id.len() == 36 && id.starts_with("prj_")
        }));
        assert_eq!(count(&conn, "SELECT count(*) FROM cloud_workspaces WHERE project_id IS NULL"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM cloud_secrets WHERE project_id IS NULL"), 0);
        let project = |owner: &str| -> String {
            conn.query_row(
                "SELECT id FROM projects WHERE coalesce(team_id, account_id) = ?1 AND is_default = 1",
                [owner],
                |r| r.get(0),
            )
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
        // Team channels go to their team's; DMs, pane threads and archives to none.
        assert_eq!(
            of("SELECT id, project_id FROM chat_rooms ORDER BY id"),
            [None, Some(project("t1")), Some(project("t1")), Some(project("t2")), None, None]
        );
        // The projection: the latest roster, without the account that's gone.
        assert_eq!(
            dump(&conn, "SELECT team_id, account_id, role, roster_version FROM team_members ORDER BY 1, 2"),
            [["t1", "alice", "owner", "2"], ["t1", "bob", "editor", "2"], ["t2", "bob", "owner", "1"]]
                .map(|r| r.map(|s| Some(s.to_owned())).to_vec())
        );

        // Again: nothing changes.
        let all = "SELECT * FROM projects ORDER BY id";
        let before = (dump(&conn, all), dump(&conn, "SELECT * FROM team_members ORDER BY 1, 2"));
        let rooms = dump(&conn, "SELECT id, project_id FROM chat_rooms ORDER BY id");
        super::migrate(&conn, 9).unwrap();
        assert_eq!((dump(&conn, all), dump(&conn, "SELECT * FROM team_members ORDER BY 1, 2")), before);
        assert_eq!(dump(&conn, "SELECT id, project_id FROM chat_rooms ORDER BY id"), rooms);
    }

    /// M130's backfill: every approved machine gets `cloud.drive` over its
    /// account, except one whose owner revoked it; a waiting one, a
    /// browser or a CLI gets none.
    #[test]
    fn the_backfill_grants_every_joined_machine_cloud_drive_once() {
        let conn = old_shape();
        conn.execute_batch(
            "INSERT INTO devices (account, id, kind, cert, approved, created) VALUES
                 ('alice', 'geek', 'daemon', '{}', 1, 1), ('alice', 'mini', 'daemon', '{}', 1, 1),
                 ('bob', 'box', 'daemon', '{}', 1, 1), ('bob', 'waiting', 'daemon', '{}', 0, 1),
                 ('bob', 'laptop', 'browser', '{}', 1, 1), ('bob', 'cli', 'cli', '{}', 1, 1);
             INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at, revoked_at)
                 VALUES ('grt_old', 'daemon', 'mini', 'cloud.drive', 'account', 'alice', 'alice', 1, 2);",
        )
        .unwrap();
        super::migrate(&conn, 5).unwrap();
        let grants = "SELECT subject_id, scope_id, granted_by, revoked_at FROM grants ORDER BY subject_id";
        let rows = |r: &[&str]| r.iter().map(|s| (*s != "-").then(|| s.to_string())).collect::<Vec<_>>();
        let want = vec![
            rows(&["box", "bob", "bob", "-"]),
            rows(&["geek", "alice", "alice", "-"]),
            rows(&["mini", "alice", "alice", "2"]),
        ];
        assert_eq!(dump(&conn, grants), want);
        assert!(dump(&conn, "SELECT id FROM grants").iter().all(|r| r[0].as_deref().unwrap().starts_with("grt_")));
        super::migrate(&conn, 9).unwrap();
        assert_eq!(dump(&conn, grants), want);
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

    #[test]
    fn new_rows_get_a_project_and_the_projection_follows_rosters() {
        use crate::db::{Team, chat::Room};
        let db = Db::memory();
        for a in ["alice", "bob"] {
            db.account_for("test", a, a, a, 1).unwrap();
        }
        let members = |ms: &[(&str, &str)]| {
            let ms: Vec<_> =
                ms.iter().map(|(a, role)| serde_json::json!({"account": a, "role": role, "name": a})).collect();
            serde_json::json!({ "name": "one", "members": ms }).to_string()
        };
        let team = Team {
            id: "t1".into(),
            name: "one".into(),
            founder: "alice".into(),
            founder_root: "r".into(),
            locked: false,
        };
        db.add_team(&team, 1, &members(&[("alice", "owner")]), 1).unwrap();
        let projected = || {
            dump(&db.c(), "SELECT account_id, role, roster_version FROM team_members ORDER BY 1")
                .into_iter()
                .map(|r| r.into_iter().map(Option::unwrap).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
        };
        assert_eq!(projected(), ["alice owner 1"]);
        db.add_roster("t1", 2, &members(&[("alice", "owner"), ("bob", "viewer")])).unwrap();
        assert_eq!(projected(), ["alice owner 2", "bob viewer 2"]);
        // A version that's already there changes nothing.
        assert!(db.add_roster("t1", 2, &members(&[("alice", "owner")])).is_err());
        assert_eq!(projected(), ["alice owner 2", "bob viewer 2"]);
        db.add_roster("t1", 3, &members(&[("bob", "owner")])).unwrap();
        assert_eq!(projected(), ["bob owner 3"]);

        // The team got its default project when it was made.
        let team_project = super::team_project(&db.c(), "t1", 2).unwrap();
        assert_eq!(count(&db.c(), "SELECT count(*) FROM projects WHERE team_id = 't1'"), 1);
        let room = |id: &str, scope: &str, kind: &str| Room {
            id: id.into(),
            scope: scope.into(),
            kind: kind.into(),
            name: id.into(),
            machine: None,
            target: None,
            created_by: "alice".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&room("c1", "team:t1", "channel"), &[]).unwrap();
        db.add_chat_room(&room("d1", "team:t1", "dm"), &["alice", "bob"]).unwrap();
        let rooms = dump(&db.c(), "SELECT id, project_id FROM chat_rooms ORDER BY id");
        assert_eq!(rooms, [vec![Some("c1".into()), Some(team_project)], vec![Some("d1".into()), None]]);

        // A workspace and a secret go to the owner's personal project, made
        // once, the first time.
        assert_eq!(count(&db.c(), "SELECT count(*) FROM projects WHERE account_id = 'bob'"), 0);
        db.put_cloud_secret("bob", "S", b"x").unwrap();
        db.put_cloud_secret("bob", "S", b"y").unwrap();
        let spec: crate::db::cloud::CloudWorkspaceSpec =
            serde_json::from_value(serde_json::json!({"name":"box"})).unwrap();
        db.create_cloud_workspace("bob", "k", &spec, 1).unwrap();
        let personal = super::personal_project(&db.c(), "bob", 2).unwrap();
        assert_eq!(count(&db.c(), "SELECT count(*) FROM projects WHERE account_id = 'bob'"), 1);
        assert_eq!(dump(&db.c(), "SELECT project_id FROM cloud_secrets"), [[Some(personal.clone())]]);
        assert_eq!(dump(&db.c(), "SELECT project_id FROM cloud_workspaces"), [[Some(personal)]]);
    }
}
