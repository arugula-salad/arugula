//! planter's drivers' apply (M233), against a fake GitHub built from what
//! planter really wrote (`tests/planter-captured/`: planter-apps' pointers,
//! shipped's redone-hello with planter's ship commit) and planter's own
//! renderer output (`crates/e2e/src/planter/fixtures/`), plus a fake API
//! (pull requests, revokes) and a fake app. The renderer needs node, as a box
//! has: a test that renders is skipped without it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde_json::{Value, json};

use super::*;

const PROJECT: &str = "prj_m233test";
/// A repository of arugula-salad's that GitHub shows anyone.
const TAKEN: &str = "a-public-repo";
const SHIPPED_T: &str = "ghs_shippedTOKENfortests000000000000"; // gitleaks:allow
const POINTERS_T: &str = "ghs_pointersTOKENfortests00000000000"; // gitleaks:allow

fn node() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

macro_rules! needs_node {
    () => {
        if !node() {
            eprintln!("skipped: no node here");
            return;
        }
    };
}

fn captured(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/planter-captured").join(rel)
}

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../e2e/src/planter/fixtures/{name}.json"));
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

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

/// A fake GitHub: bare repositories under `remotes/`, seeded from what
/// planter wrote.
struct Fake {
    root: PathBuf,
    remotes: PathBuf,
}

