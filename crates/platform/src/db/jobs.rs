//! The platform's one leased job table, `platform_jobs` (M208): the
//! background work of workspaces, runs, deploys and schedules, which the
//! platform's machines take turns at ([`crate::jobs`] is the worker).
//!
//! The lease is `cloud_jobs`' (M144), made stricter:
//! - A claim takes one due job and leases it for [`LEASE_MS`] under a new
//!   random `lease`. On Postgres its `SELECT` is `FOR UPDATE SKIP LOCKED`,
//!   so two machines never take the same job and neither waits for the
//!   other; SQLite's `BEGIN IMMEDIATE` makes claims take turns.
//! - Every write names the lease it holds and is refused ([`LeaseLost`])
//!   once the lease has run out, whether or not another claim has taken
//!   the job yet (`cloud_jobs` lets a late holder write until then). A
//!   step longer than the lease keeps it with heartbeats ([`Db::extend_job`],
//!   which the worker sends every third of a lease), and a heartbeat can't
//!   bring back a lease that has run out.
//! - A machine that dies holding a job leaves it to the next claim when
//!   the lease runs out, which carries on from the last checkpoint. A job
//!   claimed its kind's most attempts in a row without handing itself back
//!   (crashing or hanging each time) ends `failed` at the next claim.
//! - A cancel ([`Db::cancel_job`]) makes the job due at once, and a hand-back
//!   or a failure keeps it due, so its handler sees it at the next claim; a
//!   heartbeat tells a step that's running.
//!
//! **What a handler must do with outside effects** (a create at a provider,
//! a deploy): a lease is only as good as the clocks that read it, and a
//! machine can stall at any point. So before an effect it wins the state
//! change that says the effect is under way, in the database, under the
//! lease: in one transaction that checks [`holds`] and writes its own rows
//! ([`Db::under_lease`], or [`checkpoint`] in its own transaction). Only
//! then does it make the effect. After a crash the next holder finds that
//! state and finds out what the effect did (looks for what the create
//! made); it never makes the effect again blind.
//!
//! **Clocks:** a lease is written with one machine's clock and read with
//! another's. The platform's machines keep theirs in sync (NTP), and the
//! lease, two minutes, is far longer than they ever disagree by.
use super::{
    Db,
    migrate::Dialect,
    sql::{Optional as _, Param, Row, Sql, Tx, params},
};
use anyhow::{Result, bail};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// How long a claim or a heartbeat leases its job: what a machine that dies
/// mid-step costs before another takes the job over. `cloud_jobs`'
/// ([`super::cloud::LEASE_MS`]).
pub const LEASE_MS: u64 = super::cloud::LEASE_MS;

/// The furthest ahead a heartbeat may hold its job, from its own clock.
pub const MAX_EXTEND_MS: u64 = 3 * LEASE_MS;

/// The first wait after a failure (`cloud::worker`'s), doubled for each
/// failure in a row up to [`RETRY_MAX_MS`].
pub const RETRY_MS: u64 = 10_000;
pub const RETRY_MAX_MS: u64 = 3_600_000;

/// How many claims in a row a job gets before it ends `failed`, unless its
/// kind says otherwise.
pub const MAX_ATTEMPTS: u32 = 8;

/// How long a job waits after its `attempts`th claim in a row failed.
pub fn backoff(attempts: u32) -> u64 {
    (RETRY_MS << attempts.saturating_sub(1).min(16)).min(RETRY_MAX_MS)
}

/// What a job is for, and so which handler steps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobKind {
    Workspace,
    Run,
    Deploy,
    Schedule,
}

impl JobKind {
    pub const ALL: [JobKind; 4] = [JobKind::Workspace, JobKind::Run, JobKind::Deploy, JobKind::Schedule];

    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Workspace => "workspace",
            JobKind::Run => "run",
            JobKind::Deploy => "deploy",
            JobKind::Schedule => "schedule",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match JobKind::ALL.into_iter().find(|k| k.as_str() == s) {
            Some(k) => Ok(k),
            None => bail!("unknown job kind {s:?}"),
        }
    }
}

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEnd {
    Done,
    Failed,
    Cancelled,
}

