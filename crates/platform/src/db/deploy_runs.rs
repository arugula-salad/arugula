//! Deploys' runs (M219, platform step 7, native Arugula #15): the run that
//! applies an approved deploy on the deploy's own box, made by
//! `0028_deploy_runs`.
//!
//! - **One run per deploy** ([`Db::make_deploy_run`]): the run (M210),
//!   its link here and its dispatch job (M212) in one transaction, the
//!   run's trigger key (`deploy:<id>`) making a second ask answer the
//!   first. Its operation is the driver's apply of the deploy
//!   (`arugula_e2e::run::Op`, `kind deploy`): which deploy, which driver,
//!   and the hash of the deploy's operation its owner's approval signs.
//! - **Only that run starts it.** The deploy's `started` is taken only from
//!   the run linked here (`db::deploys`), and only while the approval's
//!   `expires` hasn't passed by the run's own clock.
//! - **Approved by the deploy's approval.** The run is moved `approved`
//!   with [`deploy_approval`], made from the deploy's approval as recorded
//!   (which M218's verifier checked): never by anything an agent sends.
//! - **Secrets once** ([`Db::release_deploy_secrets`]): the environment's
//!   secrets go to the run's machine once, while the run runs.
//! - **Paged once** ([`Db::mark_deploy_paged`]) when its apply failed.

use anyhow::Result;
use serde_json::json;

use super::{
    Db,
    runs::{self, NewRun, Run, TriggerKind},
    sql::{Optional as _, Row, Sql, params},
};

/// A deploy's run, as linked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployRun {
    pub deploy: String,
    pub run: String,
    pub approval: String,
    pub workspace: String,
    pub driver: String,
    pub created_at: u64,
    /// When its machine was given the environment's secrets.
    pub secrets_at: Option<u64>,
    /// When the project's owners were told its apply failed.
    pub paged_at: Option<u64>,
}

const COLS: &str = "deploy_id, run_id, approval_id, workspace_id, driver, created_at, secrets_at, paged_at";

fn row(r: &Row) -> Result<DeployRun> {
    Ok(DeployRun {
        deploy: r.get(0)?,
        run: r.get(1)?,
        approval: r.get(2)?,
        workspace: r.get(3)?,
        driver: r.get(4)?,
        created_at: r.get(5)?,
        secrets_at: r.get(6)?,
        paged_at: r.get(7)?,
    })
}

/// What a deploy's run is made from.
pub struct NewDeployRun<'a> {
    pub project: &'a str,
    pub deploy: &'a str,
    /// The approval it applies (`apv_…`).
    pub approval: &'a str,
    /// The approver's account: who started it.
    pub account: &'a str,
    pub workspace: &'a str,
    pub driver: &'a str,
    /// The run's S37 operation (`kind deploy`) for the workspace's machine.
    pub op: &'a str,
}

/// The run's approval (`approved`), from the deploy's approval as it was
/// recorded: its id, and the device that signed it. Only the deploy's own
/// run gets one, for its own operation and machine.
pub fn deploy_approval(run: &Run, approval: &str, device: &str) -> runs::Approval {
    runs::Approval::of_deploy(approval, &run.daemon, &run.op_hash, device)
}

/// Whether `run` is `deploy`'s own run, in the caller's transaction.
pub(super) fn is_its_run(c: &impl Sql, deploy: &str, run: &str) -> Result<bool> {
    Ok(c.query_row("SELECT 1 FROM deploy_runs WHERE deploy_id = ?1 AND run_id = ?2", params![deploy, run], |r| {
        r.get::<i64>(0)
    })
    .optional()?
    .is_some())
}

impl Db {
    /// Make `new.deploy`'s run, `pending`, its link here and its dispatch
    /// job, in one transaction; or answer the one made before. `None`:
    /// there's no such project.
    pub fn make_deploy_run(&self, new: &NewDeployRun, at: u64) -> Result<Option<Run>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let source = json!({ "deploy": new.deploy, "approval": new.approval });
        let run = NewRun {
            project: new.project,
            workspace: Some(new.workspace),
            trigger: TriggerKind::Manual,
            source: &source,
            op: new.op,
            started_by: new.account,
            started_by_agent: None,
            grant: None,
        };
        let key = format!("deploy:{}", new.deploy);
        let Some((run, made)) = runs::create_run_in(&tx, &run, Some(&key), at)? else { return Ok(None) };
        if made {
            tx.execute(
                &format!("INSERT INTO deploy_runs ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)"),
                params![new.deploy, run.id, new.approval, new.workspace, new.driver, at],
            )?;
            runs::dispatch(&tx, &run.id, at)?;
        }
        tx.commit()?;
        Ok(Some(run))
    }

    /// A deploy's run, if it has one.
    pub fn deploy_run(&self, deploy: &str) -> Result<Option<DeployRun>> {
        self.c()
            .query_row(&format!("SELECT {COLS} FROM deploy_runs WHERE deploy_id = ?1"), params![deploy], row)
            .optional()
    }

    /// The deploy a run applies, if it's a deploy's run.
    pub fn deploy_run_of(&self, run: &str) -> Result<Option<DeployRun>> {
        self.c().query_row(&format!("SELECT {COLS} FROM deploy_runs WHERE run_id = ?1"), params![run], row).optional()
    }

    /// The run's machine is given its environment's secrets now: `true`
    /// once, for the first ask, and only while the run is `running` and its
    /// deploy is being applied by it, all checked in the one statement that
    /// marks them given; never again.
    pub fn release_deploy_secrets(&self, run: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE deploy_runs SET secrets_at = ?2
             WHERE run_id = ?1 AND secrets_at IS NULL
               AND EXISTS (SELECT 1 FROM runs r WHERE r.id = ?1 AND r.state = 'running')
               AND EXISTS (SELECT 1 FROM deploys d
                           WHERE d.id = deploy_runs.deploy_id AND d.state = 'running' AND d.run_id = ?1)",
            params![run, now],
        )? == 1)
    }

    /// The project's owners are told the deploy's apply failed: `true`
    /// once.
    pub fn mark_deploy_paged(&self, deploy: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE deploy_runs SET paged_at = ?2 WHERE deploy_id = ?1 AND paged_at IS NULL",
            params![deploy, now],
        )? == 1)
    }
}

impl Db {
    /// Link `run` to `deploy` as its run, with nothing else: tests that
    /// start a deploy without making a run.
    #[cfg(any(test, feature = "testing"))]
    pub fn link_deploy_run_for_tests(&self, deploy: &str, run: &str) {
        self.c()
            .execute(
                "INSERT INTO deploy_runs (deploy_id, run_id, approval_id, workspace_id, driver, created_at)
                 VALUES (?1, ?2, '', '', '', 0) ON CONFLICT DO NOTHING",
                params![deploy, run],
            )
            .unwrap();
    }
}

/// What deleting an account takes from here: the links of deploys that
/// went with its projects.
pub(crate) fn erase_account(c: &impl Sql) -> Result<()> {
    c.execute("DELETE FROM deploy_runs WHERE deploy_id NOT IN (SELECT id FROM deploys)", params![])?;
    Ok(())
}
