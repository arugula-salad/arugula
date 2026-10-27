//! The arugula-release driver's apply, on a deploy's box (M221, platform
//! step 7, native Arugula #17): Arugula's own release, as `just release`
//! does it from Jake's shell. The platform's half is
//! `arugula_platform::drivers::arugula_release`; the plan both sides make
//! is `arugula_e2e::drivers::arugula_release`.
//!
//! It runs as `arugulad _arugula-release`, as the run user. The deploy it
//! applies is on its command line, as the daemon checked it (`--deploy`,
//! `--ref`, `--bump`, `--lane`, ... and the approved plan's digest, which
//! it recomputes, [`parse`]). Its one token ([`TOKEN`], the platform's
//! GitHub App's, `contents: write` on [`REPO`] alone) is in its
//! environment, read there from its standard input by the driver's shell.
//!
//! 1. **Clones [`REPO`]**'s `main` over HTTPS (commits and trees; files as
//!    they're read). The token goes to git as an `Authorization` header in
//!    git's environment, for the repository's URL only (as the apps-platform
//!    driver's does): never on a command line, in `.git/config` or in a
//!    credential store.
//! 2. **Checks the commit**: on `main`, with a `ci/green/<sha>` tag and no
//!    `ci/revoked/<sha>` one. A deploy whose id is in `main`'s history
//!    already (its `Arugula-Deploy:` trailer) is a replay: refused.
//! 3. **Runs `scripts/release bump`** as that commit has it (what `just
//!    release` runs), with the bump and the lane the plan names, the
//!    commit, and the deploy's id as a trailer. It refuses what it refuses
//!    (no notes on `main`, the tag there already, the commit released
//!    already); else it commits the bump, tags it and pushes the tag and
//!    `main` in one atomic push.
//! 4. **Revokes the token** at GitHub right after: nothing after needs it.
//! 5. **Follows the tag's release workflow** to its end, for at most
//!    [`FOLLOW_MINUTES`], with its second token ([`READ_TOKEN`], the App's:
//!    Actions runs and contents, read-only), and reads the release; then
//!    revokes that token too. The run exits 0 only once its publish job
//!    succeeded and the release is out; it reports the tag, the commits,
//!    the release's page and the workflow's run on fd 3.
//!
//! There's no roll back: a bad release is fixed by a new patch release.

