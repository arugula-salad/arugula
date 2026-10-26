//! Hosted workspaces' lifecycle on the platform's leased worker (M179).
//! Each tick leases one workspace with something to do
//! ([`Db::claim_hosted`](crate::db::Db::claim_hosted)) and takes one step:
//!
//! | State | Step |
//! |---|---|
//! | anything, delete asked | delete every box it has (by its metadata too), then `deleted` |
//! | `creating` | the create's journal: ask the hub for a ticket (ticket mode), note `sent`, make the box; `sent` with no answer kept: find it by its metadata, never make it again; `made`: record its enrollment by sandbox id |
//! | `enrolling` | wait for the hub to say its box enrolled, then `ready`; past the enrollment's end, delete the box (fail closed) |
//! | `ready`, `paused` | a rebuild (which clears what the old box's setup said), wake or pause asked; else read its state (never waking it), and near the provider's runtime cap pause and resume it between turns ([`super::hold`]) |
//! | `waking` | resumed (a wake, or the runtime cap's pause and resume): `ready` once its daemon dials in; past [`super::Config::dial_wait`], `failed` as needing a rebuild, its box paused, and the project's owners and editors pushed (M258) |
//! | a snapshot's builder, enrolled | wait for its setup and wipe, snapshot it, delete it ([`super::snapshot`], M263) |
//!
//! A people's workspace's create starts from its snapshot when it has a
//! current one, and each look at a `ready` or `paused` one may start a
//! snapshot build ([`super::snapshot::tend`]).
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
use arugula_e2e::team::TeamRole;

use super::{
    Ask,
    enroll::{EnrollAsk, EnrollGiven, LATE, NO_DELEGATION, SANDBOX_MS, TAKEN},
};
use crate::{
    Platform,
    compute::{self, Create, State, Substrate},
    db::hosted::HostedJob,
    hub::PushOptions,
};

/// How long a box runs without being held before it pauses itself
/// (memory and disk kept), unless `ARUGULA_HOSTED_IDLE_TIMEOUT` sets
/// another ([`super::Config::idle_timeout`]). [`super::hold`] holds a busy
/// one past it.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How soon a step that failed for now is tried again.
pub(super) const RETRY_MS: u64 = 5_000;
/// How soon a deleted workspace asks again to delete a snapshot the
/// provider still holds for the boxes just deleted (M263).
const SNAPSHOT_RETRY_MS: u64 = 15_000;
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
/// How long a woken box's daemon has to dial in again before the workspace
/// fails as needing a rebuild, unless `ARUGULA_HOSTED_DIAL_WAIT` sets
/// another ([`super::Config::dial_wait`]). It dials in within a second or
/// two of a resume when it's alive (M180), and its supervisor starts it
/// again within half a minute when it isn't (M258).
pub const DIAL_WAIT: Duration = Duration::from_secs(60);
/// How often a waking box's machine is looked for on the relay.
const DIAL_LOOK_MS: u64 = 1_000;
/// How long after a create whose answer was lost the box is looked for
/// by its metadata before it's taken as never made.
pub(super) const LOST_MS: u64 = 60_000;
/// How long past its enrollment's end a box has to have enrolled (the
/// hub's answer may lag the box's enroll).
const ENROLL_GRACE_MS: u64 = 30_000;

pub(crate) const GONE: &str = "The box is gone at the provider. Rebuild the workspace to make another.";
/// A wake the provider didn't answer: it's tried again, and still asked.
pub(crate) const UNAVAILABLE: &str = "The compute provider isn't answering. Arugula keeps trying to wake the box.";
const NOT_MADE: &str = "The provider didn't make the box. Rebuild the workspace to try again.";
/// A woken box whose daemon didn't dial in (M258, Arugula #124).
pub(crate) const NO_DAEMON: &str = "The box woke, but Arugula in it didn't connect. Rebuild the workspace to make its box again: work in it that wasn't pushed is lost.";

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
    let Some(mut job) = app.db.claim_hosted(now)? else {
        // Nothing else due: snapshots deleted workspaces left (M263).
        return super::snapshot::orphans(app, &*sub, now).await;
    };
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
pub(super) type Step = (Option<Ask>, u64);

