//! A hosted workspace's snapshot (M263, Arugula #140): a box set up from its
//! spec, wiped and snapshotted, which its new boxes and rebuilds start from
//! instead of running setup again. One per workspace, not per project:
//! setup runs with the workspace's own secrets, which differ.
//!
//! **Building one.** Once a workspace's own box has set up all right, the
//! worker ([`tend`]) makes a **builder**: a box of its own (a row of purpose
//! `snapshot` whose parent is the workspace), from the release's template,
//! enrolled like any box but under purpose `snapshot`, so nobody is routed
//! to it and it takes no runs or sessions. Its daemon:
//! 1. runs the workspace's setup, with the workspace's secrets;
//! 2. wipes the box as root (`arugulad _seal`): every process holding one
//!    of the secrets, its own state directory, every file holding one, shell
//!    histories, and the journal (rotated and vacuumed, and if a secret is
//!    left, journald stopped, its files removed and started again: sudo logs
//!    its command lines there); then it writes its sandbox id to
//!    `/etc/arugula/snapshot-of` and what setup did beside it, and looks
//!    again, refusing on any hit;
//! 3. says so (setup `ready`, at step `sealed`) and stops itself.
//!
//! Then the worker ([`build`]) takes a **filesystem-only** snapshot, once
//! the builder's daemon is off the relay, and deletes the builder.
//! Filesystem-only, not full: a full snapshot keeps every process's memory,
//! the enrolled daemon's own (its machine key, the setup's secrets) and any
//! secret in a heap, which no scan finds. A box from a filesystem-only one
//! boots from disk, and the template's `arugulad.service` (M258) starts the
//! daemon. That daemon sees a sandbox id that isn't the one in
//! `snapshot-of`, so it takes the box as its own at once, makes its own key
//! and enrolls as a new machine; and it says it's set up, from the record
//! beside it, without running setup.
//!
//! **When.** A build is due when the workspace has none, its spec changed
//! since the latest try, a day has passed since it (`ARUGULA_HOSTED_SNAPSHOT_HOURS`),
//! or the current one was made on another release's template. One build at
//! a time per workspace. A failed build says why on the workspace and keeps
//! the current snapshot; the next is tried when the next is due.
//!
//! **Using it** ([`source`]). A workspace's create (its first box or a
//! rebuild) starts from its current snapshot when it was built from the
//! spec it has now on this release's template. One the provider no longer
//! has is forgotten, and the box is made from the template.
//!
//! **Keeping one.** A new snapshot retires the one before. A retired one is
//! a deferred delete: E2B refuses to delete a snapshot (400) while any box
//! made from it exists, a paused one included, and for a while after the
//! last is deleted (seen live, 2026-10-26). So it isn't asked while the
//! workspace's box is made from it, and after that each look at the
//! workspace asks again until it goes; a refusal is "still in use", never
//! a build's error. Deleting the workspace deletes its builder and boxes,
//! then its snapshots, and stays `deleting`, asking again (every 15 s for
//! an hour, then every 10 minutes), until every one is gone, or for a day
//! at most (`ARUGULA_HOSTED_SNAPSHOT_HOLD_HOURS`).
//! Then it's deleted, and what's left goes on the platform's orphans
//! ([`orphans`]), which the worker asks the provider about every
//! [`ORPHAN_MS`] until each goes. A provider without snapshots (loam's E2B API) builds none, and
//! every box runs its setup as before.

use std::time::Duration;

use anyhow::Result;
use arugula_control_wire::hosted::{SEALED, SetupStatus};
use tracing::{info, warn};

use super::{
    EnrollBy,
    worker::{self, LOST_MS, RETRY_MS, Step},
};
use crate::{
    Platform,
    compute::{self, Mode, Substrate},
    db::hosted::{HostedJob, SNAPSHOT, Snapshots},
};

