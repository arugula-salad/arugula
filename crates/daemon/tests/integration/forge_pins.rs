//! M172: a GitHub block on a repo linked to a project in control is a pin
//! onto the platform's item, and the machine makes no GitHub call for it.
//! Against a fake control whose platform mirrors `acme/app#1`, and a fake
//! GitHub (`ARUGULA_GITHUB_API`) that counts every call by repository:
//! - the linked block pins, shows the mirror's item, and GitHub hears
//!   nothing of `acme/app`, while an unlinked block (`acme/other#2`) reads
//!   GitHub as before;
//! - a restart reads the pin from `layout.json` and still calls no GitHub;
//! - a restart with the platform down reads GitHub (never a blank block),
//!   and keeps the pin for when it's back;
//! - unlinked on the platform, the block reads GitHub again and drops the
//!   pin.
//!
//! The platform's side of these reads is `arugula-platform`'s
//! `forge::viewer_tests` and `forge::ops_tests`.

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

const PROJECT: &str = "prj_00000000000000000000000000000001";
const ITEM: &str = "fi_000000000000000000000000000000a1";

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The platform answers (else a 502).
    up: Arc<Mutex<bool>>,
    /// `acme/app` is linked for the machine's owner.
    linked: Arc<Mutex<bool>>,
    /// GitHub's calls, by `owner/name` (`user` for `/user…`).
    github: Arc<Mutex<HashMap<String, u32>>>,
    /// The platform's item reads.
    reads: Arc<Mutex<u32>>,
}

impl Fake {
    fn up(&self) -> bool {
        *self.up.lock().unwrap()
    }
    fn linked(&self) -> bool {
        *self.linked.lock().unwrap()
    }
    fn github(&self, repo: &str) -> u32 {
        self.github.lock().unwrap().get(repo).copied().unwrap_or(0)
    }
    fn reads(&self) -> u32 {
        *self.reads.lock().unwrap()
    }
}

fn down() -> Response {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": "the platform is down" }))).into_response()
}

fn none() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "no such item" }))).into_response()
}

fn fixture(f: &str) -> Value {
    let p = format!("{}/tests/fixtures/github/github-cli-cli-13788/{f}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The mirror's row for `acme/app#1`.
fn item() -> Value {
    json!({ "id": ITEM, "repo": "acme/app", "number": 1, "kind": "pr", "state": "open",
            "title": "from the mirror", "labels": ["bug"], "author": "jake-gh", "assignees": [],
            "reviewers": ["dan-gh"], "draft": false, "merge_state": "clean", "head_ref": "fix",
            "head_sha": "abc123", "base_ref": "main", "created_at": 1, "updated_at": 2 })
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn repo(State(f): State<Fake>, Path((owner, name)): Path<(String, String)>) -> Response {
    if !f.up() {
        return down();
    }
    let linked = f.linked() && format!("{owner}/{name}").eq_ignore_ascii_case("acme/app");
    Json(json!({ "repo": format!("{owner}/{name}"), "linked": linked, "project": linked.then_some(PROJECT),
                 "repo_id": linked.then_some(77), "login": linked.then_some("jake-gh") }))
    .into_response()
}

async fn ops(State(f): State<Fake>, Json(b): Json<Value>) -> Response {
    if !f.up() {
        return down();
    }
    let ours = f.linked() && b["repo"] == "acme/app" && b["number"] == 1 && b["op"] == "pr.get";
    if !ours {
        return none();
    }
    Json(json!({ "op": b["op"], "v": 1, "result": { "item": item(), "events": [], "drafts": [] } })).into_response()
}

/// `/api/forge/items/{id}`, `…/checks` and `…/events`.
async fn items(State(f): State<Fake>, uri: Uri) -> Response {
    if !f.up() {
        return down();
    }
    let rest = uri.path().trim_start_matches("/api/forge/items/");
    let (id, what) = rest.split_once('/').unwrap_or((rest, ""));
    if !f.linked() || id != ITEM {
        return none();
    }
    *f.reads.lock().unwrap() += 1;
    Json(match what {
        "" => item(),
        "checks" => json!({ "item": ITEM, "head_sha": "abc123", "checks": [
            { "kind": "check_run", "name": "build", "status": "completed", "conclusion": "success", "url": null, "updated_at": 2 }] }),
        "events" => json!({ "item": ITEM, "events": [
            { "id": "1", "kind": "review", "actor": "dan-gh", "review_state": "approved", "at": 3 },
            { "id": "2", "kind": "comment", "actor": "dan-gh", "review_state": null, "at": 4 }] }),
        _ => return none(),
    })
    .into_response()
}

async fn drafts(State(f): State<Fake>) -> Response {
    if !f.up() {
        return down();
    }
    Json(json!({ "drafts": [] })).into_response()
}

/// GitHub: every call counted by repository, and a PR's reads answered
/// with S23's recorded `cli/cli#13788`.
async fn github(State(f): State<Fake>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches("/github");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let repo = match parts.as_slice() {
        ["repos", owner, name, ..] => format!("{owner}/{name}"),
        _ => "user".into(),
    };
    *f.github.lock().unwrap().entry(repo).or_default() += 1;
    let body = match parts.as_slice() {
        ["user"] => json!({ "login": "jake-gh" }),
        ["user", "teams"] => json!([]),
        ["repos", _, _, "pulls", _] => fixture("item.json"),
        ["repos", _, _, "pulls", _, "reviews"] => fixture("reviews.json"),
        ["repos", _, _, "commits", _, "check-runs"] => fixture("check_runs.json"),
        ["repos", _, _, "commits", _, "status"] => json!({ "state": "pending", "total_count": 0, "statuses": [] }),
        ["repos", _, _, "issues", _, "timeline"] => fixture("timeline.json"),
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "message": "Not Found" }))).into_response(),
    };
    Json(body).into_response()
}

