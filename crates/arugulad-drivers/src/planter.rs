//! planter's drivers' apply, on a deploy's box (M233, platform step 8,
//! native Arugula #31; aru-104, aru-108). The platform's half is
//! `arugula_platform::drivers::planter`; what each does, and its plan, are
//! `arugula_e2e::deploy::planter`.
//!
//! It runs as `arugulad _planter`, as the run user. The deploy is on its
//! command line, as the daemon checked it (`--driver`, `--deploy`, `--ref`,
//! `--where`, each `--setting`, ... and the approved plan's digest, which it
//! recomputes, renderer and all): nothing it reads can change it
//! ([`parse`]). Its tokens are in its environment, installation tokens of
//! planter's GitHub App minted for this run, each for one repository; it
//! revokes them itself right after its last push.
//!
//! **planter's renderer** ([`renderer`]): planter's own `render.mjs` and
//! `prod-scaffold.mjs`, vendored, written into its directory and run with
//! `node`, so an app gets what planter gave it.
//!
//! - **Go live** ([`GO_LIVE`]): apps-platform's apply (M222) as planter's
//!   flavor: the pointer pinned at the commit, a new one asking for the
//!   app's custom domains; then the app's address answers.
//! - **Ship** ([`ship`]): the artifact the daemon fetched for the run
//!   (`--artifact`), its digest checked, unpacked as planter did (regular
//!   files only, within planter's limits, a `Dockerfile` at its root);
//!   `<app>/` of `arugula-salad/shipped` replaced in one commit (`src/`,
//!   `app.yaml` for loam and its render, the app's own `k8s/kustomization.yaml`
//!   kept); the pointer written the first time. Then `shipped`'s build pins
//!   the image, and the app's loam address answers.
//! - **Promote** ([`promote`]): planter's promotion, as a pull request on
//!   `shipped` from the approved commit. A person merges it.
//! - **Prod** ([`prod`]): planter's Prod scaffold, as a pull request on the
//!   app's repository from the approved commit. A person merges it.
//!
//! **Whose app:** a shipped app is shipped over (or promoted) only when the
//! last change to its source in `shipped` is this project's (its
//! `Arugula-Project:` trailer): not planter's from before the platform, nor
//! another project's. And as M222's, a deploy whose id is in the target's
//! history already is a replay, refused.
//!
//! It ends with what it did on fd 3 (the commit it pushed, the pull request
//! it opened), which the platform checks and keeps. Exits 0 once done (and
//! for Go live and Ship, the app answers); 2 when nothing was pushed; 3 when
//! something was and then it failed.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use arugula_e2e::{
    deploy::planter::{
        self as p, APP, APP_TOKEN, GO_LIVE, HEALTH, LOAM_DOMAIN, MAX_FILES, MAX_TARBALL, MAX_UNPACKED, ORG,
        POINTERS_BRANCH, POINTERS_REPO, POINTERS_TOKEN, PORT, PORT_ENV, PROD, PROMOTE, SCAFFOLD_BRANCH, SECRETS, SHIP,
        SHIPPED_BRANCH, SHIPPED_REPO, SHIPPED_TOKEN, SOURCE_TOKEN,
    },
    drivers::{RollBack, Settings},
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::apps_platform::{
    self as ap, AUTHOR, Applied, Config, Failed, Flavor, Git, Tokens, app_yaml, collides, ready, revoke_at, say,
};

/// The trailer its commits carry, by driver: what replays and roll backs read.
fn trailer(driver: &str) -> String {
    format!("Arugula-Driver: {driver}")
}

/// The trailer naming the project a ship was for: whose app it is.
pub const PROJECT_TRAILER: &str = "Arugula-Project";

/// Go live's flavor of apps-platform's apply (M222).
pub const GO_LIVE_FLAVOR: Flavor =
    Flavor { driver: GO_LIVE, trailer: "Arugula-Driver: planter-go-live", refuses: go_live_refuses, app: p::app_of };

fn go_live_refuses(kind: &str, reference: &str, place: &str) -> Option<String> {
    p::refuses(GO_LIVE, kind, reference, place)
}

/// How long Ship waits for `shipped`'s build to pin the image.
pub const PIN_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The deploy, as the box gave it to the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deploy {
    pub driver: String,
    pub id: String,
    pub project: String,
    pub environment: String,
    pub kind: String,
    pub reference: String,
    pub place: String,
    pub plan: String,
    pub settings: Settings,
    pub roll_back: Option<String>,
    /// Ship's artifact, as the daemon fetched it for this run.
    pub artifact: Option<PathBuf>,
}

impl Deploy {
    fn setting(&self, k: &str) -> &str {
        self.settings.get(k).map_or("", String::as_str)
    }

    /// As apps-platform's apply takes it (Go live).
    fn as_ap(&self) -> ap::Deploy {
        ap::Deploy {
            id: self.id.clone(),
            environment: self.environment.clone(),
            kind: self.kind.clone(),
            reference: self.reference.clone(),
            place: self.place.clone(),
            plan: self.plan.clone(),
            roll_back: self.roll_back.clone(),
        }
    }
}

/// The deploy from the command line and the rest of the flags, refused
/// unless the plan the owner approved is this driver's plan for exactly it
/// (its settings and its roll back too). The `DEPLOY_*` variables, when
/// set, must say the same.
pub fn parse(
    args: &[String],
    var: impl Fn(&str) -> Option<String>,
    mut cfg: Config,
) -> Result<(Config, Duration, Deploy), String> {
    let mut d = Deploy {
        driver: String::new(),
        id: String::new(),
        project: String::new(),
        environment: String::new(),
        kind: String::new(),
        reference: String::new(),
        place: String::new(),
        plan: String::new(),
        settings: Settings::new(),
        roll_back: None,
        artifact: None,
    };
    let mut pin = PIN_TIMEOUT;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let Some(v) = it.next() else { return Err(format!("{flag} has no value")) };
        let ms = || v.parse::<u64>().map(Duration::from_millis).map_err(|_| format!("{flag} takes milliseconds"));
        match flag.as_str() {
            "--driver" => d.driver = v.clone(),
            "--deploy" => d.id = v.clone(),
            "--project" => d.project = v.clone(),
            "--environment" => d.environment = v.clone(),
            "--kind" => d.kind = v.clone(),
            "--ref" => d.reference = v.clone(),
            "--where" => d.place = v.clone(),
            "--plan" => d.plan = v.clone(),
            "--roll-back" => d.roll_back = Some(v.clone()),
            "--artifact" => d.artifact = Some(PathBuf::from(v)),
            "--setting" => {
                let Some((k, val)) = v.split_once('=') else { return Err("--setting takes NAME=VALUE".into()) };
                if d.settings.insert(k.to_owned(), val.to_owned()).is_some() {
                    return Err(format!("--setting {} twice", crate::work::printable(k)));
                }
            }
            "--git-base" => cfg.git_base = v.clone(),
            "--health-base" => cfg.health_base = Some(v.clone()),
            "--api-base" => cfg.api_base = v.clone(),
            "--settle-ms" => cfg.settle = ms()?,
            "--poll-ms" => cfg.poll = ms()?,
            "--timeout-ms" => cfg.timeout = ms()?,
            "--pin-timeout-ms" => pin = ms()?,
            other => return Err(format!("{} isn't a flag here", crate::work::printable(other))),
        }
    }
    if !p::is_planter(&d.driver) {
        return Err(format!("{} isn't one of planter's drivers", crate::work::printable(&d.driver)));
    }
    if !d.project.starts_with("prj_") {
        return Err("no --project on its command line".into());
    }
    for (name, v) in [
        ("DEPLOY_ID", &d.id),
        ("DEPLOY_ENVIRONMENT", &d.environment),
        ("DEPLOY_SOURCE_KIND", &d.kind),
        ("DEPLOY_SOURCE_REF", &d.reference),
        ("DEPLOY_SOURCE_WHERE", &d.place),
        ("DEPLOY_PLAN", &d.plan),
    ] {
        if v.is_empty() {
            return Err(format!("no {name} on its command line"));
        }
        if var(name).is_some_and(|e| &e != v) {
            return Err(format!("{name} in its environment isn't the deploy on its command line"));
        }
    }
    if var("DEPLOY_SETTINGS")
        .filter(|s| !s.is_empty())
        .is_some_and(|s| serde_json::from_str::<Settings>(&s).ok().as_ref() != Some(&d.settings))
    {
        return Err("DEPLOY_SETTINGS in its environment aren't the settings on its command line".into());
    }
    if arugula_e2e::drivers::settings(&d.driver, &d.settings).ok().as_ref() != Some(&d.settings) {
        return Err("its settings aren't ones its driver takes".into());
    }
    let roll_back = d.roll_back.as_ref().map(|deploy| RollBack { deploy: deploy.clone(), to: Settings::new() });
    if roll_back.is_some() && d.driver != GO_LIVE {
        return Err(format!("{} doesn't roll back", d.driver));
    }
    if roll_back.as_ref().is_some_and(|r| !arugula_e2e::drivers::roll_back_ok(&d.driver, &d.settings, r)) {
        return Err("--roll-back isn't a deploy's id".into());
    }
    let said = var("DEPLOY_ROLL_BACK").filter(|v| !v.is_empty());
    if said.map(|v| serde_json::from_str::<RollBack>(&v).ok()).is_some_and(|r| r.is_none() || r != roll_back) {
        return Err("DEPLOY_ROLL_BACK in its environment isn't the deploy on its command line".into());
    }
    let text = arugula_e2e::drivers::plan_text(
        &d.driver,
        &d.environment,
        &d.kind,
        &d.reference,
        &d.place,
        &d.settings,
        roll_back.as_ref(),
    );
    if hex::encode(Sha256::digest(text.as_bytes())) != d.plan {
        return Err("the approved plan isn't this driver's plan for this deploy".into());
    }
    if d.driver == SHIP && d.artifact.is_none() {
        return Err("no --artifact: Ship applies what the daemon fetched for the run".into());
    }
    Ok((cfg, pin, d))
}

