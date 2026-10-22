//! Hosted workspaces' lifecycle on the platform's leased worker (M179).
//! Each tick leases one workspace with something to do
//! ([`Db::claim_hosted`](crate::db::Db::claim_hosted)) and takes one step:
//!
//! | State | Step |
//! |---|---|
//! | anything, delete asked | delete every box it has (by its metadata too), then `deleted` |
//! | `creating` | the create's journal: ask the hub for a ticket (ticket mode), note `sent`, make the box; `sent` with no answer kept: find it by its metadata, never make it again; `made`: record its enrollment by sandbox id |
//! | `enrolling` | wait for the hub to say its box enrolled, then `ready`; past the enrollment's end, delete the box (fail closed) |
//! | `ready`, `paused` | a rebuild, wake or pause asked; else read its state (never waking it), and near the provider's runtime cap pause and resume it between turns ([`super::hold`]) |
//!
//! A step that fails for now (the provider or the hub didn't answer) is
//! tried again in a few seconds, and what it was doing stays asked. A
//! failed workspace waits for a person to rebuild or delete it.
//!
//! Each step that goes through says whether the box runs, and its
//! checkpoint meters it ([`super::usage`]).

use std::{
    sync::{Arc, atomic::Ordering::SeqCst},
    time::Duration,
};

use anyhow::Result;

use super::{
    Ask,
    enroll::{EnrollAsk, EnrollGiven, LATE, NO_DELEGATION, SANDBOX_MS, TAKEN},
};
use crate::{
    Platform,
    compute::{self, Create, State, Substrate},
    db::hosted::HostedJob,
};

/// How long a box runs without being held before it pauses itself
/// (memory and disk kept), unless `ARUGULA_HOSTED_IDLE_TIMEOUT` sets
/// another ([`super::Config::idle_timeout`]). [`super::hold`] holds a busy
/// one past it.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How soon a step that failed for now is tried again.
const RETRY_MS: u64 = 5_000;
/// How often an enrolling box is looked at.
const ENROLLING_MS: u64 = 2_000;
/// How often a running box's state is read (to see it pause itself), and
/// a paused one's (to see it gone).
const READY_MS: u64 = 60_000;
const PAUSED_MS: u64 = 600_000;
/// How often a running box is looked at in the last stretch before the
/// provider's runtime cap, for an idle moment to pause and resume it in;
/// and how soon after the cap one that ran to it is.
const CAP_LOOK_MS: u64 = 15_000;
/// How recently a box that ran to the cap has to have paused to be woken
/// as one paused mid-turn.
const CAP_WAKE_MS: u64 = 120_000;
/// How long after a create whose answer was lost the box is looked for
/// by its metadata before it's taken as never made.
const LOST_MS: u64 = 60_000;
/// How long past its enrollment's end a box has to have enrolled (the
/// hub's answer may lag the box's enroll).
const ENROLL_GRACE_MS: u64 = 30_000;

pub(crate) const GONE: &str = "The box is gone at the provider. Rebuild the workspace to make another.";
/// A wake the provider didn't answer: it's tried again, and still asked.
pub(crate) const UNAVAILABLE: &str = "The compute provider isn't answering. Arugula keeps trying to wake the box.";
const NOT_MADE: &str = "The provider didn't make the box. Rebuild the workspace to try again.";

/// Two workers on each of the platform's machines, as the cloud worker
/// runs: a claim leases its workspace, so two machines never take the
/// same one, and a machine that's stopping takes no new one.
pub fn start(app: Arc<Platform>) {
    for _ in 0..2 {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                let mut worked = false;
                if !app.is_paused() && !app.is_stopping() {
                    app.busy.fetch_add(1, SeqCst);
                    match tick(&app).await {
                        Ok(w) => worked = w,
                        Err(error) => tracing::warn!(%error, "a hosted workspace's step failed"),
                    }
                    app.busy.fetch_sub(1, SeqCst);
                }
                let wait = if worked { 50 } else { 1_000 };
                // A wake someone's waiting on (M182) starts a tick now.
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
                    _ = app.wakes.kicked() => {}
                }
            }
        });
    }
}

/// One step on one workspace, if one is due: whether there was one.
pub async fn tick(app: &Platform) -> Result<bool> {
    tick_at(app, arugula_e2e::now_ms()).await
}

