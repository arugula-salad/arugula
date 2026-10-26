//! A hosted workspace's setup (M183): on the box's first boot, once it's in
//! control, the daemon reads what to set up from its workspace's spec
//! (control's `GET /api/hosted/spec`) and runs it in a pane, in a session
//! called `setup`, that anyone on the project can watch like any pane:
//! clone the repos, install the packages, then run the setup script. It
//! says how it goes at `POST /api/hosted/setup`: `setting_up` with each
//! step, then `ready`, or `failed` with the step and a fixed reason code.
//! The pane's text never goes to control: a failed setup's last lines are
//! kept here, and a client reads them from the box, end to end ([`status`]).
//!
//! - **Credentials.** Control answers each repo with what to clone it with
//!   (a read-only installation token for a GitHub repo linked to the
//!   project, else the workspace's `GIT_TOKEN`), and the values of the env
//!   the spec takes from its secrets. They go in a file only the daemon's
//!   user reads, in memory (`$XDG_RUNTIME_DIR`, else `/dev/shm`), which the
//!   setup reads and removes first thing. Git gets a credential in its
//!   environment for its clone alone (`http.extraHeader`), so nothing is
//!   written to a repo's config or the disk, and the remote stays a plain
//!   URL.
//! - **Once per box.** What it did is in `<state>/hosted-setup.json`, one of
//!   [`super::PER_BOX`]: a daemon that restarts mid-setup watches its pane
//!   again, and one that finished says so again until control has heard
//!   it. A rebuild is a new box, which runs it again.
//! - **Snapshots** (M263, [`super::snapshot`]). A snapshot's builder, once
//!   its setup ran all right, wipes itself and says `ready` at step
//!   `sealed` (or `failed`, for the wipe's reason), keeping nothing in its
//!   state directory, which the wipe removed; then it stops. A box made
//!   from the snapshot runs no setup: it says `ready` at step `snapshot`,
//!   from the record the builder left.
//! - **What a rebuild would lose** ([`status`], `GET /api/hosted/setup`):
//!   each repo's changed files and commits on no remote branch, and
//!   [`archive`], a tarball of the repos that have any.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use arugula_control_wire::hosted::{FROM_SNAPSHOT, SEALED, SetupPlan, SetupReport, SetupStatus};
use arugula_proto::{
    PaneId, Policy,
    api::{HostedRepo, HostedSetup, RunRequest},
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    mux::Api,
    pane::{CaptureFormat, CaptureScope},
    server::App,
};

/// What this box's setup did, in the state directory.
pub const STATE_FILE: &str = "hosted-setup.json";
/// Where its scripts and their notes go.
const DIR: &str = "hosted-setup";
/// The session its pane opens in.
pub const SESSION: &str = "setup";
/// How many of the pane's last lines a failure keeps, and how many bytes.
const TAIL_LINES: usize = 40;
const TAIL_MAX: usize = 4096;

/// What this box's setup did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub status: Option<SetupStatus>,
    #[serde(default)]
    pub step: String,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Why it failed: a fixed code for control (`SETUP_REASONS`).
    #[serde(default)]
    pub reason: String,
    /// A failed setup's last lines, from its pane: for clients, never sent
    /// to control.
    #[serde(default)]
    pub tail: String,
    #[serde(default)]
    pub spec_hash: String,
    /// The repos it clones, where.
    #[serde(default)]
    pub repos: Vec<Cloned>,
    /// Control has heard the latest.
    #[serde(default)]
    pub reported: bool,
}

/// A repo the setup clones, and its directory on the box.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cloned {
    pub url: String,
    pub dir: PathBuf,
}

