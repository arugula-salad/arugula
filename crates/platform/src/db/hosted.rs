//! Hosted workspaces' rows (M179): the workspaces, their secrets' names
//! and their ports, made by migration `0013`. The lifecycle is
//! [`crate::hosted`]'s; what each column means is in the migration.
//!
//! The worker leases a row as `cloud_jobs` are leased: a claim takes one
//! that's due and free (on Postgres `FOR UPDATE SKIP LOCKED`), and its
//! checkpoint frees it with when to look again. What a person asks
//! (`asked`) is written beside the lease and never overwritten by a
//! checkpoint, so a delete asked mid-create is done next.

use anyhow::{Result, bail};

use super::{
    Db, Taken,
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, params},
    taken,
};
use crate::{
    Deleted,
    hosted::{Ask, Spec},
};

/// A hosted workspace as the platform keeps it. Its box's sandbox id is
/// never in here: it's a bearer secret while the box's enrollment lasts,
/// so only the worker reads it ([`HostedJob`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HostedWorkspace {
    pub id: String,
    pub project: String,
    pub name: String,
    pub spec: Spec,
    pub provider: String,
    /// The template its box was made from.
    pub template: Option<String>,
    pub state: String,
    /// What was asked of it that the worker hasn't done yet.
    pub asked: Option<String>,
    /// The hosted machine its box enrolled as.
    pub machine: Option<String>,
    pub error: Option<String>,
    /// The spec changed since its box was built (M183 shows it).
    pub rebuild_needed: bool,
    pub created_by: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

const COLUMNS: &str = "id, project_id, name, spec, provider, template, state, asked, machine, error, \
     spec_hash, built_hash, created_by, created_at, updated_at";

