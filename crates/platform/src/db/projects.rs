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
//! Nothing reads `project_id` yet (M129). The tables are made by the
//! platform's migrations (`migrations/sqlite`), which also ran M129's
//! backfill one last time (`0003_personal_projects.sql`).

use serde::Serialize;

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
};
use crate::{Deleted, hub::Team};

/// A new project's id: `prj_` and 32 hex characters, beside `cw_` for
/// Cloud workspaces.
fn new_id() -> String {
    format!("prj_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A new grant's id: `grt_` and 32 hex characters.
fn new_grant() -> String {
    format!("grt_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A machine's power to drive its owner's Cloud workspaces (aru-047), over
/// the owner's account.
pub const CLOUD_DRIVE: &str = "cloud.drive";

/// An agent's powers in conversations (M197, platform step 6): a machine's
/// agents read rooms (and issues) in a scope, post in them (issue comments
/// too), or write issues there. Over a project (a team's channels are its
/// default project's) or the owner's account (their DMs). Never given on
/// join: the machine's owner grants each one ([`crate::grants::give`]).
pub const THREADS_READ: &str = "threads.read";
pub const THREADS_POST: &str = "threads.post";
pub const ISSUES_WRITE: &str = "issues.write";
/// An agent's power to plan a project's deploys and ask for their approval
/// (M218), over a project only. Never an approval: no grant is.
pub const DEPLOYS_PLAN: &str = "deploys.plan";
pub const AGENT_POWERS: [&str; 4] = [THREADS_READ, THREADS_POST, ISSUES_WRITE, DEPLOYS_PLAN];

/// An agent's powers over a project's runs (M216, platform step 7): its
/// machine's agents read the project's runs and their logs (`runs.read`),
/// start one of its run specs (`runs.start`, under the project's standing
/// grant, like any trigger), or start a command or an instruction of their
/// own there (`runs.command`). Over a project only, never given on join,
/// and never an approval: a deploy's is an owner's device's (aru-101). Not
/// the standing grant's `runs.start` capability
/// (`arugula_e2e::project_grant`), which is what a hosted machine checks;
/// this is who may ask the platform for a run.
pub const RUNS_READ: &str = "runs.read";
pub const RUNS_START: &str = "runs.start";
pub const RUNS_COMMAND: &str = "runs.command";
pub const RUN_POWERS: [&str; 3] = [RUNS_READ, RUNS_START, RUNS_COMMAND];

/// The reader in `hub_feed` that keeps these tables.
const READER: &str = "projects";

/// A team's default project, made now (named `name`) if it has none.
fn team_project(c: &impl Sql, team: &str, name: &str, now: u64) -> anyhow::Result<String> {
    c.execute(
        "INSERT INTO projects (id, team_id, name, is_default, created_at)
         SELECT ?4, ?1, ?2, TRUE, ?3
         WHERE NOT EXISTS (SELECT 1 FROM projects WHERE team_id = ?1 AND is_default = TRUE)
         ON CONFLICT DO NOTHING",
        params![team, name, now, new_id()],
    )?;
    c.query_row("SELECT id FROM projects WHERE team_id = ?1 AND is_default = TRUE", params![team], |r| r.get(0))
}

/// An account's personal project, made now if it has none.
fn personal_project(c: &impl Sql, account: &str, now: u64) -> anyhow::Result<String> {
    c.execute(
        "INSERT INTO projects (id, account_id, name, is_default, created_at)
         SELECT ?3, ?1, 'Personal', TRUE, ?2
         WHERE NOT EXISTS (SELECT 1 FROM projects WHERE account_id = ?1 AND is_default = TRUE)
         ON CONFLICT DO NOTHING",
        params![account, now, new_id()],
    )?;
    c.query_row("SELECT id FROM projects WHERE account_id = ?1 AND is_default = TRUE", params![account], |r| r.get(0))
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
    /// A project's name, for a grant over one (the caller fills it in).
    pub scope_name: Option<String>,
    pub granted_by: String,
    pub created_at: u64,
    pub revoked_at: Option<u64>,
}

const GRANT_COLUMNS: &str =
    "id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at, revoked_at";

fn grant_row(r: &Row) -> anyhow::Result<Grant> {
    Ok(Grant {
        id: r.get(0)?,
        subject_kind: r.get(1)?,
        subject_id: r.get(2)?,
        subject_name: None,
        power: r.get(3)?,
        scope_kind: r.get(4)?,
        scope_id: r.get(5)?,
        scope_name: None,
        granted_by: r.get(6)?,
        created_at: r.get(7)?,
        revoked_at: r.get(8)?,
    })
}

/// Whose a project is: a team's, or one account's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOwner {
    pub team: Option<String>,
    pub account: Option<String>,
    pub name: String,
}

impl Db {
    /// Whose a project is, and its name; `None`: no such project.
    pub fn project_owner(&self, project: &str) -> anyhow::Result<Option<ProjectOwner>> {
        self.c()
            .query_row("SELECT team_id, account_id, name FROM projects WHERE id = ?1", params![project], |r| {
                Ok(ProjectOwner { team: r.get(0)?, account: r.get(1)?, name: r.get(2)? })
            })
            .optional()
    }

    /// The account's personal project, made now if it has none: what its
    /// Cloud workspaces and cloud secrets belong to. The caller has checked
    /// that the account is there ([`crate::projects::personal_project`]).
    pub fn personal_project(&self, account: &str, now: u64) -> anyhow::Result<String> {
        personal_project(&self.c(), account, now)
    }

    /// A team's default project, if it has one yet.
    pub fn team_default_project(&self, team: &str) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT id FROM projects WHERE team_id = ?1 AND is_default = TRUE", params![team], |r| r.get(0))
            .optional()
    }

    /// A team's default project, made now if it has none (named `name`):
    /// what its chat channels belong to. The caller has checked that the
    /// team is there ([`crate::projects::team_project`]).
    pub fn team_project(&self, team: &str, name: &str, now: u64) -> anyhow::Result<String> {
        team_project(&self.c(), team, name, now)
    }

    /// Rewrite a team's `team_members` from its latest roster, as the hub
    /// gives it (members whose account is gone are already left out), and
    /// make its default project if it has none.
    pub fn project_roster(&self, team: &Team, now: u64) -> anyhow::Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM team_members WHERE team_id = ?1", params![team.id])?;
        for m in &team.members {
            tx.execute(
                "INSERT INTO team_members (team_id, account_id, role, name, roster_version)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (team_id, account_id) DO UPDATE SET role = excluded.role, name = excluded.name,
                     roster_version = excluded.roster_version",
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
        self.c().query_map(
            "SELECT team_id FROM team_members UNION SELECT team_id FROM projects WHERE team_id IS NOT NULL
             UNION SELECT substr(scope, 6) FROM chat_rooms WHERE kind = 'channel' AND scope LIKE 'team:%'
             ORDER BY 1",
            params![],
            |r| r.get(0),
        )
    }

    /// The teams whose channels have no project yet (a database from before
    /// M129).
    pub fn unattached_team_channels(&self) -> anyhow::Result<Vec<String>> {
        self.c().query_map(
            "SELECT DISTINCT substr(scope, 6) FROM chat_rooms
             WHERE project_id IS NULL AND kind = 'channel' AND scope LIKE 'team:%' ORDER BY 1",
            params![],
            |r| r.get(0),
        )
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
        self.c().query_row(
            &format!(
                "SELECT max(CASE WHEN revoked_at IS NULL THEN 1 ELSE 0 END) FROM grants
                 WHERE subject_kind = 'daemon' AND subject_id = ?1
                     AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2"
            ),
            params![daemon, account],
            |r| r.get::<Option<bool>>(0),
        )
    }

    /// A machine joined to `account` gets `cloud.drive` over it, unless it
    /// has had one there before (a revoked one stays revoked; a machine that
    /// joins again lost its grants when it left). Whether it got one now.
    pub fn grant_on_join(&self, daemon: &str, account: &str, now: u64) -> anyhow::Result<bool> {
        Ok(self.c().execute(
            &format!(
                "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
                 SELECT ?4, 'daemon', ?1, '{CLOUD_DRIVE}', 'account', ?2, ?2, ?3
                 WHERE NOT EXISTS (SELECT 1 FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                     AND power = '{CLOUD_DRIVE}' AND scope_kind = 'account' AND scope_id = ?2)
                 ON CONFLICT DO NOTHING"
            ),
            params![daemon, account, now, new_grant()],
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
        self.c().query_map(
            "SELECT DISTINCT subject_id, scope_id FROM grants WHERE subject_kind = 'daemon' AND scope_kind = 'account'
             ORDER BY 1, 2",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// Every grant of a machine over an account, or that the account gave
    /// it, goes: it isn't that account's machine any more.
    pub fn drop_machine_grants(&self, daemon: &str, account: &str) -> anyhow::Result<()> {
        self.c().execute(
            "DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
             AND ((scope_kind = 'account' AND scope_id = ?2) OR granted_by = ?2)",
            params![daemon, account],
        )?;
        Ok(())
    }

    /// The grants an account manages, live and revoked: those over it, and
    /// those it gave (its machines' over projects, M197). Oldest machine row
    /// first; [`crate::grants::list`] puts them in order by machine name.
    pub fn grants(&self, account: &str) -> anyhow::Result<Vec<Grant>> {
        self.c().query_map(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE (scope_kind = 'account' AND scope_id = ?1) OR granted_by = ?1
                 ORDER BY subject_id, created_at DESC, id"
            ),
            params![account],
            grant_row,
        )
    }

    fn grant(c: &impl Sql, account: &str, id: &str) -> anyhow::Result<Option<Grant>> {
        c.query_row(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE id = ?1
                 AND ((scope_kind = 'account' AND scope_id = ?2) OR granted_by = ?2)"
            ),
            params![id, account],
            grant_row,
        )
        .optional()
    }

    /// Revoke one of the grants an account manages; revoking it again
    /// changes nothing. `None`: no such grant of its.
    pub fn revoke_grant(&self, account: &str, id: &str, now: u64) -> anyhow::Result<Option<Grant>> {
        let c = self.c();
        c.execute(
            "UPDATE grants SET revoked_at = ?3 WHERE id = ?1
             AND ((scope_kind = 'account' AND scope_id = ?2) OR granted_by = ?2) AND revoked_at IS NULL",
            params![id, account, now],
        )?;
        Self::grant(&c, account, id)
    }

    /// Give one of the account's own machines `cloud.drive` over it again,
    /// as a new grant (the revoked one stays, for the record). One that's
    /// live already is the answer as it is. The caller has checked that it's
    /// the account's machine ([`crate::grants::give`]).
    pub fn give_cloud_drive(&self, account: &str, daemon: &str, by: &str, now: u64) -> anyhow::Result<Grant> {
        self.give_grant(daemon, CLOUD_DRIVE, ("account", account), by, now)
    }

    /// Give a machine `power` over a scope (`("project", id)` or
    /// `("account", id)`), granted by `by`, as a new grant. One that's live
    /// already is the answer as it is. The caller has checked that it's
    /// `by`'s machine, and that `by` reads the scope
    /// ([`crate::grants::give`]).
    pub fn give_grant(
        &self,
        daemon: &str,
        power: &str,
        (scope_kind, scope_id): (&str, &str),
        by: &str,
        now: u64,
    ) -> anyhow::Result<Grant> {
        let mut c = self.c();
        let tx = c.transaction()?;
        // A live one given at once on another machine (M144) is the
        // answer: `grants_live` keeps one.
        tx.execute(
            "INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
             SELECT ?7, 'daemon', ?1, ?2, ?3, ?4, ?5, ?6
             WHERE NOT EXISTS (SELECT 1 FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1 AND power = ?2
                 AND scope_kind = ?3 AND scope_id = ?4 AND revoked_at IS NULL)
             ON CONFLICT DO NOTHING",
            params![daemon, power, scope_kind, scope_id, by, now, new_grant()],
        )?;
        let g = tx.query_row(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1 AND power = ?2
                 AND scope_kind = ?3 AND scope_id = ?4 AND revoked_at IS NULL"
            ),
            params![daemon, power, scope_kind, scope_id],
            grant_row,
        )?;
        tx.commit()?;
        Ok(g)
    }

    /// A machine's live grant of `power` over a scope that `by` gave, if it
    /// has one: read on every use, so a revoked one refuses the next call
    /// ([`crate::grants::agent_may`]).
    pub fn live_grant(
        &self,
        daemon: &str,
        power: &str,
        (scope_kind, scope_id): (&str, &str),
        by: &str,
    ) -> anyhow::Result<Option<Grant>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1 AND power = ?2
                     AND scope_kind = ?3 AND scope_id = ?4 AND granted_by = ?5 AND revoked_at IS NULL"
                ),
                params![daemon, power, scope_kind, scope_id, by],
                grant_row,
            )
            .optional()
    }

    /// The live agent grants (threads and issues, M197) over any of these
    /// scopes: who to disclose in a room they reach.
    pub fn agent_grants_over(&self, scopes: &[(&str, &str)]) -> anyhow::Result<Vec<Grant>> {
        if scopes.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let mut args: Vec<String> = AGENT_POWERS.iter().map(|p| (*p).to_owned()).collect();
        let mut ors = vec![];
        for (kind, id) in scopes {
            ors.push("(scope_kind = ? AND scope_id = ?)");
            args.extend([(*kind).to_owned(), (*id).to_owned()]);
        }
        c.query_map(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'daemon' AND revoked_at IS NULL
                 AND power IN (?, ?, ?, ?) AND ({}) ORDER BY granted_by, subject_id, power",
                ors.join(" OR ")
            ),
            &super::sql::list(&args),
            grant_row,
        )
    }

    /// A machine's live agent grants (M197) that `by`, its owner, gave:
    /// what its agents reach (M198).
    pub fn agent_grants_of(&self, daemon: &str, by: &str) -> anyhow::Result<Vec<Grant>> {
        self.c().query_map(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                 AND granted_by = ?2 AND revoked_at IS NULL AND power IN (?3, ?4, ?5, ?6)
                 ORDER BY scope_kind, scope_id, power"
            ),
            params![daemon, by, AGENT_POWERS[0], AGENT_POWERS[1], AGENT_POWERS[2], AGENT_POWERS[3]],
            grant_row,
        )
    }

    /// A machine's live run grants (M216) that `by`, its owner, gave: the
    /// projects whose runs its agents reach.
    pub fn run_grants_of(&self, daemon: &str, by: &str) -> anyhow::Result<Vec<Grant>> {
        self.c().query_map(
            &format!(
                "SELECT {GRANT_COLUMNS} FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1
                 AND granted_by = ?2 AND revoked_at IS NULL AND scope_kind = 'project' AND power IN (?3, ?4, ?5)
                 ORDER BY scope_id, power"
            ),
            params![daemon, by, RUN_POWERS[0], RUN_POWERS[1], RUN_POWERS[2]],
            grant_row,
        )
    }

    /// The last hub change applied here; `None` before the first read.
    pub fn feed_after(&self) -> anyhow::Result<Option<u64>> {
        self.c().query_row("SELECT seq FROM hub_feed WHERE reader = ?1", params![READER], |r| r.get(0)).optional()
    }

    pub fn set_feed_after(&self, seq: u64) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO hub_feed (reader, seq) VALUES (?1, ?2)
             ON CONFLICT (reader) DO UPDATE SET seq = excluded.seq WHERE excluded.seq > hub_feed.seq",
            params![READER, seq],
        )?;
        Ok(())
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction: its personal projects (and their keys and
/// secrets, [`crate::keys::erase_projects`]), its place in every
/// team's projection, the disbanded teams' projects and projections, and
/// grants to its machines, over it, or that it gave.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> anyhow::Result<()> {
    let account = &d.account;
    crate::keys::erase_projects(c, "SELECT id FROM projects WHERE account_id = ?1", account)?;
    c.execute("DELETE FROM projects WHERE account_id = ?1", params![account])?;
    c.execute("DELETE FROM team_members WHERE account_id = ?1", params![account])?;
    for t in &d.disbanded {
        crate::keys::erase_projects(c, "SELECT id FROM projects WHERE team_id = ?1", t)?;
        c.execute("DELETE FROM projects WHERE team_id = ?1", params![t])?;
        c.execute("DELETE FROM team_members WHERE team_id = ?1", params![t])?;
    }
    c.execute(
        "DELETE FROM grants WHERE granted_by = ?1 OR (scope_kind = 'account' AND scope_id = ?1)
         OR (scope_kind = 'project' AND scope_id NOT IN (SELECT id FROM projects))",
        params![account],
    )?;
    for m in &d.machines {
        c.execute("DELETE FROM grants WHERE subject_kind = 'daemon' AND subject_id = ?1", params![m])?;
    }
    super::run_grants::erase_account(c, account)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        Db,
        sql::{Sql, Value, params},
    };
    use crate::Deleted;
    use rusqlite::Connection;

    /// A database as a build before the backfill's migration left it,
    /// with Cloud rows and a team channel that have no project.
    fn unattached() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::migrate::run_to(&mut conn, 2).unwrap();
        conn.execute_batch(
            "INSERT INTO cloud_workspaces (id, account, request_key, name, saved_spec, created)
                 VALUES ('cw_1', 'alice', 'k1', 'one', '{}', 1), ('cw_2', 'alice', 'k2', 'two', '{}', 2),
                        ('cw_3', 'bob', 'k3', 'three', '{}', 3);
             INSERT INTO cloud_secrets (account, name, sealed) VALUES ('alice', 'A', x'00'), ('dave', 'D', x'00');
             INSERT INTO chat_rooms (id, scope, kind, created_by, created)
                 VALUES ('c1', 'team:t1', 'channel', 'alice', 1), ('d1', 'team:t1', 'dm', 'alice', 1);",
        )
        .unwrap();
        conn
    }

    fn dump(conn: &impl Sql, sql: &str) -> Vec<Vec<Option<String>>> {
        let text = |v: Value| match v {
            Value::Null => None,
            Value::Int(i) => Some(i.to_string()),
            Value::Text(t) => Some(t),
            v => Some(format!("{v:?}")),
        };
        conn.query_map(sql, params![], |r| {
            let mut row = Vec::new();
            while let Ok(v) = r.get::<Value>(row.len()) {
                row.push(text(v));
            }
            Ok(row)
        })
        .unwrap()
    }

    fn count(conn: &impl Sql, sql: &str) -> u64 {
        conn.query_row(sql, params![], |r| r.get(0)).unwrap()
    }

    /// What the platform backfilled on its own (M129), now its last
    /// migration's: every account with Cloud rows gets its personal project
    /// and those rows get it. Team channels wait for their team's name from
    /// the hub (`projects::start`).
    #[test]
    fn the_backfill_gives_every_cloud_row_its_personal_project() {
        let mut conn = unattached();
        let ran = super::super::migrate::run(&mut conn).unwrap().ran;
        assert_eq!(ran[0], 3, "{ran:?}");
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
        assert_eq!(of("SELECT id, project_id FROM chat_rooms ORDER BY id"), [None, None], "not done here");

        // Again, as it ran on every start before: nothing changes.
        let all = "SELECT * FROM projects ORDER BY id";
        let before = dump(&conn, all);
        let backfill = super::super::migrate::Dialect::Sqlite.migrations()[2];
        assert_eq!(backfill.name, "personal_projects");
        conn.execute_batch(backfill.sql).unwrap();
        assert_eq!(dump(&conn, all), before);
        assert_eq!(super::super::migrate::run(&mut conn).unwrap(), Default::default());
    }

    #[test]
    fn a_project_belongs_to_one_owner_with_one_default() {
        let db = Db::test();
        let c = db.c();
        let insert = |id: &str, team: Option<&str>, account: Option<&str>, default: bool| {
            c.execute(
                "INSERT INTO projects (id, team_id, account_id, name, is_default, created_at) VALUES (?1, ?2, ?3, 'x', ?4, 0)",
                params![id, team, account, default],
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
        let db = Db::test();
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
        let db = Db::test();
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

    /// An agent's grant (M197): none until its owner gives one, one live
    /// per power and scope, listed and revoked by the owner who gave it,
    /// read live on every use, and gone with the machine.
    #[test]
    fn an_agent_grant_is_given_listed_revoked_and_dropped() {
        use super::{THREADS_POST, THREADS_READ};
        let db = Db::test();
        let team = db.team_project("t1", "one", 1).unwrap();
        let over = ("project", team.as_str());
        assert!(db.grant_on_join("geek", "alice", 1).unwrap());
        assert_eq!(db.live_grant("geek", THREADS_POST, over, "alice").unwrap(), None, "off by default");
        assert!(db.agent_grants_over(&[over]).unwrap().is_empty());
        let g = db.give_grant("geek", THREADS_POST, over, "alice", 10).unwrap();
        assert_eq!(
            (g.power.as_str(), g.scope_kind.as_str(), g.granted_by.as_str()),
            (THREADS_POST, "project", "alice")
        );
        assert_eq!(
            db.give_grant("geek", THREADS_POST, over, "alice", 11).unwrap().id,
            g.id,
            "a live one is the answer"
        );
        assert_eq!(db.live_grant("geek", THREADS_POST, over, "alice").unwrap().map(|g| g.id), Some(g.id.clone()));
        assert_eq!(db.live_grant("geek", THREADS_POST, over, "bob").unwrap(), None, "only as its giver gave it");
        assert_eq!(db.live_grant("geek", THREADS_READ, over, "alice").unwrap(), None, "one power at a time");
        let dms = db.give_grant("geek", THREADS_READ, ("account", "alice"), "alice", 12).unwrap();
        let listed: Vec<_> = db.grants("alice").unwrap().into_iter().map(|g| g.power).collect();
        assert_eq!(listed.len(), 3, "cloud.drive, and both agent grants: {listed:?}");
        assert!(db.grants("bob").unwrap().is_empty());
        assert_eq!(db.agent_grants_over(&[over, ("account", "alice")]).unwrap().len(), 2, "not cloud.drive");
        assert_eq!(db.revoke_grant("bob", &g.id, 20).unwrap(), None, "not bob's to revoke");
        assert!(db.revoke_grant("alice", &g.id, 20).unwrap().unwrap().revoked_at.is_some());
        assert_eq!(db.live_grant("geek", THREADS_POST, over, "alice").unwrap(), None, "the next use is refused");
        assert_eq!(db.agent_grants_over(&[over]).unwrap(), vec![]);
        let again = db.give_grant("geek", THREADS_POST, over, "alice", 30).unwrap();
        assert_ne!(again.id, g.id, "giving it back is a new grant");

        // geek isn't alice's any more: everything she gave it goes.
        db.drop_machine_grants("geek", "alice").unwrap();
        assert!(db.grants("alice").unwrap().is_empty(), "{:?}", db.grants("alice").unwrap());
        assert_eq!(db.live_grant("geek", THREADS_READ, ("account", "alice"), "alice").unwrap(), None);
        assert_ne!(dms.id, "");
        assert_eq!(
            db.project_owner(&team).unwrap().map(|o| (o.team, o.account, o.name)),
            Some((Some("t1".into()), None, "one".into()))
        );
        assert_eq!(db.project_owner("prj_none").unwrap(), None);
    }

    /// Deleting an account takes its personal project, its place in teams'
    /// projections, the disbanded teams' projects and projections, and
    /// grants to its machines, over it, or that it gave.
    #[test]
    fn deleting_an_account_takes_its_projects_and_grants() {
        use crate::hub::{Member, Team};
        use arugula_e2e::team::TeamRole;
        let db = Db::test();
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
