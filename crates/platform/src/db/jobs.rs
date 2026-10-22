//! The platform's one leased job table, `platform_jobs` (M208): the
//! background work of workspaces, runs, deploys and schedules, which the
//! platform's machines take turns at ([`crate::jobs`] is the worker).
//!
//! The lease is `cloud_jobs`' (M144), kept as it is so M190 can move the
//! Cloud kinds here unchanged:
//! - A claim takes one due job and leases it for [`LEASE_MS`] under a new
//!   random `lease`. On Postgres its `SELECT` is `FOR UPDATE SKIP LOCKED`,
//!   so two machines never take the same job and neither waits for the
//!   other; SQLite's `BEGIN IMMEDIATE` makes claims take turns.
//! - Every write names the lease it holds and is refused ([`LeaseLost`])
//!   once another claim has taken the job: a machine that stalled past its
//!   lease can't overwrite the one that took over. Until then it may still
//!   write, as with `cloud_jobs`.
//! - A machine that dies holding a job leaves it to the next claim when
//!   the lease runs out, which carries on from the last checkpoint.
use super::{
    Db,
    migrate::Dialect,
    sql::{Optional as _, Param, Row, Sql, params},
};
use anyhow::{Result, bail};
use serde_json::Value;

/// How long a claim leases its job: what a machine that dies mid-step costs
/// before another takes the job over. `cloud_jobs`' ([`super::cloud::LEASE_MS`]).
pub const LEASE_MS: u64 = super::cloud::LEASE_MS;

/// The first wait after a failure (`cloud::worker`'s), doubled for each
/// failure in a row up to [`RETRY_MAX_MS`].
pub const RETRY_MS: u64 = 10_000;
pub const RETRY_MAX_MS: u64 = 3_600_000;

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
    /// Someone asked it to stop.
    pub cancel: bool,
    pub created: u64,
    pub lease: String,
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

/// A write named a lease another claim has replaced (or the job ended).
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

/// Whether `job` still holds its lease, locking its row until `tx` ends so
/// no claim takes it meanwhile. What a kind's own writes check first, in
/// their transaction, to write only while holding the job (as
/// `append_conversation_events` does for `cloud_jobs`).
pub fn holds(tx: &impl Sql, job: &LeasedJob) -> Result<bool> {
    Ok(tx
        .query_row(
            &format!("SELECT id FROM platform_jobs WHERE id=?1 AND lease=?2 AND state='pending'{}", tx.for_update()),
            params![job.id, job.lease],
            |r| r.get::<String>(0),
        )
        .optional()?
        .is_some())
}

/// Write `job`'s checkpoint under its lease, in `tx` (for a kind that
/// writes its own rows in the same transaction). With `release_at` it
/// hands the job back, due again then, and its failures in a row start
/// over; without, it keeps holding it.
pub fn checkpoint(tx: &impl Sql, job: &LeasedJob, release_at: Option<u64>, now: u64) -> Result<()> {
    let checkpoint = job.checkpoint.as_ref().map(serde_json::to_string).transpose()?;
    let changed = match release_at {
        None => tx.execute(
            "UPDATE platform_jobs SET checkpoint=?1,updated=?2 WHERE id=?3 AND lease=?4 AND state='pending'",
            params![checkpoint, now, job.id, job.lease],
        )?,
        Some(at) => tx.execute(
            "UPDATE platform_jobs SET checkpoint=?1,updated=?2,next_at=?5,lease_until=0,attempts=0,error=NULL
             WHERE id=?3 AND lease=?4 AND state='pending'",
            params![checkpoint, now, job.id, job.lease, at],
        )?,
    };
    if changed != 1 {
        bail!(LeaseLost);
    }
    Ok(())
}

impl Db {
    /// Queue a job, due at `next_at`. `false` if a job with this id is
    /// there already (a create racing the same request on another machine).
    pub fn enqueue_job(&self, id: &str, kind: JobKind, payload: &Value, next_at: u64, now: u64) -> Result<bool> {
        let added = self.c().execute(
            "INSERT INTO platform_jobs(id,kind,payload,next_at,created,updated) VALUES(?1,?2,?3,?4,?5,?5)
             ON CONFLICT DO NOTHING",
            params![id, kind.as_str(), serde_json::to_string(payload)?, next_at, now],
        )?;
        Ok(added == 1)
    }

