//! Durable run logs and artifacts (M213, platform step 7): a hosted run's
//! log and the files it leaves, in the run-log bucket ([`Bucket`],
//! Cloudflare R2 for the hosted control), which the platform reads and
//! serves to the project's members with every machine asleep. Runs on
//! hosted workspaces only: that's the stated exception to aru-055 (the
//! tracker's "Run logs" decision). A person's machine never sends any.
//!
//! - **Signing** ([`upload`]). The hub's `POST /api/hosted/work/upload`
//!   takes only a hosted machine's signature, and asks here with what it
//!   proved ([`HostedMachine`]). The run must be the machine's own (its
//!   project, its workspace, its operation's daemon), running or ended
//!   within [`GRACE_MS`]; a log chunk must be the run's next, within the
//!   caps (`arugula_control_wire::run_logs`). The answer is a `PUT` URL for
//!   that one key, `<project>/<run>/log/<n>` or
//!   `<project>/<run>/artifacts/<name>`, signed for exactly its size, good
//!   for [`PUT_EXPIRES_S`].
//! - **Recording** ([`uploaded`]). Once the bytes are put, the machine says
//!   so; the platform heads the object, and records it only if it's there
//!   with that size (`run_log_chunks`, `run_artifacts`).
//! - **Reading.** A run's members read its log a page at a time through
//!   M216's `GET /api/runs/{run}/log` (and a machine's agents through
//!   `/api/daemon/runs/{run}/log`), which [`crate::run_log::page`] reads
//!   from the bucket, checking each chunk's SHA-256; they list its
//!   artifacts with signed `GET`s and fetch them ([`routes`]).
//! - **Kept** [`Bucket::ttl_ms`] (30 days): the bucket's own rule deletes
//!   the files; [`sweep`] forgets the rows then, and deletes the files it
//!   knew too, so nothing outlives the days if the rule is missing.
//!
//! Without a bucket (`ARUGULA_RUN_LOGS_BUCKET` unset) it's all off: the
//! machine is told so ([`UploadGiven::Off`]), and a run's log stays in its
//! pane only.

pub mod bucket;
pub mod routes;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use arugula_control_wire::run_logs::{
    MAX_ARTIFACT_BYTES, MAX_ARTIFACTS, MAX_CHUNK_BYTES, MAX_LOG_BYTES, Upload, UploadGiven, Uploaded,
    good_artifact_name, good_sha256,
};
pub use bucket::{Args, Bucket};
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        Taken,
        run_logs::{Issue, LogChunk},
        runs::Run,
    },
    work::HostedMachine,
};

/// How long a `PUT` URL lasts.
pub const PUT_EXPIRES_S: u64 = 15 * 60;
/// How long a member's `GET` URLs last.
pub const GET_EXPIRES_S: u64 = 60 * 60;
/// How long after a run ends its machine may still send what's left (the
/// last chunk, its artifacts).
pub const GRACE_MS: u64 = 60 * 60_000;
/// The most upload URLs one run is given in a minute: a chunk every 2 s,
/// and its artifacts at the end, fit well inside.
pub const PER_MINUTE: u64 = 120;
/// How long after an upload URL expires, with nothing recorded at its
/// key, the sweep deletes whatever was put there.
pub const UNRECORDED_MS: u64 = 10 * 60_000;
/// The furthest a chunk may reach in a pane's output (2^53).
pub const MAX_OFFSET: u64 = 1 << 53;
/// How often the sweep runs.
pub const SWEEP_EVERY: Duration = Duration::from_secs(3600);

/// A run's log chunk's key in the bucket.
pub fn log_key(project: &str, run: &str, n: u32) -> String {
    format!("{project}/{run}/log/{n}")
}

/// A run's artifact's key in the bucket.
pub fn artifact_key(project: &str, run: &str, name: &str) -> String {
    format!("{project}/{run}/artifacts/{name}")
}