fn refused(why: impl Into<String>) -> Failed {
    Failed { pushed: false, why: why.into() }
}

fn after_push(why: impl Into<String>) -> Failed {
    Failed { pushed: true, why: why.into() }
}

// ---- planter's renderer ----

/// The most the renderer may answer, in bytes.
const MAX_RENDER: usize = 4 << 20;

/// Ask planter's renderer (`arugula_e2e::deploy::planter::RENDERER`, written
/// into `work/planter-renderer/`) with `node` on `path`: its answer, or
/// what planter's code refused it with.
pub fn renderer(work: &Path, path: &str, req: &Value) -> Result<Value, String> {
    use std::io::Write as _;
    let dir = work.join("planter-renderer");
    for (file, text) in p::RENDERER {
        let at = dir.join(file);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("the renderer's directory: {e}"))?;
        }
        std::fs::write(&at, text).map_err(|e| format!("writing the renderer: {e}"))?;
    }
    let mut c = std::process::Command::new("node")
        .env_clear()
        .env("PATH", path)
        .env("HOME", work)
        .env("LANG", "C.UTF-8")
        .current_dir(&dir)
        .arg(dir.join("renderer.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node didn't start (the renderer needs it): {e}"))?;
    let body = req.to_string();
    if let Some(mut stdin) = c.stdin.take() {
        stdin.write_all(body.as_bytes()).map_err(|e| format!("the renderer's input: {e}"))?;
    }
    let out = c.wait_with_output().map_err(|e| format!("the renderer: {e}"))?;
    if out.stdout.len() > MAX_RENDER {
        return Err("the renderer said too much".into());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        format!("the renderer didn't answer: {}", crate::work::printable(err.lines().last().unwrap_or("")))
    })?;
    match v.get("error").and_then(Value::as_str) {
        Some(why) => Err(format!("planter's renderer refused it: {why}")),
        None if out.status.success() => Ok(v),
        None => Err("the renderer failed".into()),
    }
}

/// The files of a renderer's answer (`files`: path → text).
fn files_of(v: &Value) -> Vec<(String, String)> {
    v["files"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_owned()))).collect())
        .unwrap_or_default()
}

// ---- app.yaml as planter writes it ----

/// An `app.yaml` planter's ship wrote (`manifestText`), as its `yaml`
/// parse reads it: top-level `key: value`, flow maps and lists, and one
/// level of block mapping (`needs:`), comments aside. Anything else is
/// refused: a promotion is of what planter's ship wrote.
pub fn planter_manifest(text: &str) -> Result<Value, String> {
    fn scalar(s: &str) -> Result<Value, String> {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            let items: Vec<&str> = if inner.trim().is_empty() { vec![] } else { inner.split(',').collect() };
            return items.iter().map(|i| scalar(i)).collect::<Result<Vec<_>, _>>().map(Value::Array);
        }
        if let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            let mut m = serde_json::Map::new();
            for pair in inner.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once(':').ok_or_else(|| format!("{pair} isn't key: value"))?;
                m.insert(k.trim().to_owned(), scalar(v)?);
            }
            return Ok(Value::Object(m));
        }
        if s.is_empty() || s.contains(['[', ']', '{', '}', '"', '\'', '&', '*', '!', '|', '>']) {
            return Err(format!("{s:?} isn't a value this reads"));
        }
        Ok(match s {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" | "~" => Value::Null,
            n if n.bytes().all(|b| b.is_ascii_digit()) && !(n.len() > 1 && n.starts_with('0')) => {
                n.parse::<u64>().map(Value::from).map_err(|e| e.to_string())?
            }
            other => Value::from(other),
        })
    }
    let mut top = serde_json::Map::new();
    let mut block: Option<String> = None;
    for raw in text.lines() {
        let line = match raw.find(" #") {
            Some(i) => &raw[..i],
            None if raw.trim_start().starts_with('#') => "",
            None => raw,
        };
        if line.trim().is_empty() {
            continue;
        }
        let nested = line.starts_with("  ");
        let (k, v) = line.trim().split_once(':').ok_or_else(|| format!("{:?} isn't key: value", line.trim()))?;
        let (k, v) = (k.trim().to_owned(), v.trim());
        if nested {
            let Some(b) = block.as_ref() else { return Err(format!("{k} is indented under nothing")) };
            if v.is_empty() {
                return Err(format!("{b}.{k} nests deeper than this reads"));
            }
            let Some(Value::Object(m)) = top.get_mut(b) else { return Err(format!("{b} isn't a mapping")) };
            if m.insert(k.clone(), scalar(v)?).is_some() {
                return Err(format!("{b}.{k} twice"));
            }
        } else if v.is_empty() {
            if top.insert(k.clone(), Value::Object(serde_json::Map::new())).is_some() {
                return Err(format!("{k} twice"));
            }
            block = Some(k);
        } else {
            block = None;
            if top.insert(k.clone(), scalar(v)?).is_some() {
                return Err(format!("{k} twice"));
            }
        }
    }
    Ok(Value::Object(top))
}

// ---- GitHub's API ----

/// `method` `path` at GitHub's API with `token`: its status and its JSON.
async fn api(
    cfg: &Config,
    method: reqwest::Method,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("an HTTP client: {e}"))?;
    let mut r = client
        .request(method, format!("{}/{}", cfg.api_base.trim_end_matches('/'), path.trim_start_matches('/')))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugula-deploy");
    if let Some(b) = body {
        r = r.json(b);
    }
    let r = r.send().await.map_err(|e| format!("GitHub's API didn't answer: {}", e.without_url()))?;
    let status = r.status().as_u16();
    let bytes = r.bytes().await.map_err(|_| "GitHub's answer didn't arrive".to_owned())?;
    if bytes.len() > 1 << 20 {
        return Err("GitHub's answer is too long".into());
    }
    Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
}

