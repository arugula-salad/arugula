//! M124: a daemon brings the threads it kept (M61) and its conversations
//! (M81) into control's chat once, signed as itself, then moves `threads/`
//! to `threads.imported/`; a second start imports nothing. One from the
//! last release (`ARUGULA_FEATURES=""` plays it) keeps them, and says
//! nothing of importing, so control's page tells its owner to update it.
//! Control's side of the same call is `arugula-control`'s
//! `chat_wire::a_machines_old_threads_come_in_once…`.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{RawQuery, State, ws::WebSocketUpgrade},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};

const CONV: &str = "c-0000000000000000000000000000000a";

/// A control that records what a daemon tells it.
#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The features each refresh named.
    features: Arc<Mutex<Vec<String>>>,
    imports: Arc<Mutex<Vec<Value>>>,
    readers: Arc<Mutex<Vec<Value>>>,
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn trust(State(f): State<Fake>, RawQuery(q): RawQuery) -> Json<Value> {
    let features = url::form_urlencoded::parse(q.unwrap_or_default().as_bytes())
        .find(|(k, _)| k == "features")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    f.features.lock().unwrap().push(features);
    Json(json!({ "certs": [], "revocations": [] }))
}

async fn import(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    let n: usize = b["threads"]
        .as_array()
        .into_iter()
        .chain(b["archives"].as_array())
        .flatten()
        .map(|t| t["messages"].as_array().map_or(0, Vec::len))
        .sum();
    f.imports.lock().unwrap().push(b);
    Json(json!({ "rooms": 3, "messages": n, "already": 0 }))
}