/// How often a workspace's snapshot is built again, unless
/// `ARUGULA_HOSTED_SNAPSHOT_HOURS` says.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);
/// How long a deleted workspace waits for the provider to let go of its
/// snapshots, unless `ARUGULA_HOSTED_SNAPSHOT_HOLD_HOURS` says.
pub const HOLD: Duration = Duration::from_secs(24 * 3600);
/// How often an orphaned snapshot is asked to be deleted again.
pub const ORPHAN_MS: u64 = 10 * 60 * 1000;
/// How often a deleted workspace asks the provider again to let go of its
/// snapshots: often for the first hour (E2B lets go within a second or a
/// minute of its last box, seen live), then seldom until [`HOLD`].
pub const HELD_FAST_MS: u64 = 15_000;
pub const HELD_FAST_FOR_MS: u64 = 60 * 60 * 1000;
pub const HELD_SLOW_MS: u64 = 10 * 60 * 1000;
/// How long a builder has from its create to its snapshot, and its box's
/// timeout: setup runs within it.
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// How long a sealed builder's daemon has to go off the relay before it's
/// snapshotted anyway.
const OFF_WAIT_MS: u64 = 30_000;
/// How often a builder is looked at while it sets up.
const LOOK_MS: u64 = 5_000;

const PURPOSE_WORKSPACE: &str = "workspace";

pub(crate) const NO_SEAL: &str = "Arugula on the snapshot's box didn't wipe it for a snapshot: its template may be \
     older than this control. New boxes run their setup.";
pub(crate) const TOO_LONG: &str = "The snapshot's box didn't set up within an hour.";
pub(crate) const BUILDER_GONE: &str = "The box building the snapshot is gone.";
pub(crate) const PAUSED: &str = "The box building the snapshot paused before it was done.";
pub(crate) const SNAPSHOT_GONE: &str =
    "The workspace's snapshot was gone at the compute provider: boxes start from the template until the next.";
pub(crate) const UNSUPPORTED: &str = "The compute provider takes no snapshots.";

/// What a builder's failed setup says, from its step and reason code.
fn setup_failed(step: &str, reason: &str) -> String {
    match reason {
        "seal_short" => "A secret of the workspace is too short to find and wipe, so its box can't be snapshotted. \
             New boxes run their setup."
            .into(),
        "seal_left" => "A secret of the workspace was still on the snapshot's box after the wipe, so it wasn't \
             snapshotted. New boxes run their setup."
            .into(),
        "seal" => "Wiping the snapshot's box failed, so it wasn't snapshotted. New boxes run their setup.".into(),
        _ if step.is_empty() => "Setting up the snapshot's box failed.".into(),
        _ => format!("Setting up the snapshot's box failed at \"{step}\"."),
    }
}

/// Whether a build is due: never tried, the spec changed since the latest
/// try, `every` since it, or the current snapshot is another template's.
pub fn due(s: &Snapshots, spec_hash: &str, template: &str, every: Duration, now: u64) -> bool {
    let Some(at) = s.try_at else { return true };
    s.try_hash.as_deref() != Some(spec_hash)
        || now >= at.saturating_add(every.as_millis() as u64)
        || (s.status.as_deref() == Some("ready") && s.template.as_deref() != Some(template))
}

/// Whether snapshots are used here: on, with boxes that enroll by sandbox
/// id (a box from a snapshot never sees a ticket in its env).
fn on(app: &Platform) -> bool {
    app.hosted.snapshot_every().is_some() && app.hosted.enroll == EnrollBy::Sandbox
}

/// The snapshot a new box of `job`'s workspace starts from: its current
/// one, when it was built from the spec it has now on this release's
/// template.
pub fn source(app: &Platform, job: &HostedJob) -> Result<Option<String>> {
    if job.w.purpose != PURPOSE_WORKSPACE || !on(app) {
        return Ok(None);
    }
    let s = app.db.hosted_snapshots(&job.w.id)?;
    let template = super::template(app);
    Ok(match (s.id, s.hash, s.template) {
        (Some(id), Some(hash), Some(t)) if hash == job.spec_hash && template.as_deref() == Some(t.as_str()) => Some(id),
        _ => None,
    })
}