impl JobEnd {
    fn as_str(self) -> &'static str {
        match self {
            JobEnd::Done => "done",
            JobEnd::Failed => "failed",
            JobEnd::Cancelled => "cancelled",
        }
    }
}

/// A job a claim holds. Its handler changes `checkpoint` and writes it back
/// under `lease`.
#[derive(Debug, Clone)]
pub struct LeasedJob {
    pub id: String,
    pub kind: JobKind,
    pub payload: Value,
    pub checkpoint: Option<Value>,
    /// Claims since it last handed itself back cleanly, this one included.
    pub attempts: u32,
    pub created: u64,
    pub lease: String,
    /// Someone asked it to stop: set at the claim, and by a heartbeat that
    /// finds a cancel while it runs. Shared by the job's clones.
    pub cancel: Arc<AtomicBool>,
}

impl LeasedJob {
    /// Whether someone asked it to stop, as of its claim or last heartbeat.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// A job as it is, for reading (tests, operators).
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub payload: Value,
    pub checkpoint: Option<Value>,
    pub state: String,
    pub cancel: bool,
    pub attempts: u32,
    pub error: Option<String>,
    pub next_at: u64,
    pub lease_until: u64,
    pub created: u64,
    pub updated: u64,
}

/// A write named a lease that has run out or that another claim replaced
/// (or the job ended).
#[derive(Debug)]
pub struct LeaseLost;

impl std::fmt::Display for LeaseLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("job lease lost")
    }
}

impl std::error::Error for LeaseLost {}

/// What a claim's `SELECT` ends with: on Postgres it locks the job it finds
/// and passes over any that another machine's claim is locking (M144).
fn skip_locked(tx: &impl Sql) -> &'static str {
    match tx.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

fn decode(s: Option<String>) -> Result<Option<Value>> {
    Ok(s.map(|s| serde_json::from_str(&s)).transpose()?)
}

fn encode(job: &LeasedJob) -> Result<Option<String>> {
    Ok(job.checkpoint.as_ref().map(serde_json::to_string).transpose()?)
}

fn changed_one(changed: usize) -> Result<()> {
    if changed != 1 {
        bail!(LeaseLost);
    }
    Ok(())
}

/// The condition every write under a lease has: `?1` the job, `?2` its
/// lease, `?3` now. The lease must not have run out.
const HELD: &str = "id=?1 AND lease=?2 AND state='pending' AND lease_until>?3";

const COLUMNS: &str = "id,kind,payload,checkpoint,state,cancel,attempts,error,next_at,lease_until,created,updated";

fn job_row(r: &Row) -> Result<JobRow> {
    Ok(JobRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        payload: serde_json::from_str(&r.get::<String>(2)?)?,
        checkpoint: decode(r.get(3)?)?,
        state: r.get(4)?,
        cancel: r.get(5)?,
        attempts: r.get(6)?,
        error: r.get(7)?,
        next_at: r.get(8)?,
        lease_until: r.get(9)?,
        created: r.get(10)?,
        updated: r.get(11)?,
    })
}

/// Whether `job` still holds its lease at `now`, locking its row until `tx`
/// ends so no claim takes it meanwhile. What a kind's own writes check
/// first, in their transaction, to write only while holding the job (as
/// `append_conversation_events` does for `cloud_jobs`); [`Db::under_lease`]
/// does both.
pub fn holds(tx: &impl Sql, job: &LeasedJob, now: u64) -> Result<bool> {
    Ok(tx
        .query_row(
            &format!("SELECT id FROM platform_jobs WHERE {HELD}{}", tx.for_update()),
            params![job.id, job.lease, now],
            |r| r.get::<String>(0),
        )
        .optional()?
        .is_some())
}

