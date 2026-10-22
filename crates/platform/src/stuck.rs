//! What the operator sees and does when runs and deploys get stuck (M223,
//! platform step 7, native Arugula #19): `arugula-platform stuck`, which
//! docs/control-ops.md's "Runs and deploys" uses.
//!
//! **Seeing it.** [`Db::stuck`] lists, read-only:
//! - jobs a machine died holding: their lease ran out, and the next claim
//!   takes them over (nothing to do unless they stay);
//! - jobs claimed more than once in a row without handing themselves back
//!   (each time the step crashed, hung or failed);
//! - jobs that ended `failed` in the last week, with why;
//! - runs and deploys not ended that no job follows any more, which
//!   nothing will end without the operator;
//! - every deploy box not deleted, with its deploy's state and whether its
//!   job still follows it (one it doesn't is left over).
//!
//! **Unsticking it.** Each action is one the platform makes itself, and
//! only appends: no `run_events` or `deploy_events` row is changed.
//! - [`Db::operator_cancel_job`]: the job's handler sees the cancel at its
//!   next claim (a deploy's deletes its box; a run's ends, and its run
//!   fails with it, as `db::jobs` ends a job's work).
//! - [`Db::operator_fail_run`]: the run fails, by control, with the
//!   operator's reason; a deploy's run's job is made due, so the platform
//!   ends its deploy (failed if it started, paging its owners; canceled
//!   if not) and deletes its box, as when a box goes away.
//! - [`Db::operator_cancel_deploy`]: a deploy not yet running is canceled,
//!   by the platform, with the operator's reason; its job is made due, so
//!   its box is deleted. A running one ends with its run instead.
//! - [`Db::operator_delete_box`]: a deploy box is deleted by the hosted
//!   worker, at the provider too. Never a people's workspace.
//!
//! Nothing here approves anything, or records a deploy's start or end: only
//! an owner's device approves, and only a deploy's run ends it.

use anyhow::{Result, bail};
use serde_json::json;

use crate::db::{
    Db,
    deploys::{Actor, EventKind, State as DeployState},
    hosted::DEPLOY,
    jobs::{COLUMNS, JobRow, job_row},
    runs::{Move, State as RunState},
    sql::{Sql as _, params},
};
use crate::hosted::Ask;

/// How far back [`Stuck::failed`] looks.
pub const FAILED_WINDOW_MS: u64 = 7 * 86_400_000;

/// A run or a deploy not ended that no job follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unfollowed {
    pub id: String,
    pub project: String,
    pub state: String,
    pub created_at: u64,
    /// Its job's state, or `None` when it has none.
    pub job: Option<String>,
}

/// A deploy box not deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployBox {
    /// Its hosted workspace, `hw_…`: the box's `arugula_workspace`
    /// metadata at the provider.
    pub id: String,
    pub project: String,
    /// The deploy it was made for (its name).
    pub deploy: String,
    pub state: String,
    pub created_at: u64,
    /// Its deploy's state, or `None` when the deploy is gone.
    pub deploy_state: Option<String>,
    /// Its deploy's job's state, or `None` when it has none.
    pub job: Option<String>,
}

impl DeployBox {
    /// No job follows it, so nothing will delete it.
    pub fn left_over(&self) -> bool {
        self.job.as_deref() != Some("pending")
    }
}

/// What [`Db::stuck`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stuck {
    pub lapsed: Vec<JobRow>,
    pub crashing: Vec<JobRow>,
    pub failed: Vec<JobRow>,
    pub runs: Vec<Unfollowed>,
    pub deploys: Vec<Unfollowed>,
    pub boxes: Vec<DeployBox>,
}

