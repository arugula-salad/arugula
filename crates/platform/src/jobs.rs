//! The platform's job worker (M208): it claims a due job from
//! `platform_jobs` ([`crate::db::jobs`]), hands it to its kind's
//! [`Handler`] for one bounded step, and writes back what the step said:
//! due again, done, or failed (with a back-off). Workspaces, runs, deploys
//! and schedules each bring a handler (M210, M214, M217): schedules' is
//! [`crate::triggers::schedule::Ticks`], which `triggers::start` runs.
//!
//! It is `cloud::worker`'s loop, kept apart until M190 moves the Cloud
//! kinds here: four workers on each machine, a claim leases its job for
//! [`LEASE_MS`] (on Postgres `FOR UPDATE SKIP LOCKED`, so two machines
//! never take the same job), and a machine that dies holding a job leaves
//! it to the next claim when the lease runs out, which carries on from the
//! checkpoint. A machine that's stopping takes no new job.
//!
//! While a step runs the worker sends a heartbeat every third of a lease,
//! which keeps the job and tells the step about a cancel
//! ([`LeasedJob::cancelled`]). If a heartbeat finds the lease lost, the
//! step is dropped where it waits: another machine has the job. A step
//! that panics fails like one that returns an error, and so does one that
//! runs past its kind's deadline ([`Handler::deadline`]), which is dropped. What a handler must
//! do before an outside effect is in [`crate::db::jobs`]'s docs.
use crate::{
    Platform,
    db::{
        Db,
        jobs::{JobEnd, JobKind, LeaseLost, LeasedJob},
    },
};
use anyhow::Result;
use futures_util::{FutureExt as _, future::BoxFuture};
use std::{collections::BTreeMap, panic::AssertUnwindSafe, sync::Arc, time::Duration};

pub use crate::db::jobs::{LEASE_MS, MAX_ATTEMPTS};

/// How long a step may run unless its kind says otherwise: a step that
/// hangs while its heartbeats go on would hold its job for good.
pub const STEP_DEADLINE_MS: u64 = 30 * 60_000;

/// How a step ended, or didn't.
enum Ended<T> {
    Stepped(T),
    /// A heartbeat found the lease lost.
    Lost,
    /// It ran past its kind's deadline.
    Late,
}

/// `ms` for a person: `30 min`, `90 s`, `250 ms`.
fn span(ms: u64) -> String {
    match ms {
        _ if ms.is_multiple_of(60_000) => format!("{} min", ms / 60_000),
        _ if ms.is_multiple_of(1_000) => format!("{} s", ms / 1_000),
        _ => format!("{ms} ms"),
    }
}

/// What a step leaves its job as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Hand it back, due again at this time (ms).
    Again(u64),
    Done,
    /// It can't succeed: end it with this error, without retrying.
    Failed(String),
    /// It stopped because it was asked to ([`LeasedJob::cancelled`]).
    Cancelled,
}

/// The time (ms) a worker and its handlers write leases with: the system's,
/// or a test's.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> u64 + Send + Sync>);

impl Clock {
    pub fn new(now: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    pub fn now(&self) -> u64 {
        (self.0)()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new(arugula_e2e::now_ms)
    }
}

/// One kind's work. A step is bounded: it does what's next and says what's
/// next after it. Before an outside effect it wins the state change that
/// says so, under the lease ([`Db::under_lease`], [`Db::checkpoint_job`],
/// with `clock`'s time), and after a crash it finds out what the effect
/// did rather than make it again. An error retries it after a back-off from
/// its checkpoint; a lost lease ([`LeaseLost`]) means another machine has
/// it now, and nothing is written.
pub trait Handler: Send + Sync {
    fn kind(&self) -> JobKind;

    /// Claims in a row (failed, crashed or hung) before the job ends
    /// `failed`.
    fn max_attempts(&self) -> u32 {
        MAX_ATTEMPTS
    }

    /// How long (ms) one step may run, heartbeats or not. One that runs
    /// longer is dropped and counts as a failed attempt.
    fn deadline(&self) -> u64 {
        STEP_DEADLINE_MS
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>>;
}

/// The handlers a machine runs, one per kind. It claims only their kinds.
#[derive(Clone)]
pub struct Worker {
    handlers: BTreeMap<JobKind, Arc<dyn Handler>>,
    clock: Clock,
    heartbeat: Duration,
}

impl Default for Worker {
    fn default() -> Self {
        Self { handlers: BTreeMap::new(), clock: Clock::default(), heartbeat: Duration::from_millis(LEASE_MS / 3) }
    }
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