/// A people's workspace's part, each time the worker looks at it: delete
/// earlier snapshots its box isn't made from, notice a builder that
/// failed or went, and start a build when one is due.
pub async fn tend(app: &Platform, sub: &dyn Substrate, job: &HostedJob, now: u64) -> Result<()> {
    if job.w.purpose != PURPOSE_WORKSPACE {
        return Ok(());
    }
    let id = &job.w.id;
    let s = app.db.hosted_snapshots(id)?;
    if !s.retired.is_empty() {
        retire(app, sub, id, &s).await;
    }
    if s.status.as_deref() == Some("building") {
        match app.db.snapshot_builder(id)? {
            None => app.db.snapshot_failed(id, BUILDER_GONE)?,
            Some(b) if b.state == "failed" => {
                // It failed making or enrolling its box (and lost it).
                let why = b.error.unwrap_or_else(|| BUILDER_GONE.into());
                app.db.snapshot_failed(id, &format!("The snapshot's box: {why}"))?;
                super::ask(app, &b.id, super::Ask::Delete)?;
            }
            Some(_) => {}
        }
        return Ok(());
    }
    let Some(every) = app.hosted.snapshot_every() else { return Ok(()) };
    if !on(app) || job.w.asked.is_some() || !matches!(job.w.state.as_str(), "ready" | "paused") {
        return Ok(());
    }
    if job.w.setup.as_ref().is_none_or(|s| s.status != SetupStatus::Ready.as_str()) {
        return Ok(());
    }
    let Some(template) = super::template(app) else { return Ok(()) };
    if !due(&s, &job.spec_hash, &template, every, now) {
        return Ok(());
    }
    match sub.snapshots().await {
        Ok(true) => {}
        Ok(false) => return app.db.snapshot_skipped(id, &job.spec_hash, now),
        Err(e) => {
            warn!(workspace = %id, error = %e, "can't say whether the compute provider takes snapshots");
            return Ok(());
        }
    }
    let builder = super::new_id();
    if app.db.start_snapshot(&job.w, &builder, now)? {
        info!(workspace = %id, builder, project = %job.w.project, "building a hosted workspace's snapshot");
    }
    Ok(())
}

/// Ask once to delete each of `parent`'s retired snapshots, but the one its
/// box is made from: whether every one asked went. One E2B still holds for
/// its boxes (a 400), or that wasn't answered, stays retired and is asked
/// again at the next look.
async fn retire(app: &Platform, sub: &dyn Substrate, parent: &str, s: &Snapshots) -> bool {
    let mut all = true;
    for old in s.retired.iter().filter(|old| s.box_snapshot.as_ref() != Some(*old)) {
        match sub.delete_snapshot(old).await {
            Ok(()) | Err(compute::Error::Unsupported) => {
                if let Err(e) = app.db.snapshot_deleted(parent, old) {
                    warn!(workspace = parent, error = %e, "a deleted snapshot is still listed");
                }
                info!(workspace = parent, "deleted a hosted workspace's earlier snapshot");
            }
            Err(compute::Error::Refused { status: 400 | 409, .. }) => {
                all = false;
                info!(workspace = parent, "an earlier snapshot is still in use at the provider: asked again later");
            }
            Err(e) => {
                all = false;
                warn!(workspace = parent, error = %e, "an earlier snapshot will be deleted later");
            }
        }
    }
    all
}

