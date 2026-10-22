//! Applying an approved deploy (M219, platform step 7, native Arugula #15;
//! aru-102): a run on a box of its own, holding the environment's
//! credentials for that run only.
//!
//! - **The approval enqueues it.** Recording an owner's approval queues
//!   the deploy's job (`deploy` on `platform_jobs`, in the approval's own
//!   transaction). The job ([`Apply`]) checks again before anything runs:
//!   the approval's `expires` (else the deploy `expired`), the approving
//!   device (still one of the owner's browsers, not removed) and the
//!   owner's role now, by the hub's roster (else `canceled`, saying why).
//! - **A fresh box per deploy** (Jake, 2026-10-22). The job makes a hosted
//!   workspace of purpose `deploy` for the project (named after the deploy,
//!   with nothing to set up, its id kept in the job's checkpoint before its
//!   row is made). M179's worker makes its box from the release's template
//!   and enrolls it; control's enrollment tells the box it's a deploy's,
//!   so it lets nobody in, takes its one deploy run, gives up root first,
//!   and stops after. People never list it, open it or count it against
//!   their limit; M188 meters it apart. Once its daemon enrolled, the job
//!   makes the deploy's run there ([`Db::make_deploy_run`]), whose
//!   operation is the driver's apply of this deploy (`kind deploy`), and
//!   approves the run with the deploy's approval. M212's dispatch carries it
//!   to the box. A deploy never applies on a people's workspace.
//! - **The box checks it too.** The work it pulls carries the deploy's
//!   operation and the approval as signed ([`deploy_work`]); the box
//!   checks them against the owners it pinned before it starts
//!   (`arugula_e2e::deploy::check_apply`), and that the plan is its
//!   driver's, and refuses a run whose approval doesn't verify there.
//! - **Started only by its run.** The box's signed `running` report starts
//!   the deploy ([`reported`]): the log takes `started` only from the run
//!   made for it, before the approval's `expires` by the run's own clock
//!   (and the platform's, within [`START_SLACK_MS`]), and not when it's
//!   stale or another deploy is being applied. Refused, the run is failed
//!   and the box told so before it asks for secrets.
//! - **Secrets for that run only** ([`secrets`]): the box asks once, while
//!   its run runs and its deploy is being applied by it; the environment's
//!   secrets are opened from their seals then (M209), with the driver's
//!   short-lived tokens minted then ([`crate::drivers::Mint`]), and never
//!   given again.
//! - **Ended by its run.** The run's end ends the deploy: `succeeded` once
//!   the driver's check passes ([`crate::drivers::Driver::verify`], over
//!   what the box reported its apply did, M220: its log keeps what the
//!   driver takes of it, and the run links chant's ledger entry), else
//!   `failed`, which pages its project's owners with the run's log
//!   ([`page`]), once. A run that ended before it started cancels the
//!   deploy, with no `started`; when its box refused it (the approval
//!   didn't verify there: something forged or stale got past the
//!   platform), the owners are paged too, once.
//! - **The box goes after,** whatever happened: the run's end, a refusal, a
//!   box that never came up ([`BOX_UP_MS`]), or one that ran past its
//!   longest life ([`BOX_LIFE_MS`]). The job, its box in its checkpoint, is
//!   taken up by any of the platform's machines after a crash, and follows
//!   the delete to its end.
//!
//! The job follows the deploy until it ends, as a backstop for a report
//! the platform took but didn't finish handling (a restart between).

use std::sync::Arc;

use anyhow::{Context as _, Result};
use arugula_control_wire::work::{DeployWork, PasskeyAssertion, Reported, Secret, SecretsAsk, SecretsGiven};
use arugula_e2e::{
    run::{DeployApply, Op},
    team::TeamRole,
};
use axum::http::StatusCode;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        Db,
        deploy_approvals::StoredApproval,
        deploy_runs::{NewDeployRun, deploy_approval},
        deploys::{Actor, Deploy, EventKind, Refusal, State},
        issues::IssueProject,
        jobs::{JobKind, LeasedJob},
        runs::{Move, Run, State as RunState},
    },
    drivers::{self, Verdict},
    hub::PushOptions,
    jobs::{Clock, Handler, Step},
    work::HostedMachine,
};

