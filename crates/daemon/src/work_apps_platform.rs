//! The apps-platform driver's apply, on a deploy's box (M222, platform
//! step 7, native Arugula #18; aru-104). The platform's half is
//! `arugula_platform::drivers::apps_platform`; the plan both sides make is
//! `arugula_e2e::deploy::apps_platform`.
//!
//! It runs as `arugulad _apps-platform`, as the run user. The deploy it
//! applies is on its command line, as the daemon checked it (`--deploy`,
//! `--ref`, `--where`, ... and the approved plan's digest, which it
//! recomputes): the driver's shell builds that from the daemon's words,
//! and nothing it reads can change it ([`parse`]). Its tokens are in its
//! environment (read there from its standard input by the driver's shell,
//! never a file or an argument).
//!
//! 1. **Clones the list** ([`LIST_REPO`]) over HTTPS. The write token goes
//!    to git as an `Authorization` header in git's environment
//!    (`GIT_CONFIG_*`), for the list's URL only: never on a command line,
//!    in `.git/config` or in a credential store. A deploy whose id is in
//!    the list's history already (its `Arugula-Deploy:` trailer) is a
//!    replay: refused. So is a new app whose name another of the list's
//!    objects holds, or overlaps by a dash-prefix.
//! 2. **Reads the app's `app.yaml`** at the approved commit, with the read
//!    token, for the URL only: its name must be the pointer's, it must run
//!    in the cluster, and its health path is what step 5 asks. The commit
//!    must be on the app repository's default branch (GitHub serves a
//!    fork's commits by sha too); the history it fetches for that is
//!    commits only, and what it reads is bounded in size.
//! 3. **Pins the pointer** (`apps/<name>.yaml`): its `GitRepository`'s
//!    `ref` becomes `commit: <sha>`, and nothing else in it changes (custom
//!    domains, path and the rest stay). With no pointer, it writes one, as
//!    planter does, pinned. One file changes, or none if it's pinned there
//!    already.
//! 4. **Commits and pushes** to the list's branch. When the last commit to
//!    the pointer is this driver's and its parent pinned this commit, the
//!    deploy is a roll back: it commits `git revert` of that commit. A push
//!    that loses a race starts again from the list's new head, three times.
//!    Right after, it revokes both tokens at GitHub itself
//!    (`DELETE /installation/token`): the wait that follows holds none.
//! 5. **Waits for the app to be ready**: Flux applies the list (and the
//!    app's `k8s/` at the commit) inside the cluster, which nothing outside
//!    can read, so ready is the app's public address
//!    (`https://<name>.apps.arugula.io`) answering 200 at its health path,
//!    several times in a row, within ten minutes. An app whose answer
//!    reports its revision (JSON `revision`, `commit`, `version` or `sha`,
//!    or an `X-Revision` or `X-Commit` header) is held to it: it must be
//!    the commit's, or its pinned image's (`newTag: sha-<commit>`). One
//!    that reports none is ready when it answers, which an old version
//!    still serving also does: the run says so. The run exits 0 only then.
//!
//! It never prints a token. What it prints goes through the box's redactor
//! into the run's pane anyway.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use arugula_e2e::{
    deploy::apps_platform::{self as ap, APPS_DOMAIN, LIST_BRANCH, LIST_REPO},
    drivers::{RollBack, Settings},
};
use base64::Engine as _;
use sha2::{Digest as _, Sha256};

/// The name the run gets the list's write token under.
pub const WRITE_TOKEN: &str = "APPS_PLATFORM_TOKEN";
/// The name the run gets the app repository's read token under.
pub const READ_TOKEN: &str = "SOURCE_TOKEN";
/// The trailer every commit of this driver carries: what a roll back
/// reverts only.
pub const TRAILER: &str = "Arugula-Driver: apps-platform";
/// Who its commits are by.
const AUTHOR: (&str, &str) = ("Arugula deploy", "deploy@arugula.invalid");

/// Where it works and how long it waits.
#[derive(Debug, Clone)]
pub struct Config {
    /// What repositories' `owner/name.git` is after: `https://github.com/`
    /// (tests: a directory of bare repositories).
    pub git_base: String,
    /// The app's address, health path after it; `None`:
    /// `https://<name>.apps.arugula.io`.
    pub health_base: Option<String>,
    /// How long after the push before it asks: Flux's turn.
    pub settle: Duration,
    pub poll: Duration,
    pub timeout: Duration,
    /// How many answers in a row make it ready.
    pub in_a_row: u32,
    /// Its own directory (the run's `HOME`).
    pub work: PathBuf,
    /// `PATH` for git.
    pub path: String,
    /// GitHub's API, where tokens are revoked.
    pub api_base: String,
    /// The most a repository it fetches may take on disk.
    pub max_repo_bytes: u64,
    /// The most a file it reads from the app's repository may be.
    pub max_file_bytes: u64,
    /// The longest one git command may take.
    pub git_timeout: Duration,
}