/// Run `id`, if it's `m`'s own and still taking uploads at `now` (`late`:
/// how long after its grace a recording may come, for a URL signed just
/// before it ended). The refusal's words otherwise.
fn machines_run(app: &Platform, m: &HostedMachine, id: &str, now: u64, late: u64) -> Result<Result<Run, String>> {
    let Some(run) = app.db.run(id)? else { return Ok(Err("no such run".into())) };
    if run.project != m.project || run.workspace.as_deref() != Some(m.workspace.as_str()) || run.daemon != m.machine {
        return Ok(Err("the run isn't this machine's".into()));
    }
    let open = match (run.started_at, run.ended_at) {
        (Some(_), None) => true,
        (Some(_), Some(end)) => now <= end + GRACE_MS + late,
        _ => false,
    };
    if !open {
        return Ok(Err(format!("the run is {} and takes no uploads", run.state)));
    }
    Ok(Ok(run))
}

/// What a recorded chunk or artifact says of `u`: `Some(true)` when it's
/// this one (the same bytes), `Some(false)` another, `None` there's none.
fn recorded(app: &Platform, run: &Run, u: &Upload, since: u64) -> Result<Option<bool>> {
    Ok(match u {
        Upload::Log { n, offset, size, sha256, .. } => app
            .db
            .run_log_chunks(&run.id, since)?
            .into_iter()
            .find(|c| c.n == *n)
            .map(|c| (c.offset, c.size, c.sha256.as_str()) == (*offset, *size, sha256.as_str())),
        Upload::Artifact { name, size, sha256, .. } => app
            .db
            .run_artifacts(&run.id)?
            .into_iter()
            .find(|a| a.name == *name)
            .map(|a| a.size == *size && a.digest == format!("sha256:{sha256}")),
    })
}

/// The checks both calls make of `u` on `run`, past its being the
/// machine's: its form and the caps, and for a log chunk, that it's the
/// next. `Err`: the answer to give instead.
fn fits(app: &Platform, run: &Run, u: &Upload, since: u64) -> Result<Result<(), UploadGiven>> {
    let refuse = |why: &str| Ok(Err(UploadGiven::Refused { why: why.into(), next: None }));
    if !good_sha256(u.sha256()) {
        return refuse("the digest isn't a SHA-256 in hex");
    }
    match recorded(app, run, u, since)? {
        Some(true) => return Ok(Err(UploadGiven::Kept)),
        Some(false) if matches!(u, Upload::Artifact { .. }) => return refuse("the run has an artifact by that name"),
        _ => {}
    }
    match u {
        Upload::Log { n, offset, size, .. } => {
            let chunks = app.db.run_log_chunks(&run.id, since)?;
            let next = chunks.len() as u32;
            if *n != next {
                return Ok(Err(UploadGiven::Refused {
                    why: format!("the log's next chunk is {next}"),
                    next: Some(next),
                }));
            }
            if !(1..=MAX_CHUNK_BYTES).contains(size) {
                return refuse("a chunk is 1 byte to 8 MiB");
            }
            // Offsets are a pane's bytes: far below where `offset + size`
            // would wrap, or leave the database's integers.
            if offset.checked_add(*size).is_none_or(|end| end > MAX_OFFSET) {
                return refuse("the chunk's offset is past any pane's");
            }
            if *offset < chunks.last().map_or(0, LogChunk::end) {
                return refuse("the chunk starts before the last one ends");
            }
            if chunks.iter().map(|c| c.size).sum::<u64>() + size > MAX_LOG_BYTES {
                return refuse("the run's log is past the most that's kept (1 GiB)");
            }
        }
        Upload::Artifact { name, size, .. } => {
            if !good_artifact_name(name) {
                return refuse("an artifact's name is letters, digits, '.', '-' and '_', not starting with '.'");
            }
            if *size > MAX_ARTIFACT_BYTES {
                return refuse("an artifact is at most 100 MiB");
            }
        }
    }
    Ok(Ok(()))
}