impl Saved {
    pub fn load(state_dir: &Path) -> Saved {
        std::fs::read(state_dir.join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, state_dir: &Path) {
        let bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        if let Err(e) = crate::store::write_atomic(&state_dir.join(STATE_FILE), &bytes) {
            warn!(error = %e, "hosted: can't keep the setup's state");
        }
    }

    fn report(&self) -> SetupReport {
        SetupReport {
            status: self.status.unwrap_or(SetupStatus::SettingUp),
            step: self.step.clone(),
            reason: self.reason.clone(),
            pane: self.pane.map(u64::from),
            spec_hash: self.spec_hash.clone(),
        }
    }
}

/// Run this box's setup once it's in control, unless it ran already.
pub fn start(app: Arc<App>, state_dir: PathBuf) {
    tokio::spawn(async move { run(app, state_dir).await });
}

/// Whether control enrolled this box to build a workspace's snapshot.
fn builder(app: &App) -> bool {
    app.control.enrolled().is_some_and(|e| e.saved.hosted.as_ref().is_some_and(|h| h.builder))
}

async fn run(app: Arc<App>, state_dir: PathBuf) {
    let control = app.control.clone();
    while control.enrolled().is_none() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let mut saved = Saved::load(&state_dir);
    // A box from a workspace's snapshot is set up already.
    if saved.status.is_none()
        && let Some(r) = super::snapshot::from_snapshot(&super::snapshot::dir(), &super::resume::sandbox_id_file())
    {
        info!("hosted: set up from the workspace's snapshot");
        saved = Saved {
            status: Some(SetupStatus::Ready),
            step: FROM_SNAPSHOT.into(),
            spec_hash: r.spec_hash,
            repos: r.repos,
            ..Saved::default()
        };
        saved.save(&state_dir);
        report_until_heard(&app, &state_dir, &mut saved).await;
        return;
    }
    match saved.status {
        Some(SetupStatus::Ready | SetupStatus::Failed) => {
            if !saved.reported {
                report_until_heard(&app, &state_dir, &mut saved).await;
            }
            return;
        }
        // The daemon restarted mid-setup: watch its pane again.
        Some(SetupStatus::SettingUp) => {}
        None => {
            let Some(plan) = plan(&app).await else { return };
            saved = match begin(&app, &state_dir, &plan).await {
                Ok(s) => s,
                Err(e) => Saved {
                    status: Some(SetupStatus::Failed),
                    step: "start".into(),
                    reason: "start".into(),
                    tail: format!("{e:#}"),
                    spec_hash: plan.spec_hash.clone(),
                    ..Saved::default()
                },
            };
            saved.save(&state_dir);
            if saved.status == Some(SetupStatus::Failed) {
                report_until_heard(&app, &state_dir, &mut saved).await;
                return;
            }
            report_once(&app, &state_dir, &mut saved).await;
            return watch(&app, &state_dir, saved, Some(&plan)).await;
        }
    }
    watch(&app, &state_dir, saved, None).await;
}

/// Control's plan for this box, asked until it answers: `None` when it has
/// no workspace for this machine (no setup, then).
async fn plan(app: &App) -> Option<SetupPlan> {
    let mut wait = Duration::from_secs(1);
    loop {
        match app.control.hosted_plan().await {
            Ok(Some(p)) => return Some(p),
            Ok(None) => {
                info!("hosted: control has no workspace for this machine, so nothing to set up");
                return None;
            }
            Err(e) => warn!(error = %format!("{e:#}"), "hosted: can't read this box's setup yet"),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// Write the setup's scripts and its credentials, and open its pane.
async fn begin(app: &App, state_dir: &Path, plan: &SetupPlan) -> anyhow::Result<Saved> {
    let home = std::env::var_os("HOME").map(PathBuf::from).context("no $HOME to clone into")?;
    let dir = state_dir.join(DIR);
    // A fresh start: no step or exit from an earlier try.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let repos = places(&home, &plan.repos.iter().map(|r| r.url.clone()).collect::<Vec<_>>());
    std::fs::write(dir.join("script.sh"), &plan.setup)?;
    std::fs::write(dir.join("setup.sh"), script(&dir, &home, plan, &repos))?;
    let env = write_secret_file(&env_file(plan))?;
    let command = format!("bash {} {}", quote(&dir.join("setup.sh").to_string_lossy()), quote(&env.to_string_lossy()));
    let req = RunRequest {
        command: Some(command),
        session: Some(SESSION.into()),
        cwd: Some(home.to_string_lossy().into_owned()),
        policy: Some(Policy::None),
        ..RunRequest::default()
    };
    let pane = match app.mux.api(|r| Api::Run(req, r)).await {
        Some(Ok(p)) => p,
        Some(Err(e)) => {
            let _ = std::fs::remove_file(&env);
            anyhow::bail!("can't open the setup's pane: {e}");
        }
        None => {
            let _ = std::fs::remove_file(&env);
            anyhow::bail!("can't open the setup's pane: the daemon is stopping");
        }
    };
    info!(pane, repos = repos.len(), packages = plan.packages.len(), "hosted: setting up in a pane");
    Ok(Saved {
        status: Some(SetupStatus::SettingUp),
        step: "start".into(),
        pane: Some(pane),
        reason: String::new(),
        tail: String::new(),
        spec_hash: plan.spec_hash.clone(),
        repos,
        reported: false,
    })
}

/// Follow the setup's pane to its end, telling control each step. `plan`
/// is what it was given, when this daemon started it (a builder's wipe
/// looks for its secrets).
async fn watch(app: &App, state_dir: &Path, mut saved: Saved, plan: Option<&SetupPlan>) {
    let dir = state_dir.join(DIR);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let step = std::fs::read_to_string(dir.join("step")).unwrap_or_default();
        let done = std::fs::read_to_string(dir.join("done")).ok().and_then(|c| c.trim().parse::<i32>().ok());
        let handle = match saved.pane {
            Some(p) => app.mux.api(|r| Api::Pane(p, r)).await.flatten(),
            None => None,
        };
        if let Some(code) = done {
            if !step.is_empty() {
                saved.step = step;
            }
            if code == 0 && builder(app) {
                return sealed(app, state_dir, saved, plan).await;
            }
            if code == 0 {
                saved.status = Some(SetupStatus::Ready);
                saved.tail.clear();
            } else {
                saved.status = Some(SetupStatus::Failed);
                saved.reason = "exit".into();
                let text = handle.as_ref().and_then(|h| h.capture(CaptureFormat::Text, CaptureScope::Scrollback));
                saved.tail = tail(&text.unwrap_or_default());
            }
            info!(status = ?saved.status, step = saved.step, "hosted: setup ended");
            saved.reported = false;
            saved.save(state_dir);
            report_until_heard(app, state_dir, &mut saved).await;
            return;
        }
        if handle.is_none() {
            saved.status = Some(SetupStatus::Failed);
            saved.reason = "pane_closed".into();
            saved.tail = "The setup's pane closed before it finished.".into();
            saved.reported = false;
            saved.save(state_dir);
            report_until_heard(app, state_dir, &mut saved).await;
            return;
        }
        if !step.is_empty() && step != saved.step {
            saved.step = step;
            saved.reported = false;
            saved.save(state_dir);
            report_once(app, state_dir, &mut saved).await;
        }
    }
}

/// A builder's setup ran all right (M263): wipe the box for its snapshot,
/// say how that went until control hears, and stop. Nothing goes in the
/// state directory after the wipe, which removed it. A daemon that
/// restarted mid-setup has lost what setup was given, so it can't look for
/// it: it refuses.
async fn sealed(app: &App, state_dir: &Path, mut saved: Saved, plan: Option<&SetupPlan>) {
    use super::snapshot::Refused;
    let sandbox = super::resume::stamp(&super::resume::sandbox_id_file()).map(|(id, _)| id).unwrap_or_default();
    let done = match plan {
        _ if sandbox.is_empty() => Err(Refused::Failed("no sandbox id".into())),
        Some(p) => super::snapshot::seal(state_dir, &sandbox, p, &saved.repos).await,
        None => Err(Refused::Failed("the daemon restarted mid-setup".into())),
    };
    saved.step = SEALED.into();
    match &done {
        Ok(w) => {
            info!(
                killed = w.killed,
                removed = w.removed.len(),
                histories = w.histories,
                journal = w.journal,
                "hosted: wiped for the snapshot"
            );
            saved.status = Some(SetupStatus::Ready);
        }
        Err(r) => {
            // Where a secret was left, never what.
            warn!(reason = r.reason(), refused = ?r, "hosted: not wiped for the snapshot");
            saved.status = Some(SetupStatus::Failed);
            saved.reason = r.reason().into();
        }
    }
    let mut wait = Duration::from_secs(1);
    for _ in 0..30 {
        match app.control.hosted_report(&saved.report()).await {
            Ok(()) => break,
            Err(e) => warn!(error = %format!("{e:#}"), "hosted: control didn't hear the wipe"),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(30));
    }
    if done.is_ok() {
        super::snapshot::stop_self();
    }
}

/// Tell control once; the next step or the end says it again if this one
/// didn't get through.
async fn report_once(app: &App, state_dir: &Path, saved: &mut Saved) {
    match app.control.hosted_report(&saved.report()).await {
        Ok(()) => {
            saved.reported = true;
            saved.save(state_dir);
        }
        Err(e) => warn!(error = %format!("{e:#}"), "hosted: control didn't hear the setup's step"),
    }
}

/// Tell control how it ended, until it hears.
async fn report_until_heard(app: &App, state_dir: &Path, saved: &mut Saved) {
    let mut wait = Duration::from_secs(1);
    loop {
        report_once(app, state_dir, saved).await;
        if saved.reported {
            return;
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// Where each repo goes: `$HOME/<its name>`, with `-2`, `-3`... when two
/// share a name.
fn places(home: &Path, urls: &[String]) -> Vec<Cloned> {
    let mut taken: Vec<String> = Vec::new();
    urls.iter()
        .map(|url| {
            let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
            let base: String = last
                .strip_suffix(".git")
                .unwrap_or(last)
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
                .collect();
            let base = if base.is_empty() || base.starts_with('.') { "repo".to_owned() } else { base };
            let mut name = base.clone();
            let mut n = 2;
            while taken.contains(&name) {
                name = format!("{base}-{n}");
                n += 1;
            }
            taken.push(name.clone());
            Cloned { url: url.clone(), dir: home.join(name) }
        })
        .collect()
}

/// A word for the shell, as it is.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The credentials and env the setup reads first: a line each.
fn env_file(plan: &SetupPlan) -> String {
    let mut out = String::new();
    for (i, r) in plan.repos.iter().enumerate() {
        if let (Some(u), Some(p)) = (&r.username, &r.password) {
            let basic = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
            out.push_str(&format!("ARUGULA_GIT_AUTH_{i}={}\n", quote(&format!("Authorization: Basic {basic}"))));
        }
    }
    for (k, v) in &plan.env {
        // Names were checked by the platform; checked again, as they're
        // shell words here.
        if !k.is_empty() && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            out.push_str(&format!("export {k}={}\n", quote(v)));
        }
    }
    out
}

/// Write `contents` where only this user reads it, in memory when there's
/// somewhere: `$XDG_RUNTIME_DIR`, else `/dev/shm`, else the temp dir.
fn write_secret_file(contents: &str) -> anyhow::Result<PathBuf> {
    use std::io::Write as _;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(|| Some(PathBuf::from("/dev/shm")).filter(|d| d.is_dir()))
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("arugula-setup-{}.env", hex::encode(arugula_e2e::random::<8>())));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&path).with_context(|| format!("writing {}", path.display()))?.write_all(contents.as_bytes())?;
    Ok(path)
}

/// The script the setup's pane runs, with the env file as `$1`.
fn script(dir: &Path, home: &Path, plan: &SetupPlan, repos: &[Cloned]) -> String {
    let dir = quote(&dir.to_string_lossy());
    let mut s = format!(
        r#"#!/usr/bin/env bash
# This hosted workspace's setup, written by arugulad from its spec: clone
# its repos, install its packages, then run its setup script. It runs once,
# on the box's first boot; a rebuild makes a new box, which runs it again.
here={dir}
step() {{ printf '%s' "$1" > "$here/step"; printf '\n\033[1m==> %s\033[0m\n' "$1"; }}
trap 'printf "%s" "$?" > "$here/done"' EXIT
printf '\033]2;setup\007'
# What to clone with, and the env the spec takes from its secrets: read,
# then gone from the disk.
if [ -n "${{1:-}}" ]; then . "$1"; rm -f "$1"; fi
cd {home} || exit 1
"#,
        home = quote(&home.to_string_lossy()),
    );
    for (i, (r, c)) in plan.repos.iter().zip(repos).enumerate() {
        let name = c.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let to = quote(&c.dir.to_string_lossy());
        let auth = if r.password.is_some() {
            format!(
                "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http.extraHeader GIT_CONFIG_VALUE_0=\"$ARUGULA_GIT_AUTH_{i}\" "
            )
        } else {
            String::new()
        };
        s.push_str(&format!(
            r#"step {step}
if [ -e {to}/.git ]; then
  echo {there}
else
  echo {says}
  GIT_TERMINAL_PROMPT=0 {auth}git clone -- {url} {to} || exit $?
fi
unset ARUGULA_GIT_AUTH_{i}
"#,
            step = quote(&format!("clone {name}")),
            there = quote(&format!("{name} is there already")),
            says = quote(&format!("{} into {} ({})", r.url, c.dir.display(), how(&r.via))),
            url = quote(&r.url),
        ));
    }
    if !plan.packages.is_empty() {
        let packages: Vec<String> = plan.packages.iter().map(|p| quote(p)).collect();
        s.push_str(&format!(
            r#"step packages
if command -v apt-get >/dev/null 2>&1; then
  sudo=; [ "$(id -u)" = 0 ] || sudo='sudo -n'
  $sudo apt-get update -q || exit $?
  $sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -q -- {} || exit $?
else
  echo "There's no apt-get on this box to install packages with." >&2
  exit 1
fi
"#,
            packages.join(" ")
        ));
    }
    if !plan.setup.trim().is_empty() {
        // In the first repo, when there is one.
        let cwd = repos.first().map_or_else(|| quote(&home.to_string_lossy()), |c| quote(&c.dir.to_string_lossy()));
        s.push_str(&format!(
            r#"step 'setup script'
cd {cwd} || exit $?
bash -e "$here/script.sh" || exit $?
"#
        ));
    }
    if !plan.unset.is_empty() {
        s.push_str(&format!("echo {}\n", quote(&format!("Not set, so not in its env: {}", plan.unset.join(", ")))));
    }
    s.push_str("printf '\\n\\033[1mSet up.\\033[0m\\n'\n");
    s
}

/// How a repo is cloned, in words.
fn how(via: &str) -> &'static str {
    match via {
        "github_app" => "with a read-only token from the GitHub App",
        "secret" => "with the workspace's GIT_TOKEN",
        _ => "with no credentials",
    }
}

/// The pane's last lines, at most [`TAIL_LINES`] and [`TAIL_MAX`] bytes.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let mut out = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    while out.len() > TAIL_MAX {
        match out.find('\n') {
            Some(i) => out.drain(..=i),
            None => out.drain(..out.len() - TAIL_MAX),
        };
    }
    out
}

