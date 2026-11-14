//! Arugula #992: a session's project inferred from its pane's linked repo
//! follows the repo when it moves to another project, and one set by hand
//! stays. Against a fake control: control says the repo moved
//! (`projects.repo_moved` over the relay socket), and a machine that was
//! away catches up when its relay comes back. Only the label changes: the
//! panes are the same ones, still running.

use std::{
    collections::HashMap,
    process::Command,
    sync::{Arc, Mutex},
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// Each repo's project, as control links it: its id and name.
    links: Arc<Mutex<HashMap<String, (String, String)>>>,
    /// What the daemon said on its relay socket.
    said: Arc<Mutex<Vec<Value>>>,
    /// Sends on the relay socket that's up.
    to_daemon: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// Hang up the relay socket.
    hang_up: Arc<Notify>,
    dials: Arc<Mutex<u32>>,
}

impl Fake {
    fn link(&self, repo: &str, id: &str, name: &str) {
        self.links.lock().unwrap().insert(repo.into(), (id.into(), name.into()));
    }
}

async fn about(State(f): State<Fake>) -> Json<Value> {
    let url = f.url.lock().unwrap().clone();
    Json(json!({ "control": true, "daemon_auth": 2, "url": url }))
}

async fn issue_home(State(f): State<Fake>, Query(q): Query<HashMap<String, String>>) -> Response {
    let repo = q.get("repo").cloned().unwrap_or_default();
    match f.links.lock().unwrap().get(&repo) {
        Some((id, name)) => Json(json!({ "project": id, "name": name, "home": "native" })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{repo} is linked to none of your projects") })))
            .into_response(),
    }
}

async fn dial(State(f): State<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        *f.dials.lock().unwrap() += 1;
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
                () = f.hang_up.notified() => break,
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
            .route("/api/daemon/issue-home", get(issue_home))
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

/// Each session's project id, by its name.
fn projects(d: &arugula_testkit::Daemon) -> HashMap<String, Value> {
    d.get("/api/panes")
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["session_name"].as_str().unwrap_or_default().to_owned(), p["session_project"]["id"].clone()))
        .collect()
}

#[test]
fn an_inferred_project_follows_its_repo_and_one_set_by_hand_stays() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    f.link("acme/app", "prj_1", "One");
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("projectfollow").no_provider().no_tailscale().wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the relay", || f.to_daemon.lock().unwrap().is_some());

    // Two sessions with a pane in a clone of acme/app: both infer prj_1.
    let dir = d.state.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    for args in [&["init", "-q"][..], &["remote", "add", "origin", "git@github.com:acme/app.git"]] {
        assert!(Command::new("git").arg("-C").arg(&dir).args(args).status().unwrap().success());
    }
    let mut panes = vec![];
    for session in ["inferred", "byhand"] {
        let run = d.post("/api/run", json!({ "session": session, "cwd": dir }));
        panes.push(run["pane"].as_u64().unwrap());
    }
    d.wait_for("both inferred", || {
        let p = projects(&d);
        p.get("inferred") == Some(&json!("prj_1")) && p.get("byhand") == Some(&json!("prj_1"))
    });
    // One is put in prj_1 again by hand: it stays there from now on.
    let byhand =
        d.get("/api/panes").as_array().unwrap().iter().find(|p| p["session_name"] == "byhand").unwrap()["session"]
            .clone();
    rt.block_on(async {
        let (mut ws, _) = tokio_tungstenite::connect_async(d.ws("/ws")).await.unwrap();
        let set =
            json!({ "op": "set_session_project", "session": byhand, "project": { "id": "prj_1", "name": "One" } });
        let msg = json!({ "type": "intent", "id": 1, "intent": set });
        ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await.unwrap();
        let by_hand = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(Ok(m)) = ws.next().await {
                if m.to_text().is_ok_and(|t| t.contains("\"project_by_hand\":true")) {
                    return;
                }
            }
        });
        by_hand.await.expect("never set by hand");
        ws.close(None).await.ok();
    });
    let pids = |d: &arugula_testkit::Daemon| -> Vec<Value> {
        let now = d.get("/api/panes");
        panes.iter().map(|id| now.as_array().unwrap().iter().find(|p| p["id"] == *id).unwrap()["pid"].clone()).collect()
    };
    let before = pids(&d);

    // The daemon says it knows the op.
    let ops = f.said.lock().unwrap().iter().find(|v| v["t"] == "ops").cloned().unwrap();
    assert!(ops["ops"].as_array().unwrap().contains(&json!("projects.repo_moved")), "{ops}");

    // acme/app moves to prj_2, and control says so.
    f.link("acme/app", "prj_2", "Two");
    let ask = json!({ "t": "op", "id": "m1", "op": { "op": "projects.repo_moved", "repo": "acme/app" } });
    f.to_daemon.lock().unwrap().as_ref().unwrap().send(ask.to_string()).unwrap();
    d.wait_for("the op answered", || f.said.lock().unwrap().iter().any(|v| v["t"] == "op.done" && v["id"] == "m1"));
    let done = f.said.lock().unwrap().iter().find(|v| v["id"] == "m1").cloned().unwrap();
    assert_eq!(done["answer"]["answer"], "done", "{done}");
    let p = projects(&d);
    assert_eq!((&p["inferred"], &p["byhand"]), (&json!("prj_2"), &json!("prj_1")), "{p:?}");

    // Away while it moves again, to prj_3: it catches up when it's back.
    f.link("acme/app", "prj_3", "Three");
    f.hang_up.notify_waiters();
    d.wait_for("redialled", || *f.dials.lock().unwrap() >= 2 && f.to_daemon.lock().unwrap().is_some());
    d.wait_for("caught up", || projects(&d).get("inferred") == Some(&json!("prj_3")));
    assert_eq!(projects(&d)["byhand"], json!("prj_1"));

    // Linked to none now: the label stays as it was.
    f.links.lock().unwrap().clear();
    let ask = json!({ "t": "op", "id": "m2", "op": { "op": "projects.repo_moved", "repo": "acme/app" } });
    f.to_daemon.lock().unwrap().as_ref().unwrap().send(ask.to_string()).unwrap();
    d.wait_for("the op answered", || f.said.lock().unwrap().iter().any(|v| v["t"] == "op.done" && v["id"] == "m2"));
    assert_eq!(projects(&d)["inferred"], json!("prj_3"));

    // Only the label changed: the same panes, the same processes.
    assert_eq!(pids(&d), before);
}