fn key(run: &Run, u: &Upload) -> String {
    match u {
        Upload::Log { n, .. } => log_key(&run.project, &run.id, *n),
        Upload::Artifact { name, .. } => artifact_key(&run.project, &run.id, name),
    }
}

/// `POST /api/hosted/work/upload` from `m`: a URL to put `u` at, if it may.
pub async fn upload(app: &Platform, m: &HostedMachine, u: &Upload) -> Result<UploadGiven> {
    let Some(b) = &app.run_logs else { return Ok(UploadGiven::Off) };
    let now = arugula_e2e::now_ms();
    let run = match machines_run(app, m, u.run(), now, 0)? {
        Ok(r) => r,
        Err(why) => return Ok(UploadGiven::Refused { why, next: None }),
    };
    if let Err(answer) = fits(app, &run, u, now.saturating_sub(b.ttl_ms))? {
        return Ok(answer);
    }
    // Out, the URL counts against the run's caps and its project's day.
    let key = key(&run, u);
    let expires_at = now + PUT_EXPIRES_S * 1000;
    let issue = Issue {
        run: &run.id,
        project: &run.project,
        key: &key,
        kind: if matches!(u, Upload::Log { .. }) { "log" } else { "artifact" },
        size: u.size(),
        now,
        expires_at,
        per_minute: PER_MINUTE,
        daily_bytes: b.daily_bytes,
        max_artifacts: MAX_ARTIFACTS as u64,
    };
    if let Err(why) = app.db.issue_run_upload(&issue)? {
        return Ok(UploadGiven::Refused { why, next: None });
    }
    let url = b.presign("PUT", &key, Some(u.size()), PUT_EXPIRES_S, now);
    Ok(UploadGiven::Put { url, expires_at })
}

/// `POST /api/hosted/work/uploaded` from `m`: record `u`, once the bucket
/// has it with its size.
pub async fn uploaded(app: &Platform, m: &HostedMachine, u: &Upload) -> Result<Uploaded> {
    let refuse = |why: String| Ok(Uploaded::Refused { why });
    let Some(b) = &app.run_logs else { return refuse("this platform keeps no run logs".into()) };
    let now = arugula_e2e::now_ms();
    let run = match machines_run(app, m, u.run(), now, PUT_EXPIRES_S * 1000)? {
        Ok(r) => r,
        Err(why) => return refuse(why),
    };
    match fits(app, &run, u, now.saturating_sub(b.ttl_ms))? {
        Ok(()) => {}
        Err(UploadGiven::Kept) => return Ok(Uploaded::Kept),
        Err(UploadGiven::Refused { why, .. }) => return refuse(why),
        Err(other) => return refuse(format!("{other:?}")),
    }
    let key = key(&run, u);
    let head = app.http.head(b.presign("HEAD", &key, None, 60, now)).send().await;
    let size = match head {
        Ok(r) if r.status().is_success() => r
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok()),
        Ok(r) => return refuse(format!("the bucket hasn't it ({})", r.status())),
        Err(e) => {
            // Never the URL: it's signed.
            let e = e.without_url();
            warn!(run = run.id, key, error = %e, "can't reach the run-log bucket");
            anyhow::bail!("can't reach the run-log bucket: {e}");
        }
    };
    if size != Some(u.size()) {
        return refuse(format!("the bucket has {size:?} bytes there, not {}", u.size()));
    }
    let made = match u {
        Upload::Log { n, offset, size, sha256, .. } => app.db.add_run_log_chunk(&LogChunk {
            run: run.id.clone(),
            n: *n,
            offset: *offset,
            size: *size,
            sha256: sha256.clone(),
            bucket_key: key.clone(),
            created_at: now,
        }),
        Upload::Artifact { name, size, sha256, .. } => {
            let a = crate::db::runs::Artifact {
                run: run.id.clone(),
                name: name.clone(),
                size: *size,
                digest: format!("sha256:{sha256}"),
                bucket_key: key.clone(),
                created_at: now,
            };
            app.db.add_run_artifact_capped(&a, MAX_ARTIFACTS as u64).map(|a| a.is_some())
        }
    };
    match made {
        Ok(true) => {
            info!(run = run.id, key, size = u.size(), "recorded a run's upload");
            Ok(Uploaded::Kept)
        }
        // Another call recorded it meanwhile: the same, or not.
        Ok(false) => refuse("the run moved on meanwhile".into()),
        Err(e) if e.is::<Taken>() => match recorded(app, &run, u, 0)? {
            Some(true) => Ok(Uploaded::Kept),
            _ => refuse("another upload took its place".into()),
        },
        Err(e) => Err(e),
    }
}

