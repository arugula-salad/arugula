//! Moving a beta Cloud workspace off Managoat (M189, #1250): "Move to the
//! new Cloud workspaces". A Managoat-backed Cloud workspace is made again
//! as a hosted workspace ([`crate::hosted`]) in the same project, from its
//! spec. Its disk doesn't move.
//!
//! - **What's left behind** ([`plan`]): each repo's uncommitted and
//!   unpushed work, from Managoat's `git-status` and `diff` reads of its
//!   machine (which never wake it), and a download of it ([`bundle`]): a
//!   tar of each repo's patches and its untracked files, made from the same
//!   reads.
//! - **The move** ([`go`]) maps the spec ([`map`]), carries its secrets
//!   over (the account's cloud secrets it names and its environment
//!   variables, opened and sealed again with the project's key as the hosted
//!   workspace's), and copies its tabs' transcripts into the project's
//!   archive ([`crate::hosted::transcripts`]), read only. The Managoat
//!   workspace is left as it is, until the deletion.
//! - **Once.** The hosted workspace's id is kept on the Cloud workspace
//!   before anything is made, so a move asked twice (or after a lost
//!   answer) makes one hosted workspace. Its secrets and transcripts are
//!   kept by that id and the tab, so the same move again changes nothing.
//! - **The notice.** `ARUGULA_MANAGOAT_DELETE_AFTER` (a date) puts the
//!   day beta workspaces are deleted on their pages ([`overview`]); unset,
//!   nothing says so. The deletion itself is an operator's command
//!   ([`super::retire`]).
//!
//! Only a person moves their own Cloud workspace, and only where the
//! platform has hosted workspaces and their account may make them
//! (`ARUGULA_HOSTED_ACCOUNTS`).

use std::{collections::BTreeSet, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::transcripts::{MAX_TEXT, fit};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::{enabled, provider::ReadRefusal};
use crate::{
    Platform,
    auth::Person,
    db::{
        cloud::{CloudWorkspace, CloudWorkspaceSpec},
        cloud_moves::MovedTab,
        hosted::Added,
        hosted_transcripts::NewTranscript,
    },
    forge::links::membership,
    hosted::{HostedWorkspace, Spec, routes::MAX_PER_PROJECT, setup::GIT_TOKEN},
};

/// Where Managoat put a repository it wasn't told where to put.
const SPRITE_HOME: &str = "/home/sprite/";
/// The most a hosted workspace's secret may be (`hosted::routes`).
const MAX_SECRET: usize = 16 * 1024;
/// Managoat's cap on a read's `max_bytes`.
const MAX_READ: u64 = 4_194_304;
/// Untracked files a download takes, per repo and in all.
const MAX_UNTRACKED: usize = 200;
const MAX_BUNDLE: usize = 64 * 1024 * 1024;

// ---- the spec

/// Where a carried secret's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The account's cloud secret of this name.
    Secret(String),
    /// One of the spec's environment variables, a value in it.
    Value(String),
}

/// A secret the hosted workspace gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carry {
    pub name: String,
    pub source: Source,
}

/// A Cloud workspace's spec as a hosted workspace's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    /// Its name, as a hosted workspace's may be.
    pub name: String,
    pub spec: Spec,
    pub carry: Vec<Carry>,
    /// What doesn't carry over as it was, in words.
    pub notes: Vec<String>,
}

/// A hosted workspace's name for `name`: letters, digits, `-`, `_` and
/// `.`, the rest made `-`.
pub fn hosted_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' };
        if !(c == '-' && (out.is_empty() || out.ends_with('-'))) {
            out.push(c);
        }
    }
    let out: String = out.trim_start_matches(['-', '_', '.']).chars().take(60).collect();
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() { "workspace".into() } else { out }
}

/// A word for the shell, as it is.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The directory a hosted box clones each URL into, under `$HOME`, as its
/// daemon names them (`places` in arugulad's `hosted::setup`).
fn clone_dirs(urls: &[String]) -> Vec<String> {
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
            name
        })
        .collect()
}