async fn dial(up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        std::future::pending::<()>().await;
        drop(ws);
    })
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(about))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/forge/repos/{owner}/{name}", get(repo))
            .route("/api/forge/ops", post(ops))
            .route("/api/forge/items/{*rest}", get(items))
            .route("/api/projects/{project}/drafts", get(drafts))
            .route("/github/{*rest}", get(github))
            .route("/api/relay/dial", get(dial))
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "jake" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

/// A `gh` with a login for github.com, as the person's would be.
fn gh(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let gh = dir.join("gh");
    std::fs::write(&gh, "#!/bin/sh\n[ \"$1 $2\" = 'auth token' ] && echo fake-gh-token && exit 0\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default())
}

fn state(d: &arugula_testkit::Daemon, block: u64) -> Value {
    d.get(&format!("/api/blocks/{block}"))["state"].clone()
}

fn title(d: &arugula_testkit::Daemon, block: u64) -> String {
    state(d, block)["pr"]["item"]["title"].as_str().unwrap_or_default().to_owned()
}

fn pinned(d: &arugula_testkit::Daemon, block: u64) -> Option<String> {
    state(d, block)["pin"]["global_id"].as_str().map(str::to_owned)
}

fn layout(d: &arugula_testkit::Daemon) -> String {
    std::fs::read_to_string(d.state.join("layout.json")).unwrap_or_default()
}

#[test]
fn a_linked_repos_block_is_a_pin_and_calls_no_github() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.up.lock().unwrap() = true;
    *f.linked.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let recorded = fixture("item.json")["title"].as_str().unwrap().to_owned();
    let mut d = arugulad!("forgepins")
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_FORGE_POLL_MS", "200,200")
        .env("ARUGULA_GITHUB_API", format!("{url}/github"))
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    let path = gh(&d.state.join("bin"));
    d.set_env("PATH", path);
    enroll(&d.state, &url);
    d.start();
    let open = |pr: &str| {
        d.post("/api/blocks", json!({ "type": "forge", "config": { "pr": pr }, "local": true }))["block"]
            .as_u64()
            .unwrap()
    };
    let linked = open("https://github.com/acme/app/pull/1");
    let other = open("https://github.com/acme/other/pull/2");

    // The linked one is the mirror's; the other reads GitHub, as before.
    d.wait_for("the pin", || pinned(&d, linked).as_deref() == Some(ITEM));
    d.wait_for("the mirror's item", || title(&d, linked) == "from the mirror");
    d.wait_for("GitHub's item", || title(&d, other) == recorded);
    let s = state(&d, linked);
    assert_eq!(s["pr"]["checks"][0]["name"], "build");
    assert_eq!(s["pr"]["reviews"][0]["author"], "dan-gh");
    assert_eq!(s["pr"]["item"]["url"], "https://github.com/acme/app/pull/1");
    assert_eq!(s["me"], "jake-gh", "you, as the platform knows you");
    assert!(state(&d, other)["pin"].is_null());
    let reads = f.reads();
    let theirs = f.github("acme/other");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(f.github("acme/app"), 0, "no GitHub call for the pinned block");
    assert!(f.reads() > reads, "it reads the platform again");
    assert!(f.github("acme/other") > theirs, "the unlinked block polls GitHub");
    d.stop();
    assert!(layout(&d).contains(ITEM), "the pin is kept");

    // A restart reads the pin first: still no GitHub call for it.
    d.start();
    d.wait_for("the mirror's item again", || title(&d, linked) == "from the mirror");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(f.github("acme/app"), 0);

    // The platform down at a restart: GitHub's reader, never a blank block,
    // and the pin kept for when it's back.
    d.stop();
    *f.up.lock().unwrap() = false;
    d.start();
    d.wait_for("GitHub's item for it", || title(&d, linked) == recorded);
    assert!(f.github("acme/app") > 0);
    assert!(state(&d, linked)["pin"].is_null());
    d.stop();
    assert!(layout(&d).contains(ITEM), "kept while the platform is down");

    // Back: the pin again, and GitHub hears no more of it.
    *f.up.lock().unwrap() = true;
    d.start();
    d.wait_for("pinned again", || title(&d, linked) == "from the mirror" && pinned(&d, linked).is_some());
    let calls = f.github("acme/app");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(f.github("acme/app"), calls);

    // Unlinked: the platform has no item for it, so the block reads GitHub
    // and drops the pin.
    *f.linked.lock().unwrap() = false;
    d.wait_for("GitHub's reader again", || title(&d, linked) == recorded && pinned(&d, linked).is_none());
    assert!(f.github("acme/app") > calls);
    d.stop();
    assert!(!layout(&d).contains(ITEM), "the pin is gone");
}
