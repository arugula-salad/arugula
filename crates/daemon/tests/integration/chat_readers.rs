//! A daemon tells control who reads each pane's and session's thread while
//! control has chat on for its account or team, and says nothing while
//! it's off: chat's on/off is control's, not a Labs flag (native #203).
//! The threads an older daemon kept itself (M61) are no longer brought
//! into control: they stay on disk, untouched.

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

/// A control that records what a daemon tells it.
#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// The features each refresh named.
    features: Arc<Mutex<Vec<String>>>,
    /// Control's chat setting for the machine's team, sent with the flags.
    chat: Arc<Mutex<bool>>,
    /// Any call to the import route of M124, which is gone.
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
    let chat = *f.chat.lock().unwrap();
    Json(json!({ "certs": [], "revocations": [], "flags": { "account": {}, "team": { "chat": chat } } }))
}

async fn import(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    f.imports.lock().unwrap().push(b);
    Json(json!({ "rooms": 0, "messages": 0, "already": 0 }))
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

/// The threads an older daemon kept (M61): one pane's.
fn old_threads(state: &std::path::Path) {
    let dir = state.join("threads");
    std::fs::create_dir_all(&dir).unwrap();
    let m = json!({ "id": 1, "at": 1000, "who": "owner", "name": "Jake", "text": "the mean test fails" });
    std::fs::write(dir.join("pane-4.jsonl"), format!("{m}\n")).unwrap();
}

#[test]
fn reader_lists_follow_controls_chat_setting_and_old_threads_stay_put() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    *f.chat.lock().unwrap() = true;
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("chatreaders")
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
    d.wait_for("a refresh", || !f.features.lock().unwrap().is_empty());
    let said = f.features.lock().unwrap()[0].clone();
    assert!(!said.split(',').any(|x| x == "chat-import"), "{said}: it brings nothing in");

    // Sharing a session with alex's account tells control he reads its
    // thread, and with carol's as a viewer that she only watches it (she
    // reads, and doesn't post).
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
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
    let list = json!({ "kind": "session", "target": session.to_string(), "readers": [
        { "account": "a2", "from": null },
        { "account": "a4", "from": null, "watching": true },
    ] });
    d.wait_for("alex's and carol's reader list", || {
        f.readers.lock().unwrap().iter().any(|b| b["targets"].as_array().unwrap().contains(&list))
    });

    // Chat off in control: a new start, which would send them all again
    // with chat on, sends nothing.
    *f.chat.lock().unwrap() = false;
    d.stop();
    let (refreshes, sent) = (f.features.lock().unwrap().len(), f.readers.lock().unwrap().len());
    d.start();
    d.wait_for("a refresh", || f.features.lock().unwrap().len() > refreshes);
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(f.readers.lock().unwrap().len(), sent, "chat is off for it in control");

    // The old threads were never sent anywhere, and stay where they were.
    assert!(f.imports.lock().unwrap().is_empty());
    assert!(d.state.join("threads/pane-4.jsonl").exists() && !d.state.join("threads.imported").exists());
}