/// Write `job`'s checkpoint under its lease, in `tx` (for a kind that
/// writes its own rows in the same transaction). With `release_at` it
/// hands the job back, due again then (or at once if it was cancelled),
/// and its failures in a row start over; without, it keeps holding it.
pub fn checkpoint(tx: &impl Sql, job: &LeasedJob, release_at: Option<u64>, now: u64) -> Result<()> {
    let checkpoint = encode(job)?;
    changed_one(match release_at {
        None => tx.execute(
            &format!("UPDATE platform_jobs SET checkpoint=?4,updated=?3 WHERE {HELD}"),
            params![job.id, job.lease, now, checkpoint],
        )?,
        Some(at) => tx.execute(
            &format!(
                "UPDATE platform_jobs SET checkpoint=?4,updated=?3,lease_until=0,attempts=0,error=NULL,
                     next_at=CASE WHEN cancel THEN ?3 ELSE ?5 END
                 WHERE {HELD}"
            ),
            params![job.id, job.lease, now, checkpoint, at],
        )?,
    })
}

/// [`Db::enqueue_job`] in the caller's transaction: a job queued with the
/// rows it's for, or not at all.
pub fn enqueue(tx: &impl Sql, id: &str, kind: JobKind, payload: &Value, next_at: u64, now: u64) -> Result<bool> {
    let added = tx.execute(
        "INSERT INTO platform_jobs(id,kind,payload,next_at,created,updated) VALUES(?1,?2,?3,?4,?5,?5)
         ON CONFLICT DO NOTHING",
        params![id, kind.as_str(), serde_json::to_string(payload)?, next_at, now],
    )?;
    Ok(added == 1)
}

/// Queue job `id`, or make the one there due at `next_at`, in the caller's
/// transaction: for a job that stands for something lasting (a schedule,
/// M214) whose time changed. One that had ended is pending again, its
/// failures forgotten. One a step holds now keeps its lease, and is asked
/// to look again: its `cancel` is set, so its holder's hand-back makes it
/// due at once (rather than when the step, which read the old state, said)
/// and the next step sees the change. A kind that's requeued so reads
/// `cancel` as "look again", not "stop", and clears it ([`Db::uncancel_job`]).
pub fn requeue(tx: &impl Sql, id: &str, kind: JobKind, payload: &Value, next_at: u64, now: u64) -> Result<()> {
    tx.execute(
        "INSERT INTO platform_jobs(id,kind,payload,next_at,created,updated) VALUES(?1,?2,?3,?4,?5,?5)
         ON CONFLICT (id) DO UPDATE SET next_at=?4,updated=?5,
             cancel=CASE WHEN platform_jobs.state='pending' AND platform_jobs.lease_until>?5 THEN TRUE ELSE FALSE END,
             attempts=CASE WHEN platform_jobs.state='pending' THEN platform_jobs.attempts ELSE 0 END,
             error=CASE WHEN platform_jobs.state='pending' THEN platform_jobs.error ELSE NULL END,
             state='pending'",
        params![id, kind.as_str(), serde_json::to_string(payload)?, next_at, now],
    )?;
    Ok(())
}

impl Db {
    /// Queue a job, due at `next_at`. `false` if a job with this id is
    /// there already (a create racing the same request on another machine).
    pub fn enqueue_job(&self, id: &str, kind: JobKind, payload: &Value, next_at: u64, now: u64) -> Result<bool> {
        enqueue(&self.c(), id, kind, payload, next_at, now)
    }

    /// [`Db::claim_job_within`], every kind allowed [`MAX_ATTEMPTS`].
    pub fn claim_job(&self, kinds: &[JobKind], now: u64) -> Result<Option<LeasedJob>> {
        let limits: Vec<(JobKind, u32)> = kinds.iter().map(|k| (*k, MAX_ATTEMPTS)).collect();
        self.claim_job_within(&limits, now)
    }