/// Map a Cloud workspace's spec onto a hosted workspace's:
///
/// - its repositories' URLs are its repos, each cloned into `~/<name>`; a
///   repository's ref is checked out first thing in the setup script;
/// - its apt packages are its packages, and its npm packages are
///   installed (`npm install -g`) by the setup script, before its own;
/// - its environment variables and the cloud secrets it names are the
///   env it takes from its secrets, each carried over as one;
/// - the secret its repositories clone with is its `GIT_TOKEN` (a GitHub
///   repo linked to the project clones with the App instead).
///
/// Mounts, the setup timeout and networking don't carry: `notes` says so.
pub fn map(name: &str, s: &CloudWorkspaceSpec) -> Mapped {
    let mut notes = Vec::new();
    let repos: Vec<String> = s.repositories.iter().map(|r| r.url.clone()).collect();
    let dirs = clone_dirs(&repos);
    let mut setup = Vec::new();
    for (r, dir) in s.repositories.iter().zip(&dirs) {
        if r.mount != format!("{SPRITE_HOME}{dir}") {
            notes.push(format!("{} was at {}; it's cloned to ~/{dir} now.", r.url, r.mount));
        }
        if !r.git_ref.is_empty() {
            setup.push(format!("git -C \"$HOME\"/{} checkout {}", quote(dir), quote(&r.git_ref)));
        }
    }
    if !s.packages.npm.is_empty() {
        let npm: Vec<String> = s.packages.npm.iter().map(|p| quote(p)).collect();
        setup.push(format!("npm install -g -- {}", npm.join(" ")));
    }
    if !setup.is_empty() && !s.setup_script.trim().is_empty() {
        setup.push(String::new());
    }
    let mut script = setup.join("\n");
    if !script.is_empty() {
        script = format!("# Carried from the beta Cloud workspace: its refs and npm packages.\n{script}");
        if !s.setup_script.trim().is_empty() {
            script.push('\n');
        }
    }
    script.push_str(&s.setup_script);
    if s.setup_timeout_seconds.is_some() {
        notes.push("Its setup timeout doesn't carry: setup runs in a pane until it ends.".into());
    }
    if s.networking.is_some() {
        notes.push("Its networking setting doesn't carry.".into());
    }

    let mut carry: Vec<Carry> = Vec::new();
    let mut env = Vec::new();
    let mut add = |name: &str, source: Source, in_env: bool, carry: &mut Vec<Carry>, notes: &mut Vec<String>| {
        if carry.iter().any(|c| c.name == name) {
            return;
        }
        if !super::secrets::valid_name(name) {
            notes.push(format!("{name} isn't a name a hosted workspace's env may take, so it isn't carried."));
            return;
        }
        if in_env {
            env.push(name.to_owned());
        }
        carry.push(Carry { name: name.to_owned(), source });
    };
    for (k, v) in &s.env_vars {
        add(k, Source::Value(v.clone()), true, &mut carry, &mut notes);
    }
    for n in &s.secrets {
        add(n, Source::Secret(n.clone()), true, &mut carry, &mut notes);
    }
    let clone_with: Vec<&str> = {
        let mut seen = BTreeSet::new();
        s.repositories.iter().filter_map(|r| r.secret.as_deref()).filter(|n| seen.insert(*n)).collect()
    };
    if let Some(first) = clone_with.first() {
        if !carry.iter().any(|c| c.name == GIT_TOKEN) {
            carry.push(Carry { name: GIT_TOKEN.into(), source: Source::Secret((*first).into()) });
        }
        if clone_with.len() > 1 {
            notes.push(format!(
                "Its repos cloned with {}. A hosted workspace clones with one GIT_TOKEN, now {first}'s, unless the repo is linked to the project.",
                clone_with.join(", ")
            ));
        }
    }
    Mapped {
        name: hosted_name(name),
        spec: Spec { repos, packages: s.packages.apt.clone(), setup: script, env },
        carry,
        notes,
    }
}

// ---- what's left behind