/// Delete `keys` from the bucket, as of `now`, saying nothing of a key
/// that's gone already. Best effort: the bucket's rule takes what this
/// misses.
pub async fn delete_files(app: &Platform, keys: &[String], now: u64) {
    let Some(b) = &app.run_logs else { return };
    for key in keys {
        let r = app.http.delete(b.presign("DELETE", key, None, 60, now)).send().await;
        match r {
            Ok(r) if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_FOUND => {}
            Ok(r) => warn!(key, status = %r.status(), "the run-log bucket didn't delete a file; its rule will"),
            Err(e) => {
                let e = e.without_url();
                warn!(key, error = %e, "can't reach the run-log bucket to delete a file; its rule will")
            }
        }
    }
}

/// Forget the run logs and artifacts recorded more than the bucket's days
/// before `now`, and the upload URLs that expired with nothing recorded,
/// and delete their files from the bucket (its own rule does the first
/// too: this keeps the days if the rule's missing). How many files went.
pub async fn sweep(app: &Platform, now: u64) -> Result<usize> {
    let Some(b) = &app.run_logs else { return Ok(0) };
    let mut keys = app.db.sweep_run_logs(now.saturating_sub(b.ttl_ms))?;
    let unsaid = app.db.sweep_run_uploads(now, UNRECORDED_MS)?;
    if !unsaid.is_empty() {
        info!(files = unsaid.len(), "deleting what was put at upload URLs nobody said they used");
    }
    keys.extend(unsaid);
    delete_files(app, &keys, now).await;
    if !keys.is_empty() {
        info!(files = keys.len(), "forgot run logs past their days");
    }
    Ok(keys.len())
}

/// The sweep, hourly, on the platform's leader.
pub fn start(app: &Arc<Platform>) {
    if app.run_logs.is_none() {
        return;
    }
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            if !app.is_paused()
                && app.is_leader()
                && let Err(e) = sweep(&app, arugula_e2e::now_ms()).await
            {
                warn!(error = %format!("{e:#}"), "the run-log sweep failed; it runs again in an hour");
            }
            tokio::time::sleep(SWEEP_EVERY).await;
        }
    });
}

/// Bytes as the operator reads them: `1.5 MiB`.
fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let (mut v, mut u) = (n as f64, 0);
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// `run-logs`' report: a line per project, most bytes first, and the total.
pub fn render_usage(u: &[crate::db::run_logs::Usage], at: &str) -> String {
    use std::fmt::Write as _;
    let mut s = format!("runs' logs and artifacts in the run-log bucket, by the rows in {at} (read-only)\n\n");
    let _ = writeln!(s, "{:<38} {:>6} {:>12} {:>12} {:>12}", "project", "runs", "logs", "artifacts", "total");
    let (mut logs, mut artifacts) = (0, 0);
    for p in u {
        logs += p.log_bytes;
        artifacts += p.artifact_bytes;
        let _ = writeln!(
            s,
            "{:<38} {:>6} {:>12} {:>12} {:>12}",
            p.project,
            p.runs,
            bytes(p.log_bytes),
            bytes(p.artifact_bytes),
            bytes(p.log_bytes + p.artifact_bytes)
        );
    }
    let _ = writeln!(
        s,
        "\n{} projects: {} of logs, {} of artifacts, {} in all",
        u.len(),
        bytes(logs),
        bytes(artifacts),
        bytes(logs + artifacts)
    );
    s
}
