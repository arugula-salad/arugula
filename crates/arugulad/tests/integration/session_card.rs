//! Arugula #1019: control puts a machine's session on a card
//! (`sessions.set_card` over the relay socket), for Put on a card on the
//! project's Sessions page. Every pane of the session is on it, it's kept
//! across a restart, and `null` takes it back to its branch's (none here).
//! Until control asks, nothing changes. Against a fake control.

use std::sync::{Arc, Mutex};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocketUpgrade},
    },
    response::Response,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// What the daemon said on its relay socket.
    said: Arc<Mutex<Vec<Value>>>,
    /// Sends on the relay socket that's up.
    to_daemon: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn dial(State(f): State<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        let (mut tx, mut rx) = ws.split();
        let (send, mut queue) = mpsc::unbounded_channel();
        *f.to_daemon.lock().unwrap() = Some(send);
        loop {
            tokio::select! {
                m = rx.next() => {
                    let Some(Ok(m)) = m else { break };
                    let Message::Text(t) = m else { continue };
                    if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                        f.said.lock().unwrap().push(v);
                    }
                }
                Some(t) = queue.recv() => {
                    let _ = tx.send(Message::Text(t.into())).await;
                }
            }
        }
        *f.to_daemon.lock().unwrap() = None;
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

/// Each pane's card, as its chip reads it (`on_issue`), by its id.
fn cards(d: &arugula_testkit::Daemon, panes: &[u64]) -> Vec<Value> {
    let now = d.get("/api/panes");
    panes
        .iter()
        .map(|id| now.as_array().unwrap().iter().find(|p| p["id"] == *id).unwrap()["on_issue"]["issue"].clone())
        .collect()
}

/// Ask the op, and wait for its answer.
fn ask(d: &arugula_testkit::Daemon, f: &Fake, id: &str, op: Value) -> Value {
    let ask = json!({ "t": "op", "id": id, "op": op });
    f.to_daemon.lock().unwrap().as_ref().unwrap().send(ask.to_string()).unwrap();
    d.wait_for("the op answered", || f.said.lock().unwrap().iter().any(|v| v["t"] == "op.done" && v["id"] == id));
    f.said.lock().unwrap().iter().find(|v| v["t"] == "op.done" && v["id"] == id).unwrap()["answer"].clone()
}

#[test]
fn control_puts_a_session_on_a_card_and_takes_it_back() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("sessioncard").no_provider().no_tailscale().wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());
    // Its operations come just after it's on the relay.
    d.wait_for("the daemon's operations", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let ops = f.said.lock().unwrap().iter().find(|v| v["t"] == "ops").cloned().unwrap();
    assert!(ops["ops"].as_array().unwrap().contains(&json!("sessions.set_card")), "{ops}");

    // Two panes in one session, one in another; none is on a card.
    let run = |session: &str| d.post("/api/run", json!({ "session": session }))["pane"].as_u64().unwrap();
    let (a, b, other) = (run("release"), run("release"), run("scratch"));
    let session =
        d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == a).unwrap()["session"].as_u64().unwrap();
    // Until control asks, nothing changes.
    assert_eq!(cards(&d, &[a, b, other]), [Value::Null, Value::Null, Value::Null]);

    let put = json!({ "op": "sessions.set_card", "session": session.to_string(), "project": "prj_1",
        "project_name": "One", "card": "#7" });
    assert_eq!(ask(&d, &f, "c1", put)["answer"], "done");
    // Every pane of the session follows; the other session doesn't.
    assert_eq!(cards(&d, &[a, b, other]), [json!("#7"), json!("#7"), Value::Null]);

    // Kept, as set by hand, across a restart.
    d.stop();
    let layout: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
    let saved = layout["mux"]["sessions"].as_array().unwrap().iter().find(|s| s["id"] == session).unwrap().clone();
    assert_eq!(
        (&saved["work"], &saved["work_by_hand"], &saved["project"]["name"]),
        (&json!({ "project": "prj_1", "item": "#7" }), &json!(true), &json!("One")),
        "{saved}"
    );
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());
    d.wait_for("the panes back on #7", || cards(&d, &[a, b]) == [json!("#7"), json!("#7")]);

    // `null`: back to its branch's card, and these have no branch.
    let back = json!({ "op": "sessions.set_card", "session": session.to_string(), "project": "prj_1", "card": null });
    assert_eq!(ask(&d, &f, "c2", back)["answer"], "done");
    assert_eq!(cards(&d, &[a, b]), [Value::Null, Value::Null]);
    d.stop();
    let layout: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
    let saved = layout["mux"]["sessions"].as_array().unwrap().iter().find(|s| s["id"] == session).unwrap().clone();
    assert!(saved.get("work").is_none() && saved.get("work_by_hand").is_none(), "a branch may give it one: {saved}");
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());

    // No such session, or not a card: refused, with why.
    let none = json!({ "op": "sessions.set_card", "session": "999", "project": "prj_1", "card": "#7" });
    assert_eq!(ask(&d, &f, "c3", none)["answer"], "failed");
    let github = json!({ "op": "sessions.set_card", "session": session.to_string(), "project": "prj_1",
        "card": "acme/app#7" });
    assert_eq!(ask(&d, &f, "c4", github)["answer"], "failed");
}

/// Arugula #1166: Move to project moves a session one by one
/// (`sessions.set_project`). It's in the project, set by hand, so its repo
/// doesn't move it back.
#[test]
fn control_moves_a_session_to_a_project_by_hand() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("sessionproject").no_provider().no_tailscale().wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());
    // Its operations come just after it's on the relay.
    d.wait_for("the daemon's operations", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let ops = f.said.lock().unwrap().iter().find(|v| v["t"] == "ops").cloned().unwrap();
    assert!(ops["ops"].as_array().unwrap().contains(&json!("sessions.set_project")), "{ops}");

    let a = d.post("/api/run", json!({ "session": "release" }))["pane"].as_u64().unwrap();
    let session =
        d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == a).unwrap()["session"].as_u64().unwrap();
    let move_it = json!({ "op": "sessions.set_project", "session": session.to_string(), "project": "prj_2",
        "project_name": "Factorio mods" });
    assert_eq!(ask(&d, &f, "p1", move_it)["answer"], "done");
    let chip =
        d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == a).unwrap()["session_project"].clone();
    assert_eq!(chip, json!({ "id": "prj_2", "name": "Factorio mods" }));
    d.stop();
    let layout: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap()).unwrap();
    let saved = layout["mux"]["sessions"].as_array().unwrap().iter().find(|s| s["id"] == session).unwrap().clone();
    assert_eq!((&saved["project"]["id"], &saved["project_by_hand"]), (&json!("prj_2"), &json!(true)), "{saved}");
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());

    // No such session, or not a project: refused, with why.
    let none = json!({ "op": "sessions.set_project", "session": "999", "project": "prj_2", "project_name": "x" });
    assert_eq!(ask(&d, &f, "p2", none)["answer"], "failed");
    let personal = json!({ "op": "sessions.set_project", "session": session.to_string(), "project": "personal",
        "project_name": "x" });
    assert_eq!(ask(&d, &f, "p3", personal)["answer"], "failed");
}