/// A repo's work on the Managoat machine that a move leaves behind.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Leftover {
    pub mount: String,
    pub url: String,
    pub branch: Option<String>,
    /// Changed and untracked files, as `git status` says.
    pub changes: Vec<Change>,
    /// Changes to tracked files against HEAD.
    pub uncommitted: bool,
    /// Commits not on `compared_to` (its upstream, else `origin/HEAD`):
    /// `None` when there's neither to compare with.
    pub unpushed: Option<bool>,
    pub compared_to: Option<String>,
    /// More changed than Managoat said.
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub path: String,
    pub index: String,
    pub worktree: String,
}

/// A repo as it was read: what [`Leftover`] says, and the patches.
#[derive(Default)]
struct Read {
    leftover: Leftover,
    root: String,
    uncommitted: String,
    unpushed: Option<String>,
}

/// What a read refusal tells the person.
fn unread(e: &anyhow::Error) -> String {
    match e.downcast_ref::<ReadRefusal>() {
        Some(r) if r.parked => {
            "Its machine is asleep, and looking doesn't wake it. A prompt to one of its tabs does; then look again."
                .into()
        }
        Some(r) if matches!(r.code, Some("not_a_repository" | "path_not_found")) => {
            "It isn't a repository on the machine's disk any more.".into()
        }
        _ => "Managoat couldn't read it. Try again.".into(),
    }
}

fn read_text(v: &Value, field: &str) -> String {
    v[field].as_str().unwrap_or_default().to_owned()
}

/// Read one repo, mounted at `mount`, on Managoat machine `machine`.
async fn read_repo(app: &Platform, machine: &str, url: &str, mount: &str) -> Read {
    let p = &app.cloud.as_ref().expect("cloud is enabled").provider;
    let mut r = Read {
        leftover: Leftover { mount: mount.into(), url: url.into(), ..Default::default() },
        ..Default::default()
    };
    let max = MAX_READ.to_string();
    let status = p.read_cloud_workspace(machine, "git-status", &[("path", mount), ("untracked", "all")]).await;
    let head = p.read_cloud_workspace(machine, "diff", &[("path", mount), ("ref", "HEAD"), ("max_bytes", &max)]).await;
    let (status, head) = match (status, head) {
        (Ok(s), Ok(h)) => (s, h),
        (Err(e), _) | (_, Err(e)) => {
            r.leftover.error = Some(unread(&e));
            return r;
        }
    };
    r.root = status["repo_root"].as_str().unwrap_or(mount).to_owned();
    r.leftover.branch = status["branch"].as_str().map(str::to_owned);
    r.leftover.truncated =
        status["truncated"].as_bool().unwrap_or(false) || head["truncated"].as_bool().unwrap_or(false);
    r.leftover.changes = status["entries"]
        .as_array()
        .map(|es| {
            es.iter()
                .map(|e| Change {
                    path: read_text(e, "path"),
                    index: read_text(e, "index"),
                    worktree: read_text(e, "worktree"),
                })
                .collect()
        })
        .unwrap_or_default();
    r.uncommitted = read_text(&head, "diff");
    r.leftover.uncommitted = !r.uncommitted.is_empty();
    // Its upstream, else the remote's default branch: what a fresh clone has.
    for base in ["HEAD@{upstream}", "origin/HEAD"] {
        match p.read_cloud_workspace(machine, "diff", &[("path", mount), ("ref", base), ("max_bytes", &max)]).await {
            Ok(d) => {
                let diff = read_text(&d, "diff");
                r.leftover.unpushed = Some(diff != r.uncommitted);
                r.leftover.truncated |= d["truncated"].as_bool().unwrap_or(false);
                r.leftover.compared_to = Some(base.into());
                r.unpushed = Some(diff);
                break;
            }
            Err(e) if e.downcast_ref::<ReadRefusal>().is_some_and(|r| r.code == Some("ref_not_found")) => continue,
            Err(e) => {
                r.leftover.error = Some(unread(&e));
                break;
            }
        }
    }
    r
}

/// The repos on the Managoat machine's disk: those it was built with.
fn on_disk(w: &CloudWorkspace) -> &CloudWorkspaceSpec {
    w.built_spec.as_ref().unwrap_or(&w.saved_spec)
}

