//! A hosted run's durable log and artifacts (M213, platform step 7): what
//! `arugulad --hosted` sends of each run to the run-log bucket, through
//! URLs the platform signs for that run's prefix
//! (`arugula_control_wire::run_logs`). The platform's half is
//! `arugula_platform::run_logs`.
//!
//! - **Hosted machines only** ([`may_upload`]). A person's machine never
//!   sends a run's log anywhere: it has no hosted enrollment, takes no
//!   runs, and this refuses before it asks. The run's live view stays its
//!   end-to-end pane.
//! - **The log** is the run's pane's output, as the pane's log on disk has
//!   it (`PaneLog`): a byte stream from offset 0, the banner saying which
//!   run it is first. While the run runs, every [`TICK`] the machine sends
//!   what's past [`CHUNK_BYTES`] since the last chunk ([`follow`]); when it
//!   ends, the rest ([`finish`]), before its last state change is
//!   reported, so a run that says it's done has its whole log kept.
//! - **Artifacts** are the regular files the run left in its
//!   `$ARUGULA_ARTIFACTS` (`~runner/work/ID/.arugula/artifacts`), sent with
//!   their SHA-256 once it has ended and everything it ran was killed.
//!   Only files the run user owns, opened without following links, with
//!   names [`good_artifact_name`] allows: nothing of the daemon's, however
//!   the run lays its directory out.
//! - **What's sent is kept in the ledger** (`Ledger::logs`): the next
//!   chunk's number and offset, and whether the artifacts went. A restart
//!   that follows a run carries on from there. A run a restart interrupted
//!   keeps what was sent before it.
//! - **Off** when the platform has no bucket ([`UploadGiven::Off`]), and
//!   for a run whose upload the platform refused: the log stays in the
//!   pane, and the run goes on.

#[cfg(unix)]
use std::io::Read as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result};
#[cfg(unix)]
use arugula_control_wire::run_logs::MAX_ARTIFACT_BYTES;
use arugula_control_wire::run_logs::{
    ARTIFACTS_DIR, CHUNK_BYTES, MAX_ARTIFACTS, MAX_CHUNK_BYTES, Upload, UploadGiven, Uploaded, WORK_UPLOAD,
    WORK_UPLOADED, good_artifact_name,
};
use arugula_mux::store::PaneLog;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tracing::{info, warn};

use crate::{RunAs, control::Saved, mux::Api, server::App};

/// How often a running run's log is looked at.
pub const TICK: Duration = Duration::from_secs(2);
/// The longest the rest of a run's log and its artifacts may take once it
/// has ended, before its end is reported anyway.
pub const FINISH: Duration = Duration::from_secs(180);

/// What's been sent of a run, in its ledger entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    /// The next chunk's number.
    #[serde(default)]
    pub n: u32,
    /// Where the next chunk starts in the pane's output.
    #[serde(default)]
    pub offset: u64,
    /// The artifacts went (or there were none).
    #[serde(default)]
    pub artifacts: bool,
    /// Nothing more is sent, and why: the platform keeps no run logs, or
    /// refused one of this run's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<String>,
}

/// Whether this machine may send a run's log anywhere: only a hosted
/// machine in control does.
pub fn may_upload(saved: Option<&Saved>) -> Result<(), &'static str> {
    match saved {
        Some(s) if s.hosted.is_some() => Ok(()),
        _ => Err("not a hosted machine: a run's log stays in its pane"),
    }
}

fn allowed(app: &App) -> Result<(), &'static str> {
    let e = app.control.enrolled();
    may_upload(e.as_ref().map(|e| &e.saved))
}

/// Where run `run` leaves its artifacts: under its home as `as_`, or (as
/// the daemon's own user, tests only) beside its ledger entry.
pub fn artifacts_dir(state: &Path, run: &str, as_: Option<&RunAs>) -> PathBuf {
    match as_ {
        Some(a) => a.work(run).join(ARTIFACTS_DIR),
        None => state.join(crate::work::DIR).join(format!("{run}.artifacts")),
    }
}

