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
//!   pin;
//! - the item's text comes from `/content`, asked once and again only when
//!   the events show something new, and a `/content` that fails leaves the
//!   metadata view.
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
const ISSUE: &str = "fi_000000000000000000000000000000a2";

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
    /// `/content` answers (else a 404, as an older control).
    content: Arc<Mutex<bool>>,
    /// How many times `/content` was asked.
    content_reads: Arc<Mutex<u32>>,
    /// A second comment has been made on the items.
    later: Arc<Mutex<bool>>,
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
    fn content_reads(&self) -> u32 {
        *self.content_reads.lock().unwrap()
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

/// The mirror's row for `acme/app#3`, an issue.
fn issue_row() -> Value {
    json!({ "id": ISSUE, "repo": "acme/app", "number": 3, "kind": "issue", "state": "open",
            "title": "an issue from the mirror", "labels": [], "author": "jake-gh", "assignees": [],
            "reviewers": [], "draft": false, "merge_state": null, "head_ref": null,
            "head_sha": null, "base_ref": null, "created_at": 1, "updated_at": 2 })
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
    let row = match (b["number"].as_u64(), b["op"].as_str()) {
        (Some(1), Some("pr.get")) => item(),
        (Some(3), Some("issue.get")) => issue_row(),
        _ => return none(),
    };
    if !f.linked() || b["repo"] != "acme/app" {
        return none();
    }
    Json(json!({ "op": b["op"], "v": 1, "result": { "item": row, "events": [], "drafts": [] } })).into_response()
}

/// `/api/forge/items/{id}`, `…/checks`, `…/events` and `…/content`.
async fn items(State(f): State<Fake>, uri: Uri) -> Response {
    if !f.up() {
        return down();
    }
    let rest = uri.path().trim_start_matches("/api/forge/items/");
    let (id, what) = rest.split_once('/').unwrap_or((rest, ""));
    if !f.linked() || (id != ITEM && id != ISSUE) {
        return none();
    }
    let issue = id == ISSUE;
    *f.reads.lock().unwrap() += 1;
    let later = *f.later.lock().unwrap();
    Json(match what {
        "" if issue => issue_row(),
        "" => item(),
        "content" => {
            *f.content_reads.lock().unwrap() += 1;
            if !*f.content.lock().unwrap() {
                return none();
            }
            let text = |kind: &str, id: u64, body: &str, at: u64| {
                json!({ "kind": kind, "id": id, "author": "dan-gh", "body": body, "path": null,
                        "line": null, "reply_to": null, "created_at": at, "updated_at": at })
            };
            let mut comments = vec![text("comment", 11, "the first comment says hello", 4)];
            if later {
                comments.push(text("comment", 12, "a later comment says more", 5));
            }
            let (item, body) = if issue { (ISSUE, "the issue's description") } else { (ITEM, "the PR's description") };
            return Json(json!({ "item": item, "from": "github", "body": text("body", 1, body, 1), "comments": comments }))
                .into_response();
        }
        "checks" => json!({ "item": ITEM, "head_sha": "abc123", "checks": [
            { "kind": "check_run", "name": "build", "status": "completed", "conclusion": "success", "url": null, "updated_at": 2 }] }),
        "events" => {
            let ev = |id: &str, kind: &str, state: Option<&str>, at: u64| {
                json!({ "id": id, "kind": kind, "actor": "dan-gh", "review_state": state, "at": at })
            };
            let mut events = if issue {
                vec![ev("1", "comment", None, 4)]
            } else {
                vec![ev("1", "review", Some("approved"), 3), ev("2", "comment", None, 4)]
            };
            if later {
                events.push(ev("9", "comment", None, 5));
            }
            json!({ "item": id, "events": events })
        }
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

/// The block's text, as `read_forge` and `read_output` give it.
fn text(d: &arugula_testkit::Daemon, block: u64) -> String {
    d.raw("GET", &format!("/api/panes/{block}/tail"), None).1
}

#[test]
fn a_pins_text_comes_from_content_and_its_failure_leaves_the_metadata() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.up.lock().unwrap() = true;
    *f.linked.lock().unwrap() = true;
    *f.content.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("forgepinscontent")
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
    let open = |kind: &str, url: &str| {
        d.post("/api/blocks", json!({ "type": "forge", "config": { kind: url }, "local": true }))["block"]
            .as_u64()
            .unwrap()
    };

    // An issue: its description and comment are in the state and the text.
    let issue = open("issue", "https://github.com/acme/app/issues/3");
    d.wait_for("the issue's text", || text(&d, issue).contains("the first comment says hello"));
    let t = text(&d, issue);
    assert!(t.contains("the issue's description") && t.contains("read through Arugula control"), "{t}");
    let s = state(&d, issue);
    assert_eq!(s["issue"]["item"]["body"], "the issue's description");
    assert_eq!(s["issue"]["item"]["comments"], 1);
    assert_eq!(f.github("acme/app"), 0, "no GitHub call for the text either");

    // Polls go on, and /content is not asked again while nothing is new.
    let (reads, asked) = (f.reads(), f.content_reads());
    std::thread::sleep(Duration::from_millis(1500));
    assert!(f.reads() > reads + 2, "the metadata is read on every poll");
    assert_eq!(f.content_reads(), asked, "the text is kept");

    // A new comment makes the events change: asked again.
    *f.later.lock().unwrap() = true;
    d.wait_for("the later comment", || text(&d, issue).contains("a later comment says more"));
    assert_eq!(f.content_reads(), asked + 1);

    // A PR: the same, with its description.
    let pr = open("pr", "https://github.com/acme/app/pull/1");
    d.wait_for("the PR's text", || text(&d, pr).contains("the PR's description"));
    assert!(text(&d, pr).contains("the first comment says hello"));
    assert_eq!(state(&d, pr)["pr"]["item"]["body"], "the PR's description");
    d.stop();

    // /content failing (an older control): the metadata view, no error in
    // place of the body.
    *f.content.lock().unwrap() = false;
    *f.later.lock().unwrap() = false;
    d.start();
    d.wait_for("the mirror's item", || title(&d, pr) == "from the mirror");
    let s = state(&d, pr);
    assert!(s["error"].is_null() && s["pr"]["item"]["body"] == "", "{s}");
    assert_eq!(s["pr"]["item"]["comments"], 1, "the comment as an event");
    let t = text(&d, pr);
    assert!(!t.contains("description") && !t.contains("hello"), "{t}");
    assert!(t.contains("from the mirror") && t.contains("dan-gh"), "{t}");
    assert!(f.content_reads() > asked + 1, "it tried");
    d.stop();
}