/// A workspace that's being deleted, its boxes deleted already: its builder
/// goes, and every snapshot it has is retired and asked to go. When to ask
/// again, or `None` when it may be deleted now: every one went, or the
/// provider held one past [`super::Config::snapshot_hold`], and those left
/// are orphans. Asked every [`HELD_FAST_MS`] for the first
/// [`HELD_FAST_FOR_MS`] it's held, then every [`HELD_SLOW_MS`].
pub async fn forget(app: &Platform, sub: &dyn Substrate, job: &HostedJob, now: u64) -> Result<Option<u64>> {
    if job.w.purpose != PURPOSE_WORKSPACE {
        return Ok(None);
    }
    if let Some(b) = app.db.snapshot_builder(&job.w.id)? {
        super::ask(app, &b.id, super::Ask::Delete)?;
    }
    app.db.snapshots_retire_all(&job.w.id)?;
    let s = app.db.hosted_snapshots(&job.w.id)?;
    if retire(app, sub, &job.w.id, &s).await {
        return Ok(None);
    }
    let since = app.db.snapshot_held(&job.w.id, now)?;
    let until = since.saturating_add(app.hosted.snapshot_hold().as_millis() as u64);
    if now < until {
        let every = if now < since + HELD_FAST_FOR_MS { HELD_FAST_MS } else { HELD_SLOW_MS };
        return Ok(Some((now + every).min(until)));
    }
    let left = app.db.orphan_snapshots(&job.w.id, &job.w.project, now)?;
    warn!(workspace = %job.w.id, project = %job.w.project, snapshots = ?left, "a deleted workspace's snapshots are still held by the provider: kept as orphans, asked again later");
    Ok(None)
}

/// Ask the provider again to delete one orphaned snapshot that's due:
/// whether there was one.
pub async fn orphans(app: &Platform, sub: &dyn Substrate, now: u64) -> Result<bool> {
    let Some((id, ws)) = app.db.take_orphan_snapshot(now.saturating_sub(ORPHAN_MS), now)? else { return Ok(false) };
    match sub.delete_snapshot(&id).await {
        Ok(()) | Err(compute::Error::Unsupported) => {
            app.db.orphan_snapshot_gone(&id)?;
            info!(workspace = ws, snapshot = id, "deleted an orphaned hosted snapshot");
        }
        Err(e) => info!(workspace = ws, snapshot = id, error = %e, "an orphaned hosted snapshot is still held"),
    }
    Ok(true)
}