/// How often the job looks again while its run waits or runs.
pub const FOLLOW_MS: u64 = 5_000;
/// How often it looks again while its run applies it.
pub const RUNNING_MS: u64 = 30_000;
/// How often it looks at its box while it's made, enrolls, or is deleted.
pub const BOX_MS: u64 = 2_000;
/// How long a deploy's box has to come up (made and enrolled).
pub const BOX_UP_MS: u64 = 10 * 60_000;
/// A deploy's box's longest life: a run still going then is failed, and
/// the box deleted.
pub const BOX_LIFE_MS: u64 = 2 * 60 * 60_000;
/// How far past an approval's `expires` the platform's own clock may be
/// when a deploy's run starts it (the box's signed time must be within it).
pub const START_SLACK_MS: u64 = 60_000;
/// How long it waits to ask the hub again when the hub didn't answer.
pub const HUB_RETRY_MS: u64 = 30_000;

/// The `deploy` kind's handler.
pub struct Apply {
    pub app: Arc<Platform>,
}

impl Handler for Apply {
    fn kind(&self) -> JobKind {
        JobKind::Deploy
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(step(&self.app, db, job, clock.now()))
    }

    /// Never given up on: a deploy's job is what deletes its box, so a step
    /// that keeps failing is tried again (backing off), never dropped. The
    /// reaper ([`reap`]) is the backstop.
    fn max_attempts(&self) -> u32 {
        u32::MAX
    }
}

/// How long a deploy's box may live at most, all told: coming up, its run,
/// and its log. One older is the reaper's.
pub const BOX_MAX_MS: u64 = BOX_UP_MS + BOX_LIFE_MS + LOG_WAIT_MS;
/// How often the reaper looks.
pub const REAP_EVERY: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Delete every deploy's box older than [`BOX_MAX_MS`] at `now` that isn't
/// being deleted: a backstop for a job that lost track of its box. How many.
pub fn reap(app: &Platform, now: u64) -> Result<usize> {
    let old = app.db.old_deploy_boxes(now.saturating_sub(BOX_MAX_MS))?;
    for b in &old {
        warn!(workspace = b, "deleting a deploy's box past its longest life");
        crate::hosted::ask(app, b, crate::hosted::Ask::Delete)?;
    }
    Ok(old.len())
}

/// The reaper, every [`REAP_EVERY`] on each of the platform's machines (an
/// ask to delete is the same however often it's made).
pub fn start_reaper(app: Arc<Platform>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(REAP_EVERY).await;
            if app.is_paused() || app.compute.is_none() {
                continue;
            }
            if let Err(e) = reap(&app, arugula_e2e::now_ms()) {
                warn!(error = format!("{e:#}"), "the deploy-box reaper failed");
            }
        }
    });
}

/// The approval a deploy's apply goes by: its latest `once`.
fn approval_of(db: &Db, deploy: &str) -> Result<Option<StoredApproval>> {
    Ok(db.deploy_approvals(deploy)?.into_iter().rev().find(|a| a.decision == arugula_e2e::deploy::Decision::Once))
}

/// Whether the approval still holds at apply time: its device is one of
/// the approver's browsers, not removed, and the approver an owner of the
/// project now (for a team's, made one by a browser). `Err(None)`: the hub
/// didn't answer, so ask again later.
async fn recheck(app: &Platform, a: &StoredApproval, p: &IssueProject) -> Result<(), Option<String>> {
    let why = |e: arugula_control_common::ApiError| {
        if matches!(e.0, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) { Some(e.1) } else { None }
    };
    super::approving_device(app, &a.account, &a.device).await.map_err(why)?;
    if super::role(app, &a.account, p, true).await.map_err(why)? != Some(TeamRole::Owner) {
        return Err(Some("the approver isn't an owner of the project now".into()));
    }
    if let (None, Some(team)) = (&p.account, &p.team) {
        super::owner_made_by_a_browser(app, team, &a.account).await.map_err(why)?;
    }
    Ok(())
}

/// End a deploy whose run ended before it started, as the platform, saying
/// why, without paging anyone: nothing was applied.
fn cancel_quietly(app: &Platform, d: &Deploy, why: &str, now: u64) -> Result<()> {
    match app.db.record_deploy(&d.id, EventKind::Canceled, &Actor::Platform, now, json!({ "reason": why }))? {
        Ok(_) => info!(deploy = d.id, why, "a deploy's run ended before it applied it"),
        Err(r) => info!(deploy = d.id, refused = %r, "a deploy's apply couldn't end it"),
    }
    Ok(())
}