/// `GET /api/hosted/setup`: what the setup did, and where each repo stands.
pub async fn status(state_dir: &Path) -> HostedSetup {
    let saved = Saved::load(state_dir);
    let mut repos = Vec::new();
    for c in &saved.repos {
        repos.push(repo_state(c).await);
    }
    HostedSetup {
        status: saved.status.map(|s| s.as_str().to_owned()),
        step: saved.step,
        pane: saved.pane,
        tail: saved.tail,
        spec_hash: saved.spec_hash,
        repos,
    }
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn repo_state(c: &Cloned) -> HostedRepo {
    let mut r = HostedRepo { dir: c.dir.to_string_lossy().into_owned(), url: c.url.clone(), ..HostedRepo::default() };
    if !c.dir.join(".git").exists() {
        r.error = Some("not cloned".into());
        return r;
    }
    match git(&c.dir, &["status", "--porcelain"]).await {
        Ok(s) => r.changed = s.lines().filter(|l| !l.is_empty()).count() as u32,
        Err(e) => r.error = Some(e),
    }
    // A repo with no commits yet has nothing to count.
    if let Ok(n) = git(&c.dir, &["rev-list", "--count", "HEAD", "--not", "--remotes"]).await {
        r.unpushed = n.trim().parse().unwrap_or(0);
    }
    r.branch = git(&c.dir, &["symbolic-ref", "--short", "-q", "HEAD"]).await.unwrap_or_default().trim().to_owned();
    r
}

/// `GET /api/hosted/setup/archive`: a gzipped tarball of the repos with
/// work a rebuild would lose: each one's files that aren't ignored, and its
/// `.git` (its unpushed commits). `None` when no repo has any.
pub async fn archive(state_dir: &Path) -> anyhow::Result<Option<tokio::process::Child>> {
    let saved = Saved::load(state_dir);
    let mut list: Vec<u8> = Vec::new();
    let mut root: Option<PathBuf> = None;
    for c in &saved.repos {
        if !repo_state(c).await.dirty() {
            continue;
        }
        let (Some(parent), Some(name)) = (c.dir.parent(), c.dir.file_name()) else { continue };
        let parent = parent.to_path_buf();
        anyhow::ensure!(root.as_ref().is_none_or(|r| *r == parent), "the repos aren't in one directory");
        root = Some(parent);
        let name = name.to_string_lossy();
        let files = git(&c.dir, &["ls-files", "-z", "-co", "--exclude-standard"]).await.map_err(anyhow::Error::msg)?;
        for f in files.split('\0').filter(|f| !f.is_empty()) {
            list.extend_from_slice(format!("{name}/{f}\0").as_bytes());
        }
        list.extend_from_slice(format!("{name}/.git\0").as_bytes());
    }
    let Some(root) = root else { return Ok(None) };
    let mut child = tokio::process::Command::new("tar")
        .args(["-czf", "-", "--null", "-T", "-"])
        .current_dir(&root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("tar")?;
    let mut stdin = child.stdin.take().context("tar's stdin")?;
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        let _ = stdin.write_all(&list).await;
    });
    Ok(Some(child))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use arugula_control_wire::hosted::SetupRepo;

    use super::*;

    #[cfg(unix)]
    fn plan() -> SetupPlan {
        SetupPlan {
            workspace: "hw_1".into(),
            spec_hash: "h1".into(),
            repos: vec![
                SetupRepo {
                    url: "https://git.example/acme/app.git".into(),
                    via: "secret".into(),
                    username: Some("x-access-token".into()),
                    password: Some("it's-secret".into()),
                },
                SetupRepo { url: "https://git.example/other/app".into(), via: "none".into(), ..Default::default() },
            ],
            packages: vec!["jq".into()],
            setup: "make".into(),
            env: [("NPM_TOKEN".to_owned(), "n'pm".to_owned()), ("bad name".to_owned(), "x".to_owned())].into(),
            unset: vec!["OTHER".into()],
        }
    }

    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[test]
    fn repos_get_a_directory_each() {
        let got = places(
            Path::new("/home/u"),
            &["https://h/a/app.git".into(), "https://h/b/app/".into(), "https://h/c/.git".into()],
        );
        let dirs: Vec<_> = got.iter().map(|c| c.dir.to_string_lossy().into_owned()).collect();
        assert_eq!(dirs, ["/home/u/app", "/home/u/app-2", "/home/u/repo"]);
    }

    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[test]
    fn credentials_go_in_the_env_file_only() {
        let p = plan();
        let env = env_file(&p);
        let basic = base64::engine::general_purpose::STANDARD.encode("x-access-token:it's-secret");
        assert!(env.contains(&format!("ARUGULA_GIT_AUTH_0='Authorization: Basic {basic}'")), "{env}");
        assert!(!env.contains("ARUGULA_GIT_AUTH_1"), "no credentials for the second: {env}");
        assert!(env.contains(r"export NPM_TOKEN='n'\''pm'"), "{env}");
        assert!(!env.contains("bad name"), "{env}");

        let repos = places(Path::new("/home/u"), &p.repos.iter().map(|r| r.url.clone()).collect::<Vec<_>>());
        let s = script(Path::new("/st/hosted-setup"), Path::new("/home/u"), &p, &repos);
        assert!(!s.contains("secret'") && !s.contains(&basic), "no credentials in the script: {s}");
        assert!(s.contains("GIT_CONFIG_VALUE_0=\"$ARUGULA_GIT_AUTH_0\" git clone -- 'https://git.example/acme/app.git' '/home/u/app'"), "{s}");
        assert!(
            s.contains("GIT_TERMINAL_PROMPT=0 git clone -- 'https://git.example/other/app' '/home/u/app-2'"),
            "{s}"
        );
        assert!(s.contains("step 'clone app'") && s.contains("step 'clone app-2'"), "{s}");
        assert!(s.contains("apt-get install -y -q -- 'jq'"), "{s}");
        assert!(s.contains("cd '/home/u/app' || exit $?\nbash -e \"$here/script.sh\""), "{s}");
        assert!(s.contains("Not set, so not in its env: OTHER"), "{s}");
    }

    /// The script runs: it reads and removes its env file, clones each
    /// repo with what it was given, runs the setup script in the first, and
    /// leaves its steps and exit code beside it; a failing step stops it.
    // Hosted boxes are Linux: its paths and shell.
    #[cfg(unix)]
    #[test]
    fn the_script_runs() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("no git here: skipped");
            return;
        }
        let root = super::super::scratch("setup-run");
        let home = root.join("home");
        let st = root.join("st");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&st).unwrap();
        // Two "remotes": plain repos cloned by path (no http here).
        let mut urls = vec![];
        for name in ["one", "two"] {
            let r = root.join("remotes").join(name);
            std::fs::create_dir_all(&r).unwrap();
            let run = |args: &[&str]| {
                assert!(std::process::Command::new("git").current_dir(&r).args(args).output().unwrap().status.success())
            };
            run(&["init", "-q", "-b", "main"]);
            std::fs::write(r.join("README"), name).unwrap();
            run(&["add", "README"]);
            run(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "first"]);
            urls.push(r.to_string_lossy().into_owned());
        }
        let mut p = SetupPlan {
            repos: urls
                .iter()
                .map(|u| SetupRepo { url: u.clone(), via: "none".into(), ..Default::default() })
                .collect(),
            setup: "echo \"$MY_VAR\" > made-here\n".into(),
            env: [("MY_VAR".to_owned(), "from a secret".to_owned())].into(),
            ..Default::default()
        };
        let run = |p: &SetupPlan| {
            let _ = std::fs::remove_file(st.join("done"));
            let repos = places(&home, &p.repos.iter().map(|r| r.url.clone()).collect::<Vec<_>>());
            std::fs::write(st.join("script.sh"), &p.setup).unwrap();
            std::fs::write(st.join("setup.sh"), script(&st, &home, p, &repos)).unwrap();
            let env = root.join(format!("env-{}", hex::encode(arugula_e2e::random::<4>())));
            std::fs::write(&env, env_file(p)).unwrap();
            let out = std::process::Command::new("bash").arg(st.join("setup.sh")).arg(&env).output().unwrap();
            assert!(!env.exists(), "the env file is gone once read");
            (out, std::fs::read_to_string(st.join("done")).unwrap(), std::fs::read_to_string(st.join("step")).unwrap())
        };
        let (out, done, step) = run(&p);
        assert_eq!(done, "0", "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(step, "setup script");
        assert_eq!(std::fs::read_to_string(home.join("one/README")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(home.join("two/README")).unwrap(), "two");
        assert_eq!(std::fs::read_to_string(home.join("one/made-here")).unwrap(), "from a secret\n");

        // Again: what's cloned stays; a failing script fails at its step.
        p.setup = "false\necho never > never\n".into();
        let (_, done, step) = run(&p);
        assert_eq!((done.as_str(), step.as_str()), ("1", "setup script"));
        assert!(!home.join("one/never").exists(), "bash -e stops at the failure");
        p.repos.push(SetupRepo {
            url: root.join("nowhere").to_string_lossy().into(),
            via: "none".into(),
            ..Default::default()
        });
        let (_, done, step) = run(&p);
        assert_ne!(done, "0");
        assert_eq!(step, "clone nowhere");
    }

    /// What control hears of a failure is its step and a code; the pane's
    /// last lines stay here, for clients.
    #[test]
    fn reports_carry_no_pane_text() {
        let saved = Saved {
            status: Some(SetupStatus::Failed),
            step: "setup script".into(),
            reason: "exit".into(),
            tail: "make: *** [all] Error 2".into(),
            ..Saved::default()
        };
        let sent = serde_json::to_string(&saved.report()).unwrap();
        assert!(sent.contains("\"reason\":\"exit\"") && !sent.contains("Error 2"), "{sent}");
    }

    #[test]
    fn tails_keep_the_last_lines() {
        let text: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        let t = tail(&text);
        assert!(t.starts_with("line 61\n") && t.ends_with("line 100"), "{t}");
        let long = "x".repeat(TAIL_MAX * 2);
        assert_eq!(tail(&long).len(), TAIL_MAX);
    }

    /// Where the repos stand: changed files and commits on no remote
    /// branch; the archive holds only the repos with any.
    #[tokio::test]
    async fn what_a_rebuild_would_lose() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("no git here: skipped");
            return;
        }
        let root = super::super::scratch("setup-lose");
        let remote = root.join("remote");
        std::fs::create_dir_all(&remote).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let o = std::process::Command::new("git")
                .current_dir(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@e"])
                .args(args)
                .output()
                .unwrap();
            assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&remote, &["init", "-q", "-b", "main"]);
        std::fs::write(remote.join("a"), "a").unwrap();
        git(&remote, &["add", "a"]);
        git(&remote, &["commit", "-q", "-m", "a"]);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        for n in ["clean", "dirty"] {
            git(&home, &["clone", "-q", &remote.to_string_lossy(), n]);
        }
        let dirty = home.join("dirty");
        std::fs::write(dirty.join("new"), "new").unwrap();
        std::fs::write(dirty.join(".gitignore"), "ignored\n").unwrap();
        std::fs::write(dirty.join("ignored"), "big").unwrap();
        git(&dirty, &["add", ".gitignore"]);
        git(&dirty, &["commit", "-q", "-m", "local"]);
        let st = root.join("st");
        std::fs::create_dir_all(&st).unwrap();
        let saved = Saved {
            status: Some(SetupStatus::Ready),
            repos: vec![
                Cloned { url: "u1".into(), dir: home.join("clean") },
                Cloned { url: "u2".into(), dir: dirty.clone() },
                Cloned { url: "u3".into(), dir: home.join("gone") },
            ],
            ..Saved::default()
        };
        saved.save(&st);
        let s = status(&st).await;
        assert_eq!(s.status.as_deref(), Some("ready"));
        let got: Vec<_> = s.repos.iter().map(|r| (r.changed, r.unpushed, r.error.is_some())).collect();
        assert_eq!(got, [(0, 0, false), (1, 1, false), (0, 0, true)]);
        assert_eq!(s.repos[1].branch, "main");

        let child = archive(&st).await.unwrap().expect("one repo has work");
        let out = child.wait_with_output().await.unwrap();
        let tgz = root.join("a.tgz");
        std::fs::write(&tgz, &out.stdout).unwrap();
        let listed = std::process::Command::new("tar").arg("-tzf").arg(&tgz).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout);
        assert!(listed.contains("dirty/new") && listed.contains("dirty/.git/HEAD"), "{listed}");
        assert!(!listed.contains("clean/") && !listed.contains("dirty/ignored"), "{listed}");

        std::fs::remove_file(dirty.join("new")).unwrap();
        std::fs::remove_file(dirty.join("ignored")).unwrap();
        git(&dirty, &["reset", "-q", "--hard", "origin/main"]);
        assert!(archive(&st).await.unwrap().is_none(), "nothing unpushed");
    }
}
