//! Runs on hosted workspaces (M212, platform step 7): the platform's half
//! of S37's execution contract (`spikes/s37-team-permissions/README.md`,
//! q4). `arugulad --hosted` is the other half (`crates/daemon/src/work.rs`).
//!
//! - **Dispatch** ([`Dispatch`], the `run` kind on `platform_jobs`). A run
//!   made by a trigger or by hand (M214) gets one job
//!   ([`crate::db::runs::dispatch`]), which
//!   checks where it runs and marks it ready for that machine (its
//!   checkpoint). Only a hosted workspace's machine gets work: a run whose
//!   operation names any other machine (a person's, or a box that left)
//!   fails, saying so, and never reaches it. The job then follows the run
//!   until it ends: it wakes a paused box while the run waits for it or
//!   runs, and fails a run no machine took within [`TAKE_MS`].
//! - **Pulling** ([`work`]). The hub's `GET /api/hosted/work` takes only a
//!   hosted machine's signature, and asks here with what that signature
//!   proved ([`HostedMachine`]). The answer is every run that's ready for
//!   the machine and hasn't started. The same run comes on every pull until
//!   it starts: the machine's ledger, not the platform, makes it run at most
//!   once.
//! - **Reporting** ([`report`]). The machine signs each state change it
//!   makes (`arugula run event v1`), and the platform checks it with the
//!   machine's key, as its hosted certificate gives it, before
//!   [`Db::move_run`] keeps it, signature and all. A report sent again
//!   after its answer was lost is [`Reported::Already`].
//!
//! **Awake and busy.** A run that's waiting wakes its box through M182's
//! `hosted::wake` when the box is paused, or when its daemon hasn't asked
//! for its work in [`QUIET_MS`] (a box pauses itself at its timeout before
//! the platform sees it). While the run runs the machine says it's busy
//! (M180's activity), which is what holds the box's timeout (M181), so it
//! doesn't pause mid-run.
//!
//! Not here: who may start a run (M211's grant, M214's triggers), and the
//! approval a run without a grant waits for (M218).

use std::sync::Arc;

use anyhow::Result;
use arugula_control_wire::work::{Report, Reported, TAKES_RUN, Work, WorkItem};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        Db,
        jobs::{JobKind, LeasedJob},
        runs::{DaemonReport, Move, Refused, State},
    },
    jobs::{Clock, Handler, Step},
};

/// How long a ready run waits for its machine to ask for it before it
/// fails: the box's wake, its daemon's dial and a few pulls fit many times
/// over. A machine that keeps pulling it (busy with another run) keeps it.
pub const TAKE_MS: u64 = 15 * 60_000;
/// How often a waiting run's job looks again.
pub const WAITING_MS: u64 = 5_000;
/// How often a running run's job looks again.
pub const RUNNING_MS: u64 = 30_000;
/// The most runs one pull answers.
pub const PULL_LIMIT: u32 = 20;

/// A hosted machine, as the hub checked its signature (its hosted
/// certificate): the project and workspace it was enrolled into, its device
/// id, and its signing key, hex, which its reports are checked with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedMachine {
    pub project: String,
    pub workspace: String,
    pub machine: String,
    pub sign: String,
}

/// What a dispatch job keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Checkpoint {
    /// When it was marked ready for its machine.
    pub ready_at: u64,
    /// When its machine last pulled it ([`Db::note_pulled`], written only
    /// while no step holds the job).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulled_at: Option<u64>,
    /// When the job last asked for its box's wake (M182's `hosted::wake`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woke_at: Option<u64>,
}

/// How long a ready run goes unpulled before its job wakes the box: its
/// daemon pulls every few seconds while it's up.
pub const QUIET_MS: u64 = 30_000;
/// The least time between two wakes a job asks for.
pub const WAKE_EVERY_MS: u64 = 60_000;

/// The `run` kind's handler, on the platform it wakes boxes through.
pub struct Dispatch {
    pub app: Arc<Platform>,
}

