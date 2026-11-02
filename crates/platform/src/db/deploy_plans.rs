//! A chant→Fly deploy's plan (#1985), made by `0104_deploy_plans`: run on
//! a box of its own when its approval is first asked for, before anyone
//! approves it, so the owner signs chant's own plan (aru-126).
//!
//! - **One per deploy** ([`Db::ask_deploy_plan`]): the row and its job
//!   (`deploy_plan`, keyed `plan:<deploy>`) in one transaction. The job
//!   makes the box, then the plan run ([`Db::make_plan_run`]), follows it,
//!   and records what it found ([`Db::end_deploy_plan`]).
//! - **Its run** is the driver's plan of the deploy (`kind deploy`, `plan`
//!   in place of `apply`), approved by the platform ([`plan_approval`]):
//!   there's nothing of a person's to approve yet, and the run holds no
//!   secret of the environment's.
//! - **One token, once** ([`Db::release_plan_secrets`]): a GitHub token
//!   minted to push chant's ledger, given while the run runs.
//! - **Its end**: the chant gate the plan stopped at, which the asker's
//!   `requested` event then carries, or why it didn't plan.

use anyhow::Result;
use serde_json::json;

use super::{
    Db,
    runs::{self, NewRun, Run, TriggerKind},
    sql::{Optional as _, Row, Sql, params},
};

/// A deploy's plan, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployPlan {
    pub deploy: String,
    /// Who asked for its approval, as `db::deploys::Actor` writes one.
    pub asked_by: String,
    pub asked_at: u64,
    pub run: Option<String>,
    pub workspace: Option<String>,
    pub secrets_at: Option<u64>,
    /// The chant gate the plan stopped at, once it planned.
    pub gate: Option<arugula_e2e::deploy::chant::Gate>,
    /// Why it didn't plan.
    pub failed: Option<String>,
    pub ended_at: Option<u64>,
}

impl DeployPlan {
    /// Neither planned nor failed yet.
    pub fn planning(&self) -> bool {
        self.ended_at.is_none()
    }
}

const COLS: &str = "deploy_id, asked_by, asked_at, run_id, workspace_id, secrets_at, gate, failed, ended_at";

fn row(r: &Row) -> Result<DeployPlan> {
    let gate: Option<String> = r.get(6)?;
    Ok(DeployPlan {
        deploy: r.get(0)?,
        asked_by: r.get(1)?,
        asked_at: r.get(2)?,
        run: r.get(3)?,
        workspace: r.get(4)?,
        secrets_at: r.get(5)?,
        gate: gate.and_then(|g| serde_json::from_str(&g).ok()),
        failed: r.get(7)?,
        ended_at: r.get(8)?,
    })
}

/// The plan job's id for `deploy`.
pub fn job_id(deploy: &str) -> String {
    format!("plan:{deploy}")
}

/// The name of `deploy`'s plan box (a deploy's own box is named after it).
pub fn box_name(deploy: &str) -> String {
    format!("{deploy}-plan")
}

/// The deploy a plan job's id names.
pub fn deploy_of_job(id: &str) -> Option<&str> {
    id.strip_prefix("plan:")
}

/// What a plan run is made from.
pub struct NewPlanRun<'a> {
    pub project: &'a str,
    pub deploy: &'a str,
    /// Who asked: who started it.
    pub account: &'a str,
    pub workspace: &'a str,
    /// The run's S37 operation (`kind deploy`, `plan`).
    pub op: &'a str,
}

/// The plan run's approval: the platform's, for the deploy's plan. Only
/// the deploy's own plan run gets one, for its own operation and machine.
pub fn plan_approval(run: &Run, deploy: &str) -> runs::Approval {
    runs::Approval::of_deploy(&job_id(deploy), &run.daemon, &run.op_hash, "platform")
}