async fn step(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let asked = job.w.asked.clone();
    if asked.as_deref() == Some("delete") {
        delete(app, sub, job).await?;
        // Its snapshots after its boxes: E2B holds one while a box made
        // from it exists, and for a while after (M263).
        if !super::snapshot::forget(app, sub, job, now).await? {
            return Ok((None, now + SNAPSHOT_RETRY_MS));
        }
        job.w.state = "deleted".into();
        job.deleted_at = Some(now);
        return Ok((Some(Ask::Delete), now));
    }
    match job.w.state.as_str() {
        "creating" => return create(app, sub, job, now).await,
        "enrolling" => return enrolling(app, sub, job, now).await,
        _ if super::snapshot::is_builder(job) => return super::snapshot::build(app, sub, job, now).await,
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
            job.new_box = true;
            return Ok((Some(Ask::Rebuild), now));
        }
        // Nothing but a rebuild or a delete is taken while it wakes.
        _ if job.w.state == "waking" => return dialing(app, sub, job, now).await,
        Some("wake") => return wake(app, sub, job, now).await,
        Some("pause") => return pause(app, sub, job, now).await,
        _ => {}
    }
    if job.w.state == "failed" {
        return Ok((None, u64::MAX / 2));
    }
    if let Err(error) = super::snapshot::tend(app, sub, job, now).await {
        tracing::warn!(workspace = %job.w.id, error = format!("{error:#}"), "a hosted workspace's snapshot wasn't seen to");
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
pub(super) async fn delete(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob) -> Result<()> {
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
                return Ok(fail(job, "This control has no template for cloud workspaces.", now));
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
            // Whose key its agents run on (M185): the rules' digest is kept
            // beside the box (they can hold a project's key), so a turn's
            // start sees what it was given. A snapshot's builder runs no
            // agents, so it has no key.
            let builder = super::snapshot::is_builder(job);
            let rules = if builder {
                compute::Rules::new()
            } else {
                super::inference::rules_for(app, &job.w.project, job.w.created_by.as_deref())?
            };
            app.db.set_hosted_rules(&job.w.id, &super::inference::digest(&rules))?;
            // Its snapshot (M263), set up, when it has a current one.
            let snapshot = super::snapshot::source(app, job)?;
            app.db.set_box_snapshot(&job.w.id, snapshot.as_deref())?;
            job.stage = "sent".into();
            job.stage_at = Some(now);
            job.w.template = Some(template.clone());
            job.built_hash = Some(job.spec_hash.clone());
            app.db.save_hosted(job, now)?;
            let mut metadata = super::metadata(&job.w.id, &job.w.project, &template);
            if let Some(s) = &snapshot {
                metadata.insert("arugula_snapshot".into(), s.clone());
            }
            let made = sub
                .create(&Create {
                    template: snapshot.clone().unwrap_or_else(|| template.clone()),
                    env,
                    metadata,
                    // A builder has its setup's time before it pauses.
                    timeout: if builder { super::snapshot::BUILD_TIMEOUT } else { app.hosted.idle_timeout() },
                    rules,
                })
                .await;
            match made {
                Ok(c) => {
                    job.sandbox = Some(c.id);
                    job.stage = "made".into();
                    job.stage_at = Some(now);
                    Ok((None, now))
                }
                // The provider no longer has the snapshot: forgotten, and the
                // box is made from the template next.
                Err(compute::Error::Refused { .. } | compute::Error::NotFound) if snapshot.is_some() => {
                    let snapshot = snapshot.unwrap_or_default();
                    tracing::warn!(workspace = %job.w.id, error = %made.unwrap_err(), "a hosted workspace's snapshot is gone: its box is made from the template");
                    app.db.snapshot_gone(&job.w.id, &snapshot, super::snapshot::SNAPSHOT_GONE)?;
                    app.db.set_box_snapshot(&job.w.id, None)?;
                    job.stage = String::new();
                    job.stage_at = None;
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
        // A deploy's box (M219): the hub routes nobody to it, and the box
        // takes its one deploy run only. A snapshot's builder (M263): the
        // same, and it takes nothing.
        purpose: match job.w.purpose.as_str() {
            crate::db::hosted::DEPLOY => Some(arugula_control_wire::hosted::DEPLOY.to_owned()),
            crate::db::hosted::SNAPSHOT => Some(arugula_control_wire::hosted::SNAPSHOT.to_owned()),
            _ => None,
        },
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
                if let Err(error) = super::sessions_on_box_changed(app, &job.w.id, &e.machine).await {
                    tracing::warn!(workspace = %job.w.id, error = format!("{error:#}"), "a hosted workspace's sessions didn't move to its new box");
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
/// worker last looked (M182's wake on connect asks for that), and bring its
/// egress rules up to date: a project's key may have changed while it
/// slept (M185). It's `waking` until its daemon dials in ([`dialing`]).
async fn wake(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let Some(sandbox) = job.sandbox.clone() else { return Ok(with(fail(job, GONE, now), Ask::Wake)) };
    match sub.resume(&sandbox, app.hosted.idle_timeout()).await {
        Ok(_) => {
            if let Some(b) = app.db.hosted_box_of(&job.w.id)?
                && !super::inference::refresh(app, &b).await?
            {
                tracing::warn!(workspace = %job.w.id, "a woken box's key will catch up at its next turn");
            }
            job.w.error = None;
            Ok((Some(Ask::Wake), woke(job, now)))
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

/// The box was just resumed: it's `waking` from `now` (kept in `stage_at`)
/// until its daemon dials in, looked at once. When it's next due.
fn woke(job: &mut HostedJob, now: u64) -> u64 {
    job.w.state = "waking".into();
    job.stage = String::new();
    job.stage_at = Some(now);
    now
}

/// A woken box (M258, Arugula #124): `ready` once its machine is on the relay
/// again, never before. A daemon that's alive redials within a second or
/// two of the resume (M180), and one that exited is started again by the
/// template's supervisor. Past [`super::Config::dial_wait`] without it, the
/// workspace fails as needing a rebuild: its box is paused (it would only
/// run on, unheld, until its timeout), and the project's owners and editors
/// are pushed ([`push_no_daemon`]), as anyone who opens it reads it. The
/// platform holds no way into the box to start the daemon itself. While
/// the hub can't say, it stays `waking`.
async fn dialing(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let since = job.stage_at.unwrap_or(now);
    // A box that enrolled has its machine; without one, nothing can dial in.
    let machine = job.w.machine.clone().unwrap_or_default();
    if !machine.is_empty() {
        match app.hub.machines_online(std::slice::from_ref(&machine)).await {
            Ok(on) if on.contains(&machine) => {
                job.w.state = "ready".into();
                job.stage_at = None;
                return Ok((Some(Ask::Wake), now + READY_MS));
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(workspace = %job.w.id, error = %e, "the hub didn't say whether a woken hosted box dialed in");
                return Ok((None, now + RETRY_MS));
            }
        }
    }
    let wait = app.hosted.dial_wait().as_millis() as u64;
    if now < since + wait {
        return Ok((None, (now + DIAL_LOOK_MS).min(since + wait)));
    }
    tracing::warn!(workspace = %job.w.id, project = %job.w.project, %machine, waited_ms = now - since, "a hosted box woke, but its daemon didn't dial in: it needs a rebuild");
    if let Some(sandbox) = job.sandbox.clone() {
        match sub.pause(&sandbox).await {
            Ok(_) | Err(compute::Error::NotFound) => {}
            Err(e) => {
                tracing::warn!(workspace = %job.w.id, error = %e, "a hosted box whose daemon didn't dial in wasn't paused")
            }
        }
    }
    let step = fail(job, NO_DAEMON, now);
    if job.w.purpose != crate::db::hosted::DEPLOY {
        push_no_daemon(app, job).await;
    }
    Ok(step)
}

/// Push the project's owners and editors that the workspace's box woke
/// without its daemon and needs a rebuild: a tap opens the workspace. Not
/// only whoever opened it (the hub tells them), since any of them can
/// rebuild it. A push that doesn't go is only logged.
async fn push_no_daemon(app: &Platform, job: &HostedJob) {
    let to = match owners_and_editors(app, &job.w.project).await {
        Ok(to) => to,
        Err(e) => {
            return tracing::warn!(workspace = %job.w.id, error = format!("{e:#}"), "whom to push about a box without its daemon");
        }
    };
    let msg = serde_json::json!({
        "hosted": { "project": job.w.project, "workspace": job.w.id },
        "title": format!("{}: Arugula in its box didn't come back", job.w.name),
        "body": "The box woke, but its daemon didn't connect. Rebuild the workspace to make its box again.",
        "tag": format!("hosted-no-daemon-{}", job.w.id),
        "control": true,
    });
    for account in to {
        if let Err(e) = app.hub.push_with(&account, &msg, PushOptions::default()).await {
            tracing::warn!(workspace = %job.w.id, %account, error = %e, "the push about a box without its daemon didn't go");
        }
    }
}

/// Who may rebuild a project's workspaces: its account, or its team's
/// owners and editors (not its viewers).
async fn owners_and_editors(app: &Platform, project: &str) -> Result<Vec<String>> {
    let Some(p) = app.db.issue_project(project)? else { return Ok(vec![]) };
    Ok(match (&p.account, &p.team) {
        (Some(a), _) => vec![a.clone()],
        (None, Some(t)) => app
            .cache
            .team_now(&*app.hub, t)
            .await?
            .map(|t| {
                t.members
                    .iter()
                    .filter(|m| matches!(t.role(&m.account), Some(TeamRole::Owner | TeamRole::Editor)))
                    .map(|m| m.account.clone())
                    .collect()
            })
            .unwrap_or_default(),
        (None, None) => vec![],
    })
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
            Ok((None, woke(job, now)))
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
/// on, and say so on the workspace, in the log, and in a push to its maker
/// ([`push_past_cap`]). A turn that runs on pauses at the cap again a cap
/// later, and is woken and said again.
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
            job.w.error = Some(format!(
                "A turn ran past the provider's {hours}-hour limit on running without a pause: the box paused mid-turn and was woken. The turn may have lost its network connections."
            ));
            job.seen.started_at = Some(now);
            job.seen.paused_at = None;
            push_past_cap(app, job, hours).await;
            Ok((None, woke(job, now)))
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

/// Push the workspace's maker that a turn ran past the provider's cap and
/// goes on: the account that made it, while it's still there (in the
/// team, for a team's project), else the project's owners. A tap opens the
/// workspace. A push that doesn't go is only logged.
async fn push_past_cap(app: &Platform, job: &HostedJob, hours: f64) {
    let to = match past_cap_whom(app, job).await {
        Ok(to) => to,
        Err(e) => {
            return tracing::warn!(workspace = %job.w.id, error = format!("{e:#}"), "whom to push about a turn past the cap");
        }
    };
    let msg = serde_json::json!({
        "hosted": { "project": job.w.project, "workspace": job.w.id },
        "title": format!("{}: an agent turn has run past {hours} hours; it keeps going", job.w.name),
        "body": "The box paused at the provider's limit and was woken. The turn may have lost its network connections.",
        "tag": format!("hosted-cap-{}", job.w.id),
        "control": true,
    });
    for account in to {
        if let Err(e) = app.hub.push_with(&account, &msg, PushOptions::default()).await {
            tracing::warn!(workspace = %job.w.id, %account, error = %e, "the push about a turn past the cap didn't go");
        }
    }
}

async fn past_cap_whom(app: &Platform, job: &HostedJob) -> Result<Vec<String>> {
    let Some(p) = app.db.issue_project(&job.w.project)? else { return Ok(vec![]) };
    if let Some(maker) = job.w.created_by.as_deref() {
        let there = match &p.team {
            Some(t) => app.cache.team_now(&*app.hub, t).await?.is_some_and(|t| t.role(maker).is_some()),
            None => app.cache.account_there(&*app.hub, maker).await?,
        };
        if there {
            return Ok(vec![maker.to_owned()]);
        }
    }
    crate::deploys::owners(app, &p).await
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