pub(crate) async fn tick_at(app: &Platform, now: u64) -> Result<bool> {
    let Some(sub) = app.compute.clone() else { return Ok(false) };
    let Some(mut job) = app.db.claim_hosted(now)? else { return Ok(false) };
    let (done, next) = match step(app, &*sub, &mut job, now).await {
        Ok(d) => {
            job.usage = Some(super::usage::observed(&job, now));
            d
        }
        Err(error) => {
            tracing::warn!(workspace = %job.w.id, %error, "a hosted workspace's step will be tried again");
            (None, now + RETRY_MS)
        }
    };
    app.db.checkpoint_hosted(&job, done, next, now)?;
    app.wakes.stepped();
    Ok(true)
}

/// What a step carried out of what was asked, and when the workspace is
/// due again.
type Step = (Option<Ask>, u64);

async fn step(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let asked = job.w.asked.clone();
    if asked.as_deref() == Some("delete") {
        delete(app, sub, job).await?;
        job.w.state = "deleted".into();
        job.deleted_at = Some(now);
        return Ok((Some(Ask::Delete), now));
    }
    match job.w.state.as_str() {
        "creating" => return create(app, sub, job, now).await,
        "enrolling" => return enrolling(app, sub, job, now).await,
        _ => {}
    }
    match asked.as_deref() {
        Some("rebuild") => {
            delete(app, sub, job).await?;
            job.w.state = "creating".into();
            job.w.error = None;
            job.w.template = None;
            job.built_hash = None;
            job.enroll_until = None;
            return Ok((Some(Ask::Rebuild), now));
        }
        Some("wake") => return wake(app, sub, job, now).await,
        Some("pause") => return pause(app, sub, job, now).await,
        _ => {}
    }
    if job.w.state == "failed" {
        return Ok((None, u64::MAX / 2));
    }
    refresh(app, sub, job, now).await
}

/// Fail the workspace, saying why.
fn fail(job: &mut HostedJob, why: &str, now: u64) -> Step {
    job.w.state = "failed".into();
    job.w.error = Some(why.into());
    job.stage = String::new();
    job.stage_at = Some(now);
    (None, now)
}

/// Delete every box the workspace has: the one it knows, and any its
/// metadata finds (a create whose answer was lost). Its machine leaves
/// the project.
async fn delete(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob) -> Result<()> {
    let mut boxes: Vec<String> = sub
        .list(&[("arugula_workspace".to_owned(), job.w.id.clone())].into())
        .await?
        .into_iter()
        .map(|s| s.id)
        .collect();
    boxes.extend(job.sandbox.clone());
    boxes.sort();
    boxes.dedup();
    for b in &boxes {
        sub.delete(b).await?;
    }
    app.hub.hosted_gone(std::slice::from_ref(&job.w.id)).await?;
    job.sandbox = None;
    job.w.machine = None;
    job.stage = String::new();
    job.stage_at = None;
    Ok(())
}