fn row(r: &Row) -> Result<HostedWorkspace> {
    let spec: String = r.get(3)?;
    let built: Option<String> = r.get(11)?;
    Ok(HostedWorkspace {
        id: r.get(0)?,
        project: r.get(1)?,
        name: r.get(2)?,
        spec: serde_json::from_str(&spec)?,
        provider: r.get(4)?,
        template: r.get(5)?,
        state: r.get(6)?,
        asked: r.get(7)?,
        machine: r.get(8)?,
        error: r.get(9)?,
        rebuild_needed: built.is_some_and(|b| b != r.get::<String>(10).unwrap_or_default()),
        created_by: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

/// A workspace the worker holds: the row, and what only the worker reads
/// and writes.
#[derive(Debug, Clone)]
pub struct HostedJob {
    pub w: HostedWorkspace,
    pub spec_hash: String,
    pub built_hash: Option<String>,
    pub sandbox: Option<String>,
    /// The create's journal: ``, `sent` or `made`, since `stage_at`.
    pub stage: String,
    pub stage_at: Option<u64>,
    pub enroll_until: Option<u64>,
    pub deleted_at: Option<u64>,
    pub lease: String,
    /// What the step learned of the box from the provider (M188's usage).
    pub seen: crate::hosted::usage::Seen,
    /// Whether the box runs, for its checkpoint to meter (M188): set by the
    /// worker after a step that went through, never kept on the row.
    pub usage: Option<crate::hosted::usage::Observed>,
}

/// A port in a hosted workspace: its number, its label, who made it public
/// and until when.
pub type HostedPort = (u32, String, Option<String>, Option<u64>);

/// What [`Db::add_hosted_workspace`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Made(Box<HostedWorkspace>),
    /// The project has a live workspace by that name.
    NameTaken,
    /// The project has as many live workspaces as it may.
    Full,
}

/// How long a claim leases a workspace: what a machine that dies mid-step
/// costs before another takes it over.
pub const LEASE_MS: u64 = 120_000;

fn skip_locked(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

/// When a free row is due again: now, when something's asked (the worker
/// is woken by `lease_until` alone).
const DUE_NOW: &str = "lease_until = CASE WHEN lease IS NULL THEN 0 ELSE lease_until END";

impl Db {
    /// Keep a new workspace, `creating`, unless the project has one by that
    /// name or `max` live ones already.
    #[allow(clippy::too_many_arguments)]
    pub fn add_hosted_workspace(
        &self,
        id: &str,
        project: &str,
        name: &str,
        spec: &Spec,
        created_by: &str,
        max: u64,
        now: u64,
    ) -> Result<Added> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let live: u64 = tx.query_row(
            "SELECT count(*) FROM hosted_workspaces WHERE project_id = ?1 AND deleted_at IS NULL",
            params![project],
            |r| r.get(0),
        )?;
        if live >= max {
            return Ok(Added::Full);
        }
        let made = tx.execute(
            "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'creating', ?6, ?7, ?7)",
            params![id, project, name, serde_json::to_string(spec)?, spec.hash(), created_by, now],
        );
        match made.map_err(taken) {
            Err(e) if e.is::<Taken>() => return Ok(Added::NameTaken),
            r => r?,
        };
        let w = tx.query_row(&format!("SELECT {COLUMNS} FROM hosted_workspaces WHERE id = ?1"), params![id], row)?;
        tx.commit()?;
        Ok(Added::Made(Box::new(w)))
    }

    /// A project's live workspaces, by name.
    pub fn hosted_workspaces(&self, project: &str) -> Result<Vec<HostedWorkspace>> {
        self.c().query_map(
            &format!(
                "SELECT {COLUMNS} FROM hosted_workspaces WHERE project_id = ?1 AND deleted_at IS NULL ORDER BY name, id"
            ),
            params![project],
            row,
        )
    }

    /// A project's live workspace, by its id or its name.
    pub fn hosted_workspace(&self, project: &str, id_or_name: &str) -> Result<Option<HostedWorkspace>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM hosted_workspaces
                     WHERE project_id = ?1 AND (id = ?2 OR name = ?2) AND deleted_at IS NULL
                     ORDER BY CASE WHEN id = ?2 THEN 0 ELSE 1 END LIMIT 1"
                ),
                params![project, id_or_name],
                row,
            )
            .optional()
    }

    /// Ask the worker to do `what` (`hosted::ask`): whether it was taken.
    pub fn ask_hosted(&self, id: &str, what: Ask, now: u64) -> Result<bool> {
        let sql = match what {
            Ask::Delete => format!(
                "UPDATE hosted_workspaces SET asked = 'delete', state = 'deleting', updated_at = ?2, {DUE_NOW}
                 WHERE id = ?1 AND deleted_at IS NULL"
            ),
            Ask::Rebuild => format!(
                "UPDATE hosted_workspaces SET asked = 'rebuild', updated_at = ?2, {DUE_NOW}
                 WHERE id = ?1 AND deleted_at IS NULL AND COALESCE(asked, '') <> 'delete'
                     AND state NOT IN ('deleting', 'deleted')"
            ),
            // `ready` too: a box pauses itself at its timeout before the
            // worker sees it (M182 wakes on connect). A new wake's answer
            // is its own: what an earlier one said goes.
            Ask::Wake => format!(
                "UPDATE hosted_workspaces SET asked = 'wake', error = NULL, updated_at = ?2, {DUE_NOW}
                 WHERE id = ?1 AND deleted_at IS NULL AND asked IS NULL AND state IN ('paused', 'ready')"
            ),
            Ask::Pause => format!(
                "UPDATE hosted_workspaces SET asked = 'pause', updated_at = ?2, {DUE_NOW}
                 WHERE id = ?1 AND deleted_at IS NULL AND asked IS NULL AND state = 'ready'"
            ),
        };
        Ok(self.c().execute(&sql, params![id, now])? == 1)
    }

    /// Lease a workspace with something to do at `now`: one being made,
    /// enrolled or deleted, one with something asked, or a box whose state
    /// is due to be looked at. A failed one waits for a person.
    pub fn claim_hosted(&self, now: u64) -> Result<Option<HostedJob>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let found = tx
            .query_row(
                &format!(
                    "SELECT {COLUMNS}, sandbox_id, stage, stage_at, enroll_until, deleted_at FROM hosted_workspaces
                     WHERE deleted_at IS NULL AND lease_until <= ?1
                         AND (asked IS NOT NULL OR state <> 'failed')
                     ORDER BY lease_until, created_at LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| {
                    Ok(HostedJob {
                        w: row(r)?,
                        spec_hash: r.get(10)?,
                        built_hash: r.get(11)?,
                        sandbox: r.get(15)?,
                        stage: r.get(16)?,
                        stage_at: r.get(17)?,
                        enroll_until: r.get(18)?,
                        deleted_at: r.get(19)?,
                        lease: String::new(),
                        seen: Default::default(),
                        usage: None,
                    })
                },
            )
            .optional()?;
        let Some(mut job) = found else { return Ok(None) };
        let lease = hex::encode(arugula_e2e::random::<16>());
        let changed = tx.execute(
            "UPDATE hosted_workspaces SET lease = ?1, lease_until = ?2 WHERE id = ?3 AND lease_until <= ?4",
            params![lease, now + LEASE_MS, job.w.id, now],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        tx.commit()?;
        job.lease = lease;
        Ok(Some(job))
    }

    /// Write back what the worker did under its lease, and free the row
    /// until `next` (now, if something new was asked meanwhile: not the ask
    /// it claimed, which a step that failed for now tries again at `next`,
    /// not at once). `done` is
    /// the ask it carried out, cleared unless it was asked again as
    /// something else; a delete asked meanwhile keeps the state `deleting`.
    /// A workspace deleted for good loses its secrets, its ports and its
    /// thread (M196). What the step saw of its box is metered in the same
    /// transaction ([`crate::hosted::usage`]).
    pub fn checkpoint_hosted(&self, job: &HostedJob, done: Option<Ask>, next: u64, now: u64) -> Result<()> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let w = &job.w;
        let done = done.map(Ask::as_str);
        let changed = tx.execute(
            "UPDATE hosted_workspaces SET
                 state = CASE WHEN asked = 'delete' AND ?3 <> 'deleted' THEN 'deleting' ELSE ?3 END,
                 lease_until = CASE WHEN COALESCE(asked, '') NOT IN ('', COALESCE(?4, ''), COALESCE(?16, ''))
                     THEN CAST(?5 AS BIGINT)
                     ELSE CAST(?6 AS BIGINT) END,
                 asked = CASE WHEN asked = ?4 THEN NULL ELSE asked END,
                 lease = NULL, error = ?7, template = ?8, machine = ?9, sandbox_id = ?10, built_hash = ?11,
                 stage = ?12, stage_at = ?13, enroll_until = ?14, deleted_at = ?15, updated_at = ?5
             WHERE id = ?1 AND lease = ?2",
            params![
                w.id,
                job.lease,
                w.state,
                done,
                now,
                next,
                w.error,
                w.template,
                w.machine,
                job.sandbox,
                job.built_hash,
                job.stage,
                job.stage_at,
                job.enroll_until,
                job.deleted_at,
                w.asked,
            ],
        )?;
        if changed != 1 {
            bail!("hosted workspace lease lost");
        }
        if let Some(o) = &job.usage {
            super::hosted_usage::meter(&tx, &w.id, &w.project, o)?;
        }
        if job.deleted_at.is_some() {
            tx.execute("DELETE FROM hosted_workspace_secrets WHERE workspace_id = ?1", params![w.id])?;
            tx.execute("DELETE FROM hosted_ports WHERE workspace_id = ?1", params![w.id])?;
            super::objects::drop_workspace_thread(&tx, &w.id)?;
        }
        tx.commit()
    }

    /// Keep the create's journal under the lease, without freeing the row:
    /// written before the provider is asked, so a lost answer is found by
    /// its metadata rather than asked again.
    pub fn save_hosted(&self, job: &HostedJob, now: u64) -> Result<()> {
        let changed = self.c().execute(
            "UPDATE hosted_workspaces SET stage = ?3, stage_at = ?4, sandbox_id = ?5, template = ?6,
                 enroll_until = ?7, built_hash = ?8, updated_at = ?9
             WHERE id = ?1 AND lease = ?2",
            params![
                job.w.id,
                job.lease,
                job.stage,
                job.stage_at,
                job.sandbox,
                job.w.template,
                job.enroll_until,
                job.built_hash,
                now
            ],
        )?;
        if changed != 1 {
            bail!("hosted workspace lease lost");
        }
        Ok(())
    }

    /// The boxes [`crate::hosted::hold`] may hold: each workspace that's
    /// `ready` with a machine and a box, and nothing but a pause asked of
    /// it. Read by the leader alone, outside any lease: the sandbox id never
    /// leaves the platform.
    pub fn hosted_running(&self) -> Result<Vec<crate::hosted::hold::Running>> {
        self.c().query_map(
            "SELECT id, machine, sandbox_id FROM hosted_workspaces
             WHERE deleted_at IS NULL AND state = 'ready' AND machine IS NOT NULL AND sandbox_id IS NOT NULL
                 AND COALESCE(asked, 'pause') = 'pause'
             ORDER BY id",
            params![],
            |r| Ok(crate::hosted::hold::Running { workspace: r.get(0)?, machine: r.get(1)?, sandbox: r.get(2)? }),
        )
    }

    /// Keep a secret's name for a workspace, its value sealed here
    /// (`Some`, with `hosted::Config::cipher`) or in the provider's broker.
    pub fn set_hosted_secret(&self, workspace: &str, name: &str, sealed: Option<&[u8]>, now: u64) -> Result<()> {
        let store = if sealed.is_some() { "sealed" } else { "broker" };
        self.c().execute(
            "INSERT INTO hosted_workspace_secrets (workspace_id, name, store, sealed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (workspace_id, name) DO UPDATE SET store = excluded.store, sealed = excluded.sealed,
                 updated_at = excluded.updated_at",
            params![workspace, name, store, sealed, now],
        )?;
        Ok(())
    }

    /// A workspace's secrets: each name and where its value is.
    pub fn hosted_secret_names(&self, workspace: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT name, store FROM hosted_workspace_secrets WHERE workspace_id = ?1 ORDER BY name",
            params![workspace],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// A secret's sealed value, when it's sealed here.
    pub fn hosted_secret_sealed(&self, workspace: &str, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .c()
            .query_row(
                "SELECT sealed FROM hosted_workspace_secrets WHERE workspace_id = ?1 AND name = ?2",
                params![workspace, name],
                |r| r.get::<Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn remove_hosted_secret(&self, workspace: &str, name: &str) -> Result<bool> {
        Ok(self.c().execute(
            "DELETE FROM hosted_workspace_secrets WHERE workspace_id = ?1 AND name = ?2",
            params![workspace, name],
        )? == 1)
    }

    /// Note a port a dev server listens on in a workspace (M186).
    pub fn set_hosted_port(&self, workspace: &str, port: u16, label: &str, now: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO hosted_ports (workspace_id, port, label, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (workspace_id, port) DO UPDATE SET label = excluded.label",
            params![workspace, u32::from(port), label, now],
        )?;
        Ok(())
    }

    /// A workspace's ports: each port, its label, who made it public and
    /// until when.
    pub fn hosted_ports(&self, workspace: &str) -> Result<Vec<HostedPort>> {
        self.c().query_map(
            "SELECT port, label, public_by, public_until FROM hosted_ports WHERE workspace_id = ?1 ORDER BY port",
            params![workspace],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
    }
}

/// An account is gone: the workspaces of the projects that go with it (its
/// personal one and its disbanded teams') are deleted by the worker, which
/// deletes their boxes, and no workspace names it as its maker any more.
/// Runs before `projects::erase_account`, which drops those projects.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted, now: u64) -> Result<()> {
    let mut owners = vec![format!("account:{}", d.account)];
    owners.extend(d.disbanded.iter().map(|t| format!("team:{t}")));
    for o in &owners {
        let (column, id) = o.split_once(':').unwrap_or_default();
        let column = if column == "team" { "team_id" } else { "account_id" };
        c.execute(
            &format!(
                "UPDATE hosted_workspaces SET asked = 'delete', state = 'deleting', updated_at = ?2, {DUE_NOW}
                 WHERE deleted_at IS NULL AND project_id IN (SELECT id FROM projects WHERE {column} = ?1)"
            ),
            params![id, now],
        )?;
    }
    c.execute("UPDATE hosted_workspaces SET created_by = NULL WHERE created_by = ?1", params![d.account])?;
    c.execute("UPDATE hosted_ports SET public_by = NULL WHERE public_by = ?1", params![d.account])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosted::Ask;

    fn spec() -> Spec {
        Spec { repos: vec!["https://github.com/acme/app".into()], setup: "make".into(), ..Default::default() }
    }

    fn made(a: Added) -> HostedWorkspace {
        match a {
            Added::Made(w) => *w,
            other => panic!("{other:?}"),
        }
    }

    /// Names are one per live workspace in a project, a project holds a
    /// few at most, and a deleted one frees its name.
    #[test]
    fn workspaces_are_kept_per_project() {
        let db = Db::test();
        let w = made(db.add_hosted_workspace("hw_1", "prj_a", "dev", &spec(), "alice", 2, 10).unwrap());
        assert_eq!((w.state.as_str(), w.rebuild_needed, w.created_at), ("creating", false, 10));
        assert_eq!(w.spec, spec());
        assert_eq!(db.add_hosted_workspace("hw_2", "prj_a", "dev", &spec(), "alice", 2, 10).unwrap(), Added::NameTaken);
        made(db.add_hosted_workspace("hw_3", "prj_b", "dev", &spec(), "alice", 2, 10).unwrap());
        made(db.add_hosted_workspace("hw_4", "prj_a", "two", &spec(), "alice", 2, 10).unwrap());
        assert_eq!(db.add_hosted_workspace("hw_5", "prj_a", "three", &spec(), "alice", 2, 10).unwrap(), Added::Full);
        assert_eq!(
            db.hosted_workspaces("prj_a").unwrap().iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            ["dev", "two"]
        );
        assert_eq!(db.hosted_workspace("prj_a", "dev").unwrap().unwrap().id, "hw_1");
        assert_eq!(db.hosted_workspace("prj_a", "hw_4").unwrap().unwrap().name, "two");
        assert!(db.hosted_workspace("prj_b", "hw_1").unwrap().is_none(), "another project's");

        // Deleted for good: its name is free.
        let mut job = db.claim_hosted(20).unwrap().unwrap();
        assert_eq!(job.w.id, "hw_1");
        job.w.state = "deleted".into();
        job.deleted_at = Some(20);
        db.checkpoint_hosted(&job, Some(Ask::Delete), 20, 20).unwrap();
        assert!(db.hosted_workspace("prj_a", "dev").unwrap().is_none());
        made(db.add_hosted_workspace("hw_6", "prj_a", "dev", &spec(), "alice", 2, 30).unwrap());
    }

    /// A claim leases one row at a time; a checkpoint frees it until it's
    /// due, or now when something was asked meanwhile, and a delete asked
    /// mid-step outlasts the step.
    #[test]
    fn the_worker_leases_and_asks_outlast_a_step() {
        let db = Db::test();
        made(db.add_hosted_workspace("hw_1", "prj_a", "dev", &spec(), "alice", 5, 0).unwrap());
        let mut job = db.claim_hosted(100).unwrap().unwrap();
        assert!(db.claim_hosted(100).unwrap().is_none(), "held");
        assert!(db.claim_hosted(100 + LEASE_MS).unwrap().is_some(), "a lease runs out");
        assert!(db.checkpoint_hosted(&job, None, 0, 100).is_err(), "its lease was taken over");

        job = db.claim_hosted(100 + 2 * LEASE_MS).unwrap().unwrap();
        // Asked mid-create: the step's checkpoint keeps it.
        assert!(db.ask_hosted("hw_1", Ask::Delete, 200).unwrap());
        assert!(!db.ask_hosted("hw_1", Ask::Rebuild, 200).unwrap(), "a delete is asked");
        job.w.state = "enrolling".into();
        job.sandbox = Some("sbx".into());
        job.stage = "made".into();
        db.checkpoint_hosted(&job, None, 10_000_000, 300).unwrap();
        let w = db.hosted_workspace("prj_a", "hw_1").unwrap().unwrap();
        assert_eq!((w.state.as_str(), w.asked.as_deref()), ("deleting", Some("delete")));
        let job = db.claim_hosted(300).unwrap().expect("due now: something's asked");
        assert_eq!(job.sandbox.as_deref(), Some("sbx"));
        assert_eq!(job.stage, "made");
    }

    /// A step that fails for now leaves the ask it claimed asked, and is
    /// tried again when it said, not at once; one asked meanwhile is due
    /// at once.
    #[test]
    fn a_failed_step_waits_to_try_its_ask_again() {
        let db = Db::test();
        made(db.add_hosted_workspace("hw_1", "prj_a", "dev", &spec(), "alice", 5, 0).unwrap());
        let mut job = db.claim_hosted(0).unwrap().unwrap();
        job.w.state = "paused".into();
        db.checkpoint_hosted(&job, None, 1_000, 0).unwrap();
        assert!(db.ask_hosted("hw_1", Ask::Wake, 10).unwrap());
        let job = db.claim_hosted(10).unwrap().expect("due now: a wake's asked");
        assert_eq!(job.w.asked.as_deref(), Some("wake"));
        // The provider didn't answer: tried again in 5 s, still asked.
        db.checkpoint_hosted(&job, None, 5_010, 20).unwrap();
        assert!(db.claim_hosted(20).unwrap().is_none(), "not at once");
        assert!(db.claim_hosted(5_009).unwrap().is_none());
        let job = db.claim_hosted(5_010).unwrap().expect("when it said");
        assert_eq!(job.w.asked.as_deref(), Some("wake"));
        // Something else asked mid-step: due at once.
        assert!(db.ask_hosted("hw_1", Ask::Delete, 5_020).unwrap());
        db.checkpoint_hosted(&job, None, 10_000, 5_030).unwrap();
        assert!(db.claim_hosted(5_030).unwrap().is_some(), "a delete asked meanwhile");
    }

    /// Waking and pausing are asked only of a box in the right state with
    /// nothing else asked; a done ask is cleared, and the spec changing
    /// shows as "rebuild needed".
    #[test]
    fn asks_follow_the_state() {
        let db = Db::test();
        made(db.add_hosted_workspace("hw_1", "prj_a", "dev", &spec(), "alice", 5, 0).unwrap());
        assert!(!db.ask_hosted("hw_1", Ask::Wake, 1).unwrap(), "not made yet");
        assert!(!db.ask_hosted("hw_1", Ask::Pause, 1).unwrap(), "not running");
        let mut job = db.claim_hosted(1).unwrap().unwrap();
        job.w.state = "paused".into();
        job.built_hash = Some(job.spec_hash.clone());
        db.checkpoint_hosted(&job, None, 1_000, 1).unwrap();
        assert!(db.ask_hosted("hw_1", Ask::Wake, 2).unwrap());
        assert!(!db.ask_hosted("hw_1", Ask::Wake, 2).unwrap(), "asked already");
        let mut job = db.claim_hosted(2).unwrap().expect("due now");
        job.w.state = "ready".into();
        db.checkpoint_hosted(&job, Some(Ask::Wake), 1_000, 3).unwrap();
        let w = db.hosted_workspace("prj_a", "dev").unwrap().unwrap();
        assert_eq!((w.state.as_str(), w.asked.as_deref(), w.rebuild_needed), ("ready", None, false));
        assert!(db.ask_hosted("hw_1", Ask::Pause, 4).unwrap());

        db.c().execute("UPDATE hosted_workspaces SET spec_hash = 'other' WHERE id = 'hw_1'", params![]).unwrap();
        assert!(db.hosted_workspace("prj_a", "dev").unwrap().unwrap().rebuild_needed);
    }

    /// Secrets are names, with a value sealed here or in the broker; ports
    /// are listed by number; both go with a workspace deleted for good.
    #[test]
    fn secrets_and_ports() {
        let db = Db::test();
        made(db.add_hosted_workspace("hw_1", "prj_a", "dev", &spec(), "alice", 5, 0).unwrap());
        db.set_hosted_secret("hw_1", "NPM_TOKEN", Some(&[1, 2, 3]), 1).unwrap();
        db.set_hosted_secret("hw_1", "ANTHROPIC_KEY", None, 1).unwrap();
        assert_eq!(
            db.hosted_secret_names("hw_1").unwrap(),
            [("ANTHROPIC_KEY".to_owned(), "broker".to_owned()), ("NPM_TOKEN".to_owned(), "sealed".to_owned())]
        );
        assert_eq!(db.hosted_secret_sealed("hw_1", "NPM_TOKEN").unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(db.hosted_secret_sealed("hw_1", "ANTHROPIC_KEY").unwrap(), None);
        assert!(db.remove_hosted_secret("hw_1", "ANTHROPIC_KEY").unwrap());
        db.set_hosted_port("hw_1", 3000, "web", 1).unwrap();
        db.set_hosted_port("hw_1", 3000, "app", 2).unwrap();
        db.set_hosted_port("hw_1", 80, "", 2).unwrap();
        assert_eq!(
            db.hosted_ports("hw_1").unwrap(),
            [(80, String::new(), None, None), (3000, "app".into(), None, None)]
        );

        let mut job = db.claim_hosted(5).unwrap().unwrap();
        job.w.state = "deleted".into();
        job.deleted_at = Some(5);
        db.checkpoint_hosted(&job, Some(Ask::Delete), 5, 5).unwrap();
        assert!(db.hosted_secret_names("hw_1").unwrap().is_empty());
        assert!(db.hosted_ports("hw_1").unwrap().is_empty());
    }

    /// An account that goes takes its projects' workspaces with it (the
    /// worker deletes their boxes), and nothing names it after.
    #[test]
    fn an_account_that_goes_takes_its_workspaces() {
        let db = Db::test();
        let personal = db.personal_project("alice", 0).unwrap();
        let team = db.team_project("t1", "acme", 0).unwrap();
        made(db.add_hosted_workspace("hw_1", &personal, "mine", &spec(), "alice", 5, 0).unwrap());
        made(db.add_hosted_workspace("hw_2", &team, "team", &spec(), "alice", 5, 0).unwrap());
        let d = Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        erase_account(&tx, &d, 9).unwrap();
        crate::db::projects::erase_account(&tx, &d).unwrap();
        tx.commit().unwrap();
        drop(c);
        let state = |id: &str| -> (String, Option<String>) {
            db.c()
                .query_row("SELECT state, created_by FROM hosted_workspaces WHERE id = ?1", params![id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap()
        };
        assert_eq!(state("hw_1"), ("deleting".into(), None));
        assert_eq!(state("hw_2"), ("creating".into(), None), "the team's stays, without its maker");
        assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
    }
}