/// What came of one upload.
enum Step {
    Sent,
    /// Stop sending this run's: why.
    Off(String),
}

fn client() -> reqwest::Client {
    crate::roots::http().timeout(Duration::from_secs(120)).build().expect("http client")
}

/// Ask for a URL for `u`, put `bytes` there, and say so. An error is one
/// to try again.
async fn put(app: &App, http: &reqwest::Client, u: &Upload, bytes: Vec<u8>) -> Result<Step> {
    let given: UploadGiven = app.control.post_signed(WORK_UPLOAD, u).await?;
    let url = match given {
        UploadGiven::Put { url, .. } => url,
        UploadGiven::Kept => return Ok(Step::Sent),
        UploadGiven::Off => return Ok(Step::Off("the platform keeps no run logs".into())),
        UploadGiven::Refused { why, .. } => return Ok(Step::Off(why)),
    };
    let r = http
        .put(&url)
        .header(reqwest::header::CONTENT_LENGTH, bytes.len())
        .body(bytes)
        .send()
        .await
        // Never the URL in an error or a log line: it's signed.
        .map_err(|e| anyhow::anyhow!("the run-log bucket: {}", e.without_url()))?;
    anyhow::ensure!(r.status().is_success(), "the run-log bucket answered {}", r.status());
    match app.control.post_signed::<Uploaded>(WORK_UPLOADED, u).await? {
        Uploaded::Kept => Ok(Step::Sent),
        Uploaded::Refused { why } => Ok(Step::Off(why)),
    }
}

/// Keep `f`'s change to run `run`'s [`Sent`].
fn note(app: &App, run: &str, f: impl FnOnce(&mut Sent)) {
    if let Err(e) = crate::update(app.control.state_dir(), run, |l| f(&mut l.logs)) {
        warn!(run, error = %format!("{e:#}"), "hosted: can't keep what was sent of a run's log");
    }
}

fn sent(app: &App, run: &str) -> Sent {
    crate::read(app.control.state_dir(), run).ok().flatten().map(|l| l.logs).unwrap_or_default()
}

/// The pane's log, if pane `pane` is still the run's (its `epoch`).
async fn pane_log(app: &App, pane: u32, epoch: Option<u64>) -> Option<PaneLog> {
    let h = app.mux.api(|r| Api::Pane(pane, r)).await.flatten()?;
    if epoch.is_some_and(|e| e != h.epoch) {
        return None;
    }
    let dir = app.mux.store.pane_dir(pane);
    if !dir.is_dir() {
        return None;
    }
    PaneLog::open(dir).ok()
}

/// Send run `run`'s log from its pane: every whole [`CHUNK_BYTES`] past
/// what's been sent, or (`all`) everything there is. `Err`: try again.
async fn send_log(
    app: &App,
    http: &reqwest::Client,
    run: &str,
    pane: u32,
    epoch: Option<u64>,
    all: bool,
) -> Result<()> {
    loop {
        let s = sent(app, run);
        if s.off.is_some() {
            return Ok(());
        }
        let Some(log) = pane_log(app, pane, epoch).await else { return Ok(()) };
        // Bytes the pane's log dropped before they were sent are a gap.
        let from = s.offset.max(log.start());
        let have = log.end().saturating_sub(from);
        if have == 0 || (!all && have < CHUNK_BYTES) {
            return Ok(());
        }
        let size = have.min(CHUNK_BYTES).min(MAX_CHUNK_BYTES);
        let (from, bytes) = log.read_range(from, size as usize).context("the pane's log")?;
        if bytes.is_empty() {
            return Ok(());
        }
        let len = bytes.len() as u64;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let u = Upload::Log { run: run.into(), n: s.n, offset: from, size: len, sha256 };
        match put(app, http, &u, bytes).await? {
            Step::Sent => note(app, run, |s| {
                s.n += 1;
                s.offset = from + len;
            }),
            Step::Off(why) => {
                info!(run, %why, "hosted: a run's log isn't sent; it stays in its pane");
                note(app, run, |s| s.off = Some(why));
                return Ok(());
            }
        }
    }
}

