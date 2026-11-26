//! A control that refuses a body with a plain-text 422 (axum's `Json`
//! rejection) has its words passed on (Arugula #1502): a `watch create` with
//! `standing` against a control from before standing watches says so, not
//! "control answered 422". Against a fake control and a real daemon,
//! through `arugula mcp`.

use std::sync::{Arc, Mutex};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp};

const REJECTION: &str = "Failed to deserialize the JSON body into the target type: unknown field `standing`, expected one of `event`, `match`";

async fn watch(State(asked): State<Arc<Mutex<Vec<Value>>>>, Json(b): Json<Value>) -> Response {
    asked.lock().unwrap().push(b);
    (StatusCode::UNPROCESSABLE_ENTITY, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], REJECTION).into_response()
}

fn serve(asked: Arc<Mutex<Vec<Value>>>) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/daemon/watches", post(watch))
            .with_state(asked);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "jake" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

#[test]
fn a_watch_with_standing_on_an_old_control_says_what_it_doesnt_keep() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (_rt, url) = serve(asked.clone());
    let d = Daemon::child_in(&[], &[], |state| enroll(state, &url));
    let watcher = d.open("hello");
    d.wait(watcher, "idle");
    let mcp = Mcp::bridge(&d, Some(watcher));
    let said = mcp.call("watch", json!({ "kind": "create", "event": "issue.closed", "standing": true })).unwrap_err();
    assert!(said.contains("doesn't keep standing watches or exclude_self yet"), "{said}");
    assert_eq!(asked.lock().unwrap().len(), 1, "control was asked once");
}