/// The open pull request from `branch` into `repo`, if there's one: its URL.
async fn open_pull(cfg: &Config, repo: &str, branch: &str, token: &str) -> Result<Option<String>, String> {
    let owner = repo.split('/').next().unwrap_or_default();
    let (status, v) =
        api(cfg, reqwest::Method::GET, &format!("repos/{repo}/pulls?state=open&head={owner}:{branch}"), token, None)
            .await?;
    if status != 200 {
        return Err(format!("GitHub said {status} listing {repo}'s pull requests"));
    }
    Ok(v.as_array().and_then(|a| a.first()).and_then(|p| p["html_url"].as_str()).map(str::to_owned))
}

/// Open a pull request from `branch` into `base` of `repo`: its URL.
async fn open(
    cfg: &Config,
    repo: &str,
    branch: &str,
    base: &str,
    title: &str,
    body: &str,
    token: &str,
) -> Result<String, String> {
    let ask = json!({ "head": branch, "base": base, "title": title, "body": body });
    let (status, v) = api(cfg, reqwest::Method::POST, &format!("repos/{repo}/pulls"), token, Some(&ask)).await?;
    match (status, v["html_url"].as_str()) {
        (201, Some(url)) => Ok(url.to_owned()),
        _ => Err(format!(
            "GitHub said {status} opening the pull request: {}",
            crate::work::printable(v["message"].as_str().unwrap_or(""))
        )),
    }
}

// ---- git, the target's ----

/// Clone `repo`'s `branch` into `dir`, bounded in size.
async fn clone(cfg: &Config, git: &Git, repo: &str, branch: &str, dir: &Path) -> Result<(), Failed> {
    let _ = std::fs::remove_dir_all(dir);
    say(&format!("cloning {repo} ({branch})"));
    let d = dir.to_string_lossy().into_owned();
    git.run(
        &cfg.work,
        &[
            "clone",
            "--quiet",
            "--no-tags",
            "--single-branch",
            "-c",
            "core.symlinks=false",
            "--branch",
            branch,
            &cfg_url(cfg, repo),
            &d,
        ],
    )
    .await
    .map_err(refused)?;
    if du(dir) > cfg.max_repo_bytes {
        return Err(refused(format!("{repo} is over {} MiB", cfg.max_repo_bytes >> 20)));
    }
    Ok(())
}

fn cfg_url(cfg: &Config, repo: &str) -> String {
    format!("{}{repo}.git", cfg.git_base)
}

fn du(dir: &Path) -> u64 {
    let mut n = 0;
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => dirs.push(e.path()),
                Ok(m) => n += m.len(),
                Err(_) => {}
            }
        }
    }
    n
}

/// A replay: deploy `id` is in `dir`'s history already.
async fn replayed(git: &Git, dir: &Path, id: &str, repo: &str) -> Result<(), Failed> {
    let seen = git
        .run(dir, &["log", "--all", "--format=%H", "--fixed-strings", "--grep", &format!("Arugula-Deploy: {id}")])
        .await
        .map_err(refused)?;
    match seen.split_whitespace().next() {
        Some(c) => Err(refused(format!("{id} was applied to {repo} already, by {c}: not again"))),
        None => Ok(()),
    }
}

/// Push `HEAD` to `branch` (force: a branch of this driver's own).
async fn push(git: &Git, dir: &Path, branch: &str, force: bool) -> Result<(), String> {
    let spec = format!("{}HEAD:refs/heads/{branch}", if force { "+" } else { "" });
    git.run(dir, &["push", "--quiet", "origin", &spec]).await.map(|_| ())
}

/// Whether `app` in a clone of `shipped` is this project's to change: the
/// last commit to its source and `app.yaml` names it (`Arugula-Project:`).
/// `Ok` for an app that isn't there yet.
async fn owned(git: &Git, shipped: &Path, app: &str, project: &str) -> Result<(), Failed> {
    let last = git
        .run(shipped, &["log", "-1", "--format=%B", "--", &format!("{app}/src"), &format!("{app}/app.yaml")])
        .await
        .map_err(refused)?;
    if last.trim().is_empty() {
        return Ok(());
    }
    if last.lines().any(|l| l.trim() == format!("{PROJECT_TRAILER}: {project}")) {
        return Ok(());
    }
    let by = last
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{PROJECT_TRAILER}: ")).map(|p| format!("project {p}")))
        .or_else(|| last.lines().find_map(|l| l.strip_prefix("Shipped-By: ").map(|a| format!("planter ({a})"))))
        .unwrap_or_else(|| "someone else".into());
    Err(refused(format!(
        "{app} in {SHIPPED_REPO} was last shipped by {}, not this project: it isn't this project's to change",
        crate::work::printable(&by)
    )))
}

// ---- Ship ----

/// The files of a shipped tarball, unpacked and checked as planter's ship
/// did: `(path, bytes, executable)`, paths from its root.
pub type Unpacked = Vec<(String, Vec<u8>, bool)>;

/// Read the artifact at `at`, check it's `digest` (`sha256:<hex>`), and
/// read its files as planter's ship took them: a gzipped tar of regular
/// files only (no link, device or anything else), `.git` left out, at most
/// [`MAX_FILES`] files and [`MAX_UNPACKED`] bytes, with a `Dockerfile` at
/// its root. Nothing is extracted to disk: `gzip -dc` decompresses it,
/// read with a cap, and [`read_tar`] reads the entries with running caps.
pub async fn unpack(at: &Path, digest: &str) -> Result<Unpacked, String> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let meta = std::fs::symlink_metadata(at).map_err(|e| format!("the artifact: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_TARBALL as u64 {
        return Err(format!("the artifact isn't a file of at most {} MiB", MAX_TARBALL >> 20));
    }
    let bytes = std::fs::read(at).map_err(|e| format!("the artifact: {e}"))?;
    let got = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    if got != digest {
        return Err(format!("the artifact is {got}, not the {digest} approved"));
    }
    let mut gz = tokio::process::Command::new("gzip")
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .args(["-dc"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("gzip didn't start: {e}"))?;
    let mut stdin = gz.stdin.take().ok_or("gzip's input")?;
    let mut stdout = gz.stdout.take().ok_or("gzip's output")?;
    let feed = crate::task::spawn(async move {
        let _ = stdin.write_all(&bytes).await;
    });
    // Its entries' headers and padding on top of the files' bytes.
    let cap = MAX_UNPACKED as usize + (MAX_FILES + 64) * 3 * 512 + 2 * 512;
    let read = async {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = stdout.read(&mut buf).await.map_err(|e| format!("gzip: {e}"))?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..n]);
            if out.len() > cap {
                return Err(format!("it unpacks to over {} MiB", MAX_UNPACKED >> 20));
            }
        }
    };
    let tar = tokio::time::timeout(Duration::from_secs(60), read)
        .await
        .map_err(|_| "unpacking it took over a minute".to_owned())??;
    let _ = feed.await;
    let status = gz.wait().await.map_err(|e| format!("gzip: {e}"))?;
    if !status.success() {
        return Err("it isn't gzipped".into());
    }
    let files = read_tar(&tar)?;
    if !files.iter().any(|(p, ..)| p == "Dockerfile") {
        return Err("the app has no Dockerfile at its root: the platform builds its image from it".into());
    }
    Ok(files)
}