/// End a deploy that won't be applied, as the platform, saying why: and
/// tell its owners when it had a run.
async fn cancel(app: &Platform, d: &Deploy, kind: EventKind, why: &str, now: u64) -> Result<()> {
    match app.db.record_deploy(&d.id, kind, &Actor::Platform, now, json!({ "reason": why }))? {
        Ok(_) => {
            info!(deploy = d.id, why, "a deploy won't be applied");
            if let Some(dr) = app.db.deploy_run(&d.id)? {
                page(app, &d.id, &dr.run, why).await;
            }
        }
        Err(r) => info!(deploy = d.id, refused = %r, "a deploy's apply couldn't end it"),
    }
    Ok(())
}

/// What a deploy's job keeps (its checkpoint): its box, once it's chosen.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The deploy's own box (`hw_…`, a hosted workspace of purpose
    /// `deploy`): named here before its row is made, so a crash never
    /// leaves a box nobody deletes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<String>,
    /// When it was asked for.
    #[serde(default)]
    pub made_at: u64,
}

fn checkpoint_of(job: &LeasedJob) -> Checkpoint {
    job.checkpoint.as_ref().and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default()
}

fn save(db: &Db, job: &mut LeasedJob, cp: &Checkpoint, now: u64) -> Result<()> {
    job.checkpoint = Some(serde_json::to_value(cp)?);
    db.checkpoint_job(job, now)
}

/// How long a deploy's box is kept after its run ended for the run's last
/// log chunk, when its box didn't report the end itself.
pub const LOG_WAIT_MS: u64 = 2 * 60_000;

/// When to look again, if the deploy's run may still be sending its log
/// (M213) and its box must stay up until then: never when there's no
/// run-log bucket, no run, or one that never ran. A run whose box
/// reported its end has its whole log kept (the box sends the last chunk
/// before that report). One that ended otherwise (control failed or
/// canceled it while the box ran it) waits at most [`LOG_WAIT_MS`] after
/// its end, by when the box has sent what it had or never will.
fn logs_pending(app: &Platform, deploy: &str, now: u64) -> Result<Option<u64>> {
    if app.run_logs.is_none() {
        return Ok(None);
    }
    let Some(dr) = app.db.deploy_run(deploy)? else { return Ok(None) };
    let Some(run) = app.db.run(&dr.run)? else { return Ok(None) };
    if run.started_at.is_none() {
        return Ok(None);
    }
    let Some(ended) = run.ended_at else { return Ok(None) };
    let by_box = app.db.run_events(&run.id)?.last().is_some_and(|e| e.daemon.is_some() && e.to.terminal());
    let until = ended.saturating_add(LOG_WAIT_MS);
    Ok((!by_box && now < until).then(|| until.min(now + BOX_MS * 5)))
}

/// The deploy is over (or never will run): its box is deleted, and the job
/// follows the delete to its end. The box goes whatever happened, a crash
/// in between included: the job, with its box in its checkpoint, is taken
/// up again by any of the platform's machines.
fn teardown(app: &Platform, deploy: &str, cp: &Checkpoint, now: u64) -> Result<Step> {
    let Some(b) = &cp.r#box else { return Ok(Step::Done) };
    // The run's log (M213) first: its box sends the last of it before it
    // reports its run's end.
    if let Some(at) = logs_pending(app, deploy, now)? {
        return Ok(Step::Again(at));
    }
    // By its id alone: its deploy, environment or project may be gone.
    match app.db.deploy_box(b)? {
        None => Ok(Step::Done),
        Some(w) if w.state == "deleted" => Ok(Step::Done),
        Some(w) => {
            if w.asked.as_deref() != Some("delete") {
                crate::hosted::ask(app, &w.id, crate::hosted::Ask::Delete)?;
                info!(workspace = w.id, "a deploy's box is being deleted");
            }
            Ok(Step::Again(now + BOX_MS))
        }
    }
}

