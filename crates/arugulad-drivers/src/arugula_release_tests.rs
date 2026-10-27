//! Arugula's own release as a deploy (M221), against a fake GitHub: a bare
//! repository holding a workspace in miniature with the real
//! `scripts/release` in it (green commits, release notes on `main`, and a
//! commit CI passed that only a pull request's ref holds), and a fake API
//! (an HTTP server answering the release workflow's runs, its jobs and the
//! release, and taking the token's revoke, logging each in order). The
//! API's answers are shaped as GitHub's were for v0.59.0's release
//! (captured 2026-10-22, read-only): its run's fields, its eight jobs with
//! `template` and `extension` failed, and a draft release as 404 to anyone
//! unauthenticated.

#[cfg(unix)]
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::*;

#[cfg(unix)]
const TOKEN_V: &str = "ghs_m221releaseTOKENfortests000000000"; // gitleaks:allow
const READ_V: &str = "ghs_m221readTOKENfortests000000000000"; // gitleaks:allow

#[cfg(unix)]
fn tokens() -> Tokens {
    Tokens { write: Some(TOKEN_V.into()), read: Some(READ_V.into()) }
}

fn settings(bump: &str, lane: &str) -> Settings {
    [(BUMP.to_owned(), bump.to_owned()), (LANE.to_owned(), lane.to_owned())].into()
}