/// A tar stream's regular files (ustar, GNU long names, pax paths), read
/// with running caps: each path relative, with no `..` and nothing that
/// isn't printable, once; `.git` left out; directories skipped; anything
/// else (a link, a device, a FIFO) refused.
pub fn read_tar(data: &[u8]) -> Result<Unpacked, String> {
    fn cstr(b: &[u8]) -> &[u8] {
        &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
    }
    fn octal(b: &[u8]) -> Option<u64> {
        if b.first().is_some_and(|c| c & 0x80 != 0) {
            // GNU's base-256: big-endian, the top bit a flag.
            let mut n: u64 = u64::from(b[0] & 0x7f);
            for &c in &b[1..] {
                n = n.checked_mul(256)?.checked_add(u64::from(c))?;
            }
            return Some(n);
        }
        let s = std::str::from_utf8(cstr(b)).ok()?.trim_matches(|c: char| c == ' ' || c == '\0');
        if s.is_empty() {
            return Some(0);
        }
        u64::from_str_radix(s, 8).ok()
    }
    let mut files: Unpacked = Vec::new();
    let mut total = 0u64;
    let mut off = 0usize;
    let mut long: Option<String> = None;
    loop {
        let Some(h) = data.get(off..off + 512) else { return Err("it ends part way through a tar header".into()) };
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let sum: u64 =
            h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u64::from(b) }).sum();
        if octal(&h[148..156]) != Some(sum) {
            return Err("it isn't a tar (a header's checksum doesn't add up)".into());
        }
        let size = octal(&h[124..136]).ok_or("a tar header's size doesn't read")?;
        let kind = h[156];
        let start = off + 512;
        let len = usize::try_from(size).map_err(|_| "a tar entry is too big")?;
        let end = start.checked_add(len).filter(|&e| e <= data.len()).ok_or("it ends part way through a file")?;
        let body = &data[start..end];
        off = start + len.div_ceil(512) * 512;
        let mut name = String::from_utf8_lossy(cstr(&h[0..100])).into_owned();
        if &h[257..262] == b"ustar" {
            let prefix = String::from_utf8_lossy(cstr(&h[345..500])).into_owned();
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        match kind {
            // GNU's long name, for the next entry.
            b'L' => {
                if len > 4096 {
                    return Err("a tar entry's name is too long".into());
                }
                long = Some(String::from_utf8_lossy(cstr(body)).into_owned());
                continue;
            }
            // pax: its path, for the next entry; the rest ignored.
            b'x' => {
                if len > 64 * 1024 {
                    return Err("a tar entry's pax header is too long".into());
                }
                let text = String::from_utf8_lossy(body);
                for record in text.split('\n') {
                    if let Some((_, kv)) = record.split_once(' ')
                        && let Some(path) = kv.strip_prefix("path=")
                    {
                        long = Some(path.to_owned());
                    }
                }
                continue;
            }
            b'g' => continue,
            _ => {}
        }
        let name = long.take().unwrap_or(name);
        let rel = name.trim_start_matches("./").trim_end_matches('/').to_owned();
        let printable = |c: char| !c.is_control() && c != '\\';
        if rel.starts_with('/') || rel.split('/').any(|c| c == "..") || !rel.chars().all(printable) {
            return Err(format!("{} isn't a path in the app", crate::work::printable(&name)));
        }
        match kind {
            b'5' => continue,
            b'0' | 0 => {}
            _ => {
                return Err(format!(
                    "{} is not a regular file (a link or a device); ship files only",
                    crate::work::printable(&rel)
                ));
            }
        }
        if rel.is_empty() || rel == "." || rel.split('/').any(|c| c.is_empty() || c == ".") {
            return Err(format!("{} isn't a path in the app", crate::work::printable(&name)));
        }
        if rel.split('/').any(|c| c == ".git") {
            continue;
        }
        if files.len() >= MAX_FILES {
            return Err(format!("a ship carries at most {MAX_FILES} files"));
        }
        total += size;
        if total > MAX_UNPACKED {
            return Err(format!("a ship carries at most {} MiB unpacked", MAX_UNPACKED >> 20));
        }
        if files.iter().any(|(p, ..)| *p == rel) {
            return Err(format!("{} is in it twice", crate::work::printable(&rel)));
        }
        let exec = octal(&h[100..108]).is_some_and(|m| m & 0o111 != 0);
        files.push((rel, body.to_vec(), exec));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Write `bytes` at `rel` under `root`, refusing a path any existing part
/// of which is a symlink (a checkout's or a tar's could point out of it).
pub fn write_under(root: &Path, rel: &str, bytes: &[u8], exec: bool) -> Result<(), String> {
    let path = no_links(root, rel)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{rel}: {e}"))?;
    }
    // Made again since: nothing here makes links, but check once more.
    no_links(root, rel)?;
    std::fs::write(&path, bytes).map_err(|e| format!("{rel}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = if exec { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{rel}: {e}"))?;
    }
    #[cfg(not(unix))]
    let _ = exec;
    Ok(())
}

/// `root/rel`, once no existing part of it (from `root` down) is a
/// symlink, and `rel` is relative with no `..`.
pub fn no_links(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.starts_with('/') || rel.split('/').any(|c| c == ".." || c.is_empty()) {
        return Err(format!("{} isn't a path here", crate::work::printable(rel)));
    }
    let mut at = root.to_path_buf();
    for part in rel.split('/') {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("{} goes through a symlink", crate::work::printable(rel)));
            }
            _ => {}
        }
    }
    Ok(at)
}

/// Read `rel` under `root` (at most `max` bytes), refusing a symlink on the
/// way: `None` when it isn't there.
pub fn read_under(root: &Path, rel: &str, max: u64) -> Result<Option<String>, String> {
    let path = no_links(root, rel)?;
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && m.len() <= max => {
            std::fs::read_to_string(&path).map(Some).map_err(|e| format!("{rel}: {e}"))
        }
        Ok(m) if m.is_file() => Err(format!("{rel} is over {} KiB", max >> 10)),
        Ok(_) => Err(format!("{rel} isn't a file")),
        Err(_) => Ok(None),
    }
}

/// Whether `ORG/name` is a repository on GitHub, asked with no token (as
/// anyone sees it, M233's review): `Err` when GitHub says so, or doesn't
/// answer.
pub async fn not_a_repo(cfg: &Config, name: &str) -> Result<(), String> {
    let repo = format!("{ORG}/{name}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("an HTTP client: {e}"))?;
    let r = client
        .get(format!("{}/repos/{repo}", cfg.api_base.trim_end_matches('/')))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "arugula-deploy")
        .send()
        .await
        .map(|r| r.status().as_u16());
    match r {
        Ok(404) => Ok(()),
        Ok(200 | 301) => Err(format!("{repo} is a repository, whose app and image the name {name} belongs to")),
        Ok(n) => Err(format!("couldn't ask GitHub whether {repo} is a repository (it said {n})")),
        Err(_) => Err(format!("couldn't ask GitHub whether {repo} is a repository")),
    }
}

/// Whether the name `app` was only ever this project's in `shipped` and the
/// pointers' history: a ship whose secrets would load `/<app>` from
/// Infisical gets them only when nobody else ever shipped or pointed at
/// that name (the folder might hold another app's).
async fn never_anyone_elses(
    git: &Git,
    shipped: &Path,
    pointers: &Path,
    app: &str,
    project: &str,
) -> Result<(), Failed> {
    let mine = format!("{PROJECT_TRAILER}: {project}");
    for (dir, paths) in
        [(shipped, vec![format!("{app}/src"), format!("{app}/app.yaml")]), (pointers, vec![ap_pointer_path(app)])]
    {
        let mut args = vec!["log".to_owned(), "--format=%B%x1e".to_owned(), "--".to_owned()];
        args.extend(paths);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let log = git.run(dir, &args).await.map_err(refused)?;
        for body in log.split('\u{1e}').map(str::trim).filter(|b| !b.is_empty()) {
            if !body.lines().any(|l| l.trim() == mine) {
                return Err(refused(format!(
                    "{app} was shipped or pointed at before, not by this project: its Infisical folder (/{app}) may \
                     hold another app's secrets, so a ship under it can't ask for secrets"
                )));
            }
        }
    }
    Ok(())
}