/// Fail a run the platform won't send, or that no machine took, saying why.
fn fail(db: &Db, run: &str, from: State, reason: &str, now: u64) -> Result<()> {
    let detail = json!({ "reason": reason }).to_string();
    match db.move_run(run, &Move::Control { from, to: State::Failed, at: now, detail: &detail })? {
        Ok(_) => info!(run, reason, "run failed before it reached its machine"),
        // It moved meanwhile (its machine took it): what it says stands.
        Err(r) => info!(run, refused = %r, "a run's dispatch didn't fail it"),
    }
    Ok(())
}

impl Dispatch {
    async fn step(&self, db: &Db, job: &mut LeasedJob, clock: &Clock) -> Result<Step> {
        let now = clock.now();
        if job.cancelled() {
            return Ok(Step::Cancelled);
        }
        let Some(run) = db.run(&job.id)? else { return Ok(Step::Done) };
        if run.state.terminal() {
            return Ok(Step::Done);
        }
        let waiting = matches!(run.state, State::Pending | State::Approved);
        // Where it runs: a hosted workspace of its project, whose box
        // enrolled as the machine its operation names. Nothing else gets work.
        let w = match &run.workspace {
            Some(ws) => db.hosted_workspace(&run.project, ws)?,
            None => None,
        };
        let Some(w) = w.filter(|w| {
            w.machine.as_deref() == Some(run.daemon.as_str()) && !matches!(w.state.as_str(), "deleting" | "deleted")
        }) else {
            let reason = if waiting {
                "not_hosted: runs go only to their project's hosted workspaces, and this one's machine isn't one"
            } else {
                "gone: its hosted workspace was deleted or made again"
            };
            fail(db, &run.id, run.state, reason, now)?;
            return Ok(Step::Done);
        };
        let mut cp = match job.checkpoint.as_ref().and_then(|c| serde_json::from_value::<Checkpoint>(c.clone()).ok()) {
            Some(c) => c,
            None => {
                let c = Checkpoint { ready_at: now, ..Checkpoint::default() };
                job.checkpoint = Some(serde_json::to_value(&c)?);
                db.checkpoint_job(job, now)?;
                info!(run = run.id, workspace = w.id, machine = run.daemon, "run ready for its hosted machine");
                c
            }
        };
        let seen = cp.pulled_at.unwrap_or(cp.ready_at).max(cp.ready_at);
        // Awake while it waits: a box that's paused, or one whose daemon
        // hasn't asked for its work in a while (it may have paused itself
        // at its timeout, still `ready` here), is woken through M182's
        // wake, which the hub's opens share. At most once a minute; not
        // waited on here (the worker resumes the box). While the run runs
        // its machine says it's busy, which holds the box (M181).
        let quiet = w.state == "paused" || now.saturating_sub(seen) > QUIET_MS;
        if waiting && quiet && cp.woke_at.is_none_or(|t| now.saturating_sub(t) >= WAKE_EVERY_MS) {
            cp.woke_at = Some(now);
            job.checkpoint = Some(serde_json::to_value(&cp)?);
            db.checkpoint_job(job, now)?;
            let (app, project, ws, id) = (self.app.clone(), run.project.clone(), w.id.clone(), run.id.clone());
            tokio::spawn(async move {
                match crate::hosted::wake::wake(&app, &project, &ws).await {
                    Ok(a) => info!(run = id, workspace = ws, answer = ?a, "woke a hosted workspace for its run"),
                    Err(e) => {
                        warn!(run = id, workspace = ws, error = %format!("{e:#}"), "can't wake a run's workspace")
                    }
                }
            });
        }
        // Not taken: nothing pulled it for that long (a machine that's
        // busy with another run pulls it, and it waits).
        if waiting && now.saturating_sub(seen) > TAKE_MS {
            fail(db, &run.id, run.state, "not_taken: its hosted workspace didn't ask for it in 15 minutes", now)?;
            return Ok(Step::Done);
        }
        // Running: its machine says it's busy until the run ends, which
        // holds the box (M181). The job only follows it to its end.
        Ok(Step::Again(now + if waiting { WAITING_MS } else { RUNNING_MS }))
    }
}

impl Handler for Dispatch {
    fn kind(&self) -> JobKind {
        JobKind::Run
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(Dispatch::step(self, db, job, clock))
    }
}