/// The uid that owns what the run leaves: the run user's, or (tests only)
/// this daemon's.
#[cfg(unix)]
fn owner(as_: Option<&RunAs>) -> Option<u32> {
    match as_ {
        Some(a) => nix::unistd::User::from_name(&a.user).ok().flatten().map(|u| u.uid.as_raw()),
        None => Some(nix::unistd::getuid().as_raw()),
    }
}

#[cfg(not(unix))]
fn owner(_: Option<&RunAs>) -> Option<u32> {
    None
}

/// One artifact, read: opened without following a link, a regular file
/// with one name that `uid` owns, at most [`MAX_ARTIFACT_BYTES`].
#[cfg(unix)]
fn read_artifact(path: &Path, uid: u32) -> Result<Option<Vec<u8>>> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
    let mut f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    let m = f.metadata()?;
    if !m.is_file() || m.uid() != uid || m.nlink() != 1 || m.len() > MAX_ARTIFACT_BYTES {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(m.len() as usize);
    f.by_ref().take(MAX_ARTIFACT_BYTES + 1).read_to_end(&mut out)?;
    Ok((out.len() as u64 <= MAX_ARTIFACT_BYTES).then_some(out))
}

#[cfg(not(unix))]
fn read_artifact(_: &Path, _: u32) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

/// Send the artifacts run `run` left in `dir`, once. `Err`: try again.
async fn send_artifacts(app: &App, http: &reqwest::Client, run: &str, dir: &Path, as_: Option<&RunAs>) -> Result<()> {
    let s = sent(app, run);
    if s.off.is_some() || s.artifacts {
        return Ok(());
    }
    let Some(uid) = owner(as_) else {
        note(app, run, |s| s.artifacts = true);
        return Ok(());
    };
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect(),
        Err(_) => vec![],
    };
    names.sort();
    let mut skipped = 0;
    let mut count = 0;
    for name in names {
        if !good_artifact_name(&name) || count >= MAX_ARTIFACTS {
            skipped += 1;
            continue;
        }
        let Some(bytes) = read_artifact(&dir.join(&name), uid)? else {
            skipped += 1;
            continue;
        };
        count += 1;
        let u = Upload::Artifact {
            run: run.into(),
            sha256: hex::encode(Sha256::digest(&bytes)),
            size: bytes.len() as u64,
            name: name.clone(),
        };
        if let Step::Off(why) = put(app, http, &u, bytes).await? {
            info!(run, name, %why, "hosted: a run's artifact isn't sent");
            if why.contains("no run logs") {
                note(app, run, |s| s.off = Some(why));
                return Ok(());
            }
        }
    }
    if skipped > 0 {
        info!(run, skipped, "hosted: some of what a run left isn't an artifact it may send");
    }
    note(app, run, |s| s.artifacts = true);
    Ok(())
}

/// While run `run` runs in pane `pane`, send its log a chunk at a time,
/// until `done` says it ended.
pub async fn follow(
    app: Arc<App>,
    run: String,
    pane: u32,
    epoch: Option<u64>,
    mut done: tokio::sync::watch::Receiver<bool>,
) {
    if let Err(why) = allowed(&app) {
        warn!(run, why, "hosted: not sending a run's log");
        return;
    }
    let http = client();
    loop {
        if let Err(e) = send_log(&app, &http, &run, pane, epoch, false).await {
            info!(run, error = %format!("{e:#}"), "hosted: a run's log didn't go yet; trying again");
        }
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = done.changed() => return,
        }
    }
}