use std::{
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use arugula_e2e::drivers::{
    Settings,
    arugula_release::{self as ar, BRANCH, BUMP, FOLLOW_MINUTES, LANE, Lane, PUBLISH_JOB, READ_TOKEN, REPO},
};
use sha2::{Digest as _, Sha256};

use super::apps_platform::{Git, revoke_at};

/// The name the run gets its token under.
pub use arugula_e2e::drivers::arugula_release::TOKEN;

/// The trailer that names the deploy on the bump, the tag and the merge.
pub const TRAILER: &str = "Arugula-Deploy";

/// Who the bump and the merge are by (as the release workflow's Homebrew
/// commits are).
const AUTHOR: (&str, &str) = ("arugula release", "noreply@inevitable.fyi");

/// Where it works and how long it waits.
#[derive(Debug, Clone)]
pub struct Config {
    /// What `owner/name.git` is after: `https://github.com/` (tests: a
    /// directory of bare repositories).
    pub git_base: String,
    /// GitHub's API: workflow runs, releases, and where the token is
    /// revoked.
    pub api_base: String,
    /// Its own directory (the run's `HOME`).
    pub work: PathBuf,
    /// `PATH` for git and bash.
    pub path: String,
    /// The longest one git command may take.
    pub git_timeout: Duration,
    /// The longest `scripts/release bump` may take.
    pub script_timeout: Duration,
    /// How often it asks how the workflow is doing, and for how long.
    pub poll: Duration,
    pub follow: Duration,
}

impl Config {
    pub fn new(work: PathBuf) -> Self {
        Config {
            git_base: "https://github.com/".into(),
            api_base: "https://api.github.com".into(),
            work,
            path: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
            git_timeout: Duration::from_secs(600),
            script_timeout: Duration::from_secs(1200),
            poll: Duration::from_secs(30),
            follow: Duration::from_secs(FOLLOW_MINUTES * 60),
        }
    }

    fn url(&self) -> String {
        format!("{}{REPO}.git", self.git_base)
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
    /// `bump` and `lane`, as the plan names them.
    pub settings: Settings,
}

impl Deploy {
    fn lane(&self) -> Lane {
        Lane::of(&self.settings)
    }
}

/// The deploy from the command line (the operation the daemon checked)
/// and the rest of the flags, refused unless the plan the owner approved
/// is this driver's plan for exactly it. The `DEPLOY_*` variables, when
/// set, must say the same; a roll back is refused.
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
        settings: Settings::new(),
    };
    // A box that runs the fake driver (tests and testnet) says so in the
    // driver's environment; a secret can't (`ARUGULA_*` names are refused).
    let test_box = var("ARUGULA_HOSTED_FAKE_DRIVER").as_deref() == Some("1");
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
            "--bump" => _ = d.settings.insert(BUMP.into(), v.clone()),
            "--lane" => _ = d.settings.insert(LANE.into(), v.clone()),
            // Where it clones, pushes and asks: tests' and testnet's only.
            "--git-base" | "--api-base" if !test_box => {
                return Err(format!("{flag} is for a box that runs the fake driver only"));
            }
            "--git-base" => cfg.git_base = v.clone(),
            "--api-base" => cfg.api_base = v.clone(),
            "--poll-ms" => cfg.poll = ms()?,
            "--follow-ms" => cfg.follow = ms()?,
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
    // Both settings, as the environment keeps them (defaults filled in).
    if ar::settings(&d.settings).as_ref() != Ok(&d.settings) {
        return Err("--bump and --lane aren't settings this driver takes".into());
    }
    if let Some(s) = var("DEPLOY_SETTINGS").filter(|s| !s.is_empty())
        && serde_json::from_str::<Settings>(&s).ok().as_ref() != Some(&d.settings)
    {
        return Err("DEPLOY_SETTINGS in its environment isn't the settings on its command line".into());
    }
    if var("DEPLOY_ROLL_BACK").is_some_and(|r| !r.is_empty()) {
        return Err("a release never rolls back: a bad one is fixed by a new patch release".into());
    }
    let text =
        arugula_e2e::drivers::plan_text(ar::DRIVER, &d.environment, &d.kind, &d.reference, &d.place, &d.settings, None);
    if hex::encode(Sha256::digest(text.as_bytes())) != d.plan {
        return Err("the approved plan isn't this driver's plan for this deploy".into());
    }
    Ok((cfg, d))
}

/// What it released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Released {
    pub tag: String,
    pub version: String,
    /// The bump commit the tag is on.
    pub commit: String,
    /// `main` after the push (the bump merged into it).
    pub main: String,
    /// The `scripts/release` it ran: its blob id at the commit.
    pub script: String,
}

/// What following the release workflow found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Followed {
    /// The workflow's run on GitHub, once it was found.
    pub run: Option<String>,
    /// `success`, `failure`, ...: `None` while it hadn't ended.
    pub conclusion: Option<String>,
    /// The jobs that ended other than success (or skipped).
    pub failed_jobs: Vec<String>,
    /// Its publish job succeeded and the release is out (not a draft).
    pub published: bool,
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

/// Say a step on the run's output.
fn say(s: &str) {
    println!("arugula-release: {}", crate::work::printable(s));
}

