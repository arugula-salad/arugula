//! M170: a GitHub block's drafts on a repo linked to a project are the
//! platform's rows. Against a fake control: while the platform can't be
//! reached, an agent's draft stays in `layout.json`; once the repo is
//! linked, the daemon uploads it once, drops its copy and shows the
//! platform's; a restart uploads nothing; a restart that lost the drop (an
//! old `layout.json` put back) uploads it again and the platform answers
//! with the same row; and a new agent draft goes straight to the platform.
//! The platform's side is `arugula-platform`'s `forge::drafts_tests`.

use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

/// A control whose platform keeps drafts, as `arugula-platform` does: one
/// row per import key.
#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The platform answers (else it's down: a 502).
    up: Arc<Mutex<bool>>,
    drafts: Arc<Mutex<Vec<Value>>>,
    /// Every create asked for, with its import key.
    creates: Arc<Mutex<Vec<Value>>>,
}

impl Fake {
    fn up(&self) -> bool {
        *self.up.lock().unwrap()
    }
    fn rows(&self) -> Vec<Value> {
        self.drafts.lock().unwrap().clone()
    }
    fn creates(&self) -> usize {
        self.creates.lock().unwrap().len()
    }
}

const PROJECT: &str = "prj_00000000000000000000000000000001";

fn down() -> Response {
    (StatusCode::BAD_GATEWAY, Json(json!({ "error": "the platform is down" }))).into_response()
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn repo(State(f): State<Fake>, Path((owner, name)): Path<(String, String)>) -> Response {
    if !f.up() {
        return down();
    }
    let linked = format!("{owner}/{name}").eq_ignore_ascii_case("acme/app");
    Json(json!({ "repo": format!("{owner}/{name}"), "linked": linked,
                 "project": linked.then_some(PROJECT), "repo_id": linked.then_some(77) }))
    .into_response()
}

async fn create(State(f): State<Fake>, Json(b): Json<Value>) -> Response {
    if !f.up() {
        return down();
    }
    f.creates.lock().unwrap().push(b.clone());
    let key = format!("geek:{}", b["import_key"].as_str().unwrap_or_default());
    let mut rows = f.drafts.lock().unwrap();
    if let Some(d) = rows.iter().find(|d| d["import_key"] == key.as_str()) {
        return Json(json!({ "draft": d, "created": false })).into_response();
    }
    let d = json!({
        "id": format!("dft_{:032x}", rows.len() + 1), "project": PROJECT, "repo": "acme/app", "repo_id": 77,
        "number": b["number"], "kind": b["kind"], "event": b["event"], "title": null, "body": b["body"],
        "state": "waiting", "author": "a1", "agent": b["agent"], "machine": "geek", "created_at": 1,
        "updated_at": 1, "import_key": key,
    });
    rows.push(d.clone());
    Json(json!({ "draft": d, "created": true })).into_response()
}

async fn list(State(f): State<Fake>) -> Response {
    if !f.up() {
        return down();
    }
    let rows: Vec<Value> = f.rows().into_iter().filter(|d| d["state"] == "waiting").collect();
    Json(json!({ "drafts": rows })).into_response()
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
            .route("/api/forge/drafts", post(create))
            .route("/api/projects/{project}/drafts", get(list))
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

/// A `gh` with no login, so the block reads nothing (and never with the
/// person's own).
fn no_gh(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let gh = dir.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default())
}

/// The block's waiting drafts: their ids.
fn waiting(d: &arugula_testkit::Daemon, block: u64) -> Vec<String> {
    let s = d.get(&format!("/api/blocks/{block}"))["state"].clone();
    s["drafts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| x["status"] == "waiting")
        .map(|x| x["id"].as_str().unwrap().to_owned())
        .collect()
}

fn layout(d: &arugula_testkit::Daemon) -> String {
    std::fs::read_to_string(d.state.join("layout.json")).unwrap_or_default()
}

#[test]
fn a_linked_repos_drafts_go_up_once() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("forgedrafts")
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_FORGE_POLL_MS", "200,200")
        .env("ARUGULA_GITHUB_API", format!("{url}/github"))
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    let path = no_gh(&d.state.join("bin"));
    d.set_env("PATH", path);
    enroll(&d.state, &url);
    d.start();
    let block = d.post(
        "/api/blocks",
        json!({ "type": "forge", "config": { "pr": "https://github.com/acme/app/pull/1" }, "local": true }),
    )["block"]
        .as_u64()
        .unwrap();

    // The platform is down: the agent's draft stays on the block.
    let one = d.post(&format!("/api/blocks/{block}/call/comment"), json!({ "body": "kept here", "agent": true }));
    let local = one["draft"].as_str().unwrap().to_owned();
    assert!(!local.starts_with("dft_"), "{one}");
    assert_eq!(waiting(&d, block), std::slice::from_ref(&local));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(f.creates(), 0);
    d.stop();
    assert!(layout(&d).contains("kept here"), "in layout.json");
    let before = layout(&d);

    // Linked, and the platform answers: it goes up once, and the block
    // shows the platform's.
    *f.up.lock().unwrap() = true;
    d.start();
    d.wait_for("the upload", || !f.rows().is_empty());
    let row = f.rows()[0].clone();
    assert_eq!((row["body"].as_str(), row["kind"].as_str()), (Some("kept here"), Some("comment")));
    assert_eq!(row["import_key"].as_str(), Some(format!("geek:acme/app#1/{local}").as_str()));
    let id = row["id"].as_str().unwrap().to_owned();
    d.wait_for("the platform's draft on the block", || waiting(&d, block) == [id.clone()]);
    d.stop();
    let after = layout(&d);
    assert!(!after.contains("kept here") && after.contains(&id), "dropped, and noted: {after}");

    // A restart uploads nothing.
    d.start();
    d.wait_for("the platform's draft again", || waiting(&d, block) == [id.clone()]);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(f.creates(), 1);

    // A restart that lost the drop: it goes up again, and is the same row.
    d.stop();
    std::fs::write(d.state.join("layout.json"), &before).unwrap();
    d.start();
    d.wait_for("the second upload", || f.creates() == 2);
    d.wait_for("the same draft", || waiting(&d, block) == [id.clone()]);
    assert_eq!(f.rows().len(), 1, "one row");

    // A new agent draft goes straight to the platform.
    let two = d.post(&format!("/api/blocks/{block}/call/review"), json!({ "event": "approve", "agent": true }));
    assert!(two["draft"].as_str().unwrap().starts_with("dft_"), "{two}");
    assert_eq!(f.rows().len(), 2);
    assert_eq!((f.rows()[1]["kind"].as_str(), f.rows()[1]["event"].as_str()), (Some("review"), Some("APPROVE")));
    d.stop();
    assert!(!layout(&d).contains("\"method\":\"review\""), "not kept on the block");
}