impl Config {
    pub fn new(work: PathBuf) -> Self {
        Config {
            git_base: "https://github.com/".into(),
            health_base: None,
            settle: Duration::from_secs(30),
            poll: Duration::from_secs(10),
            timeout: Duration::from_secs(600),
            in_a_row: 3,
            work,
            path: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
            api_base: "https://api.github.com".into(),
            max_repo_bytes: 256 << 20,
            max_file_bytes: 64 << 10,
            git_timeout: Duration::from_secs(300),
        }
    }

    fn url(&self, repo: &str) -> String {
        format!("{}{repo}.git", self.git_base)
    }
}

/// The deploy, as the box gave it to the run.
#[derive(Debug, Clone)]
pub struct Deploy {
    pub id: String,
    pub environment: String,
    pub kind: String,
    pub reference: String,
    pub place: String,
    /// The approved plan's digest.
    pub plan: String,
    /// The deploy a roll back goes back to (M220's roll back, which the
    /// plan names): `None` for a deploy that isn't one.
    pub roll_back: Option<String>,
}

/// The deploy from the command line (the operation the daemon checked)
/// and the rest of the flags, refused unless the plan the owner approved
/// is this driver's plan for exactly it. The `DEPLOY_*` variables, when
/// set, must say the same.
pub fn parse(
    args: &[String],
    var: impl Fn(&str) -> Option<String>,
    mut cfg: Config,
) -> Result<(Config, Deploy), String> {
    let mut d = Deploy {
        id: String::new(),
        environment: String::new(),
        kind: String::new(),
        reference: String::new(),
        place: String::new(),
        plan: String::new(),
        roll_back: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let Some(v) = it.next() else { return Err(format!("{flag} has no value")) };
        let ms = || v.parse::<u64>().map(Duration::from_millis).map_err(|_| format!("{flag} takes milliseconds"));
        match flag.as_str() {
            "--deploy" => d.id = v.clone(),
            "--environment" => d.environment = v.clone(),
            "--kind" => d.kind = v.clone(),
            "--ref" => d.reference = v.clone(),
            "--where" => d.place = v.clone(),
            "--plan" => d.plan = v.clone(),
            "--roll-back" => d.roll_back = Some(v.clone()),
            "--git-base" => cfg.git_base = v.clone(),
            "--health-base" => cfg.health_base = Some(v.clone()),
            "--api-base" => cfg.api_base = v.clone(),
            "--settle-ms" => cfg.settle = ms()?,
            "--poll-ms" => cfg.poll = ms()?,
            "--timeout-ms" => cfg.timeout = ms()?,
            other => return Err(format!("{} isn't a flag here", crate::work::printable(other))),
        }
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
    // A roll back's plan names the deploy it goes back to; this driver
    // keeps nothing else for it (it reverts that deploy's commit).
    let roll_back = d.roll_back.as_ref().map(|deploy| RollBack { deploy: deploy.clone(), to: Settings::new() });
    if roll_back.as_ref().is_some_and(|r| !arugula_e2e::drivers::roll_back_ok(ap::DRIVER, &Settings::new(), r)) {
        return Err("--roll-back isn't a deploy's id".into());
    }
    let said = var("DEPLOY_ROLL_BACK").filter(|v| !v.is_empty());
    let said = said.map(|v| serde_json::from_str::<RollBack>(&v).ok());
    if said.is_some_and(|r| r.is_none() || r != roll_back) {
        return Err("DEPLOY_ROLL_BACK in its environment isn't the deploy on its command line".into());
    }
    let text = arugula_e2e::drivers::plan_text(
        ap::DRIVER,
        &d.environment,
        &d.kind,
        &d.reference,
        &d.place,
        &Settings::new(),
        roll_back.as_ref(),
    );
    let plan = hex::encode(Sha256::digest(text.as_bytes()));
    if plan != d.plan {
        return Err("the approved plan isn't this driver's plan for this deploy".into());
    }
    Ok((cfg, d))
}

/// The run's tokens, from its environment.
#[derive(Default)]
pub struct Tokens {
    pub write: Option<String>,
    pub read: Option<String>,
}

/// What the commit was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// A commit that pins the pointer: its sha.
    Pinned(String),
    /// A revert commit (its sha) of the driver's commit `of`.
    Reverted { commit: String, of: String },
    /// It was pinned there already: nothing to commit.
    Unchanged,
}

/// Why it stopped, and whether anything was pushed by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub pushed: bool,
    pub why: String,
}

fn refused(why: impl Into<String>) -> Failed {
    Failed { pushed: false, why: why.into() }
}

// ---- the pointer ----

