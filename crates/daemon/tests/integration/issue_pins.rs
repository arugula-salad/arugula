//! M203: a project's own issue (`iss_…`) as an issue block is a pin onto
//! the platform's issue, read as this machine for its owner. Against a fake
//! control whose platform has issue #1 in a's personal project:
//! - `iss_…`, and `#1` with a project, open the same pin; it reads the
//!   issue, and `layout.json` keeps only its id, number, project and title;
//! - with the platform down, what it said last stays, marked stale, and a
//!   restart then shows the kept title, stale, never a blank block;
//! - back up, it's read again; refused (no grant), nothing of it is shown
//!   and the refusal says why.
//!
//! The platform's side of these reads is `arugula-platform`'s
//! `agents_tests::a_machine_reads_its_owners_issue_with_a_grant`.

use std::sync::{Arc, Mutex};

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

const ID: &str = "iss_0123456789abcdef0123456789abcdef";
const PROJECT: &str = "prj_00000000000000000000000000000001";

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The platform answers (else a 502).
    up: Arc<Mutex<bool>>,
    /// The machine has its grant (else a 403 saying how).
    granted: Arc<Mutex<bool>>,
    title: Arc<Mutex<String>>,
    reads: Arc<Mutex<u32>>,
}

impl Fake {
    fn set(&self, up: bool, granted: bool) {
        *self.up.lock().unwrap() = up;
        *self.granted.lock().unwrap() = granted;
    }
}

fn issue(title: &str) -> Value {
    json!({ "id": ID, "project": PROJECT, "number": 1, "title": title, "body": "on deploy", "state": "open",
            "close_reason": null, "author": "a1", "agent": null, "labels": ["bug"], "assignees": [],
            "github": null, "created_at": 1, "updated_at": 2, "closed_at": null })
}

fn answer(f: &Fake) -> Response {
    if !*f.up.lock().unwrap() {
        return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "the platform is down" }))).into_response();
    }
    if !*f.granted.lock().unwrap() {
        let error = "geek has no threads.read grant over Personal: `arugula grants add geek threads.read --project …`";
        return (StatusCode::FORBIDDEN, Json(json!({ "error": error, "reason": "agent_ungranted" }))).into_response();
    }
    *f.reads.lock().unwrap() += 1;
    Json(issue(&f.title.lock().unwrap())).into_response()
}

async fn by_id(State(f): State<Fake>, Path(id): Path<String>) -> Response {
    if id != ID {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such issue" }))).into_response();
    }
    answer(&f)
}

async fn by_number(State(f): State<Fake>, Path((project, n)): Path<(String, u64)>) -> Response {
    if project != "personal" || n != 1 {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such issue" }))).into_response();
    }
    answer(&f)
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
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
            .route("/api/daemon/issues/{id}", get(by_id))
            .route("/api/daemon/projects/{project}/issues/{number}", get(by_number))
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

fn state(d: &arugula_testkit::Daemon, block: u64) -> Value {
    d.get(&format!("/api/blocks/{block}"))["state"].clone()
}

fn refresh(d: &arugula_testkit::Daemon, block: u64) -> Value {
    d.post(&format!("/api/blocks/{block}/call/refresh"), json!({}))
}

#[test]
fn a_native_issue_block_is_a_pin_read_as_the_machine() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    f.set(true, true);
    *f.title.lock().unwrap() = "The canary fails".into();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("issuepins")
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_FORGE_POLL_MS", "200,200")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    let open = |config: Value| {
        d.post("/api/blocks", json!({ "type": "forge", "config": config, "local": true }))["block"].as_u64().unwrap()
    };

    // By its id, and by its number in a project: the same pin.
    let pinned = open(json!({ "issue": ID }));
    let numbered = open(json!({ "issue": "#1", "project": "personal" }));
    for b in [pinned, numbered] {
        d.wait_for("the platform's issue", || state(&d, b)["issue"]["title"] == "The canary fails");
        let s = state(&d, b);
        assert_eq!(
            (&s["native"], &s["pin"]["kind"], &s["pin"]["global_id"]),
            (&json!(true), &json!("issue"), &json!(ID))
        );
        assert_eq!((&s["number"], &s["project"], &s["error"]), (&json!(1), &json!(PROJECT), &Value::Null));
    }
    let (_, text) = d.raw("GET", &format!("/api/panes/{pinned}/capture?format=text"), None);
    assert!(text.contains("#1 The canary fails (open)") && text.contains("on deploy"), "{text}");

    // It changed on the platform: a refresh (the web's, when it hears so)
    // reads it at once.
    *f.title.lock().unwrap() = "The canary fails twice".into();
    assert_eq!(refresh(&d, pinned)["issue"]["title"], "The canary fails twice");

    // The platform down: what it said last, marked stale.
    f.set(false, true);
    let s = refresh(&d, pinned);
    assert_eq!(s["issue"]["title"], "The canary fails twice", "{s}");
    assert!(s["pin"]["stale"].as_str().is_some_and(|w| w.contains("didn't answer")), "{s}");
    d.stop();
    let layout = std::fs::read_to_string(d.state.join("layout.json")).unwrap();
    assert!(layout.contains(ID) && layout.contains("The canary fails twice"), "the pin is kept: {layout}");
    assert!(!layout.contains("on deploy"), "not its body");

    // A restart with it still down: the kept title, stale, never blank.
    d.start();
    d.wait_for("read once", || state(&d, pinned)["loading"] == false);
    let s = state(&d, pinned);
    assert_eq!((&s["title"], &s["number"], &s["issue"]), (&json!("The canary fails twice"), &json!(1), &Value::Null));
    assert!(s["pin"]["stale"].is_string(), "{s}");

    // Back, without the grant: nothing of it, and why.
    f.set(true, false);
    let s = refresh(&d, pinned);
    assert!(s["issue"].is_null() && s["pin"]["stale"].is_null(), "{s}");
    assert!(s["error"].as_str().is_some_and(|e| e.contains("threads.read")), "{s}");

    // Granted again: read.
    f.set(true, true);
    let reads = *f.reads.lock().unwrap();
    assert_eq!(refresh(&d, pinned)["issue"]["title"], "The canary fails twice");
    assert!(*f.reads.lock().unwrap() > reads);
    d.stop();
}