/// The `ci/` verdict on `sha` from `ls-remote`'s answer: green, and not
/// revoked.
pub fn green(ls_remote: &str, sha: &str) -> Result<(), String> {
    let has = |r: &str| ls_remote.lines().any(|l| l.split_whitespace().nth(1) == Some(r));
    if has(&format!("refs/tags/ci/revoked/{sha}")) {
        return Err(format!("{sha} passed CI and then failed a re-run (ci/revoked): release another commit"));
    }
    if !has(&format!("refs/tags/ci/green/{sha}")) {
        return Err(format!("{sha} has no ci/green tag (CI may still be running): release a commit that passed CI"));
    }
    Ok(())
}

/// The tag and `main`'s commit `scripts/release bump` says it pushed:
/// `pushed TAG (SHORT) and main (SHORT)`.
pub fn pushed_line(out: &str) -> Option<(String, String)> {
    let line = out.lines().rev().find(|l| l.starts_with("pushed "))?;
    let rest = line.strip_prefix("pushed ")?;
    let (tag, rest) = rest.split_once(" (")?;
    let (_, rest) = rest.split_once(") and main (")?;
    let main = rest.strip_suffix(')')?;
    let hex = |s: &str| !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
    (hex(main) && !tag.is_empty()).then(|| (tag.to_owned(), main.to_owned()))
}