/// A new pointer for `name`, pinned at `sha` of `place`: what planter
/// writes, with the commit for its branch.
pub fn render(name: &str, place: &str, sha: &str) -> String {
    format!(
        "# {name} in prod, at https://{name}.{APPS_DOMAIN}. Written by Arugula's apps-platform driver; Flux applies\n\
         # github.com/{place}'s k8s/ at the commit below as planter-apps-deployer, into namespace planter-apps.\n\
         # Deleting this file takes the app out of prod, and prunes what it made (its volume's data too).\n\
         apiVersion: source.toolkit.fluxcd.io/v1\n\
         kind: GitRepository\n\
         metadata:\n\
         \x20 name: {name}\n\
         \x20 namespace: planter-apps-system\n\
         spec:\n\
         \x20 interval: 5m\n\
         \x20 url: https://github.com/{place}\n\
         \x20 provider: github\n\
         \x20 secretRef:\n\
         \x20   name: arugula-github-app\n\
         \x20 ref:\n\
         \x20   commit: {sha}\n\
         ---\n\
         apiVersion: kustomize.toolkit.fluxcd.io/v1\n\
         kind: Kustomization\n\
         metadata:\n\
         \x20 name: {name}\n\
         \x20 namespace: planter-apps-system\n\
         spec:\n\
         \x20 interval: 10m\n\
         \x20 retryInterval: 2m\n\
         \x20 timeout: 5m\n\
         \x20 path: ./k8s\n\
         \x20 prune: true\n\
         \x20 wait: true\n\
         \x20 serviceAccountName: planter-apps-deployer\n\
         \x20 targetNamespace: planter-apps\n\
         \x20 sourceRef:\n\
         \x20   kind: GitRepository\n\
         \x20   name: {name}\n"
    )
}

/// The pointer's documents, by `---` lines: each its lines.
fn documents(text: &str) -> Vec<Vec<&str>> {
    let mut docs = vec![vec![]];
    for line in text.lines() {
        if line.trim_end() == "---" {
            docs.push(vec![]);
        } else if let Some(d) = docs.last_mut() {
            d.push(line);
        }
    }
    docs
}

fn kind_of<'a>(doc: &[&'a str]) -> Option<&'a str> {
    doc.iter().find_map(|l| l.strip_prefix("kind: ")).map(str::trim)
}

/// A top-level field of a document's `spec` or `metadata`, as planter
/// writes them: two spaces in, `key: value`.
fn field<'a>(doc: &[&'a str], key: &str) -> Vec<&'a str> {
    let p = format!("  {key}:");
    doc.iter().filter_map(|l| l.strip_prefix(&p)).map(|v| v.split(" #").next().unwrap_or_default().trim()).collect()
}

/// `text` (a pointer) with its `GitRepository` pinned at `sha`, and the
/// path its `Kustomization` applies. Refused when it isn't a pointer this
/// driver can read for `name`, or it clones another repository than
/// `place`.
pub fn pin(text: &str, name: &str, place: &str, sha: &str) -> Result<(String, String), String> {
    let docs = documents(text);
    let repo = docs.iter().filter(|d| kind_of(d) == Some("GitRepository")).collect::<Vec<_>>();
    let kust = docs.iter().filter(|d| kind_of(d) == Some("Kustomization")).collect::<Vec<_>>();
    let ([repo], [kust]) = (repo.as_slice(), kust.as_slice()) else {
        return Err("the pointer isn't one GitRepository and one Kustomization".into());
    };
    if field(repo, "name") != [name] || field(kust, "name") != [name] {
        return Err(format!("the pointer doesn't name {name}"));
    }
    let want = format!("https://github.com/{place}");
    let url = field(repo, "url");
    if url != [want.as_str()] && url != [format!("{want}.git").as_str()] {
        return Err(format!("the pointer clones {}, not {want}", url.first().copied().unwrap_or("nothing")));
    }
    let [path] = field(kust, "path")[..] else { return Err("the pointer's Kustomization has no one path".into()) };
    let path = path.trim_matches('"').to_owned();
    // The ref: one `  ref:` line, with its block (deeper lines) or a flow
    // map after it.
    let lines: Vec<&str> = text.lines().collect();
    let start = |l: &str| l == "  ref:" || l.starts_with("  ref: ");
    let in_repo: Vec<usize> = {
        // The GitRepository's lines, by position in the file.
        let mut at = Vec::new();
        let mut doc = 0;
        let which = docs.iter().position(|d| kind_of(d) == Some("GitRepository")).unwrap_or(0);
        for (i, l) in lines.iter().enumerate() {
            if l.trim_end() == "---" {
                doc += 1;
            } else if doc == which {
                at.push(i);
            }
        }
        at
    };
    let refs: Vec<usize> = in_repo.iter().copied().filter(|&i| start(lines[i])).collect();
    let [r] = refs[..] else { return Err("the pointer's GitRepository has no one ref".into()) };
    let mut end = r + 1;
    if lines[r] == "  ref:" {
        while end < lines.len() && in_repo.contains(&end) && lines[end].starts_with("    ") {
            end += 1;
        }
    }
    let mut out: Vec<String> = lines[..r].iter().map(|l| l.to_string()).collect();
    out.push("  ref:".into());
    out.push(format!("    commit: {sha}"));
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    Ok((s, path))
}

