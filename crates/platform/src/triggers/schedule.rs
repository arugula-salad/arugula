//! Schedules (M214): a spec's cron expression read in its time zone, and
//! the `schedule` kind on `platform_jobs` that fires it. The platform
//! schedules and the workspace's machine executes: a tick makes a run and
//! queues its `run` job, which M212 carries to the workspace.
//!
//! Each schedule has one job, its own id, due at its next tick. A step
//! (under the job's lease, so one machine at a time):
//! - reads the schedule: gone ends the job, off waits for a day (turning
//!   it on makes the job due again), not yet due waits for its tick;
//! - otherwise works out the first tick after now and fires
//!   ([`db::triggers::fire`]) in one transaction that holds the lease: it
//!   wins the tick (the schedule still due at the tick read), makes the
//!   run under the key `schedule:<id>:<tick>` and queues its job.
//!
//! So a tick missed while the platform was down fires once, when it's back,
//! and the next is the first after now; a machine that dies after firing
//! leaves a schedule whose next tick is ahead, and a step whose lease ran
//! out writes nothing.
//!
//! [`db::triggers::fire`]: crate::db::triggers::fire

use anyhow::Result;
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};
use futures_util::future::BoxFuture;
use jiff::{Timestamp, tz::TimeZone};

use crate::{
    db::{
        Db,
        jobs::{JobKind, LeasedJob},
        triggers::{self, Fired},
    },
    jobs::{Clock, Handler, Step},
};

/// How long a schedule that's off waits before its job looks again.
/// Turning it on makes the job due at once.
pub const IDLE_MS: u64 = 24 * 3_600_000;

/// The least time between two ticks: a schedule starts a run on a hosted
/// workspace each time.
pub const MIN_GAP_MS: u64 = 5 * 60_000;

/// A cron expression: five fields (minute, hour, day of month, month, day
/// of week), as Vixie cron reads them, or a nickname such as `@daily`. No
/// seconds and no years.
pub fn parse(expr: &str) -> Result<Cron, String> {
    let expr = expr.trim();
    if expr.is_empty() || expr.len() > 128 {
        return Err("a cron expression: five fields, such as `0 9 * * 1-5`".into());
    }
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expr)
        .map_err(|e| format!("not a cron expression: {e}"))
}

/// A time zone by IANA's name (`UTC`, `America/Chicago`).
pub fn zone(name: &str) -> Result<TimeZone, String> {
    if name == "UTC" {
        return Ok(TimeZone::UTC);
    }
    TimeZone::get(name).map_err(|_| format!("no time zone called {name:?}: use IANA's names, such as America/Chicago"))
}

/// The first tick of `cron` in `tz` after `after` (ms).
pub fn next_after(cron: &Cron, tz: &TimeZone, after: u64) -> Result<u64, String> {
    let at = Timestamp::from_millisecond(i64::try_from(after).map_err(|_| "a time out of range")?)
        .map_err(|e| e.to_string())?
        .to_zoned(tz.clone());
    let next = cron.find_next_occurrence(&at, false).map_err(|e| format!("no next tick: {e}"))?;
    u64::try_from(next.timestamp().as_millisecond()).map_err(|_| "a tick before 1970".into())
}

/// [`next_after`] from an expression and a zone's name, as kept.
pub fn next_tick(cron: &str, timezone: &str, after: u64) -> Result<u64, String> {
    next_after(&parse(cron)?, &zone(timezone)?, after)
}

/// Check an expression and a zone a person gave: both read, and no two of
/// the next ticks closer than [`MIN_GAP_MS`]. Its next tick after `now`.
pub fn check(cron: &str, timezone: &str, now: u64) -> Result<u64, String> {
    let (c, tz) = (parse(cron)?, zone(timezone)?);
    let first = next_after(&c, &tz, now)?;
    let mut last = first;
    for _ in 0..24 {
        let next = next_after(&c, &tz, last)?;
        if next - last < MIN_GAP_MS {
            return Err(format!("a schedule ticks at most every {} minutes", MIN_GAP_MS / 60_000));
        }
        last = next;
    }
    Ok(first)
}

/// The `schedule` kind's handler: fires schedules as they come due, while
/// their makers may ([`super::Gate`]).
pub struct Ticks {
    pub gate: std::sync::Arc<dyn super::Gate>,
}

impl Handler for Ticks {
    fn kind(&self) -> JobKind {
        JobKind::Schedule
    }

    /// Never given up on for failing: a schedule's job lasts as long as the
    /// schedule, and a failure (the database, the hub) backs off up to an
    /// hour and tries again. It ends `failed` only when its expression or
    /// zone can't be read, saying so on the schedule.
    fn max_attempts(&self) -> u32 {
        u32::MAX
    }

    /// A step is a few statements and a question to the hub.
    fn deadline(&self) -> u64 {
        60_000
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(async move {
            let now = clock.now();
            // `cancel` is "look again" here (`jobs::requeue`): the schedule
            // changed while a step held it. This step reads it afresh.
            if job.cancelled() {
                db.uncancel_job(job, now)?;
            }
            let Some(s) = db.run_schedule(&job.id)? else { return Ok(Step::Done) };
            if !s.enabled {
                return Ok(Step::Again(now + IDLE_MS));
            }
            if s.next_at > now {
                return Ok(Step::Again(s.next_at));
            }
            let next = match next_tick(&s.cron, &s.timezone, now) {
                Ok(n) => n,
                // Checked when it was kept: a zone gone from IANA's list, or
                // an expression this build reads otherwise.
                Err(e) => {
                    let why = format!("stopped: {e}");
                    db.schedule_stopped(&s.id, &why)?;
                    return Ok(Step::Failed(why));
                }
            };
            let refused = match db.run_spec_by_id(&s.spec)? {
                Some(spec) => self.gate.refuses(&spec.project, s.created_by.as_deref()).await?,
                None => None,
            };
            let fired = db.under_lease(job, now, |tx| triggers::fire(tx, &s, next, refused, now))?;
            match &fired {
                Fired::Run(run) => {
                    tracing::info!(schedule = %s.id, tick = s.next_at, run = %run.id, late_ms = now - s.next_at, "a schedule fired")
                }
                Fired::Skipped(why) => {
                    tracing::warn!(schedule = %s.id, tick = s.next_at, why, "a schedule's tick made no run")
                }
                // Changed as it fired: read it again.
                Fired::Moved => return Ok(Step::Again(now)),
            }
            Ok(Step::Again(next))
        })
    }
}