impl Fake {
    fn new(name: &str) -> Self {
        let root = crate::hosted::scratch(&format!("planter-{name}"));
        let remotes = root.join("remotes");
        let f = Fake { root, remotes };
        for repo in ["arugula-salad/planter-apps", "arugula-salad/shipped", "arugula-salad/ocarina"] {
            let p = f.remotes.join(format!("{repo}.git"));
            std::fs::create_dir_all(&p).unwrap();
            git(&p, &["init", "--quiet", "--bare", "--initial-branch=main"]);
            git(&p, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
            git(&p, &["config", "uploadpack.allowFilter", "true"]);
        }
        // planter-apps as it is: ocarina's and redone-hello's pointers.
        let ocarina = std::fs::read_to_string(captured("planter-apps/ocarina.yaml")).unwrap();
        let hello = std::fs::read_to_string(captured("planter-apps/redone-hello.yaml")).unwrap();
        f.seed(
            "arugula-salad/planter-apps",
            &[("apps/.gitkeep", ""), ("apps/ocarina.yaml", &ocarina), ("apps/redone-hello.yaml", &hello)],
            "planter-apps: the pointers planter writes",
        );
        // shipped's redone-hello as planter's ship left it.
        let mut files = vec![];
        for rel in ["app.yaml", "src/Dockerfile", "src/server.js", "k8s/kustomization.yaml"] {
            files.push((
                format!("redone-hello/{rel}"),
                std::fs::read_to_string(captured(&format!("shipped/redone-hello/{rel}"))).unwrap(),
            ));
        }
        for p in ["dashboard", "deployment", "kustomization"] {
            let rel = format!("k8s/platform/{p}.yaml");
            files.push((
                format!("redone-hello/{rel}"),
                std::fs::read_to_string(captured(&format!("shipped/redone-hello/{rel}"))).unwrap(),
            ));
        }
        let files: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let msg = std::fs::read_to_string(captured("shipped/redone-hello.last-commit")).unwrap();
        f.seed("arugula-salad/shipped", &files, &msg);
        f
    }

    /// Commit `files` to `repo`'s main with `msg`: its sha.
    fn seed(&self, repo: &str, files: &[(&str, &str)], msg: &str) -> String {
        let w = self.root.join("seed").join(repo);
        if !w.exists() {
            std::fs::create_dir_all(&w).unwrap();
            git(&w, &["init", "--quiet", "--initial-branch=main"]);
            git(&w, &["remote", "add", "origin", &self.remotes.join(format!("{repo}.git")).to_string_lossy()]);
        } else {
            git(&w, &["pull", "--quiet", "--rebase", "origin", "main"]);
        }
        for (f, body) in files {
            let p = w.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        git(&w, &["add", "-A"]);
        git(&w, &["commit", "--quiet", "--allow-empty", "-m", msg]);
        git(&w, &["push", "--quiet", "origin", "HEAD:main"]);
        git(&w, &["rev-parse", "HEAD"])
    }

    fn bare(&self, repo: &str) -> PathBuf {
        self.remotes.join(format!("{repo}.git"))
    }

    fn show(&self, repo: &str, rev: &str) -> Option<String> {
        let out = Command::new("git").current_dir(self.bare(repo)).args(["show", rev]).output().unwrap();
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn cfg(&self, api: &Api) -> Config {
        let mut c = Config::new(self.root.join("work"));
        c.git_base = format!("{}/", self.remotes.display());
        c.health_base = Some(api.base.clone());
        c.api_base = api.base.clone();
        c.settle = Duration::ZERO;
        c.poll = Duration::from_millis(50);
        c.timeout = Duration::from_secs(5);
        if let Ok(p) = std::env::var("PATH") {
            c.path = p;
        }
        c
    }
}

/// A fake GitHub API and app: `GET /health` answers 200; `DELETE
/// /installation/token` is logged; pull requests are listed, opened (once
/// per head) and logged; `GET /repos/arugula-salad/<name>` answers 200 for
/// [`TAKEN`], as a repository anyone sees, else 404.
#[derive(Default)]
struct ApiState {
    log: Vec<String>,
    pulls: Vec<(String, String, Value)>,
}

struct Api {
    base: String,
    state: Arc<Mutex<ApiState>>,
}

async fn fake_api() -> Api {
    type S = Arc<Mutex<ApiState>>;
    async fn revoke(State(s): State<S>, h: axum::http::HeaderMap) -> StatusCode {
        let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        s.lock().unwrap().log.push(format!("revoke {auth}"));
        StatusCode::NO_CONTENT
    }
    async fn pulls(
        State(s): State<S>,
        UrlPath((o, r)): UrlPath<(String, String)>,
        q: axum::extract::Query<std::collections::BTreeMap<String, String>>,
    ) -> Json<Value> {
        let repo = format!("{o}/{r}");
        let head = q.get("head").cloned().unwrap_or_default();
        let s = s.lock().unwrap();
        let open: Vec<Value> = s
            .pulls
            .iter()
            .enumerate()
            .filter(|(_, (r, h, _))| *r == repo && format!("{o}:{h}") == head)
            .map(|(i, _)| json!({ "html_url": format!("https://github.com/{repo}/pull/{}", i + 1) }))
            .collect();
        Json(Value::Array(open))
    }
    async fn open(
        State(s): State<S>,
        UrlPath((o, r)): UrlPath<(String, String)>,
        h: axum::http::HeaderMap,
        Json(b): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let repo = format!("{o}/{r}");
        let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        let mut s = s.lock().unwrap();
        s.log.push(format!("pull {repo} {} {auth}", b["head"].as_str().unwrap_or_default()));
        s.pulls.push((repo.clone(), b["head"].as_str().unwrap_or_default().to_owned(), b));
        let n = s.pulls.len();
        (StatusCode::CREATED, Json(json!({ "html_url": format!("https://github.com/{repo}/pull/{n}"), "number": n })))
    }
    async fn repo(UrlPath((o, r)): UrlPath<(String, String)>) -> StatusCode {
        if o == "arugula-salad" && r == TAKEN { StatusCode::OK } else { StatusCode::NOT_FOUND }
    }
    let state: S = Arc::default();
    let r = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/installation/token", delete(revoke))
        .route("/repos/{o}/{r}/pulls", get(pulls).post(open))
        .route("/repos/{o}/{r}", get(repo))
        .with_state(state.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    Api { base, state }
}

impl Api {
    fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }
}

/// A deploy of `driver`, with its plan made as the platform makes it.
fn deploy(driver: &str, kind: &str, reference: &str, place: &str, settings: &[(&str, &str)], n: u32) -> Deploy {
    let given = settings.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let settings = arugula_e2e::drivers::settings(driver, &given).unwrap();
    let text = arugula_e2e::drivers::plan_text(driver, "production", kind, reference, place, &settings, None);
    Deploy {
        driver: driver.into(),
        id: format!("dep_{n:032x}"),
        project: PROJECT.into(),
        environment: "production".into(),
        kind: kind.into(),
        reference: reference.into(),
        place: place.into(),
        plan: hex::encode(Sha256::digest(text.as_bytes())),
        settings,
        roll_back: None,
        artifact: None,
    }
}

/// A gzipped tar of `files`, made by tar as an agent would (`tar -czf -
/// -C DIR .`): at `at`, and its digest.
fn tarball(root: &Path, files: &[(&str, &[u8])], link: bool) -> (PathBuf, String) {
    let dir = root.join("agent-src");
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    if link {
        std::os::unix::fs::symlink("/etc/passwd", dir.join("passwd")).unwrap();
    }
    let at = root.join("ship.tgz");
    let st = Command::new("tar").args(["-czf"]).arg(&at).arg("-C").arg(&dir).arg(".").status().unwrap();
    assert!(st.success());
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(std::fs::read(&at).unwrap())));
    (at, digest)
}

fn hello_src() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Dockerfile", std::fs::read(captured("shipped/redone-hello/src/Dockerfile")).unwrap()),
        ("server.js", std::fs::read(captured("shipped/redone-hello/src/server.js")).unwrap()),
    ]
}

/// planter's app.yaml files, as planter's `yaml` package parses them: the
/// real ones on GitHub, and the ones planter's ship and promote write.
#[test]
fn planters_app_yaml_reads_as_planters_yaml_parse_does() {
    for name in ["real-redone-hello", "real-mvp-redone-app", "real-ocarina", "real-beds-smoke"] {
        let fx = fixture(name);
        let got = planter_manifest(fx["request"]["appYaml"].as_str().unwrap()).unwrap();
        assert_eq!(got, fx["request"]["manifest"], "{name}");
    }
    // What planter's ship wrote reads back as the spec planter made of it.
    let real = std::fs::read_to_string(captured("shipped/redone-hello/app.yaml")).unwrap();
    assert_eq!(planter_manifest(&real).unwrap(), fixture("real-redone-hello")["request"]["manifest"]);
    for bad in ["name: [a\n", "needs:\n    deep:\n      x: 1\n", "name: \"q\"\n", "  x: 1\n", "a: 1\na: 2\n"] {
        assert!(planter_manifest(bad).is_err(), "{bad:?}");
    }
}