    /// Claim the job of one of `kinds` that has waited longest, and lease
    /// it until `now` + [`LEASE_MS`]: a bounded step of its work, not its
    /// whole life. Each kind comes with its most attempts: a job claimed
    /// that many times in a row without handing itself back ends `failed`
    /// here instead, and the claim looks again.
    pub fn claim_job_within(&self, kinds: &[(JobKind, u32)], now: u64) -> Result<Option<LeasedJob>> {
        if kinds.is_empty() {
            return Ok(None);
        }
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let names: Vec<&str> = kinds.iter().map(|(k, _)| k.as_str()).collect();
        let marks = (0..names.len()).map(|i| format!("?{}", i + 2)).collect::<Vec<_>>().join(",");
        let mut p: Vec<&dyn Param> = vec![&now];
        p.extend(super::sql::list(&names));
        let select = format!(
            "SELECT id,kind,payload,checkpoint,attempts,cancel,created FROM platform_jobs
             WHERE state='pending' AND next_at<=?1 AND lease_until<=?1 AND kind IN ({marks})
             ORDER BY next_at,created LIMIT 1{}",
            skip_locked(&tx)
        );
        loop {
            let found = tx
                .query_row(&select, &p, |r| {
                    Ok((
                        r.get::<String>(0)?,
                        JobKind::parse(&r.get::<String>(1)?)?,
                        r.get::<String>(2)?,
                        r.get::<Option<String>>(3)?,
                        r.get::<u32>(4)?,
                        r.get::<bool>(5)?,
                        r.get::<u64>(6)?,
                    ))
                })
                .optional()?;
            let Some((id, kind, payload, checkpoint, attempts, cancel, created)) = found else {
                // Commit what it ended on the way.
                tx.commit()?;
                return Ok(None);
            };
            let most = kinds.iter().find(|(k, _)| *k == kind).map_or(MAX_ATTEMPTS, |(_, n)| *n);
            if attempts >= most {
                tx.execute(
                    "UPDATE platform_jobs SET state='failed',lease=NULL,lease_until=0,updated=?2,
                         error=?3 || COALESCE(': ' || error, '')
                     WHERE id=?1",
                    params![id, now, format!("gave up after {attempts} attempts")],
                )?;
                continue;
            }
            let lease = hex::encode(arugula_e2e::random::<16>());
            let changed = tx.execute(
                "UPDATE platform_jobs SET lease=?1,lease_until=?2,attempts=attempts+1,updated=?4
                 WHERE id=?3 AND lease_until<=?4 AND state='pending'",
                params![lease, now + LEASE_MS, id, now],
            )?;
            if changed != 1 {
                tx.commit()?;
                return Ok(None);
            }
            tx.commit()?;
            return Ok(Some(LeasedJob {
                id,
                kind,
                payload: serde_json::from_str(&payload)?,
                checkpoint: decode(checkpoint)?,
                attempts: attempts + 1,
                created,
                lease,
                cancel: Arc::new(AtomicBool::new(cancel)),
            }));
        }
    }