/// planter's pointer for a shipped app (`prod.mjs`'s `pointer()` with
/// `shipped`'s directory), written by this driver: `branch: main`, as
/// planter's, since `shipped` changes only by approved ships and its build.
pub fn shipped_pointer(app: &str) -> String {
    format!(
        "# {app} in prod, at https://{app}.{LOAM_DOMAIN}. Written by Arugula's planter-ship driver; Flux applies\n\
         # github.com/{SHIPPED_REPO}'s {app}/k8s/ as planter-apps-deployer, into namespace planter-apps.\n\
         # Deleting this file takes the app out of prod, and prunes what it made (its volume's data too).\n\
         apiVersion: source.toolkit.fluxcd.io/v1\n\
         kind: GitRepository\n\
         metadata:\n\
         \x20 name: {app}\n\
         \x20 namespace: planter-apps-system\n\
         spec:\n\
         \x20 interval: 5m\n\
         \x20 url: https://github.com/{SHIPPED_REPO}\n\
         \x20 provider: github\n\
         \x20 secretRef:\n\
         \x20   name: arugula-github-app\n\
         \x20 ref:\n\
         \x20   branch: {SHIPPED_BRANCH}\n\
         ---\n\
         apiVersion: kustomize.toolkit.fluxcd.io/v1\n\
         kind: Kustomization\n\
         metadata:\n\
         \x20 name: {app}\n\
         \x20 namespace: planter-apps-system\n\
         spec:\n\
         \x20 interval: 10m\n\
         \x20 retryInterval: 2m\n\
         \x20 timeout: 5m\n\
         \x20 path: ./{app}/k8s\n\
         \x20 prune: true\n\
         \x20 wait: true\n\
         \x20 serviceAccountName: planter-apps-deployer\n\
         \x20 targetNamespace: planter-apps\n\
         \x20 sourceRef:\n\
         \x20   kind: GitRepository\n\
         \x20   name: {app}\n"
    )
}

/// What Ship pushed: `shipped`'s commit (`None`: the source was there
/// already), and the pointer's, if it wrote one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipped {
    pub commit: Option<String>,
    pub pointer: Option<String>,
}

/// Ship: unpack, commit `<app>/` to `shipped`, point the list at it the
/// first time. Not the wait: [`ship`] does that after the tokens are gone.
pub async fn ship_commit(cfg: &Config, d: &Deploy, t: &Tokens, shipped_token: Option<&str>) -> Result<Shipped, Failed> {
    if let Some(why) = p::refuses(SHIP, &d.kind, &d.reference, &d.place) {
        return Err(refused(why));
    }
    let app = d.setting(APP).to_owned();
    let at = d.artifact.as_deref().ok_or_else(|| refused("no artifact"))?;
    say(&format!("unpacking {} (the source shipped)", d.reference));
    let files = unpack(at, &d.reference).await.map_err(refused)?;
    say(&format!("{} files, a Dockerfile at its root", files.len()));
    let secrets: Vec<&str> = match d.settings.get(SECRETS) {
        Some(s) => s.split(',').collect(),
        None => vec![],
    };
    let port: u64 = d.setting(PORT).parse().map_err(|_| refused("its port setting"))?;
    let rendered = renderer(
        &cfg.work,
        &cfg.path,
        &json!({ "op": "ship", "name": app, "port": port, "health": d.setting(HEALTH),
                 "portEnv": d.setting(PORT_ENV), "secrets": secrets }),
    )
    .map_err(refused)?;
    let app_yaml_text = rendered["appYaml"].as_str().ok_or_else(|| refused("the renderer wrote no app.yaml"))?;
    let top = rendered["top"].as_str().ok_or_else(|| refused("the renderer wrote no kustomization"))?.to_owned();
    let platform = files_of(&rendered);

    let tokens = [(cfg_url(cfg, SHIPPED_REPO), shipped_token), (cfg_url(cfg, POINTERS_REPO), t.write.as_deref())];
    let git = Git::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &tokens);
    let (shipped, pointers) = (cfg.work.join("shipped"), cfg.work.join("pointers"));
    std::fs::create_dir_all(&cfg.work).map_err(|e| refused(format!("its directory: {e}")))?;
    clone(cfg, &git, POINTERS_REPO, POINTERS_BRANCH, &pointers).await?;
    clone(cfg, &git, SHIPPED_REPO, SHIPPED_BRANCH, &shipped).await?;
    replayed(&git, &shipped, &d.id, SHIPPED_REPO).await?;
    replayed(&git, &pointers, &d.id, POINTERS_REPO).await?;
    // The name: free, or this shipped app's own pointer already.
    let ppath = ap_pointer_path(&app);
    let existing = read_under(&pointers, &ppath, cfg.max_file_bytes).map_err(refused)?;
    if existing.as_deref().is_some_and(|t| !t.contains(&format!("url: https://github.com/{SHIPPED_REPO}\n"))) {
        return Err(refused(format!(
            "{app} is an app of its own repository already ({POINTERS_REPO}'s {ppath}): ship under another name"
        )));
    }
    if let Some(why) = collides(&pointers, &app, &ppath) {
        return Err(refused(why));
    }
    if !secrets.is_empty() {
        never_anyone_elses(&git, &shipped, &pointers, &app, &d.project).await?;
    }
    // Nobody's repository, as GitHub says now (the platform asked when it
    // was planned).
    not_a_repo(cfg, &app).await.map_err(refused)?;

    let mut commit = None;
    for attempt in 1..=3 {
        if let Some(text) = read_under(&shipped, &format!("{app}/app.yaml"), cfg.max_file_bytes).map_err(refused)? {
            let runtime = app_yaml(&text).runtime;
            if runtime != "loam" {
                return Err(refused(format!(
                    "{app} has been promoted to the cluster (its app.yaml says runtime {}): it changes only through a \
                     pull request on {SHIPPED_REPO} now",
                    crate::work::printable(&runtime)
                )));
            }
        }
        owned(&git, &shipped, &app, &d.project).await?;
        // The app's own kustomization (its build pins the image there) stays.
        let kust = read_under(&shipped, &format!("{app}/k8s/kustomization.yaml"), cfg.max_file_bytes)
            .map_err(refused)?
            .unwrap_or_else(|| top.clone());
        git.run(&shipped, &["rm", "-r", "-q", "--ignore-unmatch", "--", &app]).await.map_err(refused)?;
        let _ = std::fs::remove_dir_all(shipped.join(&app));
        let write = |rel: &str, bytes: &[u8], exec: bool| -> Result<(), Failed> {
            write_under(&shipped, &format!("{app}/{rel}"), bytes, exec).map_err(refused)
        };
        for (rel, bytes, exec) in &files {
            write(&format!("src/{rel}"), bytes, *exec)?;
        }
        write("app.yaml", app_yaml_text.as_bytes(), false)?;
        write("k8s/kustomization.yaml", kust.as_bytes(), false)?;
        for (rel, text) in &platform {
            write(rel, text.as_bytes(), false)?;
        }
        git.run(&shipped, &["add", "-A", "--", &app]).await.map_err(refused)?;
        if git.run(&shipped, &["diff", "--cached", "--quiet"]).await.is_ok() {
            say(&format!("{SHIPPED_REPO}'s {app}/ is this source already: nothing to commit"));
            break;
        }
        let msg = format!(
            "{app}: ship to loam, at https://{app}.{LOAM_DOMAIN}\n\nShipped through Arugula's planter-ship driver, \
             approved on an owner's device: the deploy {} to the environment {}.\n\nArugula-Deploy: {}\n\
             {PROJECT_TRAILER}: {}\n{}\n",
            d.id,
            d.environment,
            d.id,
            d.project,
            trailer(SHIP)
        );
        git.run(&shipped, &["commit", "--quiet", "-m", &msg]).await.map_err(refused)?;
        let changed = git.run(&shipped, &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).await;
        if !changed.as_deref().is_ok_and(|c| c.lines().all(|l| l.starts_with(&format!("{app}/")))) {
            return Err(refused(format!("the commit changes more than {app}/")));
        }
        let head = git.run(&shipped, &["rev-parse", "HEAD"]).await.map_err(refused)?.trim().to_owned();
        match push(&git, &shipped, SHIPPED_BRANCH, false).await {
            Ok(()) => {
                say(&format!("pushed {head} to {SHIPPED_REPO}: its build makes the image"));
                commit = Some(head);
                break;
            }
            Err(e) if attempt < 3 => {
                say(&format!("the push didn't go ({e}); again from {SHIPPED_REPO}'s head"));
                git.run(&shipped, &["fetch", "--quiet", "origin", SHIPPED_BRANCH]).await.map_err(refused)?;
                git.run(&shipped, &["reset", "--quiet", "--hard", "FETCH_HEAD"]).await.map_err(refused)?;
            }
            Err(e) => return Err(refused(e)),
        }
    }
    let pushed = commit.is_some();
    let fail = |why: String| if pushed { after_push(why) } else { refused(why) };
    let mut pointer = None;
    if existing.is_none() {
        for attempt in 1..=3 {
            write_under(&pointers, &ppath, shipped_pointer(&app).as_bytes(), false).map_err(fail)?;
            git.run(&pointers, &["add", "--", &ppath]).await.map_err(fail)?;
            let msg = format!(
                "{app}: shipped to loam at https://{app}.{LOAM_DOMAIN}\n\nFrom {SHIPPED_REPO}/{app}, through Arugula's \
                 planter-ship driver.\n\nArugula-Deploy: {}\n{PROJECT_TRAILER}: {}\n{}\n",
                d.id,
                d.project,
                trailer(SHIP)
            );
            git.run(&pointers, &["commit", "--quiet", "-m", &msg]).await.map_err(fail)?;
            let head = git.run(&pointers, &["rev-parse", "HEAD"]).await.map_err(fail)?.trim().to_owned();
            match push(&git, &pointers, POINTERS_BRANCH, false).await {
                Ok(()) => {
                    say(&format!("pointed {POINTERS_REPO}'s {ppath} at {SHIPPED_REPO}'s {app}/k8s"));
                    pointer = Some(head);
                    break;
                }
                Err(e) if attempt < 3 => {
                    say(&format!("the push didn't go ({e}); again from {POINTERS_REPO}'s head"));
                    git.run(&pointers, &["fetch", "--quiet", "origin", POINTERS_BRANCH]).await.map_err(fail)?;
                    git.run(&pointers, &["reset", "--quiet", "--hard", "FETCH_HEAD"]).await.map_err(fail)?;
                    if std::fs::metadata(pointers.join(&ppath)).is_ok() {
                        return Err(fail(format!("{ppath} appeared while this pushed: ship again")));
                    }
                }
                Err(e) => return Err(fail(e)),
            }
        }
    }
    Ok(Shipped { commit, pointer })
}