/// Run the commit's own `scripts/release bump`, after checking the
/// commit: what was released, or why not.
pub async fn release(cfg: &Config, d: &Deploy, token: Option<&str>) -> Result<Released, Failed> {
    if let Some(why) = ar::refuses(&d.kind, &d.reference, &d.place) {
        return Err(refused(why));
    }
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Err(refused(format!("no {TOKEN}: the platform mints it for the run")));
    };
    let sha = d.reference.as_str();
    let lane = d.lane();
    let url = cfg.url();
    let git = Git::with(&cfg.work, &cfg.path, cfg.git_timeout, AUTHOR, &[(url.clone(), Some(token))]);
    let repo = cfg.work.join("arugula");
    let _ = std::fs::remove_dir_all(&repo);
    let tmp = cfg.work.join("tmp");
    for dir in [&cfg.work, &tmp] {
        std::fs::create_dir_all(dir).map_err(|e| refused(format!("its directory: {e}")))?;
    }
    say(&format!("cloning {REPO}'s {BRANCH} (commits and trees; files as they're read)"));
    let repo_s = repo.to_string_lossy().into_owned();
    git.run(
        &cfg.work,
        &[
            "clone",
            "--quiet",
            "--no-checkout",
            "--filter=blob:none",
            "--no-tags",
            "--single-branch",
            "--branch",
            BRANCH,
            &url,
            &repo_s,
        ],
    )
    .await
    .map_err(refused)?;
    let main_ref = format!("refs/remotes/origin/{BRANCH}");
    let on_main = git.run(&repo, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).await.is_ok()
        && git.run(&repo, &["merge-base", "--is-ancestor", sha, &main_ref]).await.is_ok();
    if !on_main {
        return Err(refused(format!("{sha} isn't on {REPO}'s {BRANCH}")));
    }
    // A replay: this deploy released already.
    let trailer = format!("{TRAILER}: {}", d.id);
    let seen = git.run(&repo, &["log", "--format=%H", "--fixed-strings", "--grep", &trailer, &main_ref]).await;
    if let Some(c) = seen.map_err(refused)?.split_whitespace().next() {
        return Err(refused(format!("{} released already, in {c}: not again", d.id)));
    }
    let tags = [format!("refs/tags/ci/green/{sha}"), format!("refs/tags/ci/revoked/{sha}")];
    let ci = git.run(&repo, &["ls-remote", &url, &tags[0], &tags[1]]).await.map_err(refused)?;
    green(&ci, sha).map_err(refused)?;
    say(&format!("{sha} is on {BRANCH} and passed CI (ci/green)"));
    // The commit's own script, outside the checkout, by its blob id.
    let spec = format!("{sha}:scripts/release");
    let script_id = git.run(&repo, &["rev-parse", "--verify", &spec]).await.map_err(refused)?.trim().to_owned();
    let script = git.run(&repo, &["show", &script_id]).await.map_err(refused)?;
    say(&format!("scripts/release at {sha} is blob {script_id}"));
    // The lane's tags before it runs: a run stopped part way is checked
    // against them.
    let lane_tags = format!("refs/tags/{}*", lane.prefix());
    let before = git.run(&repo, &["ls-remote", &url, &lane_tags]).await.map_err(refused)?;
    let script_at = cfg.work.join("release");
    std::fs::write(&script_at, script).map_err(|e| refused(format!("writing its script: {e}")))?;
    let bump = d.settings.get(BUMP).map_or("patch", String::as_str);
    let mut args = vec!["--noprofile".to_owned(), "--norc".into(), script_at.to_string_lossy().into_owned()];
    args.extend(["bump".into(), bump.into()]);
    if lane == Lane::App {
        args.push("--app".into());
    }
    args.push(sha.into());
    say(&format!("scripts/release {}", args[3..].join(" ")));
    let mut cmd = tokio::process::Command::new("bash");
    cmd.env_clear()
        .envs(git.env().iter().map(|(k, v)| (k, v)))
        .env("GITHUB_SERVER_URL", cfg.git_base.trim_end_matches('/'))
        .env("GITHUB_REPOSITORY", REPO)
        .env("TMPDIR", &tmp)
        .env("ARUGULA_RELEASE_TRAILER", &trailer)
        .current_dir(&repo)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Its own process group: a stop takes git and everything it started.
    #[cfg(unix)]
    cmd.process_group(0);
    let child = cmd.spawn().map_err(|e| refused(format!("scripts/release didn't start: {e}")))?;
    let pid = child.id();
    let out = match tokio::time::timeout(cfg.script_timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(refused(format!("scripts/release: {e}"))),
        Err(_) => {
            stop_group(pid);
            let why = format!("scripts/release took over {}s and was stopped", cfg.script_timeout.as_secs());
            // Stopped mid-push, perhaps: a tag of this lane that's new says so.
            let after = git.run(&repo, &["ls-remote", &url, &lane_tags]).await;
            return Err(match after.as_ref().map(|a| new_tags(&before, a)) {
                Ok(new) if new.is_empty() => refused(format!("{why}: it pushed no tag")),
                Ok(new) => Failed { pushed: true, why: format!("{why}, after it pushed {}", new.join(", ")) },
                Err(e) => {
                    Failed { pushed: true, why: format!("{why}, and whether it pushed a tag can't be told: {e}") }
                }
            });
        }
    };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        println!("scripts/release: {}", crate::work::printable(line));
    }
    if !out.status.success() {
        let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default();
        // It pushes last, atomically: when it stops, nothing was pushed.
        return Err(refused(format!(
            "scripts/release stopped (exit {}): {}",
            out.status.code().unwrap_or(-1),
            crate::work::printable(last)
        )));
    }
    // Pushed: from here on, a failure says so.
    let pushed = |why: String| Failed { pushed: true, why };
    let Some((tag, main_short)) = pushed_line(&text) else {
        return Err(pushed("scripts/release exited 0 but didn't say what it pushed".into()));
    };
    let Some(version) = ar::tag_version(lane, &tag).map(str::to_owned) else {
        return Err(pushed(format!("scripts/release pushed {}, not a tag of this lane", crate::work::printable(&tag))));
    };
    let rev = |r: String| {
        let git = &git;
        let repo = &repo;
        async move {
            git.run(repo, &["rev-parse", "--verify", &format!("{r}^{{commit}}")]).await.map(|s| s.trim().to_owned())
        }
    };
    let commit = rev(tag.clone()).await.map_err(pushed)?;
    let main = rev(main_short).await.map_err(pushed)?;
    say(&format!("pushed {tag} ({commit}) and {BRANCH} ({main})"));
    Ok(Released { tag, version, commit, main, script: script_id })
}

