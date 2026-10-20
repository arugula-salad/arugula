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
//!   over its owner's account, for now. A revoked grant keeps its row.
//!
//! Nothing reads `project_id` yet (M129).

use rusqlite::{Connection, OptionalExtension, params};

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