fn ap_pointer_path(app: &str) -> String {
    arugula_e2e::deploy::apps_platform::pointer_path(app)
}

/// Wait for `shipped`'s build to pin `app`'s image after `after` (a commit
/// of `shipped`): read without a token (the repository is public), each
/// `poll`, for at most `timeout`. The image, by digest.
pub async fn pinned(cfg: &Config, app: &str, after: &str, timeout: Duration) -> Result<String, String> {
    let git = Git::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &[]);
    let dir = cfg.work.join("shipped");
    let start = Instant::now();
    let kust = format!("{app}/k8s/kustomization.yaml");
    say(&format!("waiting for {SHIPPED_REPO}'s build to pin {app}'s image"));
    loop {
        let fetched = git.run(&dir, &["fetch", "--quiet", &cfg_url(cfg, SHIPPED_REPO), SHIPPED_BRANCH]).await;
        if fetched.is_ok() {
            let since = git
                .run(&dir, &["log", "--format=%H", &format!("{after}..FETCH_HEAD"), "--", &kust])
                .await
                .unwrap_or_default();
            if !since.trim().is_empty()
                && let Ok(text) = git.run(&dir, &["show", &format!("FETCH_HEAD:{kust}")]).await
                && let Some(digest) = text.lines().find_map(|l| {
                    l.trim()
                        .strip_prefix("digest:")
                        .map(|d| d.split(" #").next().unwrap_or_default().trim().trim_matches(['"', '\'']).to_owned())
                })
                && arugula_e2e::deploy::planter::artifact_ok(&digest)
            {
                let image = format!("ghcr.io/{ORG}/{app}@{digest}");
                say(&format!("pinned: {image}"));
                return Ok(image);
            }
        }
        if start.elapsed() >= timeout {
            return Err(format!("{SHIPPED_REPO}'s build didn't pin {app}'s image in {}s", timeout.as_secs()));
        }
        tokio::time::sleep(cfg.poll).await;
    }
}

/// The whole Ship: commit, revoke both tokens, wait for the pin, then for
/// the app's loam address to answer. What it reports.
pub async fn ship(
    cfg: &Config,
    pin_timeout: Duration,
    d: &Deploy,
    t: &Tokens,
    shipped_token: Option<&str>,
) -> Result<Value, Failed> {
    let done = ship_commit(cfg, d, t, shipped_token).await;
    revoke_at(SHIP, &cfg.api_base, "shipped's write", shipped_token).await;
    revoke_at(SHIP, &cfg.api_base, "list's write", t.write.as_deref()).await;
    let done = done?;
    let app = d.setting(APP);
    let mut image = None;
    if let Some(c) = &done.commit {
        image = Some(pinned(cfg, app, c, pin_timeout).await.map_err(after_push)?);
    }
    let base = cfg.health_base.clone().unwrap_or_else(|| format!("https://{app}.{LOAM_DOMAIN}"));
    let url = format!("{}{}", base.trim_end_matches('/'), d.setting(HEALTH));
    let pushed = done.commit.is_some() || done.pointer.is_some();
    ready(cfg, &url, &[]).await.map_err(|why| Failed { pushed, why })?;
    Ok(json!({ "commit": done.commit, "pointer": done.pointer, "image": image }))
}

// ---- Promote ----

/// Promote: planter's promotion of `app` at the approved commit of
/// `shipped`, as a pull request a person merges. Its URL, and the commit.
pub async fn promote(cfg: &Config, d: &Deploy, token: Option<&str>) -> Result<Value, Failed> {
    let done = promote_pull(cfg, d, token).await;
    revoke_at(PROMOTE, &cfg.api_base, "shipped's write", token).await;
    done
}