/// The create's journal (module docs).
async fn create(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    match job.stage.as_str() {
        "" => {
            let Some(template) = super::template(app) else {
                return Ok(fail(job, "This control has no template for hosted workspaces.", now));
            };
            let Some(ask) = enroll_ask(app, job, None)? else {
                return Ok(fail(job, "The workspace's project is gone.", now));
            };
            let mut env = std::collections::BTreeMap::new();
            if app.hosted.enroll == super::EnrollBy::Ticket {
                match app.hub.hosted_enrollment(&ask).await? {
                    EnrollGiven::Ready { ticket: Some(ticket), expires } => {
                        env.insert("ARUGULA_HOSTED_TICKET".to_owned(), ticket);
                        env.insert("ARUGULA_CONTROL_URL".to_owned(), app.cfg.public_url.clone());
                        job.enroll_until = Some(expires);
                    }
                    EnrollGiven::NoDelegation => return Ok(fail(job, NO_DELEGATION, now)),
                    EnrollGiven::Ready { ticket: None, .. } | EnrollGiven::Taken => {
                        anyhow::bail!("the hub made no ticket")
                    }
                }
            }
            job.stage = "sent".into();
            job.stage_at = Some(now);
            job.w.template = Some(template.clone());
            job.built_hash = Some(job.spec_hash.clone());
            app.db.save_hosted(job, now)?;
            let made = sub
                .create(&Create {
                    template: template.clone(),
                    env,
                    metadata: super::metadata(&job.w.id, &job.w.project, &template),
                    timeout: app.hosted.idle_timeout(),
                })
                .await;
            match made {
                Ok(c) => {
                    job.sandbox = Some(c.id);
                    job.stage = "made".into();
                    job.stage_at = Some(now);
                    Ok((None, now))
                }
                // This release's template isn't built yet (M177's release
                // job runs beside the deploy): nothing was made.
                Err(compute::Error::NotFound) if template == compute::template::current() => Ok(fail(
                    job,
                    &compute::template::CreateError::NotBuilt { template: template.clone() }.to_string(),
                    now,
                )),
                // Refused: nothing was made.
                Err(compute::Error::Refused { .. } | compute::Error::NotFound) => {
                    tracing::warn!(workspace = %job.w.id, error = %made.unwrap_err(), "a hosted workspace's box wasn't made");
                    Ok(fail(job, NOT_MADE, now))
                }
                // Maybe made: looked for by its metadata next.
                Err(e) => {
                    tracing::warn!(workspace = %job.w.id, error = %e, "a hosted workspace's create has no answer");
                    Ok((None, now + RETRY_MS))
                }
            }
        }
        "sent" => {
            let found = sub.list(&[("arugula_workspace".to_owned(), job.w.id.clone())].into()).await?;
            let Some((first, rest)) = found.split_first() else {
                if now < job.stage_at.unwrap_or(now) + LOST_MS {
                    return Ok((None, now + RETRY_MS));
                }
                return Ok(fail(job, NOT_MADE, now));
            };
            for extra in rest {
                sub.delete(&extra.id).await?;
            }
            job.seen = seen(first);
            job.sandbox = Some(first.id.clone());
            job.stage = "made".into();
            job.stage_at = Some(now);
            Ok((None, now))
        }
        _ => {
            if app.hosted.enroll == super::EnrollBy::Ticket {
                job.w.state = "enrolling".into();
                job.stage = String::new();
                return Ok((None, now + ENROLLING_MS));
            }
            let since = job.stage_at.unwrap_or(now);
            let Some(ask) = enroll_ask(app, job, job.sandbox.clone())? else {
                delete(app, sub, job).await?;
                return Ok(fail(job, "The workspace's project is gone.", now));
            };
            let given = match app.hub.hosted_enrollment(&ask).await {
                Ok(g) => g,
                Err(e) if now >= since + SANDBOX_MS => {
                    tracing::warn!(workspace = %job.w.id, error = %e, "the hub didn't record a hosted box's enrollment");
                    delete(app, sub, job).await?;
                    return Ok(fail(job, LATE, now));
                }
                Err(e) => return Err(e),
            };
            match given {
                EnrollGiven::Ready { expires, .. } => {
                    job.enroll_until = Some(expires);
                    job.w.state = "enrolling".into();
                    job.stage = String::new();
                    Ok((None, now + ENROLLING_MS))
                }
                EnrollGiven::NoDelegation => {
                    delete(app, sub, job).await?;
                    Ok(fail(job, NO_DELEGATION, now))
                }
                EnrollGiven::Taken => {
                    tracing::warn!(workspace = %job.w.id, "a hosted box's enrollment was taken: deleting it");
                    delete(app, sub, job).await?;
                    Ok(fail(job, TAKEN, now))
                }
            }
        }
    }
}

/// What the hub is asked to enroll for this workspace: `None` when its
/// project is gone.
fn enroll_ask(app: &Platform, job: &HostedJob, sandbox: Option<String>) -> Result<Option<EnrollAsk>> {
    let Some((account, team)) = app.db.project_holder(&job.w.project)? else { return Ok(None) };
    let owner = match (team, account) {
        (Some(t), _) => format!("team:{t}"),
        (None, Some(a)) => format!("account:{a}"),
        (None, None) => return Ok(None),
    };
    Ok(Some(EnrollAsk {
        project: job.w.project.clone(),
        owner,
        workspace: job.w.id.clone(),
        name: job.w.name.clone(),
        sandbox,
    }))
}

/// Wait for the box to enroll; delete it if it doesn't in time.
async fn enrolling(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    match app.hub.hosted_enrolled(std::slice::from_ref(&job.w.id)).await {
        Ok(found) => {
            if let Some(e) = found.into_iter().find(|e| e.workspace == job.w.id) {
                // Someone tried its enrollment again: the hub removed the
                // machine that used it, and the box goes (fail closed).
                if e.taken {
                    tracing::warn!(workspace = %job.w.id, "a hosted box's enrollment was tried again: deleting it");
                    delete(app, sub, job).await?;
                    return Ok(fail(job, TAKEN, now));
                }
                job.w.machine = Some(e.machine);
                job.w.state = "ready".into();
                job.w.error = None;
                job.enroll_until = None;
                return Ok((None, now + READY_MS));
            }
        }
        Err(e) => tracing::warn!(workspace = %job.w.id, error = %e, "the hub didn't say whether a hosted box enrolled"),
    }
    if now > job.enroll_until.unwrap_or(0) + ENROLL_GRACE_MS {
        delete(app, sub, job).await?;
        return Ok(fail(job, LATE, now));
    }
    Ok((None, now + ENROLLING_MS))
}