async fn step(app: &Arc<Platform>, db: &Db, job: &mut LeasedJob, now: u64) -> Result<Step> {
    let mut cp = checkpoint_of(job);
    // Gone (its project deleted, say): its box goes all the same.
    let Some(d) = db.deploy(&job.id)? else { return teardown(app, &job.id, &cp, now) };
    let Some(env) = db.environment(&d.environment)? else { return teardown(app, &job.id, &cp, now) };
    let project = env.project.clone();
    if job.cancelled() {
        return teardown(app, &d.id, &cp, now);
    }
    // Its run is made: follow it (a backstop for its reports), then delete
    // its box.
    if let Some(dr) = db.deploy_run(&d.id)? {
        let run = db.run(&dr.run)?.context("a deploy's run is gone")?;
        if run.state == RunState::Pending
            && let Some(a) = approval_of(db, &d.id)?
            && let Err(r) = approve_run(db, &run, &a, now)?
        {
            info!(run = run.id, refused = %r, "a deploy's run wasn't approved");
        }
        // Not started by its approval's `expires`: it never will be. Its
        // run isn't offered again, and the deploy expired.
        if d.state == State::Approved
            && matches!(run.state, RunState::Pending | RunState::Approved)
            && approval_of(db, &d.id)?.is_none_or(|a| now >= a.expires)
        {
            let detail = json!({ "reason": "its deploy's approval expired before it started" }).to_string();
            let _ = db.move_run(
                &run.id,
                &Move::Control { from: run.state, to: RunState::Expired, at: now, detail: &detail },
            )?;
            cancel(app, &d, EventKind::Expired, "its approval expired before its run started", now).await?;
            return teardown(app, &d.id, &cp, now);
        }
        // Past the box's longest life: its run is failed, which fails the
        // deploy (and pages its owners) if it had started.
        if !run.state.terminal() && now > cp.made_at.max(dr.created_at) + BOX_LIFE_MS {
            let detail = json!({ "reason": "its deploy's box ran out of time" }).to_string();
            let _ = db.move_run(
                &run.id,
                &Move::Control { from: run.state, to: RunState::Failed, at: now, detail: &detail },
            )?;
        }
        let run = db.run(&dr.run)?.context("a deploy's run is gone")?;
        if run.state != RunState::Pending {
            reported(app, &run).await?;
        }
        let d = db.deploy(&d.id)?.context("a deploy that was there")?;
        let run = db.run(&dr.run)?.context("a deploy's run is gone")?;
        if run.state.terminal() {
            page_unpaged(app, &run.id).await?;
        }
        return Ok(match d.state {
            State::Running => Step::Again(now + RUNNING_MS),
            State::Approved => Step::Again(now + FOLLOW_MS),
            _ if !run.state.terminal() => Step::Again(now + RUNNING_MS),
            _ => return teardown(app, &d.id, &cp, now),
        });
    }
    if d.state != State::Approved {
        return teardown(app, &d.id, &cp, now);
    }
    let Some(a) = approval_of(db, &d.id)? else { return teardown(app, &d.id, &cp, now) };
    if now >= a.expires {
        cancel(app, &d, EventKind::Expired, "its approval expired before it was applied", now).await?;
        return teardown(app, &d.id, &cp, now);
    }
    let p = db.issue_project(&project)?.context("a deploy's project is gone")?;
    match recheck(app, &a, &p).await {
        Ok(()) => {}
        Err(None) => return Ok(Step::Again(now + HUB_RETRY_MS)),
        Err(Some(why)) => {
            cancel(app, &d, EventKind::Canceled, &format!("the approval doesn't hold now: {why}"), now).await?;
            return teardown(app, &d.id, &cp, now);
        }
    }
    let Some(driver) = drivers::driver(&env.driver) else {
        cancel(app, &d, EventKind::Canceled, &format!("there's no driver {} here", env.driver), now).await?;
        return teardown(app, &d.id, &cp, now);
    };
    if app.compute.is_none() {
        cancel(app, &d, EventKind::Canceled, "this platform makes no hosted boxes to apply deploys on", now).await?;
        return Ok(Step::Done);
    }
    // A box of its own: named in the checkpoint first, then made, then
    // waited for until its daemon enrolled.
    let b = match cp.r#box.clone() {
        Some(b) => b,
        None => {
            let b = crate::hosted::new_id();
            cp.r#box = Some(b.clone());
            cp.made_at = now;
            save(db, job, &cp, now)?;
            b
        }
    };
    let w = match db.hosted_any(&project, &b)? {
        Some(w) => w,
        None => {
            #[cfg(test)]
            tests::stall_point(&d.id);
            if !db.add_deploy_box(&b, &project, &d.id, &a.account, now)? {
                // Taken: another machine made the deploy's box while this
                // one stalled here (its lease ran out meanwhile). That box
                // is the deploy's: carry on with it, never cancel.
                let Some(w) = db.deploy_box_named(&project, &d.id)? else {
                    cancel(app, &d, EventKind::Canceled, "its box couldn't be made (its name is taken)", now).await?;
                    return Ok(Step::Done);
                };
                info!(deploy = d.id, workspace = w.id, "a deploy's box was made by another machine meanwhile");
                if cp.r#box.as_deref() != Some(w.id.as_str()) {
                    cp.r#box = Some(w.id.clone());
                    save(db, job, &cp, now)?;
                }
                return Ok(Step::Again(now + BOX_MS));
            }
            info!(deploy = d.id, workspace = b, "a deploy's box is being made");
            return Ok(Step::Again(now + BOX_MS));
        }
    };
    let up = w.state == "ready" && w.machine.is_some();
    if !up {
        let why = match w.state.as_str() {
            "failed" | "deleting" | "deleted" => {
                Some(format!("its box didn't come up: {}", w.error.as_deref().unwrap_or("it failed")))
            }
            _ if now > cp.made_at + BOX_UP_MS => Some("its box didn't come up in time".to_owned()),
            _ => None,
        };
        if let Some(why) = why {
            cancel(app, &d, EventKind::Canceled, &why, now).await?;
            return teardown(app, &d.id, &cp, now);
        }
        return Ok(Step::Again(now + BOX_MS));
    }
    let machine = w.machine.clone().unwrap_or_default();
    let op = Op {
        deploy: Some(DeployApply { deploy: d.id.clone(), driver: driver.name().into(), op: a.op.clone() }),
        ..Op::default()
    }
    .encode(&machine);
    let new = NewDeployRun {
        project: &project,
        deploy: &d.id,
        approval: &a.id,
        account: &a.account,
        workspace: &w.id,
        driver: driver.name(),
        op: &op,
    };
    let run = db.make_deploy_run(&new, now)?.context("a deploy's project is gone")?;
    if let Err(r) = approve_run(db, &run, &a, now)? {
        info!(run = run.id, refused = %r, "a deploy's run wasn't approved");
    }
    info!(deploy = d.id, run = run.id, workspace = w.id, driver = driver.name(), "a deploy's run is made on its box");
    Ok(Step::Again(now + FOLLOW_MS))
}