async fn promote_pull(cfg: &Config, d: &Deploy, token: Option<&str>) -> Result<Value, Failed> {
    if let Some(why) = p::refuses(PROMOTE, &d.kind, &d.reference, &d.place) {
        return Err(refused(why));
    }
    let token = token.ok_or_else(|| refused(format!("no {SHIPPED_TOKEN}")))?;
    let app = d.setting(APP).to_owned();
    let sha = d.reference.as_str();
    let branch = p::promote_branch(&app);
    if let Some(url) = open_pull(cfg, SHIPPED_REPO, &branch, token).await.map_err(refused)? {
        say(&format!("its pull request is open already: {url}"));
        return Ok(json!({ "pull": url }));
    }
    let git = Git::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &[(cfg_url(cfg, SHIPPED_REPO), Some(token))]);
    let shipped = cfg.work.join("shipped");
    std::fs::create_dir_all(&cfg.work).map_err(|e| refused(format!("its directory: {e}")))?;
    clone(cfg, &git, SHIPPED_REPO, SHIPPED_BRANCH, &shipped).await?;
    replayed(&git, &shipped, &d.id, SHIPPED_REPO).await?;
    if git.run(&shipped, &["merge-base", "--is-ancestor", sha, "HEAD"]).await.is_err() {
        return Err(refused(format!("{sha} isn't on {SHIPPED_REPO}'s {SHIPPED_BRANCH}")));
    }
    git.run(&shipped, &["checkout", "--quiet", "-B", &branch, sha]).await.map_err(refused)?;
    let text = read_under(&shipped, &format!("{app}/app.yaml"), cfg.max_file_bytes)
        .map_err(refused)?
        .ok_or_else(|| refused(format!("{app} hasn't been shipped (no {app}/app.yaml at {sha})")))?;
    let manifest = planter_manifest(&text)
        .map_err(|why| refused(format!("{app}/app.yaml isn't one planter's ship wrote ({why}): promote it by hand")))?;
    if manifest["runtime"] != "loam" {
        return Err(refused(format!("{app} is already runtime {}", manifest["runtime"])));
    }
    owned(&git, &shipped, &app, &d.project).await?;
    let old = renderer(&cfg.work, &cfg.path, &json!({ "op": "render", "manifest": manifest, "repo": app }))
        .map_err(refused)?["spec"]
        .clone();
    let rendered = renderer(
        &cfg.work,
        &cfg.path,
        &json!({ "op": "promote", "name": app, "old": { "port": old["port"], "health": old["health"],
                 "portEnv": old["portEnv"], "secrets": old["secrets"], "secretsOn": old["secretsOn"] } }),
    )
    .map_err(refused)?;
    let yaml = rendered["appYaml"].as_str().ok_or_else(|| refused("the renderer wrote no app.yaml"))?;
    write_under(&shipped, &format!("{app}/app.yaml"), yaml.as_bytes(), false).map_err(refused)?;
    for (rel, text) in files_of(&rendered) {
        write_under(&shipped, &format!("{app}/{rel}"), text.as_bytes(), false).map_err(refused)?;
    }
    git.run(&shipped, &["add", "-A", "--", &app]).await.map_err(refused)?;
    let host = format!("{app}.{}", p::SECOND_DOMAIN);
    let msg = format!(
        "{app}: promote from loam into the cluster, at https://{host}\n\nOpened through Arugula's planter-promote \
         driver, approved on an owner's device; a person merges it.\n\nArugula-Deploy: {}\n{PROJECT_TRAILER}: {}\n{}\n",
        d.id,
        d.project,
        trailer(PROMOTE)
    );
    git.run(&shipped, &["commit", "--quiet", "-m", &msg]).await.map_err(refused)?;
    let head = git.run(&shipped, &["rev-parse", "HEAD"]).await.map_err(refused)?.trim().to_owned();
    push(&git, &shipped, &branch, true).await.map_err(refused)?;
    let body = [
        format!(
            "Moves **{app}** from loam (https://{app}.{LOAM_DOMAIN}) into home-cloud's `planter-apps` namespace, at \
             https://{host}: app.yaml's `runtime` becomes `k8s`, and `k8s/platform/` is rendered again from it."
        ),
        String::new(),
        "Merging this is the review: from then on the app runs in the cluster, where an escape reaches an etcd \
         member, and it changes only through pull requests. Before merging, check that:"
            .into(),
        format!("- the code in `{app}/src/` is code you'd run in the cluster;"),
        "- its Dockerfile runs as a numeric, non-root USER (the pod is runAsNonRoot with a read-only root filesystem: \
         it may write only /tmp);"
            .into(),
        format!(
            "- it answers 200 on `{}` on port {}.",
            old["health"].as_str().unwrap_or("/health"),
            old["port"].as_u64().unwrap_or(8080)
        ),
        String::new(),
        "After the merge the build renders and pins the image, Flux runs two replicas, and home-cloud's loam \
         provisioner deletes the sandbox."
            .into(),
        String::new(),
        format!("Opened by Arugula's planter-promote driver for the deploy {}, approved on an owner's device.", d.id),
    ]
    .join("\n");
    let title = format!("Promote {app} from loam to the cluster");
    let url = open(cfg, SHIPPED_REPO, &branch, SHIPPED_BRANCH, &title, &body, token).await.map_err(after_push)?;
    say(&format!("opened {url}: a person merging it moves {app}"));
    Ok(json!({ "pull": url, "commit": head }))
}

// ---- Prod ----

/// Prod: planter's scaffold for the app's repository at the approved
/// commit, as a pull request a person merges.
pub async fn prod(cfg: &Config, d: &Deploy, token: Option<&str>) -> Result<Value, Failed> {
    let done = prod_pull(cfg, d, token).await;
    revoke_at(PROD, &cfg.api_base, "app repository's write", token).await;
    done
}