/// Run `run` ended: send the rest of its log and its artifacts, trying
/// again for at most [`FINISH`].
pub async fn finish(app: &App, run: &str, pane: u32, epoch: Option<u64>, as_: Option<&RunAs>) {
    if let Err(why) = allowed(app) {
        warn!(run, why, "hosted: not sending a run's log");
        return;
    }
    let http = client();
    let dir = artifacts_dir(app.control.state_dir(), run, as_);
    let once = async {
        let mut wait = Duration::from_millis(500);
        loop {
            let r = async {
                send_log(app, &http, run, pane, epoch, true).await?;
                send_artifacts(app, &http, run, &dir, as_).await
            }
            .await;
            match r {
                Ok(()) => return,
                Err(e) => info!(run, error = %format!("{e:#}"), "hosted: a run's log didn't all go yet; trying again"),
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(20));
        }
    };
    if tokio::time::timeout(FINISH, once).await.is_err() {
        warn!(run, "hosted: the rest of a run's log didn't go in time; what went is kept, the rest stays in its pane");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hosted machine of an account's project, enrolled under a
    /// delegation from the account's root device (the base's
    /// `hosted::runs` tests make the same).
    fn machine() -> Saved {
        use arugula_e2e::{
            Cert, DeviceKeys, Kind, Trust,
            hosted::{Delegation, HostedCert, HostedKey},
        };
        let root = DeviceKeys::generate();
        let mut root_cert = Cert::new(&root, "acct1", Kind::Browser, "laptop");
        root_cert.sign_with(&root);
        let key = HostedKey::generate();
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&root);
        let keys = DeviceKeys::generate();
        let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
        let mut cert = HostedCert::new(&d, "hw_1", "box", &noise, &sign, 2);
        key.sign(&mut cert);
        let got = arugula_control_wire::hosted::Enrollment {
            url: "http://control".into(),
            cert,
            delegation: d,
            revocations: vec![],
            team: None,
            roster: None,
            certs: Default::default(),
            trust: Some(Trust { account: "acct1".into(), root: root.id() }),
            account_certs: vec![root_cert],
            account_revocations: vec![],
            locked: false,
            lock: None,
            legacy_app_children: None,
            purpose: None,
        };
        crate::hosted::trust::saved_from(got, &keys, None).unwrap()
    }

    #[test]
    fn only_a_hosted_machine_sends_a_runs_log() {
        let saved = machine();
        assert_eq!(may_upload(Some(&saved)), Ok(()));
        let personal = Saved { hosted: None, ..saved };
        assert!(may_upload(Some(&personal)).is_err());
        assert!(may_upload(None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn an_artifact_is_a_file_the_run_owns_and_nothing_it_links_to() {
        use std::os::unix::fs::MetadataExt as _;
        let dir = crate::hosted::scratch("run-artifacts");
        let me = std::fs::metadata(&dir).unwrap().uid();
        std::fs::write(dir.join("ok.txt"), b"fine").unwrap();
        std::fs::write(dir.join("secret"), b"the daemon's").unwrap();
        std::os::unix::fs::symlink(dir.join("secret"), dir.join("link")).unwrap();
        std::fs::hard_link(dir.join("secret"), dir.join("hard")).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        assert_eq!(read_artifact(&dir.join("ok.txt"), me).unwrap(), Some(b"fine".to_vec()));
        assert_eq!(read_artifact(&dir.join("ok.txt"), me + 1).unwrap(), None, "someone else's");
        assert_eq!(read_artifact(&dir.join("link"), me).unwrap(), None, "a link");
        assert_eq!(read_artifact(&dir.join("hard"), me).unwrap(), None, "a second name");
        assert_eq!(read_artifact(&dir.join("sub"), me).unwrap(), None, "a directory");
        assert_eq!(read_artifact(&dir.join("nope"), me).unwrap(), None);
        let as_ = RunAs { user: "runner".into(), home: "/home/runner".into() };
        assert_eq!(artifacts_dir(&dir, "run_1", Some(&as_)), Path::new("/home/runner/work/run_1/.arugula/artifacts"));
        assert_eq!(artifacts_dir(&dir, "run_1", None), dir.join("work/run_1.artifacts"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