/// Kill process group `pid` (a stopped `scripts/release` and what it
/// started).
fn stop_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// The tags in `after` (`ls-remote`'s answer) that `before` didn't have.
pub fn new_tags(before: &str, after: &str) -> Vec<String> {
    let refs = |s: &str| -> Vec<String> {
        s.lines()
            .filter_map(|l| l.split_whitespace().nth(1))
            .filter(|r| !r.ends_with("^{}"))
            .map(|r| r.trim_start_matches("refs/tags/").to_owned())
            .collect()
    };
    let had = refs(before);
    refs(after).into_iter().filter(|t| !had.contains(t)).collect()
}

/// GitHub's API, with the run's read token ([`READ_TOKEN`]: Actions runs
/// and contents, read-only), which has the App's rate limit. Without it, or
/// once it's expired (it lasts an hour), as anyone: Arugula is public, but
/// an address gets 60 asks an hour that way.
struct Api {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl Api {
    fn new(base: &str, token: Option<&str>) -> Result<Self, String> {
        let http = crate::roots::http()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("an HTTP client: {e}"))?;
        Ok(Api { http, base: base.trim_end_matches('/').to_owned(), token: token.map(str::to_owned) })
    }

    /// `GET path`: its JSON, or what went wrong (its status, `0` when it
    /// wasn't reached).
    async fn get(&mut self, path: &str) -> Result<serde_json::Value, u16> {
        let ask = |token: Option<&str>| {
            let mut req = self
                .http
                .get(format!("{}{path}", self.base))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "arugula-release");
            if let Some(t) = token {
                req = req.bearer_auth(t);
            }
            req.send()
        };
        let mut r = ask(self.token.as_deref()).await.map_err(|_| 0u16)?;
        if r.status().as_u16() == 401 && self.token.is_some() {
            say("the read token is no longer good (it lasts an hour): asking as anyone");
            self.token = None;
            r = ask(None).await.map_err(|_| 0u16)?;
        }
        let status = r.status().as_u16();
        if status != 200 {
            return Err(status);
        }
        let mut body = Vec::new();
        let mut r = r;
        while let Ok(Some(chunk)) = r.chunk().await {
            body.extend_from_slice(&chunk);
            if body.len() > 4 << 20 {
                return Err(413);
            }
        }
        serde_json::from_slice(&body).map_err(|_| 502u16)
    }
}