impl Db {
    /// Runs and deploys that may be stuck at `now`, and the jobs that
    /// follow them, read-only.
    pub fn stuck(&self, now: u64) -> Result<Stuck> {
        let c = self.c();
        let jobs = |cond: &str, arg: u64| {
            c.query_map(
                &format!("SELECT {COLUMNS} FROM platform_jobs WHERE {cond} ORDER BY updated, id"),
                params![arg],
                job_row,
            )
        };
        let unfollowed = |sql: &str| {
            c.query_map(sql, params![], |r| {
                Ok(Unfollowed {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    state: r.get(2)?,
                    created_at: r.get(3)?,
                    job: r.get(4)?,
                })
            })
        };
        Ok(Stuck {
            lapsed: jobs("state = 'pending' AND lease_until > 0 AND lease_until <= ?1", now)?,
            crashing: jobs("state = 'pending' AND attempts >= ?1", 2)?,
            failed: jobs("state = 'failed' AND updated >= ?1", now.saturating_sub(FAILED_WINDOW_MS))?,
            runs: unfollowed(
                "SELECT r.id, r.project_id, r.state, r.created_at, j.state FROM runs r
                 LEFT JOIN platform_jobs j ON j.id = r.id
                 WHERE r.state IN ('pending', 'approved', 'running') AND (j.id IS NULL OR j.state <> 'pending')
                 ORDER BY r.created_at, r.id",
            )?,
            deploys: unfollowed(
                "SELECT d.id, e.project_id, d.state, d.created_at, j.state FROM deploys d
                 JOIN environments e ON e.id = d.environment_id
                 LEFT JOIN platform_jobs j ON j.id = d.id
                 WHERE d.state IN ('approved', 'running') AND (j.id IS NULL OR j.state <> 'pending')
                 ORDER BY d.created_at, d.id",
            )?,
            boxes: c.query_map(
                "SELECT w.id, w.project_id, w.name, w.state, w.created_at, d.state, j.state
                 FROM hosted_workspaces w
                 LEFT JOIN deploys d ON d.id = w.name
                 LEFT JOIN platform_jobs j ON j.id = w.name
                 WHERE w.purpose = 'deploy' AND w.state <> 'deleted'
                 ORDER BY w.created_at, w.id",
                params![],
                |r| {
                    Ok(DeployBox {
                        id: r.get(0)?,
                        project: r.get(1)?,
                        deploy: r.get(2)?,
                        state: r.get(3)?,
                        created_at: r.get(4)?,
                        deploy_state: r.get(5)?,
                        job: r.get(6)?,
                    })
                },
            )?,
        })
    }

    /// Make job `id` due at `now`, pending again if it had ended, with its
    /// failures in a row forgotten: unless a machine holds it now (its step
    /// sees what changed at its next).
    fn wake_job(&self, id: &str, now: u64) -> Result<()> {
        self.c().execute(
            "UPDATE platform_jobs SET state = 'pending', cancel = FALSE, attempts = 0, error = NULL,
                 next_at = ?2, updated = ?2
             WHERE id = ?1 AND lease_until <= ?2",
            params![id, now],
        )?;
        Ok(())
    }

    /// Ask job `id` to stop, as the operator: `false` when it has ended or
    /// isn't there.
    pub fn operator_cancel_job(&self, id: &str, now: u64) -> Result<bool> {
        self.cancel_job(id, now)
    }

    /// Fail run `id`, by control, with `reason`: refused when it has
    /// ended. A deploy's run: its deploy's job is made due, to end the
    /// deploy and delete its box.
    pub fn operator_fail_run(&self, id: &str, reason: &str, now: u64) -> Result<()> {
        let Some(run) = self.run(id)? else { bail!("no run {id}") };
        if run.state.terminal() {
            bail!("run {id} has ended: {}", run.state);
        }
        let detail = json!({ "reason": format!("operator: {reason}") }).to_string();
        if let Err(r) =
            self.move_run(id, &Move::Control { from: run.state, to: RunState::Failed, at: now, detail: &detail })?
        {
            bail!("run {id} wasn't failed: {r}");
        }
        if let Some(dr) = self.deploy_run_of(id)? {
            self.wake_job(&dr.deploy, now)?;
        }
        Ok(())
    }