    /// Reading the time from `clock` (tests).
    pub fn clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Sending a heartbeat this often while a step runs (tests; a third of
    /// a lease otherwise).
    pub fn heartbeat(mut self, every: Duration) -> Self {
        self.heartbeat = every;
        self
    }

    pub fn kinds(&self) -> Vec<JobKind> {
        self.handlers.keys().copied().collect()
    }

    /// One step of the job that has waited longest, if one is due: whether
    /// there was one.
    pub async fn tick(&self, db: &Db) -> Result<bool> {
        let limits: Vec<(JobKind, u32)> = self.handlers.iter().map(|(k, h)| (*k, h.max_attempts())).collect();
        let Some(mut job) = db.claim_job_within(&limits, self.clock.now())? else {
            return Ok(false);
        };
        let handler = self.handlers[&job.kind].clone();
        let beat = job.clone();
        let deadline = self.clock.now().saturating_add(handler.deadline());
        let outcome = {
            let step = AssertUnwindSafe(handler.step(db, &mut job, &self.clock)).catch_unwind();
            tokio::pin!(step);
            let mut every = tokio::time::interval_at(tokio::time::Instant::now() + self.heartbeat, self.heartbeat);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    outcome = &mut step => break Ended::Stepped(outcome),
                    _ = every.tick() => {
                        let now = self.clock.now();
                        if now >= deadline {
                            break Ended::Late;
                        }
                        match db.extend_job(&beat, now + LEASE_MS, now) {
                            Ok(_) => {}
                            Err(error) if error.is::<LeaseLost>() => break Ended::Lost,
                            Err(error) => tracing::warn!(job = %beat.id, %error, "job heartbeat failed; trying again"),
                        }
                    }
                }
            }
        };
        let now = self.clock.now();
        let max = handler.max_attempts();
        let saved = match outcome {
            Ended::Lost => {
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), "job lease lost mid-step; the step was dropped");
                return Ok(true);
            }
            // Dropped where it waits, as on a lost lease, but still held: a
            // failed attempt, keeping what it last wrote.
            Ended::Late => {
                let error = format!("step passed its deadline of {}", span(handler.deadline()));
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), attempts = job.attempts, %error, "job step dropped");
                db.fail_job_as_written(&job, &error, max, now).map(|_| ())
            }
            Ended::Stepped(Ok(Ok(Step::Again(at)))) => db.release_job(&job, at, now),
            Ended::Stepped(Ok(Ok(Step::Done))) => db.finish_job(&job, JobEnd::Done, None, now),
            Ended::Stepped(Ok(Ok(Step::Failed(error)))) => db.finish_job(&job, JobEnd::Failed, Some(&error), now),
            Ended::Stepped(Ok(Ok(Step::Cancelled))) => db.finish_job(&job, JobEnd::Cancelled, None, now),
            Ended::Stepped(Ok(Err(error))) if error.is::<LeaseLost>() => Err(error),
            Ended::Stepped(Ok(Err(error))) => {
                let error = format!("{error:#}");
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), attempts = job.attempts, %error, "job step failed");
                db.fail_job(&job, &error, max, now).map(|_| ())
            }
            Ended::Stepped(Err(panic)) => {
                let error = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let error = format!("the step panicked: {error}");
                tracing::warn!(job = %job.id, kind = job.kind.as_str(), attempts = job.attempts, %error, "job step failed");
                db.fail_job_as_written(&job, &error, max, now).map(|_| ())
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
/// platform isn't paused or stopping. Each call claims only its worker's
/// kinds ([`crate::triggers::start`]: schedules, M214).
pub fn start(app: Arc<Platform>, worker: Worker) {
    let worker = Arc::new(worker);
    for _ in 0..4 {
        let (app, worker) = (app.clone(), worker.clone());
        tokio::spawn(async move {
            loop {
                if !app.is_paused() && !app.is_stopping() {
                    let _held = Busy::new(&app);
                    if let Err(error) = worker.tick(&app.db).await {
                        tracing::warn!(%error, "platform job worker failed");
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
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
