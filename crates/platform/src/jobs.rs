//! The platform's job worker (M208): it claims a due job from
//! `platform_jobs` ([`crate::db::jobs`]), hands it to its kind's
//! [`Handler`] for one bounded step, and writes back what the step said:
//! due again, done, or failed (with a back-off). Workspaces, runs, deploys
//! and schedules each bring a handler (M210, M214, M217); none does yet,
//! so nothing starts the worker.
//!
//! It is `cloud::worker`'s loop, kept apart until M190 moves the Cloud
//! kinds here: four workers on each machine, a claim leases its job for
//! [`LEASE_MS`] (on Postgres `FOR UPDATE SKIP LOCKED`, so two machines
//! never take the same job), a step's handler checkpoints before an
//! effect that mustn't be repeated, and a machine that dies holding a job
//! leaves it to the next claim when the lease runs out, which carries on
//! from the checkpoint. A machine that's stopping takes no new job.
use crate::{
    Platform,
    db::{
        Db,
        jobs::{JobEnd, JobKind, LeaseLost, LeasedJob},
    },
};
use anyhow::Result;
use futures_util::future::BoxFuture;
use std::{collections::BTreeMap, sync::Arc};

pub use crate::db::jobs::LEASE_MS;

/// How many claims in a row may fail before a job ends `failed`, unless
/// its handler says otherwise.
pub const MAX_ATTEMPTS: u32 = 8;

/// What a step leaves its job as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Hand it back, due again at this time (ms).
    Again(u64),
    Done,
    /// It can't succeed: end it with this error, without retrying.
    Failed(String),
    /// It stopped because it was asked to ([`LeasedJob::cancel`]).
    Cancelled,
}

/// One kind's work. A step is bounded: it does what's next, writes the job's
/// checkpoint ([`Db::checkpoint_job`]) before any effect a retry mustn't
/// repeat, and says what's next. An error retries it after a back-off from
/// its checkpoint; a lost lease ([`LeaseLost`]) means another machine has
/// it now, and nothing is written.
pub trait Handler: Send + Sync {
    fn kind(&self) -> JobKind;

    fn max_attempts(&self) -> u32 {
        MAX_ATTEMPTS
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, now: u64) -> BoxFuture<'a, Result<Step>>;
}

/// The handlers a machine runs, one per kind. It claims only their kinds.
#[derive(Default, Clone)]
pub struct Worker {
    handlers: BTreeMap<JobKind, Arc<dyn Handler>>,
}

impl Worker {
    pub fn new() -> Self {
        Self::default()
    }

    /// With `handler` for its kind.
    pub fn with(mut self, handler: impl Handler + 'static) -> Self {
        let kind = handler.kind();
        assert!(self.handlers.insert(kind, Arc::new(handler)).is_none(), "two handlers for {kind:?} jobs");
        self
    }

    pub fn kinds(&self) -> Vec<JobKind> {
        self.handlers.keys().copied().collect()
    }

    /// One step of the job that has waited longest, if one is due: whether
    /// there was one.
    pub async fn tick(&self, db: &Db, now: u64) -> Result<bool> {
        let Some(mut job) = db.claim_job(&self.kinds(), now)? else {
            return Ok(false);
        };
        let handler = self.handlers[&job.kind].clone();
        let saved = match handler.step(db, &mut job, now).await {
            Ok(Step::Again(at)) => db.release_job(&job, at, now),
            Ok(Step::Done) => db.finish_job(&job, JobEnd::Done, None, now),
            Ok(Step::Failed(error)) => db.finish_job(&job, JobEnd::Failed, Some(&error), now),
            Ok(Step::Cancelled) => db.finish_job(&job, JobEnd::Cancelled, None, now),
            Err(error) if error.is::<LeaseLost>() => Err(error),
            Err(error) => {
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), attempts = job.attempts, error = %format!("{error:#}"), "job step failed; it will retry");
                db.fail_job(&job, &format!("{error:#}"), handler.max_attempts(), now).map(|ended| {
                    if ended {
                        tracing::warn!(job = %job.id, kind = job.kind.as_str(), "job failed for good");
                    }
                })
            }
        };
        match saved {
            // Another machine took it over: what it writes stands.
            Err(error) if error.is::<LeaseLost>() => {
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), "job lease lost; another machine has it");
                Ok(true)
            }
            r => r.map(|()| true),
        }
    }
}

/// Four workers on this machine, each claiming a step at a time while the
/// platform isn't paused or stopping. Nothing calls it until a kind has a
/// handler (M210).
pub fn start(app: Arc<Platform>, worker: Worker) {
    let worker = Arc::new(worker);
    for _ in 0..4 {
        let (app, worker) = (app.clone(), worker.clone());
        tokio::spawn(async move {
            loop {
                if !app.is_paused() && !app.is_stopping() {
                    let _held = Busy::new(&app);
                    if let Err(error) = worker.tick(&app.db, arugula_e2e::now_ms()).await {
                        tracing::warn!(%error, "platform job worker failed");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }
}

/// Counts a tick in [`Platform::busy`] while it runs, so a stopping machine
/// waits for it ([`Platform::drain`]), as `cloud::worker`'s do.
struct Busy<'a>(&'a Platform);

impl<'a> Busy<'a> {
    fn new(app: &'a Platform) -> Self {
        app.busy.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(app)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