/// What planter's ship and promote write, read back by this driver's
/// reader and rendered, is what planter rendered (its fixtures' specs).
#[test]
fn what_planter_wrote_renders_as_planter_rendered_it() {
    needs_node!();
    let work = crate::hosted::scratch("planter-render-back");
    for name in ["ship-hello", "ship-secrets-port", "promote-hello"] {
        let fx = fixture(name);
        let manifest = planter_manifest(fx["planter"]["appYaml"].as_str().unwrap()).unwrap();
        let path = std::env::var("PATH").unwrap();
        let repo = fx["planter"]["spec"]["name"].clone();
        let got = renderer(&work, &path, &json!({ "op": "render", "manifest": manifest, "repo": repo })).unwrap();
        assert_eq!(got["spec"], fx["planter"]["spec"], "{name}");
        assert_eq!(got["files"], fx["planter"]["files"], "{name}");
    }
    // What planter's code refuses, said as planter says it.
    let path = std::env::var("PATH").unwrap();
    let e = renderer(&work, &path, &json!({ "op": "render", "manifest": { "name": "Bad" } })).unwrap_err();
    assert!(e.starts_with("planter's renderer refused it: name \"Bad\""), "{e}");
}

/// planter's pointer for a shipped app, as this driver writes it, is the
/// one planter wrote for redone-hello but for who wrote it.
#[test]
fn a_shipped_pointer_is_planters() {
    let real = std::fs::read_to_string(captured("planter-apps/redone-hello.yaml")).unwrap();
    let ours = shipped_pointer("redone-hello");
    let line1 = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert_eq!(line1(&ours), line1(&real));
    assert!(
        ours.starts_with("# redone-hello in prod, at https://redone-hello.run.inevitable.fyi. Written by Arugula's")
    );
}

/// The deploy comes from its command line, its plan recomputed with its
/// driver, settings and renderer: another app, another port, a setting the
/// environment's variables don't say, or another driver is refused.
#[test]
fn the_deploy_comes_from_its_command_line_and_its_plan() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let d = deploy(SHIP, "artifact", &digest, ARTIFACT_WHERE, &[("app", "hello")], 1);
    let args = |d: &Deploy, extra: &[&str]| -> Vec<String> {
        let mut a: Vec<String> = [
            "--driver",
            &d.driver,
            "--deploy",
            &d.id,
            "--project",
            &d.project,
            "--environment",
            &d.environment,
            "--kind",
            &d.kind,
            "--ref",
            &d.reference,
            "--where",
            &d.place,
            "--plan",
            &d.plan,
            "--artifact",
            "/tmp/x",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        for (k, v) in &d.settings {
            a.extend(["--setting".to_owned(), format!("{k}={v}")]);
        }
        a.extend(extra.iter().map(|s| (*s).to_owned()));
        a
    };
    let none = |_: &str| None;
    let (_, _, got) = parse(&args(&d, &[]), none, Config::new("/w".into())).unwrap();
    assert_eq!(got.settings, d.settings);
    assert_eq!(got.artifact.as_deref(), Some(Path::new("/tmp/x")));
    let mut other = d.clone();
    other.settings.insert("app".into(), "other".into());
    assert!(parse(&args(&other, &[]), none, Config::new("/w".into())).unwrap_err().contains("approved plan"));
    let mut port = d.clone();
    port.settings.insert("port".into(), "3000".into());
    assert!(parse(&args(&port, &[]), none, Config::new("/w".into())).unwrap_err().contains("approved plan"));
    let said = serde_json::to_string(&other.settings).unwrap();
    let e = parse(&args(&d, &[]), |k| (k == "DEPLOY_SETTINGS").then(|| said.clone()), Config::new("/w".into()));
    assert!(e.unwrap_err().contains("DEPLOY_SETTINGS"));
    let mut promote = d.clone();
    promote.driver = PROMOTE.into();
    assert!(parse(&args(&promote, &[]), none, Config::new("/w".into())).is_err());
    let mut fake = d.clone();
    fake.driver = "fake".into();
    assert!(parse(&args(&fake, &[]), none, Config::new("/w".into())).unwrap_err().contains("planter's drivers"));
    assert!(
        parse(&args(&d, &["--roll-back", &format!("dep_{}", "b".repeat(32))]), none, Config::new("/w".into())).is_err()
    );
}