/// Follow the release workflow `lane`'s tag on `r.commit` runs, to its end
/// or [`Config::follow`], then read the release.
pub async fn follow(cfg: &Config, lane: Lane, r: &Released, token: Option<&str>) -> Followed {
    let mut f = Followed::default();
    let mut api = match Api::new(&cfg.api_base, token) {
        Ok(a) => a,
        Err(why) => {
            say(&why);
            return f;
        }
    };
    let start = Instant::now();
    let workflow = lane.workflow();
    say(&format!("following {workflow} for {} (at most {} minutes)", r.tag, cfg.follow.as_secs() / 60));
    let runs = format!("/repos/{REPO}/actions/runs?event=push&head_sha={}&per_page=30", r.commit);
    let (mut last, mut id) = (String::new(), None::<u64>);
    loop {
        let now = match api.get(&runs).await {
            Ok(v) => {
                let run = v["workflow_runs"].as_array().and_then(|runs| {
                    runs.iter().find(|w| {
                        w["path"].as_str().is_some_and(|p| p == workflow || p.starts_with(&format!("{workflow}@")))
                    })
                });
                match run {
                    None => "hasn't started".to_owned(),
                    Some(w) => {
                        id = w["id"].as_u64();
                        f.run = w["html_url"].as_str().filter(|u| ar::run_url_ok(u)).map(str::to_owned);
                        let status = w["status"].as_str().unwrap_or("unknown").to_owned();
                        if status == "completed" {
                            f.conclusion = w["conclusion"].as_str().map(str::to_owned);
                        }
                        status
                    }
                }
            }
            Err(403 | 429) => "can't be asked now (GitHub's rate limit): asking again".to_owned(),
            Err(n) => format!("can't be asked now (GitHub said {n})"),
        };
        if now != last {
            say(&format!("{workflow} {now}"));
            last = now;
        }
        if f.conclusion.is_some() || start.elapsed() >= cfg.follow {
            break;
        }
        tokio::time::sleep(cfg.poll).await;
    }
    let Some(id) = id.filter(|_| f.conclusion.is_some()) else {
        say(&format!(
            "{workflow} didn't end within {} minutes: see {}",
            cfg.follow.as_secs() / 60,
            f.run.as_deref().unwrap_or("its actions page")
        ));
        return f;
    };
    // Its jobs: the publish job's verdict, and those that failed.
    let mut publish_ok = false;
    for _ in 0..3 {
        match api.get(&format!("/repos/{REPO}/actions/runs/{id}/jobs?per_page=100")).await {
            Ok(v) => {
                for j in v["jobs"].as_array().into_iter().flatten() {
                    let (name, c) = (j["name"].as_str().unwrap_or_default(), j["conclusion"].as_str().unwrap_or(""));
                    if name == PUBLISH_JOB {
                        publish_ok = c == "success";
                    }
                    if !["success", "skipped"].contains(&c) && f.failed_jobs.len() < 8 {
                        f.failed_jobs.push(crate::work::printable(name).chars().take(40).collect());
                    }
                }
                break;
            }
            Err(_) => tokio::time::sleep(cfg.poll.min(Duration::from_secs(10))).await,
        }
    }
    // The release itself: out, not a draft (GitHub shows a draft to no one
    // unauthenticated: 404).
    let mut out = false;
    for _ in 0..3 {
        match api.get(&format!("/repos/{REPO}/releases/tags/{}", r.tag)).await {
            Ok(v) => {
                out = v["draft"] == serde_json::Value::Bool(false);
                break;
            }
            Err(404) => break,
            Err(_) => tokio::time::sleep(cfg.poll.min(Duration::from_secs(10))).await,
        }
    }
    f.published = publish_ok && out;
    let conclusion = f.conclusion.clone().unwrap_or_default();
    if f.published {
        say(&format!("{} is out: {}", r.tag, ar::release_url(&r.tag)));
    } else {
        say(&format!("{} isn't out: its {PUBLISH_JOB} job didn't succeed, or the release is still a draft", r.tag));
    }
    if !f.failed_jobs.is_empty() {
        say(&format!("{workflow} ended {conclusion}; jobs that didn't succeed: {}", f.failed_jobs.join(", ")));
    }
    f
}

/// What it says on fd 3 (at most 2 KiB): the tag, the commits, the
/// release's page, the workflow's run and how it ended.
pub fn result(r: Option<&Released>, f: Option<&Followed>) -> serde_json::Value {
    let mut v = serde_json::json!({ "repo": REPO });
    if let Some(r) = r {
        v["tag"] = r.tag.clone().into();
        v["version"] = r.version.clone().into();
        v["commit"] = r.commit.clone().into();
        v["main"] = r.main.clone().into();
        v["release"] = ar::release_url(&r.tag).into();
        v["script"] = r.script.clone().into();
    }
    if let Some(f) = f {
        v["published"] = f.published.into();
        v["workflow"] = serde_json::json!({
            "run": f.run, "conclusion": f.conclusion, "failed_jobs": f.failed_jobs,
        });
    }
    v
}