    /// `f` in one transaction that first checks `job` still holds its lease
    /// at `now` ([`holds`], [`LeaseLost`] if not) and keeps its row locked:
    /// how a kind wins its own state change before an outside effect.
    pub fn under_lease<T>(&self, job: &LeasedJob, now: u64, f: impl FnOnce(&Tx<'_, '_>) -> Result<T>) -> Result<T> {
        let mut conn = self.c();
        let tx = conn.immediate()?;
        if !holds(&tx, job, now)? {
            bail!(LeaseLost);
        }
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Write `job`'s checkpoint while it holds the lease ([`checkpoint`]):
    /// before a step whose effect must not be repeated, so a machine that
    /// takes the job over finds it.
    pub fn checkpoint_job(&self, job: &LeasedJob, now: u64) -> Result<()> {
        checkpoint(&self.c(), job, None, now)
    }

    /// Write `job`'s checkpoint and hand it back, due again at `next_at`
    /// (at once if it was cancelled).
    pub fn release_job(&self, job: &LeasedJob, next_at: u64, now: u64) -> Result<()> {
        checkpoint(&self.c(), job, Some(next_at), now)
    }

    /// Keep holding `job` until `until`, at most [`MAX_EXTEND_MS`] past
    /// `now` (a heartbeat). Refused once the lease has run out: it can't be
    /// brought back. Whether the job has been cancelled since it was
    /// claimed, which it also sets on `job`.
    pub fn extend_job(&self, job: &LeasedJob, until: u64, now: u64) -> Result<bool> {
        let until = until.min(now + MAX_EXTEND_MS);
        let cancel = self
            .c()
            .query_row(
                &format!("UPDATE platform_jobs SET lease_until=?4,updated=?3 WHERE {HELD} RETURNING cancel"),
                params![job.id, job.lease, now, until],
                |r| r.get::<bool>(0),
            )
            .optional()?;
        let Some(cancel) = cancel else { bail!(LeaseLost) };
        if cancel {
            job.cancel.store(true, Ordering::SeqCst);
        }
        Ok(cancel)
    }

    /// `job`'s step failed: hand it back after a [`backoff`] that grows
    /// with its failures in a row (at once if it was cancelled), keeping
    /// its checkpoint, or end it `failed` once it has been tried
    /// `max_attempts` times. `true` if it ended.
    pub fn fail_job(&self, job: &LeasedJob, error: &str, max_attempts: u32, now: u64) -> Result<bool> {
        self.fail(job, Some(encode(job)?), error, max_attempts, now)
    }

    /// [`Db::fail_job`], leaving the checkpoint as it was last written: for
    /// a step that panicked, whose `job` may be half changed.
    pub fn fail_job_as_written(&self, job: &LeasedJob, error: &str, max_attempts: u32, now: u64) -> Result<bool> {
        self.fail(job, None, error, max_attempts, now)
    }

    fn fail(
        &self,
        job: &LeasedJob,
        checkpoint: Option<Option<String>>,
        error: &str,
        max_attempts: u32,
        now: u64,
    ) -> Result<bool> {
        let end = job.attempts >= max_attempts;
        let write = checkpoint.is_some();
        let checkpoint = checkpoint.flatten();
        changed_one(if end {
            self.c().execute(
                &format!(
                    "UPDATE platform_jobs SET checkpoint=CASE WHEN ?6 THEN ?4 ELSE checkpoint END,updated=?3,error=?5,
                         state='failed',lease=NULL,lease_until=0
                     WHERE {HELD}"
                ),
                params![job.id, job.lease, now, checkpoint, error, write],
            )?
        } else {
            self.c().execute(
                &format!(
                    "UPDATE platform_jobs SET checkpoint=CASE WHEN ?6 THEN ?4 ELSE checkpoint END,updated=?3,error=?5,
                         lease_until=0,next_at=CASE WHEN cancel THEN ?3 ELSE ?7 END
                     WHERE {HELD}"
                ),
                params![job.id, job.lease, now, checkpoint, error, write, now + backoff(job.attempts)],
            )?
        })?;
        Ok(end)
    }

    /// End `job` under its lease. It's never claimed again.
    pub fn finish_job(&self, job: &LeasedJob, end: JobEnd, error: Option<&str>, now: u64) -> Result<()> {
        changed_one(self.c().execute(
            &format!(
                "UPDATE platform_jobs SET checkpoint=?4,updated=?3,state=?5,error=COALESCE(?6,error),lease=NULL,lease_until=0
                 WHERE {HELD}"
            ),
            params![job.id, job.lease, now, encode(job)?, end.as_str(), error],
        )?)
    }

    /// Clear `job`'s `cancel` under its lease: for a kind that reads it as
    /// "look again" ([`requeue`]) once it has.
    pub fn uncancel_job(&self, job: &LeasedJob, now: u64) -> Result<()> {
        changed_one(self.c().execute(
            &format!("UPDATE platform_jobs SET cancel=FALSE,updated=?3 WHERE {HELD}"),
            params![job.id, job.lease, now],
        )?)?;
        job.cancel.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Ask job `id` to stop: its handler sees it at its next claim, which is
    /// made due now, or at its next heartbeat if it's running. `false` if it
    /// has ended or isn't there.
    pub fn cancel_job(&self, id: &str, now: u64) -> Result<bool> {
        let changed = self.c().execute(
            "UPDATE platform_jobs SET cancel=TRUE,next_at=?2,updated=?2 WHERE id=?1 AND state='pending'",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    pub fn platform_job(&self, id: &str) -> Result<Option<JobRow>> {
        self.c().query_row(&format!("SELECT {COLUMNS} FROM platform_jobs WHERE id=?1"), params![id], job_row).optional()
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
pub(crate) mod tests;