/// A command line for `sha`, with the plan made for it (or `plan`).
fn args(sha: &str, bump: &str, lane: &str, plan: Option<&str>) -> Vec<String> {
    let made = hex::encode(Sha256::digest(
        arugula_e2e::drivers::plan_text(ar::DRIVER, "release", "commit", sha, REPO, &settings(bump, lane), None)
            .as_bytes(),
    ));
    [
        "--deploy",
        "dep_1",
        "--environment",
        "release",
        "--kind",
        "commit",
        "--ref",
        sha,
        "--where",
        REPO,
        "--plan",
        plan.unwrap_or(&made),
        "--bump",
        bump,
        "--lane",
        lane,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The deploy is read from the command line, as the daemon checked it:
/// the approved plan must be this driver's plan for exactly it (its bump
/// and lane too), the `DEPLOY_*` variables must agree, and a roll back is
/// refused.
#[test]
fn the_deploy_comes_from_the_command_line_and_its_plan() {
    let sha = "a".repeat(40);
    let none = |_: &str| None;
    let cfg = || Config::new(PathBuf::from("/w"));
    let (_, d) = parse(&args(&sha, "patch", "daemon", None), none, cfg()).unwrap();
    assert_eq!((d.reference.as_str(), d.lane()), (sha.as_str(), Lane::Daemon));
    let (_, d) = parse(&args(&sha, "minor", "app", None), none, cfg()).unwrap();
    assert_eq!((d.settings[BUMP].as_str(), d.lane()), ("minor", Lane::App));
    // The plan for a patch, the command line a minor: refused.
    let patch_plan = args(&sha, "patch", "daemon", None)[11].clone();
    let e = parse(&args(&sha, "minor", "daemon", Some(&patch_plan)), none, cfg()).unwrap_err();
    assert!(e.contains("approved plan"), "{e}");
    let e = parse(&args(&"b".repeat(40), "patch", "daemon", Some(&patch_plan)), none, cfg()).unwrap_err();
    assert!(e.contains("approved plan"), "another commit: {e}");
    // Settings it doesn't take, or left out.
    assert!(parse(&args(&sha, "huge", "daemon", None), none, cfg()).unwrap_err().contains("--bump"));
    let mut short = args(&sha, "patch", "daemon", None);
    short.truncate(14);
    assert!(parse(&short, none, cfg()).unwrap_err().contains("--bump and --lane"));
    // The environment must agree, and a roll back never runs.
    let a = args(&sha, "patch", "daemon", None);
    let other = |k: &str| (k == "DEPLOY_SOURCE_REF").then(|| "c".repeat(40));
    assert!(parse(&a, other, cfg()).unwrap_err().contains("DEPLOY_SOURCE_REF"));
    let set = |k: &str| (k == "DEPLOY_SETTINGS").then(|| r#"{"bump":"major","lane":"daemon"}"#.to_owned());
    assert!(parse(&a, set, cfg()).unwrap_err().contains("DEPLOY_SETTINGS"));
    let same = |k: &str| (k == "DEPLOY_SETTINGS").then(|| r#"{"bump":"patch","lane":"daemon"}"#.to_owned());
    assert!(parse(&a, same, cfg()).is_ok());
    let back = |k: &str| (k == "DEPLOY_ROLL_BACK").then(|| r#"{"deploy":"dep_x","to":{}}"#.to_owned());
    assert!(parse(&a, back, cfg()).unwrap_err().contains("never rolls back"));
    let mut flag = a.clone();
    flag.extend(["--roll-back".into(), format!("dep_{}", "b".repeat(32))]);
    assert!(parse(&flag, none, cfg()).unwrap_err().contains("--roll-back isn't a flag"));
    // Fakes to clone from and ask: only on a box that runs the fake driver.
    for f in ["--git-base", "--api-base"] {
        let mut fakes = a.clone();
        fakes.extend([f.to_owned(), "http://127.0.0.1:9/".to_owned()]);
        assert!(parse(&fakes, none, cfg()).unwrap_err().contains("fake driver only"), "{f}");
        let test_box = |k: &str| (k == "ARUGULA_HOSTED_FAKE_DRIVER").then(|| "1".to_owned());
        assert!(parse(&fakes, test_box, cfg()).is_ok(), "{f}");
    }
}

#[test]
fn what_scripts_release_says_is_read() {
    let out = "v0.1.1: 0.1.0 -> 0.1.1 on 0123456789ab (two)\npushed v0.1.1 (aaaaaaaaaaaa) and main (bbbbbbbbbbbb)\n";
    assert_eq!(pushed_line(out), Some(("v0.1.1".into(), "bbbbbbbbbbbb".into())));
    assert_eq!(pushed_line("pushed v1 (a) and main (zz)"), None, "not hex");
    assert_eq!(pushed_line("nothing"), None);
    let sha = "d".repeat(40);
    let green = format!("1111\trefs/tags/ci/green/{sha}\n");
    assert!(super::green(&green, &sha).is_ok());
    assert!(super::green("", &sha).unwrap_err().contains("no ci/green"));
    let revoked = format!("{green}2222\trefs/tags/ci/revoked/{sha}\n");
    assert!(super::green(&revoked, &sha).unwrap_err().contains("ci/revoked"));
    assert!(super::green(&format!("1111\trefs/tags/ci/green/{sha}0\n"), &sha).is_err(), "another commit's");
    let before = "1\trefs/tags/v0.1.0\n2\trefs/tags/v0.1.0^{}\n";
    assert_eq!(new_tags(before, &format!("{before}3\trefs/tags/v0.1.1\n4\trefs/tags/v0.1.1^{{}}\n")), ["v0.1.1"]);
    assert!(new_tags(before, before).is_empty());
}

/// The fake GitHub API's state: how many times the runs are asked before
/// the workflow ends, how it ends, and the release.
struct Gh {
    base: String,
    #[cfg_attr(not(unix), allow(dead_code))]
    log: Arc<Mutex<Vec<String>>>,
    state: Arc<Mutex<GhState>>,
}

#[derive(Clone)]
struct GhState {
    /// Asks of the runs before it's completed (`usize::MAX`: never).
    asks_before_done: usize,
    asked: usize,
    conclusion: String,
    publish: String,
    draft: bool,
    /// The `Authorization` each GET came with (`""`: none).
    auths: Vec<String>,
    /// A token is refused (401), as an expired one is.
    expired: bool,
    /// The app's lane's workflow, and the tag its run is for.
    app: bool,
    tag: String,
}

impl Gh {
    #[cfg(unix)]
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn set(&self, f: impl FnOnce(&mut GhState)) {
        f(&mut self.state.lock().unwrap());
    }
}

/// The answer to one request, and what it logs.
fn answer(req: &str, st: &mut GhState) -> (String, String, String) {
    let first = req.lines().next().unwrap_or_default().to_owned();
    let path = first.split_whitespace().nth(1).unwrap_or_default().to_owned();
    let header = |name: &str| {
        req.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    };
    if first.starts_with("DELETE /installation/token ") {
        return (
            "204 No Content".into(),
            String::new(),
            format!("revoke {}", header("authorization").unwrap_or_default()),
        );
    }
    let auth = header("authorization").unwrap_or_default();
    st.auths.push(auth.clone());
    if st.expired && !auth.is_empty() {
        return ("401 Unauthorized".into(), r#"{"message":"Bad credentials"}"#.into(), "401".into());
    }
    let repo = format!("/repos/{REPO}");
    if let Some(q) = path.strip_prefix(&format!("{repo}/actions/runs?")) {
        let sha = q.split('&').find_map(|kv| kv.strip_prefix("head_sha=")).unwrap_or_default();
        st.asked += 1;
        let done = st.asked > st.asks_before_done;
        // Right after the push, GitHub lists no run yet.
        let runs = if st.asked == 1 {
            "[]".to_owned()
        } else {
            let (status, conclusion) =
                if done { ("completed", format!("\"{}\"", st.conclusion)) } else { ("in_progress", "null".into()) };
            let (name, file, id) = if st.app {
                ("app-release", "app-release", 39845854047u64)
            } else {
                ("release", "release", 39845854046)
            };
            format!(
                "[{{\"conclusion\":{conclusion},\"event\":\"push\",\"head_branch\":\"{}\",\"head_sha\":\"{sha}\",\
                 \"html_url\":\"https://github.com/{REPO}/actions/runs/{id}\",\"id\":{id},\"name\":\"{name}\",\
                 \"path\":\".github/workflows/{file}.yml\",\"status\":\"{status}\"}}]",
                st.tag
            )
        };
        let n = if runs == "[]" { 0 } else { 1 };
        return ("200 OK".into(), format!("{{\"total_count\":{n},\"workflow_runs\":{runs}}}"), "runs".into());
    }
    if path.starts_with(&format!("{repo}/actions/runs/3984585404")) && path.contains("/jobs") {
        let jobs: Vec<String> = [
            ("windows", "success"),
            ("linux", "success"),
            ("macos", "success"),
            ("template", "failure"),
            ("platform", "success"),
            ("publish", st.publish.as_str()),
            ("control", "success"),
            ("extension", "failure"),
        ]
        .iter()
        .map(|(n, c)| format!("{{\"name\":\"{n}\",\"status\":\"completed\",\"conclusion\":\"{c}\"}}"))
        .collect();
        return ("200 OK".into(), format!("{{\"total_count\":8,\"jobs\":[{}]}}", jobs.join(",")), "jobs".into());
    }
    if let Some(tag) = path.strip_prefix(&format!("{repo}/releases/tags/")) {
        if st.draft {
            return ("404 Not Found".into(), r#"{"message":"Not Found"}"#.into(), format!("release {tag}"));
        }
        let body = format!(
            "{{\"tag_name\":\"{tag}\",\"draft\":false,\"prerelease\":false,\
             \"html_url\":\"https://github.com/{REPO}/releases/tag/{tag}\",\"published_at\":\"2026-10-22T20:18:01Z\"}}"
        );
        return ("200 OK".into(), body, format!("release {tag}"));
    }
    ("404 Not Found".into(), String::new(), format!("404 {first}"))
}

async fn gh() -> Gh {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let state = Arc::new(Mutex::new(GhState {
        asks_before_done: 2,
        asked: 0,
        conclusion: "failure".into(),
        publish: "success".into(),
        draft: false,
        auths: vec![],
        expired: false,
        app: false,
        tag: "v0.1.1".into(),
    }));
    let (lg, st) = (log.clone(), state.clone());
    crate::task::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let (lg, st) = (lg.clone(), st.clone());
            crate::task::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let got = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..got]).into_owned();
                let (status, body, said) = answer(&req, &mut st.lock().unwrap());
                lg.lock().unwrap().push(said);
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    Gh { base, log, state }
}

/// A fake GitHub's repository: Arugula in miniature (the daemon at 0.1.0,
/// the app at 0.1.0, a client fixture), its real `scripts/release`; two
/// green commits, a third CI hasn't passed, then notes for 0.1.1 and the
/// app's 0.2.0 on `main`; and a green commit only a pull request's ref
/// holds.
#[cfg(unix)]
const SEED: &str = include_str!("work_arugula_release_seed.sh");

#[cfg(unix)]
struct Rig {
    root: PathBuf,
    two: String,
    three: String,
    pr: String,
    ids: AtomicUsize,
}

#[cfg(unix)]
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).env("GIT_CONFIG_GLOBAL", "/dev/null").args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[cfg(unix)]
impl Rig {
    fn new(name: &str) -> Self {
        let root = crate::hosted::scratch(&format!("arugula-release-{name}"));
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/release");
        let out = Command::new("bash").args(["-c", SEED, "seed"]).arg(&root).arg(script).output().unwrap();
        assert!(out.status.success(), "seed: {}", String::from_utf8_lossy(&out.stderr));
        let shas = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let [two, three, pr] = shas.split(' ').map(str::to_owned).collect::<Vec<_>>().try_into().unwrap();
        Rig { root, two, three, pr, ids: AtomicUsize::new(1) }
    }

    fn bare(&self) -> PathBuf {
        self.root.join("remotes/arugula-salad/arugula.git")
    }

    fn main(&self) -> String {
        git(&self.bare(), &["rev-parse", "main"])
    }

    fn tag(&self, tag: &str) -> Option<String> {
        let out = Command::new("git")
            .current_dir(self.bare())
            .args(["rev-parse", "--verify", "-q", &format!("{tag}^{{commit}}")])
            .output()
            .unwrap();
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn cfg(&self, gh: &Gh) -> Config {
        let mut c = Config::new(self.root.join("work"));
        c.git_base = format!("{}/", self.root.join("remotes").display());
        c.api_base = gh.base.clone();
        c.poll = Duration::from_millis(20);
        c.follow = Duration::from_secs(10);
        if let Ok(p) = std::env::var("PATH") {
            c.path = p;
        }
        c
    }

    /// A new deploy (its own id) of `sha`.
    fn deploy(&self, sha: &str, bump: &str, lane: &str) -> Deploy {
        let n = self.ids.fetch_add(1, Ordering::SeqCst);
        Deploy {
            id: format!("dep_{n:032x}"),
            environment: "release".into(),
            kind: "commit".into(),
            reference: sha.into(),
            place: REPO.into(),
            plan: String::new(),
            settings: settings(bump, lane),
        }
    }

    /// Every file under the root holding `needle`.
    fn holding(&self, needle: &str) -> Vec<PathBuf> {
        fn walk(d: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else { out.push(p) }
            }
        }
        let mut files = vec![];
        walk(&self.root, &mut files);
        files
            .into_iter()
            .filter(|p| std::fs::read(p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes())))
            .collect()
    }
}

/// A patch release from the newest green commit: the real script bumps
/// it, tags v0.1.1 and pushes it with main (the bump merged in), each with
/// the deploy's trailer; the token is revoked before the API is asked
/// anything; the workflow is followed to its end, and the release is out
/// though two other jobs failed (as v0.59.0's did). The same deploy again is a replay, and
/// another of the same commit finds it released. No token in any file.
// A deploy's box is Linux: git and bash.
#[cfg(unix)]
#[tokio::test]
async fn a_release_tags_pushes_main_and_follows_its_workflow() {
    let r = Rig::new("flow");
    let gh = gh().await;
    let cfg = r.cfg(&gh);
    let before = r.main();
    let d = r.deploy(&r.two, "patch", "daemon");
    let (released, followed) = apply(&cfg, &d, &tokens()).await;
    let rel = released.unwrap();
    assert_eq!((rel.tag.as_str(), rel.version.as_str()), ("v0.1.1", "0.1.1"));
    assert_eq!(r.tag("v0.1.1").as_deref(), Some(rel.commit.as_str()), "the tag is on GitHub");
    assert_eq!(git(&r.bare(), &["rev-parse", &format!("{}^", rel.commit)]), r.two, "a bump on the commit");
    assert_eq!(r.main(), rel.main);
    assert_eq!(git(&r.bare(), &["rev-parse", "main^1"]), before, "main: what it was, the bump merged in");
    assert_eq!(git(&r.bare(), &["rev-parse", "main^2"]), rel.commit);
    let trailer = format!("Arugula-Deploy: {}", d.id);
    for what in ["v0.1.1", "main", "main^2"] {
        let msg = git(&r.bare(), &["cat-file", "-p", what]);
        assert!(msg.lines().any(|l| l == trailer), "{what}: {msg}");
    }
    assert!(git(&r.bare(), &["show", "main:Cargo.toml"]).contains("version = \"0.1.1\""));
    let f = followed.unwrap();
    assert!(f.published, "{f:?}");
    assert_eq!(f.run.as_deref(), Some("https://github.com/arugula-salad/arugula/actions/runs/39845854046"));
    let failed = ["template".to_owned(), "extension".to_owned()];
    assert_eq!((f.conclusion.as_deref(), f.failed_jobs.as_slice()), (Some("failure"), failed.as_slice()));
    let log = gh.log();
    assert_eq!(log[0], format!("revoke Bearer {TOKEN_V}"), "the write token revoked first: {log:?}");
    assert!(
        log.ends_with(&["jobs".to_owned(), "release v0.1.1".to_owned(), format!("revoke Bearer {READ_V}")]),
        "the read token revoked once it's followed: {log:?}"
    );
    let auths = gh.state.lock().unwrap().auths.clone();
    assert!(auths.iter().all(|a| a.is_empty() || *a == format!("Bearer {READ_V}")), "{auths:?}");
    assert!(auths.iter().filter(|a| !a.is_empty()).count() >= 4, "followed with the read token: {auths:?}");
    assert_eq!(rel.script, git(&r.bare(), &["rev-parse", &format!("{}:scripts/release", r.two)]), "its blob id");
    let v = result(Some(&rel), Some(&f));
    assert_eq!(v["tag"], "v0.1.1");
    assert_eq!(v["release"], "https://github.com/arugula-salad/arugula/releases/tag/v0.1.1");
    assert_eq!(v["workflow"]["failed_jobs"], serde_json::json!(["template", "extension"]));
    assert_eq!(v["script"], rel.script.as_str());
    assert!(v.to_string().len() < 2048);

    // The same deploy again: a replay, refused before the script runs.
    let main = r.main();
    let (again, followed) = apply(&cfg, &d, &tokens()).await;
    let e = again.unwrap_err();
    assert!(!e.pushed && e.why.contains("released already"), "{e:?}");
    assert!(followed.is_none());
    // Another deploy of the same commit: the script finds it released.
    let (again, _) = apply(&cfg, &r.deploy(&r.two, "patch", "daemon"), &tokens()).await;
    let e = again.unwrap_err();
    assert!(!e.pushed && e.why.contains("already released in v0.1.1"), "{e:?}");
    assert_eq!(r.main(), main, "nothing pushed");

    assert!(r.holding(TOKEN_V).is_empty(), "no token in any file");
}

/// The app's lane: a minor bump of crates/desktop, tagged app-v0.2.0, and
/// its own workflow followed.
// A deploy's box is Linux: git and bash.
#[cfg(unix)]
#[tokio::test]
async fn the_apps_lane_tags_app_v() {
    let r = Rig::new("app");
    let gh = gh().await;
    gh.set(|s| {
        s.app = true;
        s.tag = "app-v0.2.0".into();
    });
    let (released, followed) = apply(&r.cfg(&gh), &r.deploy(&r.two, "minor", "app"), &tokens()).await;
    let rel = released.unwrap();
    assert_eq!(rel.tag, "app-v0.2.0");
    assert!(git(&r.bare(), &["show", "main:crates/desktop/Cargo.toml"]).contains("version = \"0.2.0\""));
    assert!(git(&r.bare(), &["show", "main:Cargo.toml"]).contains("version = \"0.1.0\""), "the daemon stays");
    let f = followed.unwrap();
    assert_eq!(f.run.as_deref(), Some("https://github.com/arugula-salad/arugula/actions/runs/39845854047"), "{f:?}");
    assert!(f.published);
}

/// Nothing is pushed for a commit CI hasn't passed, one CI passed that
/// isn't on main (a pull request's), a release whose notes aren't on main,
/// another repository, or without the token.
// A deploy's box is Linux: git and bash.
#[cfg(unix)]
#[tokio::test]
async fn what_it_refuses_pushes_nothing() {
    let r = Rig::new("refuse");
    let gh = gh().await;
    let cfg = r.cfg(&gh);
    let main = r.main();
    for (d, why) in [
        (r.deploy(&r.three, "patch", "daemon"), "has no ci/green tag"),
        (r.deploy(&r.pr, "patch", "daemon"), "isn't on arugula-salad/arugula's main"),
        (r.deploy(&r.two, "minor", "daemon"), "main has no docs/releases/0.2.0.md"),
        (Deploy { place: "acme/arugula".into(), ..r.deploy(&r.two, "patch", "daemon") }, "arugula-salad/arugula only"),
    ] {
        let (got, followed) = apply(&cfg, &d, &tokens()).await;
        let e = got.unwrap_err();
        assert!(!e.pushed && e.why.contains(why), "{why}: {e:?}");
        assert!(followed.is_none());
    }
    let (got, _) = apply(&cfg, &r.deploy(&r.two, "patch", "daemon"), &Tokens::default()).await;
    assert!(got.unwrap_err().why.contains("no RELEASE_TOKEN"));
    assert_eq!(r.main(), main, "nothing pushed");
    assert!(r.tag("v0.1.1").is_none() && r.tag("v0.2.0").is_none());
    assert!(
        gh.log().iter().all(|l| l.starts_with("revoke ")),
        "the token is revoked each time, and nothing else is asked: {:?}",
        gh.log()
    );
}

fn released() -> Released {
    Released {
        tag: "v0.1.1".into(),
        version: "0.1.1".into(),
        commit: "e".repeat(40),
        main: "f".repeat(40),
        script: "a".repeat(40),
    }
}

/// A workflow whose publish job failed, a release still a draft, and a
/// workflow that doesn't end in time: none is out.
#[tokio::test]
async fn a_release_that_isnt_published_or_doesnt_end_isnt_out() {
    let gh = gh().await;
    let mut cfg = Config::new(PathBuf::from("/nonexistent"));
    cfg.api_base = gh.base.clone();
    cfg.poll = Duration::from_millis(10);
    cfg.follow = Duration::from_secs(10);
    gh.set(|s| s.publish = "failure".into());
    let f = follow(&cfg, Lane::Daemon, &released(), Some(READ_V)).await;
    assert!(!f.published && f.failed_jobs.contains(&"publish".to_owned()), "{f:?}");
    gh.set(|s| {
        s.publish = "success".into();
        s.draft = true;
        s.asked = 0;
    });
    assert!(!follow(&cfg, Lane::Daemon, &released(), Some(READ_V)).await.published, "a draft (404 to anyone)");
    gh.set(|s| {
        s.draft = false;
        s.asked = 0;
        s.asks_before_done = usize::MAX;
    });
    // An expired read token: asked again as anyone.
    gh.set(|s| {
        s.asked = 0;
        s.asks_before_done = 1;
        s.expired = true;
        s.auths.clear();
    });
    assert!(follow(&cfg, Lane::Daemon, &released(), Some(READ_V)).await.published, "as anyone");
    let auths = gh.state.lock().unwrap().auths.clone();
    assert_eq!(auths[..2], [format!("Bearer {READ_V}"), String::new()], "{auths:?}");
    assert!(auths[2..].iter().all(String::is_empty), "then as anyone: {auths:?}");
    gh.set(|s| {
        s.asked = 0;
        s.asks_before_done = usize::MAX;
        s.expired = false;
    });
    cfg.follow = Duration::from_millis(200);
    let f = follow(&cfg, Lane::Daemon, &released(), Some(READ_V)).await;
    assert!(!f.published && f.conclusion.is_none(), "{f:?}");
    assert_eq!(
        f.run.as_deref(),
        Some("https://github.com/arugula-salad/arugula/actions/runs/39845854046"),
        "found, not ended"
    );
}

/// A `scripts/release` that runs past its time is stopped, its process
/// group with it, and the tags say whether it pushed one.
// A deploy's box is Linux: git, bash and process groups.
#[cfg(unix)]
#[tokio::test]
async fn a_script_past_its_time_is_stopped_and_its_tags_read() {
    let r = Rig::new("timeout");
    let gh = gh().await;
    let mut cfg = r.cfg(&gh);
    cfg.script_timeout = Duration::from_millis(1);
    let main = r.main();
    let (got, followed) = apply(&cfg, &r.deploy(&r.two, "patch", "daemon"), &tokens()).await;
    let e = got.unwrap_err();
    assert!(!e.pushed && e.why.contains("was stopped: it pushed no tag"), "{e:?}");
    assert!(followed.is_none());
    assert_eq!(r.main(), main);
    assert!(r.tag("v0.1.1").is_none());
}