impl Db {
    /// Plan `deploy` (its approval asked for by `by` at `at`): the row and
    /// its job, once. `false` when it was asked for before.
    pub fn ask_deploy_plan(&self, deploy: &str, by: &str, at: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let made = tx.execute(
            "INSERT INTO deploy_plans (deploy_id, asked_by, asked_at) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
            params![deploy, by, at],
        )? == 1;
        if made {
            super::jobs::enqueue(
                &tx,
                &job_id(deploy),
                super::jobs::JobKind::DeployPlan,
                &json!({ "deploy": deploy }),
                at,
                at,
            )?;
        }
        tx.commit()?;
        Ok(made)
    }

    /// `deploy`'s plan, if it was asked for.
    pub fn deploy_plan(&self, deploy: &str) -> Result<Option<DeployPlan>> {
        self.c()
            .query_row(&format!("SELECT {COLS} FROM deploy_plans WHERE deploy_id = ?1"), params![deploy], row)
            .optional()
    }

    /// The plan `run` runs, if it's a plan run.
    pub fn deploy_plan_of_run(&self, run: &str) -> Result<Option<DeployPlan>> {
        self.c().query_row(&format!("SELECT {COLS} FROM deploy_plans WHERE run_id = ?1"), params![run], row).optional()
    }

    /// Keep the box the plan runs on, once.
    pub fn set_plan_box(&self, deploy: &str, workspace: &str) -> Result<()> {
        self.c().execute(
            "UPDATE deploy_plans SET workspace_id = ?2 WHERE deploy_id = ?1 AND workspace_id IS NULL",
            params![deploy, workspace],
        )?;
        Ok(())
    }

    /// Make the plan's run, `pending`, linked here, and its dispatch job,
    /// in one transaction; or answer the one made before. `None`: there's
    /// no such project.
    pub fn make_plan_run(&self, new: &NewPlanRun, at: u64) -> Result<Option<Run>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let source = json!({ "deploy": new.deploy, "plan": true });
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
        let key = job_id(new.deploy);
        let Some((run, made)) = runs::create_run_in(&tx, &run, Some(&key), at)? else { return Ok(None) };
        if made {
            tx.execute(
                "UPDATE deploy_plans SET run_id = ?2, workspace_id = ?3 WHERE deploy_id = ?1",
                params![new.deploy, run.id, new.workspace],
            )?;
            runs::dispatch(&tx, &run.id, at)?;
        }
        tx.commit()?;
        Ok(Some(run))
    }

    /// The plan run's machine is given its token now: `true` once, for the
    /// first ask, and only while the run is `running` and its plan hasn't
    /// ended, checked in the one statement that marks it given.
    pub fn release_plan_secrets(&self, run: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE deploy_plans SET secrets_at = ?2
             WHERE run_id = ?1 AND secrets_at IS NULL AND ended_at IS NULL
               AND EXISTS (SELECT 1 FROM runs r WHERE r.id = ?1 AND r.state = 'running')",
            params![run, now],
        )? == 1)
    }

    /// The plan ended: chant's gate, or why not. Once; `true` for the call
    /// that ended it.
    pub fn end_deploy_plan(
        &self,
        deploy: &str,
        gate: Result<&arugula_e2e::deploy::chant::Gate, &str>,
        now: u64,
    ) -> Result<bool> {
        let (gate, failed) = match gate {
            Ok(g) => (Some(serde_json::to_string(g)?), None),
            Err(why) => (None, Some(why.to_owned())),
        };
        Ok(self.c().execute(
            "UPDATE deploy_plans SET gate = ?2, failed = ?3, ended_at = ?4 WHERE deploy_id = ?1 AND ended_at IS NULL",
            params![deploy, gate, failed, now],
        )? == 1)
    }
}

/// Plans whose deploys are gone go with them.
pub(crate) fn erase_account(c: &impl Sql) -> Result<()> {
    c.execute("DELETE FROM deploy_plans WHERE deploy_id NOT IN (SELECT id FROM deploys)", params![])?;
    Ok(())
}