/// Resume a paused box, or one that may have paused itself since the
/// worker last looked (M182's wake on connect asks for that).
async fn wake(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let Some(sandbox) = job.sandbox.clone() else { return Ok(with(fail(job, GONE, now), Ask::Wake)) };
    match sub.resume(&sandbox, app.hosted.idle_timeout()).await {
        Ok(_) => {
            job.w.state = "ready".into();
            job.w.error = None;
            Ok((Some(Ask::Wake), now + READY_MS))
        }
        Err(compute::Error::NotFound) => Ok(with(fail(job, GONE, now), Ask::Wake)),
        Err(e @ compute::Error::Refused { .. }) => {
            job.w.error = Some(format!("Waking the box failed: {e}"));
            Ok((Some(Ask::Wake), now + PAUSED_MS))
        }
        // Not now: still asked, and tried again soon. Whoever waits on the
        // wake (M182) hears so.
        Err(e) => {
            tracing::warn!(workspace = %job.w.id, error = %e, "a hosted box's wake will be tried again");
            job.w.error = Some(UNAVAILABLE.into());
            Ok((None, now + RETRY_MS))
        }
    }
}

/// Pause a running box. Only between turns: while its daemon says busy
/// (or the hub can't say), the ask is done without pausing, and the box
/// pauses itself after the turn, since nothing holds it then.
async fn pause(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let Some(sandbox) = job.sandbox.clone() else { return Ok(with(fail(job, GONE, now), Ask::Pause)) };
    if !matches!(working(app, job).await, Ok(None | Some(false))) {
        tracing::info!(workspace = %job.w.id, "not pausing a hosted box mid-turn: it pauses itself after");
        return Ok((Some(Ask::Pause), now + READY_MS));
    }
    match sub.pause(&sandbox).await {
        Ok(_) => {
            job.w.state = "paused".into();
            Ok((Some(Ask::Pause), now + PAUSED_MS))
        }
        Err(compute::Error::NotFound) => Ok(with(fail(job, GONE, now), Ask::Pause)),
        Err(e @ compute::Error::Refused { .. }) => {
            job.w.error = Some(format!("Pausing the box failed: {e}"));
            Ok((Some(Ask::Pause), now + READY_MS))
        }
        Err(e) => Err(e.into()),
    }
}

fn with((_, next): Step, done: Ask) -> Step {
    (Some(done), next)
}

/// Read the box's state, never waking it: a running box that pauses
/// itself at its timeout shows `paused`. The leader holds a busy one
/// ([`super::hold`]); here, near the provider's runtime cap, a running box
/// is paused and resumed at an idle moment, and one that ran to the cap
/// mid-turn is woken.
async fn refresh(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let Some(sandbox) = job.sandbox.clone() else { return Ok(fail(job, GONE, now)) };
    match sub.status(&sandbox).await {
        Ok(Some(s)) => {
            job.seen = seen(&s);
            match s.state {
                State::Running => {
                    job.w.state = "ready".into();
                    near_cap(app, sub, job, &s, now).await
                }
                State::Paused => {
                    job.w.state = "paused".into();
                    paused_at_cap(app, sub, job, &s, now).await
                }
            }
        }
        Ok(None) | Err(compute::Error::NotFound) => Ok(fail(job, GONE, now)),
        Err(e) => Err(e.into()),
    }
}

/// Whether the workspace's daemon says it's working: `None` when it isn't
/// on the relay or never said (silent counts as idle), an error when the
/// hub didn't answer.
async fn working(app: &Platform, job: &HostedJob) -> Result<Option<bool>> {
    let Some(machine) = job.w.machine.clone() else { return Ok(None) };
    let said = app.hub.activity(std::slice::from_ref(&machine)).await?;
    Ok(said.into_iter().find(|a| a.machine == machine).map(|a| a.busy))
}

fn ms(t: &str) -> Option<u64> {
    arugula_forge_github::util::time(t).and_then(|t| u64::try_from(t).ok())
}