/// Where the app's `app.yaml` is, from the path its pointer applies
/// (`./k8s`: `app.yaml`; `./web/k8s`: `web/app.yaml`). Refused when the
/// path climbs out.
pub fn app_yaml_path(kustomization_path: &str) -> Result<String, String> {
    let p = kustomization_path.trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') || p.split('/').any(|c| c == ".." || c.is_empty()) {
        return Err(format!("the pointer applies {kustomization_path}, which this driver doesn't read"));
    }
    Ok(match p.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/app.yaml"),
        None => "app.yaml".into(),
    })
}

/// What the driver reads of an `app.yaml`: its top-level `name`, `health`
/// (`/health` unless it says) and `runtime` (`k8s` unless it says).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppYaml {
    pub name: Option<String>,
    pub health: String,
    pub runtime: String,
    /// The revisions its answer may report: the commit, and the image its
    /// `k8s/kustomization.yaml` pins there (filled in by [`read_app`]).
    pub revisions: Vec<String>,
}

pub fn app_yaml(text: &str) -> AppYaml {
    let mut a = AppYaml { name: None, health: "/health".into(), runtime: "k8s".into(), revisions: vec![] };
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.split(" #").next().unwrap_or_default().trim().trim_matches(|c| c == '"' || c == '\'');
        match k.trim() {
            "name" => a.name = Some(v.to_owned()),
            "health" if !v.is_empty() => a.health = v.to_owned(),
            "runtime" if !v.is_empty() => a.runtime = v.to_owned(),
            _ => {}
        }
    }
    a
}