/// `GET /api/hosted/work` for `m`: the runs ready for it, with what it
/// needs to check each before it starts. `takes` is what it said it takes:
/// only runs, so far.
pub async fn work(app: &Platform, m: &HostedMachine, takes: &[String]) -> Result<Work> {
    if !takes.iter().any(|t| t == TAKES_RUN) {
        return Ok(Work::default());
    }
    let mut items = Vec::new();
    let ready = app.db.ready_runs(&m.project, &m.workspace, &m.machine, PULL_LIMIT)?;
    // On every pull, for the runs going as well as the ones to start.
    let revokes = app.db.run_grant_revokes(&m.project)?;
    let now = arugula_e2e::now_ms();
    for id in ready {
        // Its machine is asking for it: it isn't `not_taken` while it does.
        app.db.note_pulled(&id, now)?;
        let Some(run) = app.db.run(&id)? else { continue };
        // The grant it was started under, revoked or not: the machine
        // checks it, with the revokes, and refuses one taken back.
        let grant = match (&run.grant, run.state) {
            (Some(g), State::Pending) => app.db.run_grant(&run.project, g)?.map(|g| g.grant),
            _ => None,
        };
        items.push(WorkItem {
            run: run.id,
            project: run.project,
            workspace: m.workspace.clone(),
            op: run.op,
            op_hash: run.op_hash,
            state: run.state.as_str().to_owned(),
            last_seq: run.last_seq,
            grant,
            revokes: revokes.clone(),
        });
    }
    Ok(Work { items, revokes })
}

fn refused(why: impl Into<String>) -> Reported {
    Reported::Refused { why: why.into(), now: None, next: None }
}

/// `POST /api/hosted/work/report` from `m`: its signed state change, kept
/// if the signature is its own and the move is S37's.
pub async fn report(app: &Platform, m: &HostedMachine, r: &Report) -> Result<Reported> {
    let (Some(from), Some(to)) = (State::parse(&r.from), State::parse(&r.to)) else {
        return Ok(refused("not a run's state"));
    };
    let ev = arugula_e2e::run::Event {
        run: &r.run,
        seq: r.seq,
        from: Some(from),
        to,
        at: r.at,
        detail: &r.detail,
        daemon: &m.machine,
    };
    let Some(checked) = DaemonReport::verify(&ev, &m.sign, &r.sig) else {
        warn!(run = r.run, machine = m.machine, "a hosted machine's run report isn't signed by it");
        return Ok(refused("the report isn't signed by this machine"));
    };
    let Some(run) = app.db.run(&r.run)? else { return Ok(refused(Refused::NoRun.to_string())) };
    if run.project != m.project || run.workspace.as_deref() != Some(m.workspace.as_str()) {
        return Ok(refused(Refused::OtherDaemon.to_string()));
    }
    match app.db.move_run(&r.run, &Move::Daemon(&checked))? {
        Ok((run, _)) => {
            info!(run = run.id, machine = m.machine, from = %from, to = %to, "a hosted machine moved its run");
            Ok(Reported::Taken)
        }
        Err(why) => {
            // The same event, kept before: a retry whose answer was lost.
            let kept = app.db.run_events(&r.run)?.into_iter().find(|e| e.seq == r.seq);
            if kept.is_some_and(|e| e.sig.as_deref() == Some(r.sig.as_str())) {
                return Ok(Reported::Already);
            }
            info!(run = r.run, machine = m.machine, refused = %why, "refused a hosted machine's run report");
            let (now, next) = match &why {
                Refused::Moved { now } => (Some(now.as_str().to_owned()), None),
                Refused::Seq { next } => (None, Some(*next)),
                _ => (None, None),
            };
            Ok(Reported::Refused { why: why.to_string(), now, next })
        }
    }
}

/// The job worker with every kind's handler this platform has: runs'
/// dispatch (M212) and schedules' ticks (M214). Other kinds join it here
/// as they land: one worker claims every kind.
pub fn start(app: &Arc<Platform>) {
    let ticks = crate::triggers::schedule::Ticks { gate: Arc::new(crate::triggers::Members(app.clone())) };
    crate::jobs::start(app.clone(), crate::jobs::Worker::new().with(Dispatch { app: app.clone() }).with(ticks));
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