/// The whole apply: release, revoke the write token (nothing after needs
/// it), follow the workflow, then revoke the read token.
pub async fn apply(cfg: &Config, d: &Deploy, t: &Tokens) -> (Result<Released, Failed>, Option<Followed>) {
    let released = release(cfg, d, t.write.as_deref()).await;
    revoke_at("arugula-release", &cfg.api_base, "release", t.write.as_deref()).await;
    let out = match released {
        Ok(r) => {
            let f = follow(cfg, d.lane(), &r, t.read.as_deref()).await;
            (Ok(r), Some(f))
        }
        Err(e) => (Err(e), None),
    };
    revoke_at("arugula-release", &cfg.api_base, "read", t.read.as_deref()).await;
    out
}

/// The run's tokens, from its environment: [`TOKEN`], which writes, and
/// [`READ_TOKEN`], which follows the workflow.
#[derive(Default)]
pub struct Tokens {
    pub write: Option<String>,
    pub read: Option<String>,
}

/// Write the result on fd 3, when this process has one (a deploy's box
/// gives it its result pipe there).
#[cfg(unix)]
fn report(v: &serde_json::Value) {
    use std::{io::Write as _, os::fd::FromRawFd as _};
    // SAFETY: F_GETFD only asks whether fd 3 is open.
    if unsafe { nix::libc::fcntl(3, nix::libc::F_GETFD) } < 0 {
        return;
    }
    // SAFETY: fd 3 is open, and this process's only use of it is here.
    let mut f = unsafe { std::fs::File::from_raw_fd(3) };
    let _ = f.write_all(v.to_string().as_bytes());
}

#[cfg(not(unix))]
fn report(_: &serde_json::Value) {}

/// Close fd 3 on exec, when it's open: only this process writes the
/// result.
#[cfg(unix)]
fn keep_result_fd() {
    // SAFETY: F_GETFD and F_SETFD only read and set fd 3's close-on-exec
    // flag.
    unsafe {
        let flags = nix::libc::fcntl(3, nix::libc::F_GETFD);
        if flags >= 0 {
            nix::libc::fcntl(3, nix::libc::F_SETFD, flags | nix::libc::FD_CLOEXEC);
        }
    }
}

#[cfg(not(unix))]
fn keep_result_fd() {}

/// `arugulad _arugula-release --deploy ID --environment NAME --kind KIND
/// --ref SHA --where OWNER/NAME --plan DIGEST --bump PART --lane LANE
/// [--git-base URL] [--api-base URL] [--poll-ms N] [--follow-ms N]`, from
/// the driver's shell: the deploy as the daemon checked it, and the box's
/// own flags for tests and testnet. Exits 0 once the release is out; 2
/// when nothing was pushed; 3 when the tag was pushed and the release
/// isn't out (yet).
pub fn cli(args: &[String]) -> ! {
    // Its result pipe is its own: scripts/release, git and what they start
    // don't get it.
    keep_result_fd();
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let work = var("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from).join("arugula-release");
    let mut cfg = Config::new(work.clone());
    if let Some(p) = var("PATH") {
        cfg.path = p;
    }
    let (cfg, d) = match parse(args, var, cfg) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("arugula-release: {}", crate::work::printable(&why));
            std::process::exit(2)
        }
    };
    let tokens = Tokens { write: var(TOKEN), read: var(READ_TOKEN) };
    let code = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => {
            let (released, followed) = rt.block_on(apply(&cfg, &d, &tokens));
            report(&result(released.as_ref().ok(), followed.as_ref()));
            match (released, followed) {
                (Ok(_), Some(f)) if f.published => 0,
                (Ok(r), _) => {
                    eprintln!("arugula-release: {} was pushed, and its release isn't out: see the workflow", r.tag);
                    3
                }
                (Err(f), _) => {
                    eprintln!("arugula-release: {}", crate::work::printable(&f.why));
                    if f.pushed { 3 } else { 2 }
                }
            }
        }
        Err(e) => {
            eprintln!("arugula-release: {e}");
            2
        }
    };
    drop(tokens);
    let _ = std::fs::remove_dir_all(&work);
    std::process::exit(code)
}

#[cfg(test)]
#[path = "work_arugula_release_tests.rs"]
mod tests;
