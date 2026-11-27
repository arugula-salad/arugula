//! A terminal running Claude Code with Arugula's hooks waits on its person
//! (Arugula #1514): Claude's `Notification` hook, through `arugula
//! attention needs-input`, puts the pane in the list of asks control is
//! told, as a permission for `permission_prompt` and a question for any
//! other notification. A pane that needs input some other way isn't in it.
//! Against a fake control; a machine that joined it.

#![cfg(unix)]

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::agentd::Daemon;
use serde_json::{Value, json};

/// The lists of asks a fake control was told (the bodies of `PUT
/// /api/daemon/asks`, in order).
#[derive(Clone, Default)]
struct FakeControl {
    asks: Arc<Mutex<Vec<Value>>>,
}

async fn fake_control(f: FakeControl) -> String {
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post, put},
    };
    async fn told(State(f): State<FakeControl>, Json(b): Json<Value>) -> Json<Value> {
        f.asks.lock().unwrap().push(b);
        Json(json!({}))
    }
    let ok = || async { Json(json!({})) };
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(ok))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(ok))
        // Every session is in the owner's default project (Arugula #1163).
        .route(
            "/api/daemon/issue-home",
            get(|| async {
                Json(json!({ "project": "prj_1", "name": "One", "home": "native",
                    "owner": { "kind": "personal", "account": "a1" } }))
            }),
        )
        .route(arugula_control_wire::boards::AGENT_BLOCK_STATE, put(ok))
        .route(arugula_control_wire::boards::AGENT_MACHINE_SESSIONS, put(ok))
        .route(arugula_control_wire::boards::AGENT_PANE_ASKS, put(told))
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

/// A daemon that joined the fake control at `url`.
fn joined(url: String) -> Daemon {
    Daemon::child_in(&[], &[], |state| {
        let keys = arugula_e2e::DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
        let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
        let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
        std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
    })
}

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// A shell pane, quiet.
fn terminal(d: &Daemon) -> u64 {
    let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.wait_for("a shell", || {
        d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == pane && p["running"] == true)
    });
    pane
}

/// What Claude Code's hook does in `pane`: `arugula attention needs-input`
/// with the hook's input on stdin.
fn hook(d: &Daemon, pane: u64, input: Value) {
    let mut child = arugula_testkit::command(cli_bin())
        .arg("--socket")
        .arg(d.sock())
        .args(["attention", "needs-input", "--pane", &format!("%{pane}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn notification(kind: Option<&str>) -> Value {
    let mut h = json!({ "hook_event_name": "Notification", "message": "Claude needs your permission to read x" });
    if let Some(k) = kind {
        h["notification_type"] = k.into();
    }
    h
}

/// The newest list of asks that has `pane` in it, as its entry.
async fn ask_of(f: &FakeControl, pane: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found = f
            .asks
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|b| b["asks"].as_array()?.iter().find(|a| a["pane"] == pane).cloned());
        if let Some(a) = found {
            return a;
        }
        assert!(Instant::now() < deadline, "control was never told %{pane} waits: {:?}", f.asks.lock().unwrap());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hooked_terminal_s_permission_prompt_is_a_permission_ask() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let pane = terminal(&d);
    d.wait_for("its session's project", || {
        d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == pane && !p["session_project"].is_null())
    });
    hook(&d, pane, notification(Some("permission_prompt")));
    let ask = ask_of(&fake, pane).await;
    assert_eq!(ask["kind"], "permission", "{ask}");
    assert_eq!(ask["project"], "prj_1", "its session's project, as a block state's: {ask}");
    let mut keys: Vec<_> = ask.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["kind", "pane", "project", "since"],
        "no parent, and a person isn't the only one who may answer: {ask}"
    );
    assert!(ask["since"].as_u64().is_some_and(|s| s > 0), "{ask}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hooked_terminal_s_other_notification_is_a_question_ask() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let pane = terminal(&d);
    hook(&d, pane, notification(Some("elicitation_dialog")));
    assert_eq!(ask_of(&fake, pane).await["kind"], "question");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_that_needs_input_without_a_hook_is_no_ask() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let plain = terminal(&d);
    d.post(&format!("/api/panes/{plain}/attention"), json!({ "state": "needs_input", "why": "waiting" }));
    // A pane that is told after it: when it's in a list, the other was
    // looked at too.
    let hooked = terminal(&d);
    hook(&d, hooked, notification(Some("permission_prompt")));
    ask_of(&fake, hooked).await;
    for b in fake.asks.lock().unwrap().iter() {
        assert!(b["asks"].as_array().unwrap().iter().all(|a| a["pane"] != plain), "%{plain} is no ask: {b}");
    }
}

/// The pane already needed input (its screen said so) when the hook says
/// what for: the ask is the hook's.
#[tokio::test(flavor = "multi_thread")]
async fn a_hook_after_a_plain_need_says_what_it_asks() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let pane = terminal(&d);
    d.post(&format!("/api/panes/{pane}/attention"), json!({ "state": "needs_input", "why": "waiting" }));
    hook(&d, pane, notification(Some("permission_prompt")));
    assert_eq!(ask_of(&fake, pane).await["kind"], "permission");
}
