//! The apps-platform driver's apply (M222), against a fake GitHub (bare
//! repositories in a directory, fetched by sha and partially as GitHub
//! allows, with a commit only a pull request's ref holds, as a fork's) and
//! a fake app and API (an HTTP server that answers the app's health path
//! as told, and takes token revokes, logging both in order).

#[cfg(unix)]
use std::{
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use std::{path::PathBuf, time::Duration};

#[cfg(unix)]
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::*;

const WRITE: &str = "ghs_writeTOKENfortests0000000000000000"; // gitleaks:allow
const READ: &str = "ghs_readTOKENfortests00000000000000000"; // gitleaks:allow
/// The image the app's first commit pins (its build's own commit).
const IMAGE_A: &str = "1234567890abcdef1234567890abcdef12345678";

/// planter's pointer for another app, and one for this app with a custom
/// domain asked for and its branch tracked.
const OTHER: &str = "apiVersion: source.toolkit.fluxcd.io/v1\nkind: GitRepository\nmetadata:\n  name: other\n";
const PLANTERS: &str = "# beds-smoke in prod. Written by planter.
apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: beds-smoke
  namespace: planter-apps-system
spec:
  interval: 5m
  url: https://github.com/arugula-salad/beds-smoke
  provider: github
  secretRef:
    name: arugula-github-app
  ref:
    branch: main
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: beds-smoke
  namespace: planter-apps-system
  annotations: {platform.arugula.io/domains: beds.example.com}
spec:
  interval: 10m
  path: ./k8s
  prune: true
  sourceRef:
    kind: GitRepository
    name: beds-smoke
";

#[cfg(unix)]
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[cfg(unix)]
/// A fake GitHub: `<root>/remotes/<owner>/<name>.git`, bare.
struct Fake {
    root: PathBuf,
    remotes: PathBuf,
    /// The app's commits on its default branch: one, then another.
    a: String,
    b: String,
    /// A commit only a pull request's ref holds (a fork's).
    fork: String,
    ids: AtomicUsize,
}

#[cfg(unix)]
impl Fake {
    fn new(name: &str) -> Self {
        let root = crate::hosted::scratch(&format!("apps-platform-{name}"));
        let remotes = root.join("remotes");
        let bare = |repo: &str| {
            let p = remotes.join(format!("{repo}.git"));
            std::fs::create_dir_all(&p).unwrap();
            git(&p, &["init", "--quiet", "--bare", "--initial-branch=main"]);
            // As GitHub: any commit fetched by its sha, and partially.
            git(&p, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
            git(&p, &["config", "uploadpack.allowFilter", "true"]);
            p
        };
        bare("arugula-salad/planter-apps");
        bare("arugula-salad/beds-smoke");
        let f =
            Fake { root, remotes, a: String::new(), b: String::new(), fork: String::new(), ids: AtomicUsize::new(1) };
        f.seed("arugula-salad/planter-apps", &[("apps/.gitkeep", ""), ("apps/other.yaml", OTHER)], "main");
        let app = "name: beds-smoke\nhealth: /healthz # what it answers\nruntime: k8s\n";
        let kust = format!("images:\n  - name: ghcr.io/arugula-salad/beds-smoke\n    newTag: sha-{IMAGE_A}\n");
        let a = f.seed("arugula-salad/beds-smoke", &[("app.yaml", app), ("k8s/kustomization.yaml", &kust)], "main");
        let b = f.seed("arugula-salad/beds-smoke", &[("k8s/kustomization.yaml", "b")], "main");
        let fork = f.seed("arugula-salad/beds-smoke", &[("k8s/kustomization.yaml", "fork")], "refs/pull/1/head");
        Fake { a, b, fork, ..f }
    }

    /// Commit `files` to `repo` and push it to `to`: its sha.
    fn seed(&self, repo: &str, files: &[(&str, &str)], to: &str) -> String {
        let w = self.root.join("seed").join(repo);
        if !w.exists() {
            std::fs::create_dir_all(&w).unwrap();
            git(&w, &["init", "--quiet", "--initial-branch=main"]);
            git(&w, &["remote", "add", "origin", &self.remotes.join(format!("{repo}.git")).to_string_lossy()]);
        }
        for (f, body) in files {
            let p = w.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        git(&w, &["add", "-A"]);
        git(&w, &["commit", "--quiet", "-m", "seed"]);
        let sha = git(&w, &["rev-parse", "HEAD"]);
        git(&w, &["push", "--quiet", "origin", &format!("HEAD:{to}")]);
        if to != "main" {
            git(&w, &["reset", "--quiet", "--hard", "HEAD^"]);
        }
        sha
    }

    fn list(&self) -> PathBuf {
        self.remotes.join("arugula-salad/planter-apps.git")
    }

    fn head(&self) -> String {
        git(&self.list(), &["rev-parse", "main"])
    }

    fn pointer(&self) -> Option<String> {
        let out =
            Command::new("git").current_dir(self.list()).args(["show", "main:apps/beds-smoke.yaml"]).output().unwrap();
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Put planter's pointer for the app in the list, as planter would.
    fn planter_pointer(&self) {
        self.seed("arugula-salad/planter-apps", &[("apps/beds-smoke.yaml", PLANTERS)], "main");
    }

    fn cfg(&self, h: &Health) -> Config {
        let mut c = Config::new(self.root.join("work"));
        c.git_base = format!("{}/", self.remotes.display());
        c.health_base = Some(h.base.clone());
        c.api_base = h.base.clone();
        c.settle = Duration::ZERO;
        c.poll = Duration::from_millis(20);
        c.timeout = Duration::from_secs(5);
        if let Ok(p) = std::env::var("PATH") {
            c.path = p;
        }
        c
    }

    /// A new deploy (its own id) of commit `sha`.
    fn deploy(&self, sha: &str) -> Deploy {
        let n = self.ids.fetch_add(1, Ordering::SeqCst);
        Deploy {
            id: format!("dep_{n:032x}"),
            environment: "production".into(),
            kind: "commit".into(),
            reference: sha.into(),
            place: "arugula-salad/beds-smoke".into(),
            plan: String::new(),
            roll_back: None,
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

fn tokens() -> Tokens {
    Tokens { write: Some(WRITE.into()), read: Some(READ.into()) }
}

#[cfg(unix)]
/// A fake app and GitHub API: answers 503 to its first `fails` requests to
/// `/healthz`, then 200 (with `revision` in a JSON body when one is set);
/// takes `DELETE /installation/token`. Logs `health` and `revoke <its
/// Authorization header>` in order.
struct Health {
    base: String,
    log: Arc<Mutex<Vec<String>>>,
    revision: Arc<Mutex<Option<String>>>,
}

#[cfg(unix)]
impl Health {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn healths(&self) -> usize {
        self.log().iter().filter(|l| *l == "health").count()
    }
}

#[cfg(unix)]
async fn health(fails: usize) -> Health {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let revision = Arc::new(Mutex::new(None::<String>));
    let asked = Arc::new(AtomicUsize::new(0));
    let (lg, rv) = (log.clone(), revision.clone());
    crate::task::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let (lg, rv, asked) = (lg.clone(), rv.clone(), asked.clone());
            crate::task::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let got = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..got]).into_owned();
                let (status, body) = if req.starts_with("DELETE /installation/token ") {
                    let auth = req
                        .lines()
                        .find_map(|l| l.strip_prefix("authorization: ").or_else(|| l.strip_prefix("Authorization: ")))
                        .unwrap_or_default();
                    lg.lock().unwrap().push(format!("revoke {auth}"));
                    ("204 No Content", String::new())
                } else if req.starts_with("GET /healthz ") {
                    lg.lock().unwrap().push("health".into());
                    if asked.fetch_add(1, Ordering::SeqCst) < fails {
                        ("503 Service Unavailable", String::new())
                    } else {
                        let rev = rv.lock().unwrap().clone();
                        (
                            "200 OK",
                            rev.map(|r| format!("{{\"status\":\"healthy\",\"revision\":\"{r}\"}}")).unwrap_or_default(),
                        )
                    }
                } else {
                    ("404 Not Found", String::new())
                };
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    Health { base, log, revision }
}

/// The deploy is read from the command line, as the daemon checked it:
/// the approved plan must be this driver's plan for exactly it, and the
/// `DEPLOY_*` variables, where set, must say the same.
#[test]
fn the_deploy_comes_from_the_command_line_and_its_plan() {
    let sha = "a".repeat(40);
    let plan = arugula_e2e::deploy::plan_digest("apps-platform", "production", "commit", &sha, "arugula-salad/app");
    let args = |r: &str, p: &str| -> Vec<String> {
        [
            "--deploy",
            "dep_1",
            "--environment",
            "production",
            "--kind",
            "commit",
            "--ref",
            r,
            "--where",
            "arugula-salad/app",
            "--plan",
            p,
            "--poll-ms",
            "5",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let none = |_: &str| None;
    let (cfg, d) = parse(&args(&sha, &plan), none, Config::new("/w".into())).unwrap();
    assert_eq!(
        (d.reference.as_str(), d.place.as_str(), cfg.poll),
        (sha.as_str(), "arugula-salad/app", Duration::from_millis(5))
    );
    // Another commit than the approved plan's.
    let other = "b".repeat(40);
    let e = parse(&args(&other, &plan), none, Config::new("/w".into())).unwrap_err();
    assert!(e.contains("approved plan"), "{e}");
    // The environment says another: refused, whichever it'd pick.
    let e = parse(&args(&sha, &plan), |k| (k == "DEPLOY_SOURCE_REF").then(|| other.clone()), Config::new("/w".into()))
        .unwrap_err();
    assert!(e.contains("DEPLOY_SOURCE_REF"), "{e}");
    let same = |k: &str| (k == "DEPLOY_SOURCE_REF").then(|| sha.clone());
    assert!(parse(&args(&sha, &plan), same, Config::new("/w".into())).is_ok());
    let mut missing = args(&sha, &plan);
    missing.drain(6..8);
    assert!(parse(&missing, none, Config::new("/w".into())).unwrap_err().contains("DEPLOY_SOURCE_REF"));
    assert!(parse(&["--else".into(), "x".into()], none, Config::new("/w".into())).is_err());

    // A roll back's plan (M220's) names the deploy it goes back to: the
    // command line must too, and the environment can't say another.
    let to = format!("dep_{}", "b".repeat(32));
    let r = arugula_e2e::drivers::RollBack { deploy: to.clone(), to: Default::default() };
    let text = arugula_e2e::drivers::plan_text(
        "apps-platform",
        "production",
        "commit",
        &sha,
        "arugula-salad/app",
        &Default::default(),
        Some(&r),
    );
    let back = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(text.as_bytes()));
    let mut with = args(&sha, &back);
    with.extend(["--roll-back".into(), to.clone()]);
    let (_, d) = parse(&with, none, Config::new("/w".into())).unwrap();
    assert_eq!(d.roll_back.as_deref(), Some(to.as_str()));
    assert!(parse(&args(&sha, &back), none, Config::new("/w".into())).unwrap_err().contains("approved plan"));
    let mut with_plain = args(&sha, &plan);
    with_plain.extend(["--roll-back".into(), to.clone()]);
    assert!(parse(&with_plain, none, Config::new("/w".into())).unwrap_err().contains("approved plan"));
    let json = serde_json::to_string(&r).unwrap();
    assert!(parse(&with, |k| (k == "DEPLOY_ROLL_BACK").then(|| json.clone()), Config::new("/w".into())).is_ok());
    let other =
        serde_json::to_string(&arugula_e2e::drivers::RollBack { deploy: format!("dep_{}", "c".repeat(32)), ..r })
            .unwrap();
    let e = parse(&with, |k| (k == "DEPLOY_ROLL_BACK").then(|| other.clone()), Config::new("/w".into())).unwrap_err();
    assert!(e.contains("DEPLOY_ROLL_BACK"), "{e}");
    with.splice(with.len() - 1.., ["dep_1".to_owned()]);
    assert!(parse(&with, none, Config::new("/w".into())).unwrap_err().contains("--roll-back"));
}

#[test]
fn a_revision_is_read_from_the_answer() {
    use reqwest::header::{HeaderMap, HeaderValue};
    let none = HeaderMap::new();
    assert_eq!(reported(&none, br#"{"status":"healthy","revision":"ABCDEF1"}"#).as_deref(), Some("abcdef1"));
    assert_eq!(reported(&none, br#"{"version":"sha-abcdef12"}"#).as_deref(), Some("abcdef12"));
    assert_eq!(reported(&none, b"ok"), None);
    let mut h = HeaderMap::new();
    h.insert("x-revision", HeaderValue::from_static("1234567"));
    assert_eq!(reported(&h, b"").as_deref(), Some("1234567"));
    let revs = vec![IMAGE_A.to_owned()];
    assert!(is_one_of(&IMAGE_A[..7], &revs) && is_one_of(IMAGE_A, &revs));
    assert!(!is_one_of(&IMAGE_A[..6], &revs), "too short to tell");
    assert!(!is_one_of("ffffffff", &revs));
    assert_eq!(
        pinned_images("images:\n  - name: x\n    newTag: sha-ABCDEF12 # pinned\n    newTag: v1\n"),
        ["abcdef12"]
    );
}

#[test]
fn a_pin_changes_the_ref_and_nothing_else() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let fresh = render("beds-smoke", "arugula-salad/beds-smoke", &a);
    let (pinned, path) = pin(&fresh, "beds-smoke", "arugula-salad/beds-smoke", &b).unwrap();
    assert_eq!(pinned, render("beds-smoke", "arugula-salad/beds-smoke", &b), "the render pins the same way");
    assert_eq!(path, "./k8s");
    // planter's: branch to commit, the annotation and the rest as they were.
    let (p, _) = pin(PLANTERS, "beds-smoke", "arugula-salad/beds-smoke", &a).unwrap();
    assert_eq!(p, PLANTERS.replace("  ref:\n    branch: main\n", &format!("  ref:\n    commit: {a}\n")));
    // A flow map, as Jake's own list writes.
    let flow = PLANTERS.replace("  ref:\n    branch: main\n", "  ref: {branch: main}\n");
    assert_eq!(pin(&flow, "beds-smoke", "arugula-salad/beds-smoke", &a).unwrap().0, p);
    for (text, why) in [
        (PLANTERS.replace("arugula-salad/beds-smoke\n", "arugula-salad/shipped\n"), "clones"),
        (PLANTERS.replace("  name: beds-smoke\n  namespace", "  name: else\n  namespace"), "doesn't name"),
        (PLANTERS.replace("  ref:\n    branch: main\n", ""), "no one ref"),
        (OTHER.to_owned(), "isn't one"),
    ] {
        let e = pin(&text, "beds-smoke", "arugula-salad/beds-smoke", &a).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
}

#[test]
fn app_yaml_is_read_beside_what_the_pointer_applies() {
    assert_eq!(app_yaml_path("./k8s").unwrap(), "app.yaml");
    assert_eq!(app_yaml_path("./redone-hello/k8s").unwrap(), "redone-hello/app.yaml");
    for bad in ["./", "../k8s", "/k8s", "./a//k8s"] {
        assert!(app_yaml_path(bad).is_err(), "{bad}");
    }
    let a = app_yaml("# x\nname: guestbook\nneeds:\n  name: nested\nhealth: \"/up\" # it says\n");
    assert_eq!((a.name.as_deref(), a.health.as_str(), a.runtime.as_str()), (Some("guestbook"), "/up", "k8s"));
    assert_eq!(app_yaml("name: x\nruntime: loam\n").runtime, "loam");
    assert_eq!(app_yaml("name: x\n").health, "/health");
}

/// The tokens reach git as headers for their own repository only, in its
/// environment: never an argument.
#[test]
fn a_token_goes_to_git_in_its_environment_for_its_own_repo() {
    let cfg = Config::new(PathBuf::from("/w"));
    let g = Git::new(&cfg, "arugula-salad/planter-apps", "arugula-salad/app", &tokens());
    let env: std::collections::HashMap<&str, &str> = g.env().iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let header = |t: &str| {
        format!(
            "Authorization: Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"))
        )
    };
    let pairs: Vec<(&str, &str)> = (0..env["GIT_CONFIG_COUNT"].parse::<usize>().unwrap())
        .map(|i| (env[format!("GIT_CONFIG_KEY_{i}").as_str()], env[format!("GIT_CONFIG_VALUE_{i}").as_str()]))
        .collect();
    let (wh, rh) = (header(WRITE), header(READ));
    assert!(pairs.contains(&("http.https://github.com/arugula-salad/planter-apps.git.extraHeader", wh.as_str())));
    assert!(pairs.contains(&("http.https://github.com/arugula-salad/app.git.extraHeader", rh.as_str())));
    assert!(pairs.contains(&("credential.helper", "")), "no helper keeps it");
    assert_eq!(env["GIT_CONFIG_GLOBAL"], "/dev/null");
    assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
    assert!(!g.env().iter().any(|(k, v)| v.contains(WRITE) || k.contains(WRITE)), "only as a header");
}

/// A deploy commits exactly the pointer, pinned, and waits for the app;
/// the next pins the next commit; a roll back is a revert commit of it;
/// deploying what's pinned commits nothing. Both tokens are revoked right
/// after the push, before the app is asked anything. No token is in any
/// file.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn deploys_pin_the_pointer_wait_for_the_app_and_roll_back_by_revert() {
    let f = Fake::new("flow");
    let h = health(2).await;
    let cfg = f.cfg(&h);
    let before = f.head();

    // The first deploy: a new pointer, and the app answers after two 503s.
    let first = f.deploy(&f.a);
    let one = apply(&cfg, &first, &tokens()).await.unwrap();
    let Applied::Pinned(c1) = one else { panic!("{one:?}") };
    assert_eq!(f.head(), c1);
    assert_eq!(git(&f.list(), &["rev-parse", "main^"]), before, "one commit");
    assert_eq!(git(&f.list(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]), "apps/beds-smoke.yaml");
    assert_eq!(f.pointer().unwrap(), render("beds-smoke", "arugula-salad/beds-smoke", &f.a));
    let msg = git(&f.list(), &["log", "-1", "--format=%B", "main"]);
    assert!(msg.contains(TRAILER) && msg.contains(&format!("Arugula-Deploy: {}", first.id)), "{msg}");
    assert!(h.healths() >= 5, "it waited: two 503s, then three 200s");
    let log = h.log();
    assert_eq!(
        &log[..2],
        [format!("revoke Bearer {WRITE}"), format!("revoke Bearer {READ}")],
        "revoked first: {log:?}"
    );

    // The next commit.
    let two = apply(&cfg, &f.deploy(&f.b), &tokens()).await.unwrap();
    let Applied::Pinned(c2) = two else { panic!("{two:?}") };
    assert_eq!(f.pointer().unwrap(), render("beds-smoke", "arugula-salad/beds-smoke", &f.b));
    assert_eq!(git(&f.list(), &["rev-parse", "main^"]), c1);

    // Rolling back to the first: a revert commit of the second.
    let back = apply(&cfg, &f.deploy(&f.a), &tokens()).await.unwrap();
    let Applied::Reverted { commit, of } = back else { panic!("{back:?}") };
    assert_eq!((f.head(), of.clone()), (commit, c2.clone()));
    assert_eq!(f.pointer().unwrap(), render("beds-smoke", "arugula-salad/beds-smoke", &f.a));
    let msg = git(&f.list(), &["log", "-1", "--format=%B", "main"]);
    assert!(msg.starts_with("Roll back beds-smoke to") && msg.contains(&format!("This reverts commit {c2}")), "{msg}");
    assert_eq!(git(&f.list(), &["diff", "main", &c1, "--", "apps/beds-smoke.yaml"]), "", "as the first left it");

    // What's pinned already: nothing to commit, and it still checks.
    let head = f.head();
    assert_eq!(apply(&cfg, &f.deploy(&f.a), &tokens()).await.unwrap(), Applied::Unchanged);
    assert_eq!(f.head(), head);

    // The first deploy sent again (a replay): refused, nothing pushed.
    let e = apply(&cfg, &first, &tokens()).await.unwrap_err();
    assert!(!e.pushed && e.why.contains("applied to arugula-salad/planter-apps already"), "{e:?}");
    assert_eq!(f.head(), head);

    assert!(f.holding(WRITE).is_empty() && f.holding(READ).is_empty(), "no token in any file");
}

/// An environment's own fine-grained token isn't revoked by the box: it
/// isn't an installation's.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn an_environments_own_token_is_left_alone() {
    let f = Fake::new("pat");
    let h = health(0).await;
    let pat = "github_pat_fortests000000000000000000"; // gitleaks:allow
    apply(&f.cfg(&h), &f.deploy(&f.a), &Tokens { write: Some(pat.into()), read: Some(READ.into()) }).await.unwrap();
    let log = h.log();
    assert!(log.contains(&format!("revoke Bearer {READ}")) && !log.iter().any(|l| l.contains(pat)), "{log:?}");
}

/// The app's answer holds it to its revision when it reports one: the
/// commit's pinned image is ready; another revision (the old version still
/// serving) never is.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn an_app_that_reports_its_revision_is_held_to_it() {
    let f = Fake::new("revision");
    let h = health(0).await;
    *h.revision.lock().unwrap() = Some(IMAGE_A[..12].to_owned());
    apply(&f.cfg(&h), &f.deploy(&f.a), &tokens()).await.unwrap();
    // The next commit pins no image: only its own sha would do, and the app
    // still says the first's.
    let mut cfg = f.cfg(&h);
    cfg.timeout = Duration::from_millis(300);
    let e = apply(&cfg, &f.deploy(&f.b), &tokens()).await.unwrap_err();
    assert!(e.pushed && e.why.contains("wasn't ready") && e.why.contains("not this deploy's"), "{e:?}");
    *h.revision.lock().unwrap() = Some(f.b.clone());
    let e = apply(&cfg, &f.deploy(&f.b), &tokens()).await.unwrap();
    assert_eq!(e, Applied::Unchanged, "pinned already, and now it reports this commit");
}

/// A pointer planter wrote keeps everything but its ref.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn a_planter_pointer_keeps_its_domains() {
    let f = Fake::new("planter");
    f.planter_pointer();
    let h = health(0).await;
    apply(&f.cfg(&h), &f.deploy(&f.b), &tokens()).await.unwrap();
    let p = f.pointer().unwrap();
    assert!(p.contains("platform.arugula.io/domains: beds.example.com"), "{p}");
    assert!(p.contains(&format!("  ref:\n    commit: {}\n", f.b)) && !p.contains("branch: main"), "{p}");
    // Before the driver's first commit was planter's branch, not this
    // commit: going to another commit is a pin, forward.
    apply(&f.cfg(&h), &f.deploy(&f.a), &tokens()).await.unwrap();
    assert!(git(&f.list(), &["log", "-1", "--format=%B", "main"]).starts_with("Deploy beds-smoke"));
    apply(&f.cfg(&h), &f.deploy(&f.b), &tokens()).await.unwrap();
    assert!(git(&f.list(), &["log", "-1", "--format=%B", "main"]).starts_with("Roll back"), "back again: a revert");
}

/// Nothing is pushed for a commit that isn't the app's, a fork's commit
/// (served by its sha, not on the default branch), an app.yaml over the
/// size bound, or a source this driver doesn't deploy; an app that never
/// answers fails the run after the push.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn what_it_refuses_pushes_nothing_and_an_app_never_ready_fails() {
    let f = Fake::new("refuse");
    let h = health(usize::MAX).await;
    let mut cfg = f.cfg(&h);
    cfg.timeout = Duration::from_millis(300);
    let head = f.head();
    let gone = "f".repeat(40);
    let e = apply(&cfg, &f.deploy(&gone), &tokens()).await.unwrap_err();
    assert!(!e.pushed && e.why.contains("isn't on arugula-salad/beds-smoke's default branch"), "{e:?}");
    let e = apply(&cfg, &f.deploy(&f.fork), &tokens()).await.unwrap_err();
    assert!(!e.pushed && e.why.contains("isn't on arugula-salad/beds-smoke's default branch"), "a fork's: {e:?}");
    let e = apply(&cfg, &Deploy { kind: "artifact".into(), ..f.deploy(&f.a) }, &tokens()).await.unwrap_err();
    assert!(!e.pushed && e.why.contains("not an artifact"), "{e:?}");
    let e = apply(&cfg, &Deploy { place: "acme/beds-smoke".into(), ..f.deploy(&f.a) }, &tokens()).await.unwrap_err();
    assert!(!e.pushed, "{e:?}");
    let e = apply(&cfg, &Deploy { place: "arugula-salad/planter-apps".into(), ..f.deploy(&f.a) }, &tokens())
        .await
        .unwrap_err();
    assert!(!e.pushed, "the list itself: {e:?}");
    let small = Config { max_file_bytes: 10, ..cfg.clone() };
    let e = apply(&small, &f.deploy(&f.a), &tokens()).await.unwrap_err();
    assert!(!e.pushed && e.why.contains("app.yaml at") && e.why.contains("over"), "{e:?}");
    assert_eq!(f.head(), head, "nothing pushed");

    // Pushed, never ready: the run fails, saying so.
    let e = apply(&cfg, &f.deploy(&f.a), &tokens()).await.unwrap_err();
    assert!(e.pushed && e.why.contains("wasn't ready"), "{e:?}");
    assert_ne!(f.head(), head);
}

/// A new app can't take a name another of the list's objects holds, or
/// one that overlaps another app's by a dash-prefix.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn a_new_app_doesnt_collide_with_the_lists_objects() {
    for (file, body, why) in [
        ("apps/beds.yaml", OTHER.replace("other", "beds"), "overlaps the app beds"),
        ("clusters/extra.yaml", OTHER.replace("other", "beds-smoke"), "already holds a Flux object named beds-smoke"),
    ] {
        let f = Fake::new(&format!("collide-{}", why.len()));
        f.seed("arugula-salad/planter-apps", &[(file, &body)], "main");
        let h = health(0).await;
        let head = f.head();
        let e = apply(&f.cfg(&h), &f.deploy(&f.a), &tokens()).await.unwrap_err();
        assert!(!e.pushed && e.why.contains(why), "{e:?}");
        assert_eq!(f.head(), head);
    }
}

// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn an_app_yaml_for_another_app_or_loam_is_refused() {
    for (yaml, why) in
        [("name: else\n", "names else, not beds-smoke"), ("name: beds-smoke\nruntime: loam\n", "runtime loam")]
    {
        let f = Fake::new(&format!("yaml-{}", why.len()));
        let sha = f.seed("arugula-salad/beds-smoke", &[("app.yaml", yaml)], "main");
        let h = health(0).await;
        let head = f.head();
        let e = apply(&f.cfg(&h), &f.deploy(&sha), &tokens()).await.unwrap_err();
        assert!(!e.pushed && e.why.contains(why), "{e:?}");
        assert_eq!(f.head(), head);
    }
}

/// A push that loses a race starts again from the list's new head.
// A deploy's box is Linux: git with its PATH, hooks and a shell.
#[cfg(unix)]
#[tokio::test]
async fn a_push_that_loses_a_race_is_made_again_on_the_new_head() {
    let f = Fake::new("race");
    let h = health(0).await;
    let cfg = f.cfg(&h);
    // A hook that, once, lets someone else push first.
    let hook = f.list().join("hooks/pre-receive");
    let flag = f.root.join("raced");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nif [ ! -e '{0}' ]; then touch '{0}'; echo 'someone else first' >&2; exit 1; fi\n",
            flag.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let applied = apply(&cfg, &f.deploy(&f.a), &tokens()).await.unwrap();
    assert!(matches!(applied, Applied::Pinned(_)), "{applied:?}");
    assert!(flag.exists(), "the first push was refused");
    assert_eq!(f.pointer().unwrap(), render("beds-smoke", "arugula-salad/beds-smoke", &f.a));
}