/// Approve the deploy's run with the deploy's approval.
fn approve_run(
    db: &Db,
    run: &Run,
    a: &StoredApproval,
    now: u64,
) -> Result<std::result::Result<(), crate::db::runs::Refused>> {
    let approval = deploy_approval(run, &a.id, &a.device);
    Ok(db.move_run(&run.id, &Move::Approved { from: RunState::Pending, at: now, approval: &approval })?.map(|_| ()))
}

/// What a deploy's run carries to its box: the deploy's operation and its
/// approval as signed, for the box to check. `None` when `run` isn't a
/// deploy's.
pub fn deploy_work(db: &Db, run: &str) -> Result<Option<DeployWork>> {
    let Some(dr) = db.deploy_run_of(run)? else { return Ok(None) };
    // An approval a start used up goes to no other run.
    if db.approval_consumed_by(&dr.deploy)?.is_some_and(|by| by != run) {
        return Ok(None);
    }
    let Some((_, operation)) = db.deploy_operation(&dr.deploy)? else { return Ok(None) };
    let Some(a) = db.deploy_approvals(&dr.deploy)?.into_iter().find(|a| a.id == dr.approval) else {
        return Ok(None);
    };
    let env = db.environment(&operation.environment)?.context("a deploy's environment is gone")?;
    let mut secrets = env.secret_names.clone();
    if let Some(d) = drivers::driver(&env.driver) {
        secrets.extend(d.mints(&env).iter().map(|m| m.name().to_owned()));
    }
    let roll_back = db.deploy_roll_back(&dr.deploy)?;
    Ok(Some(DeployWork {
        deploy: dr.deploy,
        operation,
        approval: a.signed(),
        passkey: a.passkey.map(|p| PasskeyAssertion {
            id: p.id,
            client_data_json: p.client_data_json,
            authenticator_data: p.authenticator_data,
            signature: p.signature,
        }),
        roll_back,
        approval_id: Some(dr.approval),
        driver: dr.driver,
        secrets,
        settings: env.settings,
    }))
}