/// The commits a `kustomization.yaml` pins images at: each `newTag:
/// sha-<hex>`.
pub fn pinned_images(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("newTag:"))
        .map(|v| v.split(" #").next().unwrap_or_default().trim().trim_matches(|c| c == '"' || c == '\''))
        .filter_map(|v| v.strip_prefix("sha-"))
        .filter(|h| h.len() >= 7 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Why a new app may not be `name` in the list at `list` (its pointer,
/// `own`, aside): another of the list's Flux objects is named so, or
/// another app's name overlaps it by a dash-prefix (`foo` and `foo-bar`:
/// an app owns `<name>-*`).
pub fn collides(list: &Path, name: &str, own: &str) -> Option<String> {
    let mut files = vec![];
    let mut dirs = vec![list.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_name() == ".git" {
                continue;
            }
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "yaml" || x == "yml") {
                files.push(p);
            }
        }
    }
    for p in files {
        let rel = p.strip_prefix(list).unwrap_or(&p).to_string_lossy().into_owned();
        if rel == own {
            continue;
        }
        if let Some(other) = rel.strip_prefix("apps/").and_then(|r| r.strip_suffix(".yaml"))
            && (name.starts_with(&format!("{other}-")) || other.starts_with(&format!("{name}-")))
        {
            return Some(format!("{name} overlaps the app {other} by a dash-prefix"));
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        if documents(&text).iter().any(|d| d.iter().any(|l| l.trim_end() == format!("  name: {name}"))) {
            return Some(format!("{rel} already holds a Flux object named {name}"));
        }
    }
    None
}

/// A health path the driver asks: `/`, then printable, no space.
fn good_health(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 200 && p.bytes().all(|b| b.is_ascii_graphic())
}

// ---- git ----

/// git, as the run user, in an empty environment but its own: the tokens
/// as `Authorization` headers for their own repository's URL only, through
/// `GIT_CONFIG_*` (never an argument, never a file), with no credential
/// helper, prompt, or global or system config.
pub struct Git {
    env: Vec<(String, String)>,
    timeout: Duration,
}

impl Git {
    pub fn new(cfg: &Config, list: &str, source: &str, t: &Tokens) -> Self {
        let tokens = [(cfg.url(list), t.write.as_deref()), (cfg.url(source), t.read.as_deref())];
        Self::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &tokens)
    }

    /// git in `home`, with `path`, committing as `author`, each token for
    /// its own URL only (M221's arugula-release driver too).
    pub fn with(
        home: &Path,
        path: &str,
        timeout: Duration,
        author: (&str, &str),
        tokens: &[(String, Option<&str>)],
    ) -> Self {
        let mut config: Vec<(String, String)> = vec![("credential.helper".into(), String::new())];
        for (url, token) in tokens {
            if let Some(token) = token {
                let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
                config.push((format!("http.{url}.extraHeader"), format!("Authorization: Basic {basic}")));
            }
        }
        let home = home.to_string_lossy().into_owned();
        let mut env: Vec<(String, String)> = [
            ("PATH", path),
            ("HOME", home.as_str()),
            ("LANG", "C"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_AUTHOR_NAME", author.0),
            ("GIT_AUTHOR_EMAIL", author.1),
            ("GIT_COMMITTER_NAME", author.0),
            ("GIT_COMMITTER_EMAIL", author.1),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        env.push(("GIT_CONFIG_COUNT".into(), config.len().to_string()));
        for (i, (k, v)) in config.into_iter().enumerate() {
            env.push((format!("GIT_CONFIG_KEY_{i}"), k));
            env.push((format!("GIT_CONFIG_VALUE_{i}"), v));
        }
        Git { env, timeout }
    }

    /// The environment git gets (tests check no argument holds a token; the
    /// arugula-release driver runs `scripts/release` in it).
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// `git args` in `dir`: its standard output, or what went wrong (git's
    /// own last lines, which never hold a header).
    pub async fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let run = tokio::process::Command::new("git")
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        let out = tokio::time::timeout(self.timeout, run)
            .await
            .map_err(|_| {
                format!("git {} took over {}s", args.first().copied().unwrap_or_default(), self.timeout.as_secs())
            })?
            .map_err(|e| format!("git didn't start: {e}"))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let last: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = last[last.len().saturating_sub(3)..].join(" / ");
        Err(format!("git {}: {}", args.first().copied().unwrap_or_default(), crate::work::printable(&tail)))
    }
}

/// How much `dir` holds on disk, in bytes.
fn size_of(dir: &Path) -> u64 {
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

// ---- the apply ----

/// Say a step on the run's output.
fn say(s: &str) {
    println!("apps-platform: {}", crate::work::printable(s));
}

/// Apply `d`: clone, pin, commit, push. What was committed, or why not.
pub async fn commit(cfg: &Config, d: &Deploy, t: &Tokens) -> Result<(Applied, AppYaml), Failed> {
    if let Some(why) = ap::refuses(&d.kind, &d.reference, &d.place) {
        return Err(refused(why));
    }
    let name = ap::app_name(&d.place).unwrap_or_default().to_owned();
    let sha = d.reference.as_str();
    let path = ap::pointer_path(&name);
    let git = Git::new(cfg, LIST_REPO, &d.place, t);
    let list = cfg.work.join("list");
    let source = cfg.work.join("source");
    for dir in [&list, &source] {
        let _ = std::fs::remove_dir_all(dir);
    }
    std::fs::create_dir_all(&cfg.work).map_err(|e| refused(format!("its directory: {e}")))?;
    say(&format!("cloning {LIST_REPO} ({LIST_BRANCH})"));
    let list_s = list.to_string_lossy().into_owned();
    git.run(
        &cfg.work,
        &["clone", "--quiet", "--no-tags", "--single-branch", "--branch", LIST_BRANCH, &cfg.url(LIST_REPO), &list_s],
    )
    .await
    .map_err(refused)?;
    if size_of(&list) > cfg.max_repo_bytes {
        return Err(refused(format!("{LIST_REPO} is over {} MiB", cfg.max_repo_bytes >> 20)));
    }
    let mut fetched: Option<AppYaml> = None;
    for attempt in 1..=3 {
        // A replay: this deploy is in the list's history already.
        let trailer = format!("Arugula-Deploy: {}", d.id);
        let seen =
            git.run(&list, &["log", "--format=%H", "--fixed-strings", "--grep", &trailer]).await.map_err(refused)?;
        if let Some(c) = seen.split_whitespace().next() {
            return Err(refused(format!("{} was applied to {LIST_REPO} already, by {c}: not again", d.id)));
        }
        let file = list.join(&path);
        let now = match std::fs::read_to_string(&file) {
            Ok(s) => Some(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(refused(format!("reading {path}: {e}"))),
        };
        if let Some(why) = collides(&list, &name, &path) {
            return Err(refused(why));
        }
        let (new, kpath) = match &now {
            Some(text) => pin(text, &name, &d.place, sha).map_err(refused)?,
            None => (render(&name, &d.place, sha), "./k8s".to_owned()),
        };
        if fetched.is_none() {
            fetched = Some(read_app(cfg, &git, &source, d, &name, &kpath).await?);
        }
        let app = fetched.clone().unwrap_or_else(|| app_yaml(""));
        if now.as_deref() == Some(new.as_str()) {
            say(&format!("{path} pins {sha} already: nothing to commit"));
            return Ok((Applied::Unchanged, app));
        }
        let short = &sha[..12];
        let reverts = match &now {
            Some(_) => revertible(&git, &list, &path, &new).await,
            None => None,
        };
        let applied = match reverts {
            Some(of) => {
                say(&format!("{path}: rolling back, as a revert of {of}"));
                git.run(&list, &["revert", "--no-commit", &of]).await.map_err(refused)?;
                let after = std::fs::read_to_string(&file).map_err(|e| refused(format!("reading {path}: {e}")))?;
                if after != new {
                    return Err(refused("the revert didn't pin the commit asked for"));
                }
                let msg = format!(
                    "Roll back {name} to {short} ({})\n\nThis reverts commit {of}, the deploy before it: \
                     {path} pins https://github.com/{} at {sha} again, for the environment {}.\n\n\
                     Arugula-Deploy: {}\n{TRAILER}\n",
                    d.id, d.place, d.environment, d.id
                );
                git.run(&list, &["commit", "--quiet", "-m", &msg]).await.map_err(refused)?;
                Applied::Reverted { commit: String::new(), of }
            }
            None => {
                say(&format!("{path}: pinning https://github.com/{} at {sha}", d.place));
                if let Some(dir) = file.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| refused(format!("{path}: {e}")))?;
                }
                std::fs::write(&file, &new).map_err(|e| refused(format!("writing {path}: {e}")))?;
                git.run(&list, &["add", "--", &path]).await.map_err(refused)?;
                let msg = format!(
                    "Deploy {name} at {short} ({})\n\n{path} pins https://github.com/{} at {sha}, for the \
                     environment {}. Flux applies it.\n\nArugula-Deploy: {}\n{TRAILER}\n",
                    d.id, d.place, d.environment, d.id
                );
                git.run(&list, &["commit", "--quiet", "-m", &msg]).await.map_err(refused)?;
                Applied::Pinned(String::new())
            }
        };
        let head = git.run(&list, &["rev-parse", "HEAD"]).await.map_err(refused)?.trim().to_owned();
        // Exactly the one file, or nothing goes.
        let changed = git.run(&list, &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).await;
        if changed.as_deref().map(|c| c.lines().collect::<Vec<_>>()) != Ok(vec![path.as_str()]) {
            return Err(refused(format!("the commit changes more than {path}")));
        }
        match git.run(&list, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{LIST_BRANCH}")]).await {
            Ok(_) => {
                say(&format!("pushed {head} to {LIST_REPO}: Flux applies it"));
                return Ok((
                    match applied {
                        Applied::Reverted { of, .. } => Applied::Reverted { commit: head, of },
                        _ => Applied::Pinned(head),
                    },
                    app,
                ));
            }
            Err(e) if attempt < 3 => {
                say(&format!("the push didn't go ({e}); again from the list's head"));
                git.run(&list, &["fetch", "--quiet", "origin", LIST_BRANCH]).await.map_err(refused)?;
                git.run(&list, &["reset", "--quiet", "--hard", "FETCH_HEAD"]).await.map_err(refused)?;
            }
            Err(e) => return Err(refused(e)),
        }
    }
    Err(refused("the push didn't go three times"))
}

/// The commit a roll back to `new` reverts, if there's one: the last
/// commit to `path` is this driver's, changed only `path`, and its parent's
/// `path` is `new` exactly.
async fn revertible(git: &Git, list: &Path, path: &str, new: &str) -> Option<String> {
    let c = git.run(list, &["log", "-1", "--format=%H", "--", path]).await.ok()?.trim().to_owned();
    // Its parents, unsimplified: one.
    let parents = git.run(list, &["rev-list", "--parents", "-n", "1", &c]).await.ok()?;
    let [_, parent] = parents.split_whitespace().collect::<Vec<_>>()[..] else { return None };
    let parent = parent.to_owned();
    let body = git.run(list, &["log", "-1", "--format=%B", &c]).await.ok()?;
    if !body.lines().any(|l| l.trim() == TRAILER) {
        return None;
    }
    let changed = git.run(list, &["diff-tree", "--no-commit-id", "--name-only", "-r", &c]).await.ok()?;
    if changed.lines().collect::<Vec<_>>() != [path] {
        return None;
    }
    let before = git.run(list, &["show", &format!("{parent}:{path}")]).await.ok()?;
    (before == new).then_some(c)
}

/// The app's `app.yaml` at the commit: the commit must be on its
/// repository's default branch (its history fetched commits only, the
/// files read one by one, each bounded), named as the pointer, run in the
/// cluster; with the revisions its answer may report.
async fn read_app(
    cfg: &Config,
    git: &Git,
    source: &Path,
    d: &Deploy,
    name: &str,
    kpath: &str,
) -> Result<AppYaml, Failed> {
    let at = app_yaml_path(kpath).map_err(refused)?;
    let sha = d.reference.as_str();
    std::fs::create_dir_all(source).map_err(|e| refused(format!("its directory: {e}")))?;
    git.run(source, &["init", "--quiet"]).await.map_err(refused)?;
    git.run(source, &["remote", "add", "origin", &cfg.url(&d.place)]).await.map_err(refused)?;
    say(&format!("reading {}'s default branch, commits only", d.place));
    git.run(source, &["fetch", "--quiet", "--no-tags", "--filter=tree:0", "origin", "+HEAD:refs/remotes/origin/HEAD"])
        .await
        .map_err(|e| refused(format!("{}'s history: {e}", d.place)))?;
    if size_of(source) > cfg.max_repo_bytes {
        return Err(refused(format!("{}'s history is over {} MiB", d.place, cfg.max_repo_bytes >> 20)));
    }
    // A fork's commit is served by its sha too: only the default branch's.
    let on_branch = git.run(source, &["merge-base", "--is-ancestor", sha, "refs/remotes/origin/HEAD"]).await.is_ok();
    if !on_branch {
        return Err(refused(format!("{sha} isn't on {}'s default branch", d.place)));
    }
    let file = |path: String| async move {
        let spec = format!("{sha}:{path}");
        let size = git.run(source, &["cat-file", "-s", &spec]).await.ok()?.trim().parse::<u64>().ok()?;
        if size > cfg.max_file_bytes {
            return Some(Err(format!("{path} at {sha} is over {} KiB", cfg.max_file_bytes >> 10)));
        }
        git.run(source, &["cat-file", "blob", &spec]).await.ok().map(Ok)
    };
    say(&format!("reading {at} of {} at {sha}", d.place));
    let text = match file(at.clone()).await {
        Some(Ok(t)) => t,
        Some(Err(why)) => return Err(refused(why)),
        None => return Err(refused(format!("{} has no {at} at {sha}", d.place))),
    };
    let mut app = app_yaml(&text);
    if app.name.as_deref() != Some(name) {
        return Err(refused(format!("its app.yaml names {}, not {name}", app.name.as_deref().unwrap_or("nothing"))));
    }
    if app.runtime != "k8s" {
        return Err(refused(format!(
            "its app.yaml says runtime {}: this driver deploys apps that run in the cluster",
            crate::work::printable(&app.runtime)
        )));
    }
    if !good_health(&app.health) {
        return Err(refused("its app.yaml's health path isn't one this driver asks"));
    }
    app.revisions = vec![sha.to_owned()];
    let kust = format!("{}/kustomization.yaml", kpath.trim_start_matches("./").trim_end_matches('/'));
    if let Some(Ok(k)) = file(kust).await {
        app.revisions.extend(pinned_images(&k));
    }
    Ok(app)
}

/// Revoke an installation token at GitHub, as itself (`ghs_`; a
/// fine-grained token of the environment's own is left to its owner).
/// Best effort: it lasts an hour at most, and the platform revokes it too
/// when the run ends.
pub async fn revoke(cfg: &Config, what: &str, token: Option<&str>) {
    revoke_at("apps-platform", &cfg.api_base, what, token).await;
}

/// [`revoke`] at `api_base`, said as `driver`'s (M221's arugula-release
/// driver too).
pub async fn revoke_at(driver: &str, api_base: &str, what: &str, token: Option<&str>) {
    let say = |s: &str| println!("{driver}: {}", crate::work::printable(s));
    let Some(token) = token.filter(|t| t.starts_with("ghs_")) else { return };
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build();
    let sent = match client {
        Ok(c) => c
            .delete(format!("{}/installation/token", api_base.trim_end_matches('/')))
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "arugula-deploy")
            .send()
            .await
            .map(|r| r.status().as_u16()),
        Err(_) => Ok(0),
    };
    match sent {
        Ok(204) => say(&format!("revoked the {what} token")),
        Ok(n) => say(&format!("revoking the {what} token: GitHub said {n} (it expires within the hour)")),
        Err(_) => say(&format!("revoking the {what} token didn't go through (it expires within the hour)")),
    }
}