    /// Claim the job of one of `kinds` that has waited longest, and lease
    /// it until `now` + [`LEASE_MS`]. A bounded step of its work, not its
    /// whole life.
    pub fn claim_job(&self, kinds: &[JobKind], now: u64) -> Result<Option<LeasedJob>> {
        if kinds.is_empty() {
            return Ok(None);
        }
        let mut conn = self.c();
        let tx = conn.immediate()?;
        let names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        let marks = (0..names.len()).map(|i| format!("?{}", i + 2)).collect::<Vec<_>>().join(",");
        let mut p: Vec<&dyn Param> = vec![&now];
        p.extend(super::sql::list(&names));
        let found = tx
            .query_row(
                &format!(
                    "SELECT id,kind,payload,checkpoint,attempts,cancel,created FROM platform_jobs
                     WHERE state='pending' AND next_at<=?1 AND lease_until<=?1 AND kind IN ({marks})
                     ORDER BY next_at,created LIMIT 1{}",
                    skip_locked(&tx)
                ),
                &p,
                |r| {
                    Ok((
                        r.get::<String>(0)?,
                        r.get::<String>(1)?,
                        r.get::<String>(2)?,
                        r.get::<Option<String>>(3)?,
                        r.get::<u32>(4)?,
                        r.get::<bool>(5)?,
                        r.get::<u64>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, kind, payload, checkpoint, attempts, cancel, created)) = found else {
            return Ok(None);
        };
        let lease = hex::encode(arugula_e2e::random::<16>());
        let changed = tx.execute(
            "UPDATE platform_jobs SET lease=?1,lease_until=?2,attempts=attempts+1,updated=?4
             WHERE id=?3 AND lease_until<=?4 AND state='pending'",
            params![lease, now + LEASE_MS, id, now],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(LeasedJob {
            id,
            kind: JobKind::parse(&kind)?,
            payload: serde_json::from_str(&payload)?,
            checkpoint: decode(checkpoint)?,
            attempts: attempts + 1,
            cancel,
            created,
            lease,
        }))
    }

    /// Write `job`'s checkpoint while it holds the lease ([`checkpoint`]):
    /// before a step whose effect must not be repeated, so a machine that
    /// takes the job over finds it.
    pub fn checkpoint_job(&self, job: &LeasedJob, now: u64) -> Result<()> {
        checkpoint(&self.c(), job, None, now)
    }

    /// Write `job`'s checkpoint and hand it back, due again at `next_at`.
    pub fn release_job(&self, job: &LeasedJob, next_at: u64, now: u64) -> Result<()> {
        checkpoint(&self.c(), job, Some(next_at), now)
    }

    /// Keep holding `job` until `until` (a heartbeat, for a step longer
    /// than [`LEASE_MS`]).
    pub fn extend_job(&self, job: &LeasedJob, until: u64, now: u64) -> Result<()> {
        let changed = self.c().execute(
            "UPDATE platform_jobs SET lease_until=?1,updated=?2 WHERE id=?3 AND lease=?4 AND state='pending'",
            params![until, now, job.id, job.lease],
        )?;
        if changed != 1 {
            bail!(LeaseLost);
        }
        Ok(())
    }

    /// `job`'s step failed: hand it back after a [`backoff`] that grows
    /// with its failures in a row, keeping its checkpoint, or end it
    /// `failed` once it has been tried `max_attempts` times. `true` if it
    /// ended.
    pub fn fail_job(&self, job: &LeasedJob, error: &str, max_attempts: u32, now: u64) -> Result<bool> {
        let checkpoint = job.checkpoint.as_ref().map(serde_json::to_string).transpose()?;
        let end = job.attempts >= max_attempts;
        let changed = if end {
            self.c().execute(
                "UPDATE platform_jobs SET checkpoint=?1,updated=?2,error=?5,state='failed',lease=NULL,lease_until=0
                 WHERE id=?3 AND lease=?4 AND state='pending'",
                params![checkpoint, now, job.id, job.lease, error],
            )?
        } else {
            self.c().execute(
                "UPDATE platform_jobs SET checkpoint=?1,updated=?2,error=?5,next_at=?6,lease_until=0
                 WHERE id=?3 AND lease=?4 AND state='pending'",
                params![checkpoint, now, job.id, job.lease, error, now + backoff(job.attempts)],
            )?
        };
        if changed != 1 {
            bail!(LeaseLost);
        }
        Ok(end)
    }

    /// End `job` under its lease. It's never claimed again.
    pub fn finish_job(&self, job: &LeasedJob, end: JobEnd, error: Option<&str>, now: u64) -> Result<()> {
        let checkpoint = job.checkpoint.as_ref().map(serde_json::to_string).transpose()?;
        let changed = self.c().execute(
            "UPDATE platform_jobs SET checkpoint=?1,updated=?2,state=?5,error=COALESCE(?6,error),lease=NULL,lease_until=0
             WHERE id=?3 AND lease=?4 AND state='pending'",
            params![checkpoint, now, job.id, job.lease, end.as_str(), error],
        )?;
        if changed != 1 {
            bail!(LeaseLost);
        }
        Ok(())
    }

    /// Ask job `id` to stop: its handler sees `cancel` at its next claim,
    /// which is made due now. `false` if it has ended or isn't there.
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