/// What a run's box reported its apply did (M220): its last event's
/// `result`, `Null` when none.
fn reported_result(db: &Db, run: &Run) -> Result<Value> {
    let last = db.run_events(&run.id)?.pop();
    let detail: Value = last.and_then(|e| serde_json::from_str(&e.detail).ok()).unwrap_or_default();
    Ok(detail.get("result").cloned().unwrap_or_default())
}

/// Why a run ended, from its last event: its reason, or its exit code.
fn why_it_ended(db: &Db, run: &Run) -> Result<String> {
    let last = db.run_events(&run.id)?.pop();
    let detail: Value = last.and_then(|e| serde_json::from_str(&e.detail).ok()).unwrap_or_default();
    Ok(match (detail["reason"].as_str(), detail["exit"].as_i64()) {
        (Some(r), _) => r.to_owned(),
        (None, Some(n)) => format!("the apply exited {n}"),
        _ => format!("its run {}", run.state),
    })
}

/// A deploy's run moved (its box's signed report, or the job looking):
/// move its deploy on. `running` starts it, from the run made for it only;
/// refused (stale, busy, its approval expired), the run is failed, and
/// the answer to the box says so (`Some`). Its end ends the deploy.
pub async fn reported(app: &Platform, run: &Run) -> Result<Option<Reported>> {
    let db = &app.db;
    let Some(dr) = db.deploy_run_of(&run.id)? else { return Ok(None) };
    let Some(d) = db.deploy(&dr.deploy)? else { return Ok(None) };
    let now = arugula_e2e::now_ms();
    let by = Actor::run(&run.id);
    let started_by_it = |d: &Deploy| d.run.as_deref() == Some(run.id.as_str());
    if d.state == State::Approved && run.state == RunState::Running {
        // Its run's own time (the box's, signed), and the platform's own
        // clock too: neither past the approval's `expires` (the platform's
        // by more than a minute's slack).
        let at = run.started_at.unwrap_or(now);
        let late = approval_of(db, &d.id)?.is_none_or(|a| now > a.expires + START_SLACK_MS);
        let refused = if late {
            Some(Refusal::ApprovalExpired)
        } else {
            db.record_deploy(&d.id, EventKind::Started, &by, at, json!({}))?.err()
        };
        if let Some(why) = refused
            && !db.deploy(&d.id)?.is_some_and(|now| started_by_it(&now))
        {
            let reason = format!("deploy_refused: {why}");
            info!(deploy = d.id, run = run.id, %why, "a deploy's run may not start it");
            if run.state == RunState::Running {
                let detail = json!({ "reason": reason }).to_string();
                let _ = db.move_run(
                    &run.id,
                    &Move::Control { from: RunState::Running, to: RunState::Failed, at: now, detail: &detail },
                )?;
            }
            let kind = if why == Refusal::ApprovalExpired { EventKind::Expired } else { EventKind::Canceled };
            cancel(app, &d, kind, &reason, now).await?;
            return Ok(Some(Reported::Refused {
                why: reason,
                now: Some(RunState::Failed.as_str().into()),
                next: None,
            }));
        }
    } else if d.state == State::Approved && run.state.terminal() {
        // Ended before it started: nothing was applied, so no `started`.
        // Its box refused it (the run's last event is the box's, signed):
        // something forged or stale got past the platform, so the owners
        // are paged, once. Never taken, or failed by the platform: quietly.
        let reason = why_it_ended(db, run)?;
        let why = format!("its run ended before it applied: {reason}");
        cancel_quietly(app, &d, &why, now)?;
        crate::drivers::revoke_run(app, &run.id).await;
        let by_box = db.run_events(&run.id)?.pop().is_some_and(|e| e.daemon.is_some());
        if by_box {
            let title = "A deploy box refused an approval".to_owned();
            let body = format!("The deploy box refused the approval for {}: {reason}", d.id);
            page_with(app, &d.id, &run.id, &title, &body, &format!("deploy-refused-{}", d.id)).await;
        }
        return Ok(None);
    }
    let Some(d) = db.deploy(&dr.deploy)? else { return Ok(None) };
    if run.state.terminal() {
        crate::drivers::revoke_run(app, &run.id).await;
    }
    if d.state != State::Running || !started_by_it(&d) || !run.state.terminal() {
        return Ok(None);
    }
    let at = run.ended_at.unwrap_or(now);
    let env = db.environment(&d.environment)?.context("a deploy's environment is gone")?;
    let driver = drivers::driver(&env.driver);
    // What the box said it did (M220): the run links chant's ledger entry
    // whenever it wrote one, a failed apply's too (aru-098), and the log
    // keeps what the driver checked of it.
    let result = reported_result(db, run)?;
    let kept = driver.map(|dr| dr.kept(&env, &d, &result)).unwrap_or_default();
    if let Some(entry) = driver.and_then(|dr| dr.ledger(&env, &d, &result))
        && db.link_chant_ledger(&run.id, &entry)?
    {
        info!(deploy = d.id, run = run.id, entry, "a deploy's run linked chant's ledger entry");
    }
    let verdict = match (run.state, driver) {
        (RunState::Succeeded, Some(driver)) => driver.verify(&env, &d, run, &result),
        (RunState::Succeeded, None) => Verdict::Unhealthy(format!("there's no driver {} here to check it", env.driver)),
        _ => Verdict::Unhealthy(why_it_ended(db, run)?),
    };
    let with_kept = |mut v: Value| {
        if let Some(m) = v.as_object_mut() {
            m.extend(kept.clone());
        }
        v
    };
    match verdict {
        Verdict::Healthy => {
            let detail = with_kept(json!({ "run": run.id }));
            if db.record_deploy(&d.id, EventKind::Succeeded, &by, at, detail)?.is_ok() {
                info!(deploy = d.id, run = run.id, "a deploy was applied");
            }
        }
        Verdict::Unhealthy(why) => {
            let detail = with_kept(json!({ "run": run.id, "reason": why }));
            if db.record_deploy(&d.id, EventKind::Failed, &by, at, detail)?.is_ok() {
                warn!(deploy = d.id, run = run.id, why, "a deploy's apply failed");
                page(app, &d.id, &run.id, &why).await;
            }
        }
    }
    Ok(None)
}