/// The revision an answer reports, if it does: an `X-Revision` or
/// `X-Commit` header, or a JSON body's `revision`, `commit`, `version` or
/// `sha`; lowercase, `sha-` taken off.
pub fn reported(headers: &reqwest::header::HeaderMap, body: &[u8]) -> Option<String> {
    let header = ["x-revision", "x-commit"].iter().find_map(|h| headers.get(*h)?.to_str().ok().map(str::to_owned));
    let field = || {
        let v: serde_json::Value = serde_json::from_slice(body).ok()?;
        ["revision", "commit", "version", "sha"].iter().find_map(|k| v.get(*k)?.as_str().map(str::to_owned))
    };
    let r = header.or_else(field)?.trim().to_ascii_lowercase();
    Some(r.strip_prefix("sha-").unwrap_or(&r).to_owned()).filter(|r| !r.is_empty())
}

/// Whether a reported revision is one of `revisions` (the whole commit,
/// or seven or more of its first characters).
pub fn is_one_of(reported: &str, revisions: &[String]) -> bool {
    reported.len() >= 7 && revisions.iter().any(|r| r.starts_with(reported))
}

/// Wait for `url` to answer 200, `in_a_row` times in a row, within the
/// timeout: what ready is from outside the cluster.
pub async fn ready(cfg: &Config, url: &str, revisions: &[String]) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("an HTTP client: {e}"))?;
    let start = Instant::now();
    if !cfg.settle.is_zero() {
        say(&format!("waiting {}s for Flux to apply it", cfg.settle.as_secs()));
        tokio::time::sleep(cfg.settle).await;
    }
    let (mut ok, mut last, mut unversioned) = (0, String::new(), false);
    loop {
        let (now, good) = match client.get(url).send().await {
            Ok(r) => {
                let status = r.status().as_u16();
                let headers = r.headers().clone();
                let mut body = Vec::new();
                let mut r = r;
                while let Ok(Some(chunk)) = r.chunk().await {
                    body.extend_from_slice(&chunk);
                    if body.len() > 64 << 10 {
                        break;
                    }
                }
                match (status, reported(&headers, &body)) {
                    (200, Some(rev)) if is_one_of(&rev, revisions) => (format!("answers 200, revision {rev}"), true),
                    (200, Some(rev)) => (
                        format!("answers 200, but revision {} (not this deploy's)", crate::work::printable(&rev)),
                        false,
                    ),
                    (200, None) => {
                        unversioned = true;
                        ("answers 200".to_owned(), true)
                    }
                    (n, _) => (format!("answers {n}"), false),
                }
            }
            Err(e) if e.is_timeout() => ("doesn't answer in 10s".to_owned(), false),
            Err(_) => ("can't be reached".to_owned(), false),
        };
        ok = if good { ok + 1 } else { 0 };
        if now != last {
            say(&format!("{url} {now}"));
            last = now;
        }
        if ok >= cfg.in_a_row {
            if unversioned {
                say(&format!(
                    "{url} is ready, as far as can be told: it doesn't report its revision, so an old version still \
                     serving would look the same (report it as JSON `revision` to be held to it)"
                ));
            } else {
                say(&format!("{url} is ready, on this deploy's revision"));
            }
            return Ok(());
        }
        if start.elapsed() >= cfg.timeout {
            return Err(format!("{url} wasn't ready in {}s: it {last}", cfg.timeout.as_secs()));
        }
        tokio::time::sleep(cfg.poll).await;
    }
}