/// Whether the Managoat machine can be read without waking it, and why not.
fn readable(w: &CloudWorkspace, machine: Option<&str>) -> Result<(), String> {
    match (w.status.as_str(), w.sandbox_status.as_deref(), machine) {
        (_, _, None) => Err("It never had a machine, so there's nothing on a disk to keep.".into()),
        ("ready", Some("suspended"), _) => Err(
            "Its machine is asleep, and looking doesn't wake it. A prompt to one of its tabs does; then look again."
                .into(),
        ),
        ("ready", _, _) => Ok(()),
        ("provisioning", _, _) => Err("It's still setting up, so there's nothing to read yet.".into()),
        _ => Err("Its machine isn't there to read.".into()),
    }
}

async fn read_all(app: &Platform, w: &CloudWorkspace, machine: Option<&str>) -> Result<Vec<Read>, String> {
    readable(w, machine)?;
    let machine = machine.unwrap_or_default();
    let mut reads = Vec::new();
    for r in &on_disk(w).repositories {
        reads.push(read_repo(app, machine, &r.url, &r.mount).await);
    }
    Ok(reads)
}

// ---- who, and where to

/// The account's Cloud workspace, its row, and its project, for a move.
async fn source(
    app: &Platform,
    account: &str,
    id: &str,
) -> Result<(CloudWorkspace, crate::db::cloud_moves::MoveSource, String), ApiError> {
    enabled(app, account)?;
    let missing = || err(StatusCode::NOT_FOUND, "cloud workspace not found");
    let w = app.db.cloud_workspace(account, id)?.ok_or_else(missing)?;
    let s = app.db.cloud_move_source(account, id)?.ok_or_else(missing)?;
    let project = s.project.clone().ok_or_else(|| err(StatusCode::CONFLICT, "it isn't in a project"))?;
    Ok((w, s, project))
}

/// Why this account can't move its Cloud workspaces here, if it can't.
fn why_not(app: &Platform, account: &str) -> Option<&'static str> {
    if app.compute.is_none() {
        Some("The new Cloud workspaces aren't set up on this control yet.")
    } else if !app.hosted.allows(account) {
        Some("The new Cloud workspaces aren't open to your account yet.")
    } else {
        None
    }
}

/// The hosted workspace a Cloud workspace moved to, while it's there.
#[derive(Debug, Serialize)]
pub struct Moved {
    pub id: String,
    pub project: String,
    pub name: String,
    /// False while the move is under way (or was cut short: moving again
    /// finishes it).
    pub done: bool,
}

fn moved(app: &Platform, s: &crate::db::cloud_moves::MoveSource, project: &str) -> anyhow::Result<Option<Moved>> {
    let Some(to) = &s.moved_to else { return Ok(None) };
    Ok(app.db.hosted_workspace(project, to)?.filter(|h| &h.id == to).map(|h| Moved {
        id: h.id,
        project: project.to_owned(),
        name: h.name,
        done: s.moved_at.is_some(),
    }))
}

// ---- routes

/// `GET /api/cloud/moving`: the day beta Cloud workspaces are deleted,
/// when it's set, and whether this account can move them.
pub async fn overview(State(app): State<Arc<Platform>>, session: Person) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    let why = why_not(&app, &session.account);
    Ok(Json(json!({
        "delete_after": app.hosted.managoat_delete_after.map(|d| d.to_string()),
        "can_move": why.is_none(),
        "why_not": why,
    })))
}