    /// Cancel deploy `id`, not yet running, by the platform, with
    /// `reason`; its job is made due, to delete its box.
    pub fn operator_cancel_deploy(&self, id: &str, reason: &str, now: u64) -> Result<()> {
        let Some(d) = self.deploy(id)? else { bail!("no deploy {id}") };
        if d.state == DeployState::Running {
            bail!("deploy {id} is running: fail its run ({}) instead", d.run.as_deref().unwrap_or("none"));
        }
        let detail = json!({ "reason": format!("operator: {reason}") });
        if let Err(r) = self.record_deploy(id, EventKind::Canceled, &Actor::Platform, now, detail)? {
            bail!("deploy {id} wasn't canceled: {r}");
        }
        if self.platform_job(id)?.is_some() {
            self.wake_job(id, now)?;
        }
        Ok(())
    }

    /// Delete deploy box `id` (`hw_…`), by the hosted worker: refused for
    /// anything but a deploy's box.
    pub fn operator_delete_box(&self, id: &str, now: u64) -> Result<()> {
        let purpose: Option<String> =
            self.c().query_row("SELECT purpose FROM hosted_workspaces WHERE id = ?1", params![id], |r| r.get(0)).ok();
        match purpose.as_deref() {
            None => bail!("no hosted workspace {id}"),
            Some(DEPLOY) => {}
            Some(other) => bail!("{id} is a {other}, not a deploy's box: people delete their own workspaces"),
        }
        if !self.ask_hosted(id, Ask::Delete, now)? {
            bail!("{id} is deleted already");
        }
        Ok(())
    }
}

/// `t` (ms) for a person, as UTC.
fn when(t: u64) -> String {
    jiff::Timestamp::from_millisecond(t as i64).map_or_else(|_| t.to_string(), |t| t.strftime("%F %T UTC").to_string())
}

impl Stuck {
    /// For the operator's terminal.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut o = String::new();
        let job = |o: &mut String, j: &JobRow| {
            let _ = writeln!(
                o,
                "  {} {}  {}  attempts {}  updated {}{}",
                j.kind,
                j.id,
                j.state,
                j.attempts,
                when(j.updated),
                j.error.as_deref().map(|e| format!("  error: {e}")).unwrap_or_default()
            );
        };
        let _ =
            writeln!(o, "jobs whose machine died holding them (the next claim takes them over): {}", self.lapsed.len());
        self.lapsed.iter().for_each(|j| job(&mut o, j));
        let _ = writeln!(o, "jobs claimed again and again without finishing a step: {}", self.crashing.len());
        self.crashing.iter().for_each(|j| job(&mut o, j));
        let _ = writeln!(o, "jobs that failed in the last week: {}", self.failed.len());
        self.failed.iter().for_each(|j| job(&mut o, j));
        for (what, list) in [("runs", &self.runs), ("deploys", &self.deploys)] {
            let _ = writeln!(o, "{what} not ended that no job follows: {}", list.len());
            for u in list {
                let _ = writeln!(
                    o,
                    "  {}  {}  {}  made {}  job {}",
                    u.id,
                    u.project,
                    u.state,
                    when(u.created_at),
                    u.job.as_deref().unwrap_or("none")
                );
            }
        }
        let left = self.boxes.iter().filter(|b| b.left_over()).count();
        let _ = writeln!(o, "deploy boxes not deleted: {} ({left} left over)", self.boxes.len());
        for b in &self.boxes {
            let _ = writeln!(
                o,
                "  {}  {}  {}  for {} ({})  made {}  job {}{}",
                b.id,
                b.project,
                b.state,
                b.deploy,
                b.deploy_state.as_deref().unwrap_or("gone"),
                when(b.created_at),
                b.job.as_deref().unwrap_or("none"),
                if b.left_over() { "  LEFT OVER" } else { "" }
            );
        }
        o
    }
}