async fn prod_pull(cfg: &Config, d: &Deploy, token: Option<&str>) -> Result<Value, Failed> {
    if let Some(why) = p::refuses(PROD, &d.kind, &d.reference, &d.place) {
        return Err(refused(why));
    }
    let token = token.ok_or_else(|| refused(format!("no {APP_TOKEN}")))?;
    let name = p::app_of(&d.place).unwrap_or_default().to_owned();
    let sha = d.reference.as_str();
    if let Some(url) = open_pull(cfg, &d.place, SCAFFOLD_BRANCH, token).await.map_err(refused)? {
        say(&format!("its pull request is open already: {url}"));
        return Ok(json!({ "pull": url }));
    }
    let git = Git::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &[(cfg_url(cfg, &d.place), Some(token))]);
    let repo = cfg.work.join("app");
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).map_err(|e| refused(format!("its directory: {e}")))?;
    git.run(&repo, &["init", "--quiet"]).await.map_err(refused)?;
    // A link in the app's repository is checked out as a file, never
    // followed.
    git.run(&repo, &["config", "core.symlinks", "false"]).await.map_err(refused)?;
    git.run(&repo, &["remote", "add", "origin", &cfg_url(cfg, &d.place)]).await.map_err(refused)?;
    let head = git.run(&repo, &["ls-remote", "--symref", "origin", "HEAD"]).await.map_err(refused)?;
    let base = head
        .lines()
        .find_map(|l| l.strip_prefix("ref: refs/heads/").and_then(|r| r.split_whitespace().next()))
        .map(str::to_owned)
        .ok_or_else(|| refused(format!("{} has no default branch", d.place)))?;
    say(&format!("reading {} ({base})", d.place));
    git.run(
        &repo,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "--filter=blob:none",
            "origin",
            &format!("+refs/heads/{base}:refs/remotes/origin/{base}"),
        ],
    )
    .await
    .map_err(refused)?;
    if git.run(&repo, &["merge-base", "--is-ancestor", sha, &format!("refs/remotes/origin/{base}")]).await.is_err() {
        return Err(refused(format!("{sha} isn't on {}'s default branch, {base}", d.place)));
    }
    git.run(&repo, &["checkout", "--quiet", "-B", SCAFFOLD_BRANCH, sha]).await.map_err(refused)?;
    if du(&repo) > cfg.max_repo_bytes {
        return Err(refused(format!("{} is over {} MiB", d.place, cfg.max_repo_bytes >> 20)));
    }
    if std::fs::symlink_metadata(repo.join("k8s/kustomization.yaml")).is_ok() {
        return Err(refused(format!("{} has a k8s/ at {sha} already: plan a {GO_LIVE} deploy", d.place)));
    }
    let dir = d.setting(p::APP_DIR).trim_end_matches('/').to_owned();
    let read = |rel: &str| read_under(&repo, rel, cfg.max_file_bytes).map_err(refused);
    let package = read(&format!("{dir}/package.json"))?
        .ok_or_else(|| refused(format!("{dir}/package.json is missing: prod needs the app member to be a Node app")))?;
    let lockfile = no_links(&repo, &format!("{dir}/package-lock.json"))
        .map_err(refused)?
        .symlink_metadata()
        .is_ok_and(|m| m.is_file());
    let mut options = serde_json::Map::new();
    for (k, key) in [(p::HEALTH, "health"), (p::PORT_ENV, "portEnv"), (p::DATA_ENV, "dataEnv"), (p::METRICS, "metrics")]
    {
        if let Some(v) = d.settings.get(k) {
            options.insert(key.into(), v.clone().into());
        }
    }
    for (k, key) in [(p::SECRETS, "secrets"), (p::POSTGRES, "postgres")] {
        if d.settings.contains_key(k) {
            options.insert(key.into(), true.into());
        }
    }
    match d.settings.get(p::VOLUME).map(String::as_str) {
        Some("none") => drop(options.insert("volume".into(), Value::Null)),
        Some(v) => drop(options.insert("volume".into(), v.into())),
        None => {}
    }
    let rendered = renderer(
        &cfg.work,
        &cfg.path,
        &json!({ "op": "scaffold", "name": name, "app": { "dir": dir, "fields": {} }, "packageJson": package,
                 "lockfile": lockfile, "options": options }),
    )
    .map_err(refused)?;
    let files = files_of(&rendered);
    for (rel, text) in &files {
        write_under(&repo, rel, text.as_bytes(), false).map_err(refused)?;
        git.run(&repo, &["add", "--", rel]).await.map_err(refused)?;
    }
    let url = format!("https://{name}.{}", p::APPS_DOMAIN);
    let msg = format!(
        "prod: build {name}'s image and deploy it to {url}\n\nThe Dockerfile, the build-and-pin workflow and a starter \
         k8s/, from planter's scaffold, through Arugula's planter-prod driver.\n\nArugula-Deploy: {}\n\
         {PROJECT_TRAILER}: {}\n{}\n",
        d.id,
        d.project,
        trailer(PROD)
    );
    git.run(&repo, &["commit", "--quiet", "-m", &msg]).await.map_err(refused)?;
    let commit = git.run(&repo, &["rev-parse", "HEAD"]).await.map_err(refused)?.trim().to_owned();
    push(&git, &repo, SCAFFOLD_BRANCH, true).await.map_err(refused)?;
    let body = [
        format!("Gives {} what it needs to run in prod at {url}, on home-cloud's `planter-apps` namespace.", d.place),
        String::new(),
        "- `Dockerfile`: the app member as an image.".into(),
        format!(
            "- `.github/workflows/build.yml`: on a merge to {base} that touches the app, test it, build \
             `ghcr.io/{ORG}/{name}` (amd64 and arm64), and commit the image pin into `k8s/kustomization.yaml`. That \
             pin commit is the deploy."
        ),
        format!(
            "- `k8s/`: a Deployment, Service, IngressRoute and Certificate for {url}, and a Grafana dashboard of its \
             traffic, resources, logs and traces. The repo owns it from here on."
        ),
        String::new(),
        format!("Merging this starts the first build. When it has pinned an image, deploy it with a `{GO_LIVE}` environment."),
        String::new(),
        format!("Opened by Arugula's planter-prod driver for the deploy {}, approved on an owner's device.", d.id),
    ]
    .join("\n");
    let pull = open(cfg, &d.place, SCAFFOLD_BRANCH, &base, &format!("Deploy {name} to prod"), &body, token)
        .await
        .map_err(after_push)?;
    say(&format!("opened {pull}: a person merges it"));
    Ok(json!({ "pull": pull, "commit": commit }))
}

// ---- the command line ----

/// Its result, on fd 3 if it was given one (the daemon's result pipe),
/// close-on-exec from the start so nothing it runs inherits it. Nothing
/// elsewhere: a deploy's box is Linux.
pub struct ResultFd(Option<std::fs::File>);

impl ResultFd {
    #[cfg(unix)]
    pub fn take() -> Self {
        use std::os::fd::FromRawFd as _;
        // SAFETY: fd 3 is only borrowed here, to set its flag; it's checked
        // to be open first by that call succeeding.
        let open = unsafe { std::os::fd::BorrowedFd::borrow_raw(3) };
        let ok = nix::fcntl::fcntl(open, nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC)).is_ok();
        // SAFETY: fd 3 is open (fcntl took it), and nothing else here owns it.
        ResultFd(ok.then(|| unsafe { std::fs::File::from_raw_fd(3) }))
    }

    #[cfg(not(unix))]
    pub fn take() -> Self {
        ResultFd(None)
    }

    pub fn say(self, v: &Value) {
        if let Some(mut f) = self.0 {
            use std::io::Write as _;
            let _ = f.write_all(v.to_string().as_bytes());
        }
    }
}

/// `arugulad _planter --driver NAME --deploy ID --project PRJ --environment
/// NAME --kind KIND --ref REF --where WHERE --plan DIGEST [--setting K=V]...
/// [--roll-back DEP] [--artifact PATH] [--git-base URL] [--api-base URL]
/// [--health-base URL] [--settle-ms N] [--poll-ms N] [--timeout-ms N]
/// [--pin-timeout-ms N]`, from the driver's shell: the deploy as the daemon
/// checked it, and the box's own flags for tests and testnet.
pub fn cli(args: &[String]) -> ! {
    let result = ResultFd::take();
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let work = var("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from).join("planter");
    let mut cfg = Config::new(work.clone());
    if let Some(path) = var("PATH") {
        cfg.path = path;
    }
    let (cfg, pin, d) = match parse(args, var, cfg) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("planter: {}", crate::work::printable(&why));
            std::process::exit(2)
        }
    };
    let label: &'static str = match d.driver.as_str() {
        PROD => PROD,
        GO_LIVE => GO_LIVE,
        SHIP => SHIP,
        _ => PROMOTE,
    };
    ap::set_label(label);
    let code = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => {
            let done = rt.block_on(async {
                match d.driver.as_str() {
                    GO_LIVE => {
                        let t = Tokens { write: var(POINTERS_TOKEN), read: var(SOURCE_TOKEN) };
                        ap::apply_as(&cfg, &GO_LIVE_FLAVOR, &d.as_ap(), &t, Some(&p::domains(&d.settings))).await.map(
                            |a| match a {
                                Applied::Pinned(c) | Applied::Reverted { commit: c, .. } => json!({ "commit": c }),
                                Applied::Unchanged => json!({}),
                            },
                        )
                    }
                    SHIP => {
                        let t = Tokens { write: var(POINTERS_TOKEN), read: None };
                        ship(&cfg, pin, &d, &t, var(SHIPPED_TOKEN).as_deref()).await
                    }
                    PROMOTE => promote(&cfg, &d, var(SHIPPED_TOKEN).as_deref()).await,
                    _ => prod(&cfg, &d, var(APP_TOKEN).as_deref()).await,
                }
            });
            match done {
                Ok(v) => {
                    result.say(&v);
                    0
                }
                Err(f) => {
                    eprintln!("{label}: {}", crate::work::printable(&f.why));
                    if f.pushed { 3 } else { 2 }
                }
            }
        }
        Err(e) => {
            eprintln!("planter: {e}");
            2
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    std::process::exit(code)
}

#[cfg(all(test, unix))]
#[path = "planter_tests.rs"]
mod tests;