/// What a move would make.
#[derive(Debug, Serialize)]
pub struct Made {
    pub name: String,
    pub repos: Vec<String>,
    pub packages: Vec<String>,
    pub setup: bool,
    pub env: Vec<String>,
    /// The secrets it gets, by name: never a value.
    pub secrets: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlanQuery {
    /// Read the machine (the default); `false` answers the rest alone.
    #[serde(default)]
    pub read: Option<bool>,
}

/// `GET /api/cloud/workspaces/{id}/move`: what a move leaves behind, per
/// repo, read from its Managoat machine now (unless `?read=false`), and
/// what it makes.
pub async fn plan(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
    Query(q): Query<PlanQuery>,
) -> Result<Json<Value>, ApiError> {
    let (w, s, project) = source(&app, &session.account, &id).await?;
    let m = map(&w.name, &w.saved_spec);
    let reads = if q.read == Some(false) {
        Err("Not read.".to_owned())
    } else {
        read_all(&app, &w, s.machine.as_deref()).await
    };
    let why = why_not(&app, &session.account);
    let tabs = app.db.cloud_tabs_ever(&session.account, &id)?.len();
    let made = Made {
        name: m.name,
        repos: m.spec.repos,
        packages: m.spec.packages,
        setup: !m.spec.setup.trim().is_empty(),
        env: m.spec.env,
        secrets: m.carry.into_iter().map(|c| c.name).collect(),
    };
    Ok(Json(json!({
        "id": w.id,
        "name": w.name,
        "project": project,
        "delete_after": app.hosted.managoat_delete_after.map(|d| d.to_string()),
        "can_move": why.is_none(),
        "why_not": why,
        "moved": moved(&app, &s, &project)?,
        "makes": made,
        "notes": m.notes,
        "transcripts": tabs,
        "unread": reads.as_ref().err(),
        "repos": reads.as_ref().map(|r| r.iter().map(|r| &r.leftover).collect::<Vec<_>>()).unwrap_or_default(),
    })))
}

/// `GET /api/cloud/workspaces/{id}/move/bundle`: a tar of what a move
/// leaves behind, per repo: `uncommitted.patch` (against HEAD),
/// `unpushed.patch` (against its upstream, which a fresh clone takes with
/// `git apply`), and its untracked files under `untracked/`. All of it is
/// read through Managoat, which redacts secrets it knows of.
pub async fn bundle(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (w, s, _) = source(&app, &session.account, &id).await?;
    let reads = read_all(&app, &w, s.machine.as_deref()).await.map_err(|e| err(StatusCode::CONFLICT, &e))?;
    let p = &app.cloud.as_ref().expect("cloud is enabled").provider;
    let machine = s.machine.as_deref().unwrap_or_default();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut readme = format!(
        "What \"{}\" left on its beta Cloud workspace's disk, read {}.\n\n\
         Each repo's directory has, when there's any:\n\
         - unpushed.patch: everything that isn't on its upstream (or origin/HEAD), committed or not.\n  \
           In a fresh clone: git apply unpushed.patch\n\
         - uncommitted.patch: just what isn't committed, against HEAD.\n\
         - untracked/: files git doesn't track.\n\n\
         Managoat read these and redacts the secrets it knows of.\n\n",
        w.name,
        jiff::Timestamp::now().strftime("%Y-%m-%d %H:%M UTC"),
    );
    let mut total = 0usize;
    let dirs = clone_dirs(&reads.iter().map(|r| r.leftover.url.clone()).collect::<Vec<_>>());
    for (r, dir) in reads.iter().zip(&dirs) {
        let l = &r.leftover;
        readme.push_str(&format!("{dir}/: {} at {}", l.url, l.mount));
        if let Some(b) = &l.branch {
            readme.push_str(&format!(" (branch {b})"));
        }
        readme.push('\n');
        if let Some(e) = &l.error {
            readme.push_str(&format!("  Not read: {e}\n"));
        }
        if l.truncated {
            readme.push_str("  Managoat cut what it read: not everything is here.\n");
        }
        if !r.uncommitted.is_empty() {
            files.push((format!("{dir}/uncommitted.patch"), r.uncommitted.clone().into_bytes()));
        }
        if let Some(u) = r.unpushed.as_ref().filter(|u| !u.is_empty() && l.unpushed == Some(true)) {
            files.push((format!("{dir}/unpushed.patch"), u.clone().into_bytes()));
        }
        let untracked: Vec<&Change> = l.changes.iter().filter(|c| c.worktree == "untracked").collect();
        for (n, c) in untracked.iter().enumerate() {
            let Some(rel) = safe_path(&c.path) else {
                readme.push_str(&format!("  Left out {:?}: not a path inside the repo.\n", c.path));
                continue;
            };
            if n >= MAX_UNTRACKED || total >= MAX_BUNDLE {
                readme.push_str(&format!("  Left out {rel}: the download is full.\n"));
                continue;
            }
            let path = format!("{}/{rel}", r.root.trim_end_matches('/'));
            let max = MAX_READ.to_string();
            let file = match p.read_cloud_workspace(machine, "file", &[("path", &path), ("max_bytes", &max)]).await {
                Ok(f) => f,
                Err(e) => {
                    readme.push_str(&format!("  Left out {rel}: {}\n", unread(&e)));
                    continue;
                }
            };
            let content = read_text(&file, "content");
            let bytes = if file["encoding"] == "base64" {
                base64::engine::general_purpose::STANDARD.decode(content.as_bytes()).unwrap_or_default()
            } else {
                content.into_bytes()
            };
            if file["truncated"].as_bool().unwrap_or(false) {
                readme.push_str(&format!("  {rel} is cut: only its first {} bytes were read.\n", bytes.len()));
            }
            total += bytes.len();
            files.push((format!("{dir}/untracked/{rel}"), bytes));
        }
    }
    files.insert(0, ("README.txt".into(), readme.into_bytes()));
    let name = format!("{}-left-behind.tar", hosted_name(&w.name));
    info!(account = %session.account, cloud_workspace = %id, files = files.len(), "downloaded what a move leaves behind");
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-tar".to_owned()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        tar(&files),
    )
        .into_response())
}