async fn readers(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    let n = b["targets"].as_array().map_or(0, Vec::len);
    f.readers.lock().unwrap().push(b);
    Json(json!({ "stored": n }))
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
            .route("/api/daemon/trust", get(trust))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/daemon/chat/import", post(import))
            .route("/api/daemon/chat/readers", post(readers))
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

/// The threads an older daemon kept: pane 4's (one quoting it, one from a
/// tailnet login), session 2's, and a conversation with a member.
fn old_threads(state: &std::path::Path) {
    let dir = state.join("threads");
    std::fs::create_dir_all(&dir).unwrap();
    let lines = |v: &[Value]| v.iter().map(|m| format!("{m}\n")).collect::<String>();
    std::fs::write(
        dir.join("pane-4.jsonl"),
        lines(&[
            json!({ "id": 1, "at": 1000, "who": "owner", "name": "Jake", "text": "@alex the mean test fails",
                    "quote": { "pane": 4, "text": "test mean ... FAILED" }, "mentions": ["account:a2"] }),
            json!({ "id": 2, "at": 1100, "who": "tailnet:sam@example.com", "name": "sam", "text": "drive-by" }),
        ]),
    )
    .unwrap();
    std::fs::write(
        dir.join("session-2.jsonl"),
        lines(&[json!({ "id": 1, "at": 900, "who": "account:a2", "name": "alex", "text": "session talk" })]),
    )
    .unwrap();
    let conv = json!({ "id": CONV, "title": "calc", "owner": "owner", "owner_name": "Jake", "created": 500,
                       "members": [{ "who": "account:a2", "name": "alex", "joined": 600 }] });
    std::fs::write(dir.join(format!("conv-{CONV}.json")), conv.to_string()).unwrap();
    std::fs::write(
        dir.join(format!("conv-{CONV}.jsonl")),
        lines(&[json!({ "id": 1, "at": 700, "who": "owner", "name": "Jake", "text": "let's fix calc" })]),
    )
    .unwrap();
}

#[test]
fn a_daemon_brings_its_threads_into_control_once() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("chatimport")
        .labs()
        .no_wisp()
        .no_tailscale()
        .env_remove("ARUGULA_FEATURES")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    old_threads(&d.state);
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the import", || !f.imports.lock().unwrap().is_empty());
    d.wait_for("threads/ moved aside", || d.state.join("threads.imported/pane-4.jsonl").exists());
    assert!(f.features.lock().unwrap()[0].split(',').any(|x| x == "chat-import"), "it says it imports");

    let body = f.imports.lock().unwrap()[0].clone();
    let threads = body["threads"].as_array().unwrap();
    let pane = threads.iter().find(|t| t["kind"] == "pane").unwrap();
    assert_eq!(pane["target"], "4");
    assert_eq!(
        pane["messages"],
        json!([
            { "id": 1, "at": 1000, "account": "a1", "name": "Jake", "text": "@alex the mean test fails",
              "quote": "test mean ... FAILED", "pane": "4", "mentions": ["a2"] },
            { "id": 2, "at": 1100, "account": null, "name": "sam", "text": "drive-by",
              "quote": null, "pane": null, "mentions": [] },
        ]),
        "numbers, authors (by account where they had one) and quotes"
    );
    let session = threads.iter().find(|t| t["kind"] == "session").unwrap();
    assert_eq!((session["target"].as_str(), session["messages"][0]["account"].as_str()), (Some("2"), Some("a2")));
    let archive = &body["archives"][0];
    assert_eq!((archive["id"].as_str(), archive["title"].as_str()), (Some(CONV), Some("calc")));
    assert_eq!(archive["members"], json!([{ "account": "a1", "from": null }, { "account": "a2", "from": 600 }]));
    assert_eq!(archive["messages"][0]["text"], "let's fix calc");
    // Who reads the live panes' and sessions' threads came with it.
    assert!(body["readers"].as_array().is_some_and(|r| !r.is_empty()), "{body}");

    // Nothing is left to import (M125: the daemon keeps no store of its own).
    assert!(!d.state.join("threads").exists());

    // The reader lists stay current: sharing a session with alex's account
    // tells control he reads its thread, and with carol's as a viewer that
    // she only watches it (she reads, and doesn't post).
    let session = body["readers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "session")
        .map(|r| r["target"].as_str().unwrap().parse::<u64>().unwrap())
        .expect("a session's list");
    d.post(
        "/api/acl",
        json!({ "session": session, "principal": "account:a2", "name": "alex", "role": "editor", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    d.post(
        "/api/acl",
        json!({ "session": session, "principal": "account:a4", "name": "carol", "role": "viewer", "history": true,
                "root": DeviceKeys::generate().id() }),
    );
    let alex = json!({ "kind": "session", "target": session.to_string(), "readers": [
        { "account": "a2", "from": null },
        { "account": "a4", "from": null, "watching": true },
    ] });
    d.wait_for("alex's reader list", || {
        f.readers.lock().unwrap().iter().any(|b| b["targets"].as_array().unwrap().contains(&alex))
    });

    // A second start imports nothing.
    d.stop();
    let refreshes = f.features.lock().unwrap().len();
    d.start();
    d.wait_for("a refresh", || f.features.lock().unwrap().len() > refreshes);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(f.imports.lock().unwrap().len(), 1, "imported once");
    assert!(d.state.join("threads.imported/session-2.jsonl").exists());
}

#[test]
fn the_last_release_keeps_its_threads_and_says_nothing_of_importing() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("chatimportold")
        .labs()
        .no_wisp()
        .no_tailscale()
        .env("ARUGULA_FEATURES", "")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    old_threads(&d.state);
    enroll(&d.state, &url);
    d.start();
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    std::thread::sleep(Duration::from_secs(2));
    let said = f.features.lock().unwrap()[0].clone();
    assert!(!said.split(',').any(|x| x == "chat-import"), "{said}: control's page says to update it");
    assert!(f.imports.lock().unwrap().is_empty() && f.readers.lock().unwrap().is_empty());
    assert!(d.state.join("threads/pane-4.jsonl").exists() && !d.state.join("threads.imported").exists());
}