/// A builder's step, once its box is enrolled (module docs): wait for its
/// setup and its wipe, then snapshot it and delete it.
pub(super) async fn build(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    let Some(parent) = job.w.parent.clone() else { return finish(app, sub, job, now).await };
    let live = app
        .db
        .hosted_any(&job.w.project, &parent)?
        .filter(|p| p.purpose == PURPOSE_WORKSPACE && !matches!(p.state.as_str(), "deleting" | "deleted"));
    if live.is_none() {
        return finish(app, sub, job, now).await;
    }
    let failed = |why: &str| app.db.snapshot_failed(&parent, why);
    let late = now > job.w.created_at.saturating_add(BUILD_TIMEOUT.as_millis() as u64);
    if job.w.state == "paused" {
        failed(PAUSED)?;
        return finish(app, sub, job, now).await;
    }
    let setup = job.w.setup.clone();
    match setup.as_ref().map(|s| (s.status.as_str(), s.step.as_str())) {
        Some(("ready", SEALED)) => {}
        Some(("ready", _)) => {
            failed(NO_SEAL)?;
            return finish(app, sub, job, now).await;
        }
        Some(("failed", step)) => {
            let reason = setup.as_ref().map_or("", |s| s.reason.as_str());
            failed(&setup_failed(step, reason))?;
            return finish(app, sub, job, now).await;
        }
        _ if late => {
            failed(TOO_LONG)?;
            return finish(app, sub, job, now).await;
        }
        _ => return Ok((None, now + LOOK_MS)),
    }
    let Some(sandbox) = job.sandbox.clone() else {
        failed(BUILDER_GONE)?;
        return finish(app, sub, job, now).await;
    };
    // A snapshot asked for whose answer was lost: the box's listing finds
    // it, and it's never asked for twice.
    if job.stage == "snapshotting" {
        match sub.list_snapshots(Some(&sandbox)).await {
            Ok(found) if !found.is_empty() => {
                for extra in &found[1..] {
                    let _ = sub.delete_snapshot(extra).await;
                }
                return built(app, sub, job, &parent, &found[0], now).await;
            }
            Ok(_) if now < job.stage_at.unwrap_or(now) + LOST_MS => return Ok((None, now + RETRY_MS)),
            Ok(_) => job.stage = String::new(),
            Err(e) => {
                warn!(workspace = %parent, error = %e, "can't list a builder's snapshots yet");
                return Ok((None, now + RETRY_MS));
            }
        }
    }
    // The daemon stops itself once it says it's sealed: nothing writes to
    // the disk after the wipe. Snapshotted once it's off the relay.
    let said = setup.map_or(now, |s| s.at);
    if let Some(m) = job.w.machine.clone()
        && now < said + OFF_WAIT_MS
    {
        match app.hub.machines_online(std::slice::from_ref(&m)).await {
            Ok(on) if on.contains(&m) => return Ok((None, now + 1_000)),
            Ok(_) => {}
            Err(e) => {
                warn!(workspace = %parent, error = %e, "the hub didn't say whether a builder's daemon stopped");
                return Ok((None, now + 1_000));
            }
        }
    }
    job.stage = "snapshotting".into();
    job.stage_at = Some(now);
    app.db.save_hosted(job, now)?;
    match sub.snapshot(&sandbox, Mode::Filesystem).await {
        Ok(id) => built(app, sub, job, &parent, &id, now).await,
        // Maybe made: the listing looks next.
        Err(compute::Error::Unreachable) if !late => Ok((None, now + RETRY_MS)),
        Err(e @ (compute::Error::Unavailable { .. } | compute::Error::Invalid)) if !late => {
            warn!(workspace = %parent, error = %e, "a hosted workspace's snapshot will be tried again");
            job.stage = String::new();
            Ok((None, now + RETRY_MS))
        }
        Err(compute::Error::Unsupported) => {
            failed(UNSUPPORTED)?;
            finish(app, sub, job, now).await
        }
        Err(compute::Error::NotFound) => {
            failed(BUILDER_GONE)?;
            finish(app, sub, job, now).await
        }
        Err(e) => {
            warn!(workspace = %parent, error = %e, "a hosted workspace's snapshot wasn't made");
            failed(&format!("The snapshot wasn't made: {e}."))?;
            finish(app, sub, job, now).await
        }
    }
}

/// The builder made snapshot `id`: it's the workspace's current one (or,
/// with the workspace gone, deleted), the one before is retired, and the
/// builder goes.
async fn built(
    app: &Platform,
    sub: &dyn Substrate,
    job: &mut HostedJob,
    parent: &str,
    id: &str,
    now: u64,
) -> Result<Step> {
    let template = job.w.template.clone().unwrap_or_default();
    if app.db.snapshot_built(parent, id, &job.spec_hash, &template, now)? {
        info!(workspace = parent, builder = %job.w.id, "a hosted workspace's snapshot was made");
        let s = app.db.hosted_snapshots(parent)?;
        retire(app, sub, parent, &s).await;
    } else if !app.db.snapshot_retire(parent, id)? {
        // The workspace is gone for good: asked once, and logged if not.
        if let Err(e) = sub.delete_snapshot(id).await {
            warn!(workspace = parent, error = %e, "a gone workspace's new snapshot wasn't deleted");
        }
    }
    finish(app, sub, job, now).await
}

/// The builder is done with: its box goes, and its row.
async fn finish(app: &Platform, sub: &dyn Substrate, job: &mut HostedJob, now: u64) -> Result<Step> {
    worker::delete(app, sub, job).await?;
    job.w.state = "deleted".into();
    job.deleted_at = Some(now);
    Ok((None, now))
}

/// Whether `job` is a snapshot's builder.
pub fn is_builder(job: &HostedJob) -> bool {
    job.w.purpose == SNAPSHOT
}