/// A deploy's run ended and its deploy failed by it, but its owners
/// weren't paged: its run's job gave up, failing both in one transaction
/// (M223, `db::jobs`). Page them now, once. The run's job or the deploy's,
/// whichever looks next, calls it.
pub async fn page_unpaged(app: &Platform, run: &str) -> Result<()> {
    let Some(dr) = app.db.deploy_run_of(run)? else { return Ok(()) };
    if dr.paged_at.is_some() {
        return Ok(());
    }
    let Some(d) = app.db.deploy(&dr.deploy)? else { return Ok(()) };
    if d.state != State::Failed || d.run.as_deref() != Some(run) {
        return Ok(());
    }
    let last = app.db.deploy_history(&d.id)?.pop().and_then(|e| e.detail_json().ok());
    let why = last.and_then(|v| v["reason"].as_str().map(str::to_owned)).unwrap_or_else(|| "it failed".into());
    page(app, &d.id, run, &why).await;
    Ok(())
}

/// Tell the deploy's project's owners its apply failed (or never ran),
/// with its run's log: once, by the hub's push. In the background: a
/// failure is only logged.
pub async fn page(app: &Platform, deploy: &str, run: &str, why: &str) {
    let found = (|| -> Result<_> {
        let d = app.db.deploy(deploy)?.context("no deploy")?;
        let env = app.db.environment(&d.environment)?.context("no environment")?;
        let p = app.db.issue_project(&env.project)?.context("no project")?;
        Ok((d, env, p))
    })();
    let Ok((d, env, p)) = found else { return };
    let short: String = d.source.reference.chars().take(12).collect();
    let title = format!("A deploy to {} failed", env.name);
    let body = format!(
        "{}: {} {short} from {}: {why}. Its run's log: /p/{}/past/{run}",
        p.name,
        d.source.kind.as_str(),
        d.source.place,
        p.id
    );
    page_with(app, deploy, run, &title, &body, &format!("deploy-failed-{deploy}")).await;
}