/// A path inside a repo, as git status gives it: relative, with no `..`.
fn safe_path(p: &str) -> Option<String> {
    let p = p.trim_end_matches('/');
    let ok = !p.is_empty()
        && !p.starts_with('/')
        && !p.chars().any(char::is_control)
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..");
    ok.then(|| p.to_owned())
}

/// What a move made.
#[derive(Debug, Serialize)]
pub struct MovedTo {
    pub workspace: HostedWorkspace,
    pub project: String,
    /// Transcripts copied into the project's archive.
    pub transcripts: usize,
    pub secrets: Vec<String>,
    pub notes: Vec<String>,
}

/// `POST /api/cloud/workspaces/{id}/move`: make it again as a hosted
/// workspace in its project, with its secrets and its tabs' transcripts.
/// The Managoat workspace stays. Asked again, it's the same hosted
/// workspace.
pub async fn go(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<MovedTo>, ApiError> {
    let account = session.account.as_str();
    let (w, s, project) = source(&app, account, &id).await?;
    if let Some(why) = why_not(&app, account) {
        return Err(err(StatusCode::FORBIDDEN, why));
    }
    if w.status == "deleting" {
        return Err(err(StatusCode::CONFLICT, "it's being deleted"));
    }
    let (project, role) = membership(&app, account, &project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't make a project's hosted workspaces"));
    }
    let mut m = map(&w.name, &w.saved_spec);
    m.spec.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &format!("it can't move as it is: {e}")))?;
    let cloud = enabled(&app, account)?;
    let ciphers = match (&cloud.secret_cipher, &app.hosted.cipher) {
        (Some(from), Some(to)) => Some((from, to.clone())),
        _ if m.carry.is_empty() => None,
        _ => return Err(err(StatusCode::SERVICE_UNAVAILABLE, "this control can't keep secrets")),
    };

    // A move made before, or one cut short: its hosted workspace, unless
    // it was deleted since, when this one starts afresh.
    if let Some(to) = &s.moved_to {
        match app.db.hosted_deleted(to)? {
            Some(false) if s.moved_at.is_some() => {
                let workspace = app
                    .db
                    .hosted_workspace(&project, to)?
                    .ok_or_else(|| err(StatusCode::CONFLICT, "it moved to another project's workspace"))?;
                return Ok(Json(MovedTo {
                    workspace,
                    transcripts: app.db.hosted_transcripts(&project, Some(to))?.len(),
                    secrets: app.db.hosted_secret_names(to)?.into_iter().map(|(n, _)| n).collect(),
                    project,
                    notes: m.notes,
                }));
            }
            Some(true) => app.db.drop_cloud_move(&id, to)?,
            _ => {}
        }
    }
    let hw = app.db.claim_cloud_move(account, &id, &crate::hosted::new_id())?;
    let now = now_ms();

    let mut secrets = Vec::new();
    if let Some((from, to)) = &ciphers {
        for c in &m.carry {
            let value = match &c.source {
                Source::Value(v) => Some(v.clone()),
                Source::Secret(n) => match app.db.cloud_secret(account, n)? {
                    Some((p, sealed)) => {
                        Some(crate::keys::open_cloud(&app.db.c(), from, account, n, p.as_deref(), &sealed)?)
                    }
                    None => {
                        m.notes.push(format!("{n} was deleted, so {} isn't set.", c.name));
                        None
                    }
                },
            };
            let Some(value) = value.filter(|v| !v.is_empty()) else { continue };
            if value.len() > MAX_SECRET {
                m.notes.push(format!(
                    "{} is over 16 KiB, which a hosted workspace's secret can't be: set it again.",
                    c.name
                ));
                continue;
            }
            let sealed =
                crate::keys::seal_value(&app.db.c(), to, &project, &crate::keys::hosted_name(&hw, &c.name), &value)?;
            app.db.set_hosted_secret(&hw, &c.name, Some(&sealed), now)?;
            secrets.push(c.name.clone());
        }
    }

    let tabs = app.db.cloud_tabs_ever(account, &id)?;
    let mut transcripts = 0;
    for t in &tabs {
        if copy_transcript(&app, account, &project, &hw, &w.name, t, now)? {
            transcripts += 1;
        }
    }

    if app.db.hosted_deleted(&hw)?.is_none() {
        let mut made = None;
        for n in 1..=20 {
            let name = if n == 1 { m.name.clone() } else { format!("{}-{n}", m.name) };
            match app.db.add_hosted_workspace(&hw, &project, &name, &m.spec, account, MAX_PER_PROJECT, now)? {
                Added::Made(_) => {
                    made = Some(name);
                    break;
                }
                // Or its id: the same move at once, which made it.
                Added::NameTaken if app.db.hosted_deleted(&hw)?.is_some() => {
                    made = Some(name);
                    break;
                }
                Added::NameTaken => continue,
                Added::Full => {
                    forget(&app, &id, &hw)?;
                    return Err(err(
                        StatusCode::CONFLICT,
                        &format!("a project has at most {MAX_PER_PROJECT} hosted workspaces: delete one first"),
                    ));
                }
            }
        }
        if made.is_none() {
            forget(&app, &id, &hw)?;
            return Err(err(
                StatusCode::CONFLICT,
                &format!("this project has hosted workspaces called {} already", m.name),
            ));
        }
    }
    app.db.finish_cloud_move(&id, &hw, now_ms())?;
    let workspace = app
        .db
        .hosted_workspace(&project, &hw)?
        .ok_or_else(|| err(StatusCode::CONFLICT, "its hosted workspace was deleted as it was made"))?;
    info!(%account, %project, cloud_workspace = %id, workspace = %hw, transcripts, secrets = secrets.len(), "moved a Cloud workspace off Managoat");
    app.analytics.capture(
        account,
        "cloud workspace moved",
        json!({ "repos": m.spec.repos.len(), "secrets": secrets.len(), "transcripts": transcripts }),
    );
    Ok(Json(MovedTo { workspace, project, transcripts, secrets, notes: m.notes }))
}