/// A running box in the last stretch before the provider's runtime cap
/// ([`super::hold::cycle_window`]): at the first look its daemon says idle,
/// pause and resume it, which restarts the provider's clock. Looked at
/// often until then, and just after the cap.
async fn near_cap(
    app: &Platform,
    sub: &dyn Substrate,
    job: &mut HostedJob,
    s: &compute::Sandbox,
    now: u64,
) -> Result<Step> {
    let cap = app.hosted.runtime_cap().as_millis() as u64;
    let Some(started) = ms(&s.started_at) else { return Ok((None, now + READY_MS)) };
    let (ends, window) = (started + cap, super::hold::cycle_window(app.hosted.runtime_cap()));
    if now + window < ends {
        return Ok((None, (now + READY_MS).min(ends - window)));
    }
    let later = (now + CAP_LOOK_MS).min(ends + CAP_LOOK_MS).max(now + 1_000);
    match working(app, job).await {
        Ok(Some(false)) => {}
        Ok(_) => return Ok((None, later)),
        Err(e) => {
            tracing::warn!(workspace = %job.w.id, error = %e, "the hub didn't say whether a hosted box near its runtime cap is working");
            return Ok((None, later));
        }
    }
    // Between turns: the pause and the resume take a second or two. A turn
    // that starts in that second finds its box paused for it.
    let sandbox = s.id.clone();
    match sub.pause(&sandbox).await {
        Ok(_) => {}
        Err(compute::Error::NotFound) => return Ok(fail(job, GONE, now)),
        Err(e) => return Err(e.into()),
    }
    match sub.resume(&sandbox, app.hosted.idle_timeout()).await {
        Ok(_) => {
            tracing::info!(workspace = %job.w.id, ran_ms = now.saturating_sub(started), "paused and resumed a hosted box between turns, before the provider's runtime cap");
            job.seen.started_at = Some(now);
            Ok((None, now + READY_MS))
        }
        Err(compute::Error::NotFound) => Ok(fail(job, GONE, now)),
        // Paused, memory kept: opening the workspace wakes it.
        Err(e) => {
            tracing::warn!(workspace = %job.w.id, error = %e, "a hosted box paused before its runtime cap didn't resume");
            job.w.state = "paused".into();
            job.w.error =
                Some(format!("Resuming the box after a pause before the provider's runtime limit failed: {e}"));
            job.seen = Default::default();
            Ok((None, now + PAUSED_MS))
        }
    }
}

/// A box that paused at the provider's runtime cap just now, its daemon
/// not saying idle: a single turn ran to the cap. Wake it so the turn goes
/// on, and say so on the workspace and in the log.
async fn paused_at_cap(
    app: &Platform,
    sub: &dyn Substrate,
    job: &mut HostedJob,
    s: &compute::Sandbox,
    now: u64,
) -> Result<Step> {
    let cap = app.hosted.runtime_cap();
    let (Some(started), Some(paused_at)) = (ms(&s.started_at), ms(&s.end_at)) else {
        return Ok((None, now + PAUSED_MS));
    };
    if !super::hold::ran_to_cap(started, paused_at, cap) || now > paused_at + CAP_WAKE_MS {
        return Ok((None, now + PAUSED_MS));
    }
    match working(app, job).await {
        Ok(Some(false)) => return Ok((None, now + PAUSED_MS)),
        Ok(_) => {}
        Err(e) => return Err(e),
    }
    let hours = cap.as_secs_f64() / 3600.0;
    tracing::warn!(workspace = %job.w.id, project = %job.w.project, cap_hours = hours, "a turn ran past the provider's runtime cap: its hosted box paused mid-turn; waking it");
    match sub.resume(&s.id, app.hosted.idle_timeout()).await {
        Ok(_) => {
            job.w.state = "ready".into();
            job.w.error = Some(format!(
                "A turn ran past the provider's {hours}-hour limit on running without a pause: the box paused mid-turn and was woken. The turn may have lost its network connections."
            ));
            job.seen.started_at = Some(now);
            job.seen.paused_at = None;
            Ok((None, now + READY_MS))
        }
        Err(compute::Error::NotFound) => Ok(fail(job, GONE, now)),
        Err(e) => {
            job.w.error = Some(format!(
                "A turn ran past the provider's {hours}-hour limit on running without a pause, and the box paused mid-turn. Waking it failed: {e}"
            ));
            Ok((None, now + PAUSED_MS))
        }
    }
}

/// What a box as the provider reports it says for its usage (M188): when
/// it started, when it paused itself (its `end_at`, once paused), and its
/// size.
fn seen(s: &compute::Sandbox) -> super::usage::Seen {
    let ms = |t: &str| arugula_forge_github::util::time(t).and_then(|t| u64::try_from(t).ok());
    super::usage::Seen {
        started_at: (s.state == State::Running).then(|| ms(&s.started_at)).flatten(),
        paused_at: (s.state == State::Paused).then(|| ms(&s.end_at)).flatten(),
        class: s.cpus.zip(s.memory_mb).map(|(cpus, memory_mb)| super::usage::Class { cpus, memory_mb }),
    }
}