/// The whole apply: commit, then wait until ready.
/// The whole apply: commit, revoke the tokens (nothing after needs them),
/// then wait until ready.
pub async fn apply(cfg: &Config, d: &Deploy, t: &Tokens) -> Result<Applied, Failed> {
    let committed = commit(cfg, d, t).await;
    revoke(cfg, "list's write", t.write.as_deref()).await;
    revoke(cfg, "app repository's read", t.read.as_deref()).await;
    let (applied, app) = committed?;
    let name = ap::app_name(&d.place).unwrap_or_default();
    let base = cfg.health_base.clone().unwrap_or_else(|| format!("https://{name}.{APPS_DOMAIN}"));
    let url = format!("{}{}", base.trim_end_matches('/'), app.health);
    ready(cfg, &url, &app.revisions).await.map_err(|why| Failed { pushed: applied != Applied::Unchanged, why })?;
    Ok(applied)
}

/// `arugulad _apps-platform --deploy ID --environment NAME --kind KIND
/// --ref SHA --where OWNER/NAME --plan DIGEST [--git-base URL]
/// [--health-base URL] [--api-base URL] [--settle-ms N] [--poll-ms N]
/// [--timeout-ms N]`, from the driver's shell: the deploy as the daemon
/// checked it, and the box's own flags for tests and testnet. Exits 0
/// once the app is ready; 2 when nothing was pushed; 3 when the pointer
/// was pushed and the app isn't ready.
pub fn cli(args: &[String]) -> ! {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let work = var("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from).join("apps-platform");
    let mut cfg = Config::new(work.clone());
    if let Some(p) = var("PATH") {
        cfg.path = p;
    }
    let (cfg, d) = match parse(args, var, cfg) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("apps-platform: {}", crate::work::printable(&why));
            std::process::exit(2)
        }
    };
    let t = Tokens { write: var(WRITE_TOKEN), read: var(READ_TOKEN) };
    let code = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => match rt.block_on(apply(&cfg, &d, &t)) {
            Ok(_) => 0,
            Err(f) => {
                eprintln!("apps-platform: {}", crate::work::printable(&f.why));
                if f.pushed { 3 } else { 2 }
            }
        },
        Err(e) => {
            eprintln!("apps-platform: {e}");
            2
        }
    };
    drop(t);
    let _ = std::fs::remove_dir_all(&work);
    std::process::exit(code)
}

#[cfg(test)]
#[path = "work_apps_platform_tests.rs"]
mod tests;