/// Ship, end to end against shipped and planter-apps as planter left
/// them: an agent's source (redone-hello's, as an agent tars it) is
/// unpacked and checked, committed to `<app>/` with planter's app.yaml and
/// render, the pointer written as planter's; both tokens revoked right after
/// the pushes, before the wait; once shipped's build pins the image and the
/// app answers, it reports the commit and the image. Then: the same deploy
/// again is a replay; another digest than approved, a link in the tar, no
/// Dockerfile, planter's own redone-hello, another app's repository's
/// name, and a promoted app are each refused, with nothing pushed.
#[tokio::test(flavor = "multi_thread")]
async fn ship_commits_the_source_planters_way_and_waits_for_the_pin() {
    needs_node!();
    let f = Fake::new("ship");
    let api = fake_api().await;
    let cfg = f.cfg(&api);
    let src = hello_src();
    let files: Vec<(&str, &[u8])> = src.iter().map(|(p, b)| (*p, b.as_slice())).collect();
    let (at, digest) = tarball(&f.root, &files, false);
    let mut d = deploy(SHIP, "artifact", &digest, ARTIFACT_WHERE, &[("app", "hello2")], 1);
    d.artifact = Some(at.clone());
    let t = Tokens { write: Some(POINTERS_T.into()), read: None };

    // shipped's build, pinning the image after the ship's commit lands.
    let bare = f.bare("arugula-salad/shipped");
    let builder = tokio::task::spawn_blocking({
        let (bare, root) = (bare.clone(), f.root.clone());
        move || {
            for _ in 0..200 {
                let out =
                    Command::new("git").current_dir(&bare).args(["log", "-1", "--format=%B", "main"]).output().unwrap();
                if String::from_utf8_lossy(&out.stdout).contains("hello2: ship to loam") {
                    let w = root.join("builder");
                    git(&root, &["clone", "--quiet", &bare.to_string_lossy(), &w.to_string_lossy()]);
                    let k = w.join("hello2/k8s/kustomization.yaml");
                    let text = std::fs::read_to_string(&k).unwrap();
                    std::fs::write(&k, text.replace("newTag: unpinned", &format!("digest: sha256:{}", "7".repeat(64))))
                        .unwrap();
                    git(&w, &["commit", "--quiet", "-am", "deploy(hello2): pin image 777777777777"]);
                    git(&w, &["push", "--quiet", "origin", "HEAD:main"]);
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        }
    });
    let got = ship(&cfg, Duration::from_secs(20), &d, &t, Some(SHIPPED_T)).await.unwrap();
    assert!(builder.await.unwrap(), "the ship's commit reached shipped");
    let commit = got["commit"].as_str().unwrap().to_owned();
    assert_eq!(got["image"], format!("ghcr.io/arugula-salad/hello2@sha256:{}", "7".repeat(64)));
    assert!(got["pointer"].as_str().is_some_and(|p| p.len() == 40));

    // What planter's ship writes, for hello2: its fixture's app.yaml and render.
    let want = fixture("ship-hello");
    let swap = |s: &str| s.replace("redone-hello", "hello2");
    let at_commit = |rel: &str| f.show("arugula-salad/shipped", &format!("{commit}:hello2/{rel}")).unwrap();
    assert_eq!(at_commit("app.yaml"), swap(want["planter"]["appYaml"].as_str().unwrap()));
    for (rel, text) in want["planter"]["files"].as_object().unwrap() {
        assert_eq!(at_commit(rel), swap(text.as_str().unwrap()), "{rel}");
    }
    assert_eq!(at_commit("k8s/kustomization.yaml"), swap(want["planter"]["top"].as_str().unwrap()));
    assert_eq!(at_commit("src/server.js").as_bytes(), src[1].1.as_slice());
    let msg = git(&bare, &["log", "-1", "--format=%B", &commit]);
    for says in [
        format!("Arugula-Deploy: {}", d.id),
        format!("Arugula-Project: {PROJECT}"),
        "Arugula-Driver: planter-ship".into(),
    ] {
        assert!(msg.contains(&says), "{says} in {msg}");
    }
    assert_eq!(
        f.show("arugula-salad/planter-apps", "main:apps/hello2.yaml").unwrap(),
        shipped_pointer("hello2"),
        "the pointer, planter's"
    );
    // Both revoked, before any wait (the log has nothing after them but health).
    let log = api.log();
    assert_eq!(&log[..2], [format!("revoke Bearer {SHIPPED_T}"), format!("revoke Bearer {POINTERS_T}")]);

    // The same deploy again: a replay, refused.
    let again = ship_commit(&cfg, &d, &t, Some(SHIPPED_T)).await.unwrap_err();
    assert!(again.why.contains("already") && !again.pushed, "{}", again.why);

    // Each refused, nothing pushed.
    let heads =
        || (git(&bare, &["rev-parse", "main"]), git(&f.bare("arugula-salad/planter-apps"), &["rev-parse", "main"]));
    let before = heads();
    let mut n = 10;
    let refused = |why: &str, d: Deploy| {
        let cfg = cfg.clone();
        let t = Tokens { write: Some(POINTERS_T.into()), read: None };
        let why = why.to_owned();
        async move {
            let e = ship_commit(&cfg, &d, &t, Some(SHIPPED_T)).await.unwrap_err();
            assert!(e.why.contains(&why) && !e.pushed, "{why}: {}", e.why);
        }
    };
    let mut with = |app: &str, at: &Path, digest: &str| {
        n += 1;
        let mut d = deploy(SHIP, "artifact", digest, ARTIFACT_WHERE, &[("app", app)], n);
        d.artifact = Some(at.to_owned());
        d
    };
    let other = format!("sha256:{}", "0".repeat(64));
    refused("not the", with("hello2", &at, &other)).await;
    let (linked, ld) = tarball(&f.root.join("l"), &files, true);
    refused("not a regular file", with("hello3", &linked, &ld)).await;
    let (bare_src, bd) = tarball(&f.root.join("b"), &[("server.js", b"x")], false);
    refused("no Dockerfile", with("hello3", &bare_src, &bd)).await;
    let (ok, od) = tarball(&f.root.join("o"), &files, false);
    refused("planter (redone (redone.arugula.io))", with("redone-hello", &ok, &od)).await;
    refused("an app of its own repository", with("ocarina", &ok, &od)).await;
    assert_eq!(heads(), before, "nothing pushed");
    f.seed(
        "arugula-salad/shipped",
        &[("promoted/app.yaml", "name: promoted\nruntime: k8s\n")],
        &format!("promoted\n\n{PROJECT_TRAILER}: {PROJECT}\n"),
    );
    let before = heads();
    refused("promoted to the cluster", with("promoted", &ok, &od)).await;
    assert_eq!(heads(), before, "nothing pushed");
}

/// Promote, against shipped with an app this project shipped: the pull
/// request from `promote/<app>` at the approved commit, its app.yaml and
/// render exactly planter's promotion's (its fixture), opened with the
/// token, which is then revoked; asked again, the open one is found, not a
/// second. An app planter shipped (not this project) is refused.
#[tokio::test(flavor = "multi_thread")]
async fn promote_opens_planters_pull_request() {
    needs_node!();
    let f = Fake::new("promote");
    let api = fake_api().await;
    let cfg = f.cfg(&api);
    // redone-hello, shipped again by this project (as planter wrote it).
    let sha = f.seed(
        "arugula-salad/shipped",
        &[("redone-hello/src/README", "x")],
        &format!("redone-hello: ship\n\n{PROJECT_TRAILER}: {PROJECT}\n"),
    );
    let d = deploy(PROMOTE, "commit", &sha, SHIPPED_REPO, &[("app", "redone-hello")], 1);
    let got = promote(&cfg, &d, Some(SHIPPED_T)).await.unwrap();
    assert_eq!(got["pull"], "https://github.com/arugula-salad/shipped/pull/1");
    let want = fixture("promote-hello");
    let at = |rel: &str| f.show("arugula-salad/shipped", &format!("promote/redone-hello:redone-hello/{rel}")).unwrap();
    assert_eq!(at("app.yaml"), want["planter"]["appYaml"].as_str().unwrap());
    for (rel, text) in want["planter"]["files"].as_object().unwrap() {
        assert_eq!(at(rel), text.as_str().unwrap(), "{rel}");
    }
    let parent = git(&f.bare("arugula-salad/shipped"), &["rev-parse", "promote/redone-hello^"]);
    assert_eq!(parent, sha, "from the approved commit");
    assert_eq!(
        api.log(),
        [
            format!("pull arugula-salad/shipped promote/redone-hello Bearer {SHIPPED_T}"),
            format!("revoke Bearer {SHIPPED_T}")
        ]
    );
    let body = api.state.lock().unwrap().pulls[0].2.clone();
    assert_eq!(body["base"], "main");
    assert_eq!(body["title"], "Promote redone-hello from loam to the cluster");
    // Again: the open one.
    let d2 = deploy(PROMOTE, "commit", &sha, SHIPPED_REPO, &[("app", "redone-hello")], 2);
    assert_eq!(promote(&cfg, &d2, Some(SHIPPED_T)).await.unwrap()["pull"], got["pull"]);
    assert_eq!(api.state.lock().unwrap().pulls.len(), 1);

    // planter's own, untouched since: not this project's.
    let f = Fake::new("promote-planters");
    let api = fake_api().await;
    let head = git(&f.bare("arugula-salad/shipped"), &["rev-parse", "main"]);
    let d = deploy(PROMOTE, "commit", &head, SHIPPED_REPO, &[("app", "redone-hello")], 3);
    let e = promote(&f.cfg(&api), &d, Some(SHIPPED_T)).await.unwrap_err();
    assert!(e.why.contains("not this project") && !e.pushed, "{}", e.why);
}

/// Prod, against an app repository with ocarina's app member (its real
/// package.json): planter's scaffold on `planter/prod` from the approved
/// commit, exactly its fixture's files, and the pull request. A repository
/// with a k8s/ already, and a commit off the default branch, are refused.
#[tokio::test(flavor = "multi_thread")]
async fn prod_opens_planters_scaffold_pull_request() {
    needs_node!();
    let f = Fake::new("prod");
    let api = fake_api().await;
    let cfg = f.cfg(&api);
    let want = fixture("scaffold-ocarina");
    let pkg = want["request"]["packageJson"].as_str().unwrap();
    let sha = f.seed("arugula-salad/ocarina", &[("app/package.json", pkg), ("app/package-lock.json", "{}")], "app");
    let d = deploy(PROD, "commit", &sha, "arugula-salad/ocarina", &[], 1);
    let got = prod(&cfg, &d, Some(SHIPPED_T)).await.unwrap();
    assert_eq!(got["pull"], "https://github.com/arugula-salad/ocarina/pull/1");
    for (rel, text) in want["planter"]["files"].as_object().unwrap() {
        let at = f.show("arugula-salad/ocarina", &format!("planter/prod:{rel}")).unwrap();
        assert_eq!(at, text.as_str().unwrap(), "{rel}");
    }
    let body = api.state.lock().unwrap().pulls[0].2.clone();
    assert_eq!((body["base"].as_str(), body["title"].as_str()), (Some("main"), Some("Deploy ocarina to prod")));

    let f = Fake::new("prod-k8s");
    let api = fake_api().await;
    let sha = f.seed("arugula-salad/ocarina", &[("app/package.json", pkg), ("k8s/kustomization.yaml", "x")], "k8s");
    let d = deploy(PROD, "commit", &sha, "arugula-salad/ocarina", &[], 2);
    let e = prod(&f.cfg(&api), &d, Some(SHIPPED_T)).await.unwrap_err();
    assert!(e.why.contains("has a k8s/") && !e.pushed, "{}", e.why);
    let d = deploy(PROD, "commit", &"e".repeat(40), "arugula-salad/ocarina", &[], 3);
    assert!(prod(&f.cfg(&api), &d, Some(SHIPPED_T)).await.is_err());
}

/// Go live: apps-platform's apply as planter's flavor. A new app's pointer
/// asks for the custom domains its approved plan lists, as planter's Go
/// live's did, only when its app.yaml routes exactly those (sorted by
/// planter's validate()), and carries planter-go-live's trailer; ocarina's existing pointer (planter's, tracking main) is pinned
/// at the commit, nothing else in it changing.
#[tokio::test(flavor = "multi_thread")]
async fn go_live_pins_and_asks_for_custom_domains() {
    needs_node!();
    let f = Fake::new("go-live");
    let api = fake_api().await;
    let cfg = f.cfg(&api);
    let app = "name: ocarina\nhealth: /health\nruntime: k8s\nneeds:\n  domain: [ocarina.apps.arugula.io]\n  telemetry: true\n";
    let sha = f.seed("arugula-salad/ocarina", &[("app.yaml", app), ("k8s/kustomization.yaml", "x")], "app");
    let d = deploy(GO_LIVE, "commit", &sha, "arugula-salad/ocarina", &[], 1);
    let t = Tokens { write: Some(POINTERS_T.into()), read: Some(SHIPPED_T.into()) };
    ap::apply_as(&cfg, &GO_LIVE_FLAVOR, &d.as_ap(), &t, Some(&[])).await.unwrap();
    let pointer = f.show("arugula-salad/planter-apps", "main:apps/ocarina.yaml").unwrap();
    let real = std::fs::read_to_string(captured("planter-apps/ocarina.yaml")).unwrap();
    assert_eq!(pointer, real.replace("    branch: main\n", &format!("    commit: {sha}\n")), "pinned, nothing else");
    let msg = git(&f.bare("arugula-salad/planter-apps"), &["log", "-1", "--format=%B", "main"]);
    assert!(msg.contains("Arugula-Driver: planter-go-live"), "{msg}");

    // A new app with a custom domain.
    let f = Fake::new("go-live-new");
    let app = "name: fern\nruntime: k8s\nneeds:\n  domain:\n    - fern.apps.arugula.io\n    - shop.example.com\n";
    std::fs::create_dir_all(f.bare("arugula-salad/fern")).unwrap();
    git(&f.bare("arugula-salad/fern"), &["init", "--quiet", "--bare", "--initial-branch=main"]);
    let sha = f.seed("arugula-salad/fern", &[("app.yaml", app), ("k8s/kustomization.yaml", "x")], "app");
    let cfg = f.cfg(&api);
    // The plan must name it: one that asks for none pushes nothing.
    let none = deploy(GO_LIVE, "commit", &sha, "arugula-salad/fern", &[], 2);
    let e =
        ap::apply_as(&cfg, &GO_LIVE_FLAVOR, &none.as_ap(), &t, Some(&p::domains(&none.settings))).await.unwrap_err();
    assert!(e.why.contains("[shop.example.com]") && !e.pushed, "{}", e.why);
    assert!(f.show("arugula-salad/planter-apps", "main:apps/fern.yaml").is_none());
    let d = deploy(GO_LIVE, "commit", &sha, "arugula-salad/fern", &[("domains", "shop.example.com")], 3);
    ap::apply_as(&cfg, &GO_LIVE_FLAVOR, &d.as_ap(), &t, Some(&p::domains(&d.settings))).await.unwrap();
    let pointer = f.show("arugula-salad/planter-apps", "main:apps/fern.yaml").unwrap();
    assert!(pointer.contains("    platform.arugula.io/domains: shop.example.com\n"), "{pointer}");
    assert!(
        pointer.contains(&format!("    commit: {sha}\n"))
            && pointer.contains("Written by Arugula's planter-go-live driver")
    );
}

#[test]
fn needs_domain_reads_planters_lists() {
    use super::ap::needs_domain;
    assert_eq!(needs_domain("name: x\n").unwrap(), Vec::<String>::new());
    assert_eq!(needs_domain("needs:\n  domain: [a.b.c, d.e]\n").unwrap(), ["a.b.c", "d.e"]);
    assert_eq!(
        needs_domain("needs:\n  telemetry: true\n  domain:\n    - a.b.c\n    - 'd.e' # x\n").unwrap(),
        ["a.b.c", "d.e"]
    );
    assert!(needs_domain("needs:\n  domain: a.b\n").is_err());
    assert!(needs_domain("needs:\n  domain: [a b]\n").is_err());
}

/// A link, `..`, an absolute path, a device or a hard link in a ship's tar
/// is refused before anything is written; so is a tar over planter's
/// limits, by the running count and size, and anything that isn't a gzipped
/// tar. What's read keeps each file's bytes and its executable bit.
#[tokio::test]
async fn a_ships_tar_is_read_with_caps_and_never_extracted() {
    let root = crate::hosted::scratch("planter-tar");
    let digest_of = |p: &Path| format!("sha256:{}", hex::encode(Sha256::digest(std::fs::read(p).unwrap())));
    let tar = |name: &str, args: &[&str], dir: &Path| -> (PathBuf, String) {
        let at = root.join(format!("{name}.tgz"));
        let st = Command::new("tar").arg("-czf").arg(&at).arg("-C").arg(dir).args(args).status().unwrap();
        assert!(st.success(), "{name}");
        let d = digest_of(&at);
        (at, d)
    };
    let src = root.join("src");
    std::fs::create_dir_all(src.join("bin")).unwrap();
    std::fs::write(src.join("Dockerfile"), "FROM scratch\n").unwrap();
    std::fs::write(src.join("bin/run"), "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(src.join("bin/run"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let (ok, d) = tar("ok", &["."], &src);
    let files = unpack(&ok, &d).await.unwrap();
    let names: Vec<(&str, bool)> = files.iter().map(|(p, _, x)| (p.as_str(), *x)).collect();
    assert_eq!(names, [("Dockerfile", false), ("bin/run", true)]);
    assert_eq!(files[0].1, b"FROM scratch\n");

    let refused = |what: &str, at: &Path, digest: &str| {
        let (what, at, digest) = (what.to_owned(), at.to_owned(), digest.to_owned());
        async move {
            let e = unpack(&at, &digest).await.unwrap_err();
            assert!(e.contains(&what), "{what}: {e}");
        }
    };
    refused("not the", &ok, &format!("sha256:{}", "0".repeat(64))).await;
    // A symlink, and a hard link.
    let linked = root.join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(linked.join("Dockerfile"), "x").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", linked.join("passwd")).unwrap();
    let (at, d) = tar("symlink", &["."], &linked);
    refused("not a regular file", &at, &d).await;
    let hard = root.join("hard");
    std::fs::create_dir_all(&hard).unwrap();
    std::fs::write(hard.join("Dockerfile"), "x").unwrap();
    std::fs::hard_link(hard.join("Dockerfile"), hard.join("again")).unwrap();
    let (at, d) = tar("hardlink", &["."], &hard);
    refused("not a regular file", &at, &d).await;
    // `..` and an absolute path (tar -P keeps them).
    let (at, d) = tar("dotdot", &["-P", "../src/Dockerfile"], &root.join("hard"));
    refused("isn't a path in the app", &at, &d).await;
    let abs = src.join("Dockerfile");
    let (at, d) = tar("absolute", &["-P", abs.to_str().unwrap()], &src);
    refused("isn't a path in the app", &at, &d).await;
    // No Dockerfile; not gzipped; not a tar.
    let bare = root.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(bare.join("server.js"), "x").unwrap();
    let (at, d) = tar("bare", &["."], &bare);
    refused("no Dockerfile", &at, &d).await;
    let plain = root.join("plain.tgz");
    std::fs::write(&plain, b"not gzip").unwrap();
    refused("gzipped", &plain, &digest_of(&plain)).await;
    // Over planter's limits: more files than it takes, and more bytes (a
    // file of zeros, which gzips small: the running caps stop it).
    let many = root.join("many");
    std::fs::create_dir_all(&many).unwrap();
    std::fs::write(many.join("Dockerfile"), "x").unwrap();
    for i in 0..MAX_FILES {
        std::fs::write(many.join(format!("f{i}")), "").unwrap();
    }
    let (at, d) = tar("many", &["."], &many);
    refused("at most 2000 files", &at, &d).await;
    let big = root.join("big");
    std::fs::create_dir_all(&big).unwrap();
    std::fs::write(big.join("Dockerfile"), "x").unwrap();
    std::fs::File::create(big.join("zeros")).unwrap().set_len(MAX_UNPACKED + 1).unwrap();
    let (at, d) = tar("big", &["."], &big);
    let e = unpack(&at, &d).await.unwrap_err();
    assert!(e.contains("MiB"), "{e}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A path through a symlink is never written or read: a checkout's link
/// (an app's repository could hold one) can't point a write, or a read of
/// its package.json, out of the clone.
#[test]
fn paths_through_links_are_refused() {
    let root = crate::hosted::scratch("planter-links");
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("package.json"), "{}").unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::os::unix::fs::symlink(&outside, repo.join("app")).unwrap();
    assert!(read_under(&repo, "app/package.json", 1 << 16).unwrap_err().contains("symlink"));
    assert!(write_under(&repo, "app/Dockerfile", b"x", false).unwrap_err().contains("symlink"));
    assert!(!outside.join("Dockerfile").exists(), "nothing written through it");
    std::fs::create_dir_all(repo.join("k8s")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", repo.join("k8s/kustomization.yaml")).unwrap();
    assert!(write_under(&repo, "k8s/kustomization.yaml", b"x", false).is_err());
    for bad in ["../x", "/etc/x", "a//b"] {
        assert!(write_under(&repo, bad, b"x", false).is_err(), "{bad}");
    }
    write_under(&repo, "k8s/platform/deployment.yaml", b"ok", false).unwrap();
    assert_eq!(read_under(&repo, "k8s/platform/deployment.yaml", 16).unwrap().as_deref(), Some("ok"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Prod against a repository whose app member's package.json is a link to
/// a file outside it: checked out as the link's text (core.symlinks off),
/// it isn't the app's package.json, and nothing is pushed.
#[tokio::test(flavor = "multi_thread")]
async fn prod_never_follows_a_link_in_the_apps_repository() {
    needs_node!();
    let f = Fake::new("prod-link");
    let api = fake_api().await;
    let w = f.root.join("seed/arugula-salad/ocarina");
    let pkg = fixture("scaffold-ocarina")["request"]["packageJson"].as_str().unwrap().to_owned();
    let outside = f.root.join("outside.json");
    std::fs::write(&outside, &pkg).unwrap();
    let sha = f.seed("arugula-salad/ocarina", &[("README", "x")], "readme");
    std::fs::create_dir_all(w.join("app")).unwrap();
    std::os::unix::fs::symlink(&outside, w.join("app/package.json")).unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "--quiet", "-m", "a link"]);
    git(&w, &["push", "--quiet", "origin", "HEAD:main"]);
    let linked = git(&w, &["rev-parse", "HEAD"]);
    assert_ne!(linked, sha);
    let d = deploy(PROD, "commit", &linked, "arugula-salad/ocarina", &[], 1);
    let e = prod(&f.cfg(&api), &d, Some(SHIPPED_T)).await.unwrap_err();
    assert!(!e.pushed, "{}", e.why);
    assert!(f.show("arugula-salad/ocarina", "planter/prod:Dockerfile").is_none(), "nothing pushed");
}

/// Ship's names, checked on the box before anything is pushed: an
/// arugula-salad repository GitHub shows anyone is refused; and secrets,
/// which load `/<app>` from Infisical, only for a name nobody else ever
/// shipped or pointed at (planter-apps' history holds a pointer someone
/// took out), while the same ship without secrets goes.
#[tokio::test(flavor = "multi_thread")]
async fn ship_names_are_checked_on_github_and_secrets_need_a_name_never_used() {
    needs_node!();
    let f = Fake::new("ship-names");
    let api = fake_api().await;
    let cfg = f.cfg(&api);
    let src = hello_src();
    let files: Vec<(&str, &[u8])> = src.iter().map(|(p, b)| (*p, b.as_slice())).collect();
    let (at, digest) = tarball(&f.root, &files, false);
    let t = Tokens { write: Some(POINTERS_T.into()), read: None };
    let with = |app: &str, extra: &[(&str, &str)], n: u32| {
        let mut st = vec![("app", app)];
        st.extend_from_slice(extra);
        let mut d = deploy(SHIP, "artifact", &digest, ARTIFACT_WHERE, &st, n);
        d.artifact = Some(at.clone());
        d
    };
    let heads = || git(&f.bare("arugula-salad/shipped"), &["rev-parse", "main"]);
    let before = heads();
    let e = ship_commit(&cfg, &with(TAKEN, &[], 1), &t, Some(SHIPPED_T)).await.unwrap_err();
    assert!(e.why.contains("is a repository") && !e.pushed, "{}", e.why);
    // A pointer someone else wrote, and took out.
    f.seed("arugula-salad/planter-apps", &[("apps/old.yaml", "x")], "old: into prod");
    let w = f.root.join("seed/arugula-salad/planter-apps");
    git(&w, &["rm", "-q", "apps/old.yaml"]);
    git(&w, &["commit", "--quiet", "-m", "old: out of prod"]);
    git(&w, &["push", "--quiet", "origin", "HEAD:main"]);
    let e = ship_commit(&cfg, &with("old", &[("secrets", "STRIPE_KEY")], 2), &t, Some(SHIPPED_T)).await.unwrap_err();
    assert!(e.why.contains("may hold another app's secrets") && !e.pushed, "{}", e.why);
    assert_eq!(heads(), before, "nothing pushed");
    let done = ship_commit(&cfg, &with("old", &[], 3), &t, Some(SHIPPED_T)).await.unwrap();
    assert!(done.commit.is_some(), "without secrets it ships");
    // This project's own name since: its secrets come too.
    let src2: Vec<(&str, &[u8])> = vec![("Dockerfile", b"FROM scratch\n"), ("server.js", b"2")];
    let (at2, digest2) = tarball(&f.root.join("two"), &src2, false);
    let mut d = deploy(SHIP, "artifact", &digest2, ARTIFACT_WHERE, &[("app", "fresh"), ("secrets", "STRIPE_KEY")], 4);
    d.artifact = Some(at2);
    assert!(ship_commit(&cfg, &d, &t, Some(SHIPPED_T)).await.unwrap().commit.is_some());
}