/// Undo what a move to `hw` kept before it found it couldn't make the
/// workspace: its secrets and transcripts, and the claim.
fn forget(app: &Platform, id: &str, hw: &str) -> anyhow::Result<()> {
    warn!(cloud_workspace = %id, workspace = %hw, "a move off Managoat made nothing");
    for (name, _) in app.db.hosted_secret_names(hw)? {
        app.db.remove_hosted_secret(hw, &name)?;
    }
    app.db.forget_moved_transcripts(hw)?;
    app.db.drop_cloud_move(id, hw)
}

/// The machine a copied tab's transcript is kept under: none that dials
/// in, and one per tab, so copying it again keeps it once.
pub fn managoat_machine(tab: &str) -> String {
    format!("managoat:{tab}")
}

/// Copy tab `t`'s transcript into `project`'s archive, under hosted
/// workspace `hw`: whether it had anything to copy.
fn copy_transcript(
    app: &Platform,
    account: &str,
    project: &str,
    hw: &str,
    workspace_name: &str,
    t: &MovedTab,
    now: u64,
) -> anyhow::Result<bool> {
    let events = app.db.cloud_events_ever(account, &t.id)?;
    let text = transcript_text(&events);
    if text.trim().is_empty() {
        return Ok(false);
    }
    let (body, cut) = fit(&text, MAX_TEXT);
    let agent = match t.spec.runtime.as_str() {
        "claude" | "codex" => t.spec.runtime.as_str(),
        _ => "agent",
    };
    let title: String = format!("{} (beta, in {workspace_name})", t.spec.agent.name)
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    let machine = managoat_machine(&t.id);
    let new = NewTranscript {
        project,
        workspace: hw,
        machine: &machine,
        block: 0,
        agent,
        title: title.trim(),
        cwd: Some(t.spec.cwd.as_str()).filter(|c| c.len() <= 4096),
        started_at: t.created,
        closed_at: t.closed.unwrap_or(now),
        cut,
        body: &body,
    };
    app.db.add_hosted_transcript(
        &crate::hosted::transcripts::new_id(),
        &new,
        crate::hosted::transcripts::MAX_PER_PROJECT,
        now,
    )?;
    Ok(true)
}