/// Push the deploy's project's owners `title` and `body`, with its run's
/// log, once per deploy (`deploy_runs.paged_at`).
async fn page_with(app: &Platform, deploy: &str, run: &str, title: &str, body: &str, tag: &str) {
    let go = async {
        if !app.db.mark_deploy_paged(deploy, arugula_e2e::now_ms())? {
            return Ok(());
        }
        let d = app.db.deploy(deploy)?.context("no deploy")?;
        let env = app.db.environment(&d.environment)?.context("no environment")?;
        let p = app.db.issue_project(&env.project)?.context("no project")?;
        let to = super::owners(app, &p).await?;
        let log = format!("/p/{}/past/{run}", p.id);
        let msg = json!({
            "deploy": d.id,
            "run": run,
            "log": log,
            "title": title,
            "body": body,
            "tag": tag,
            "control": true,
        });
        for account in to {
            if let Err(e) = app.hub.push_with(&account, &msg, PushOptions::default()).await {
                warn!(deploy, %account, error = %e, "a failed deploy's page didn't go");
            }
        }
        info!(deploy, run, tag, "paged the owners: a deploy failed");
        anyhow::Ok(())
    };
    if let Err(e) = go.await {
        warn!(deploy, error = format!("{e:#}"), "couldn't page the owners about a failed deploy");
    }
}

/// `POST /api/hosted/work/secrets` from `m` (M219): its deploy run's
/// environment's secrets and the driver's minted tokens, once, while the
/// run runs and its deploy is being applied by it. Values are opened here,
/// now, and never written anywhere: the log names the secrets, never
/// their values.
pub async fn secrets(app: &Platform, m: &HostedMachine, ask: &SecretsAsk) -> Result<SecretsGiven> {
    let refused = |why: &str| {
        info!(run = ask.run, machine = m.machine, why, "refused a deploy run's secrets");
        Ok(SecretsGiven::Refused { why: why.into() })
    };
    let db = &app.db;
    let Some(run) = db.run(&ask.run)? else { return refused("no such run") };
    if run.project != m.project || run.workspace.as_deref() != Some(m.workspace.as_str()) || run.daemon != m.machine {
        return refused("the run isn't this machine's");
    }
    if run.state != RunState::Running {
        return refused("the run isn't running");
    }
    let Some(dr) = db.deploy_run_of(&run.id)? else { return refused("the run isn't a deploy's") };
    let Some(d) = db.deploy(&dr.deploy)? else { return refused("no such deploy") };
    if d.state != State::Running || d.run.as_deref() != Some(run.id.as_str()) {
        return refused("its deploy isn't being applied by this run");
    }
    let env = db.environment(&d.environment)?.context("a deploy's environment is gone")?;
    // Once, and only while the run runs and applies its deploy: checked in
    // the same statement that marks them given.
    if !db.release_deploy_secrets(&run.id, arugula_e2e::now_ms())? {
        return refused("its secrets were given already (once per run), or it isn't applying its deploy now");
    }
    let mut out = Vec::new();
    if !env.secret_names.is_empty() {
        let Some(master) = app.secret_key.as_deref() else { return refused("this platform keeps no secrets") };
        for name in &env.secret_names {
            let stored = crate::keys::environment_name(&env.id, name);
            match crate::keys::open(db, master, &env.project, &stored)? {
                Some(value) => out.push(Secret { name: name.clone(), value, revoke: None }),
                None => return refused(&format!("the environment's secret {name} isn't set")),
            }
        }
    }
    if let Some(driver) = drivers::driver(&env.driver) {
        for mint in driver.mints(&env) {
            match drivers::mint(app, mint, &d, &run.id).await {
                Ok(s) => out.push(s),
                Err(e) => return refused(&format!("minting {}: {e:#}", mint.name())),
            }
        }
    }
    let names: Vec<&str> = out.iter().map(|s| s.name.as_str()).collect();
    info!(run = run.id, deploy = d.id, ?names, "gave a deploy run its secrets, once");
    Ok(SecretsGiven::Given { secrets: out })
}

#[cfg(test)]
#[path = "apply_tests.rs"]
pub(crate) mod tests;