/// A tab's events as plain text, as the transcript view shows them: each
/// prompt after `> `, the agent's text and thinking with its chunks joined,
/// each tool call and its output, and each question asked.
pub fn transcript_text(events: &[Value]) -> String {
    let mut out = String::new();
    let mut last = "";
    for e in events {
        for b in e["blocks"].as_array().into_iter().flatten() {
            let kind = b["kind"].as_str().unwrap_or_default();
            let body = match &b["body"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                v => v.to_string(),
            };
            let joins = matches!(kind, "text" | "thinking" | "thought");
            if joins && kind == last {
                out.push_str(&body);
                continue;
            }
            let said = |s: &str| b[s].as_str().unwrap_or_default().to_owned();
            let piece = match kind {
                "init" | "raw" => continue,
                "prompt" => body.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"),
                "text" => body,
                "thinking" | "thought" => format!("(thinking) {body}"),
                "tool_call" | "tool_use" => {
                    let what =
                        [said("name"), said("summary")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>();
                    format!("[tool] {}", if what.is_empty() { body } else { what.join(": ") })
                }
                "tool_result" if body.trim().is_empty() || matches!(body.trim(), "{}" | "[]") => continue,
                "tool_result" => body.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"),
                "permission_request" => format!("[asked to approve] {}", said("summary")),
                "elicitation_request" => format!("[asked] {body}"),
                _ if body.trim().is_empty() => continue,
                _ => body,
            };
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&piece);
            last = kind;
        }
    }
    out
}

// ---- the download's tar

/// A ustar archive of `files`: each path at most 255 bytes, split on a `/`
/// into its prefix and name. A path that can't be is left out.
pub fn tar(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mtime = now_ms() / 1000;
    for (path, bytes) in files {
        let Some((prefix, name)) = split_path(path) else { continue };
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{:011o}\0", bytes.len()).as_bytes());
        h[136..148].copy_from_slice(format!("{mtime:011o}\0").as_bytes());
        h[148..156].copy_from_slice(b"        ");
        h[156] = b'0';
        h[257..265].copy_from_slice(b"ustar\x0000");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

fn split_path(path: &str) -> Option<(&str, &str)> {
    if path.len() <= 100 {
        return Some(("", path));
    }
    path.match_indices('/')
        .map(|(i, _)| (&path[..i], &path[i + 1..]))
        .find(|(prefix, name)| prefix.len() <= 155 && !name.is_empty() && name.len() <= 100)
}
