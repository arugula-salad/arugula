//! `wait`'s cloud targets, a run and agent sessions, served underneath by
//! a short-lived cloud watch
//! (https://app.arugula.io/issues/iss_1545620a176cab44b21084c27493241e):
//! the call asks control nothing until the watch's message arrives on the
//! relay socket, then reads what happened and answers as before. One that
//! doesn't fire by its time, or whose client goes away, cancels its watch;
//! a cloud that keeps no watches is long-polled as before. Against a fake
//! control and a real daemon, through `arugula mcp`.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use axum::{
    Json, Router,
    extract::{
        Request, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp};

const RUN: &str = "run_0123456789abcdef0123456789abcdef";

/// What the fake does with a watch it's asked for.
#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    /// Made, and it fires a moment later: what it watches ends.
    #[default]
    Fire,
    /// Made, and never fires.
    Never,
    /// Made, and what it watches ends, but its message is lost on the way.
    Lost,
    /// Refused: this cloud keeps no watches (404).
    Refuse,
}

#[derive(Clone, Default)]
struct Fake {
    mode: Arc<Mutex<Mode>>,
    /// The run ended; the sessions settled.
    ended: Arc<Mutex<bool>>,
    settled: Arc<Mutex<Vec<String>>>,
    /// Watches made, ever: each one's id is new.
    made: Arc<std::sync::atomic::AtomicUsize>,
    /// Every call: method and path with its query, and its body.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// What the daemon said on its relay socket, and what's for it to hear.
    said: Arc<Mutex<Vec<Value>>>,
    outbox: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn asked(&self, what: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(c, _)| c.starts_with(what)).map(|(_, b)| b.clone()).collect()
    }

    fn reset(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
        *self.ended.lock().unwrap() = false;
        self.settled.lock().unwrap().clear();
        self.calls.lock().unwrap().clear();
    }
}

fn run(f: &Fake) -> Value {
    let ended = *f.ended.lock().unwrap();
    let (state, seq) = if ended { ("succeeded", 3) } else { ("running", 2) };
    let mut r = json!({ "id": RUN, "project": "prj_1", "workspace": "hw_1", "trigger": "manual", "state": state,
        "started_by": "a1", "started_by_agent": "m1", "last_seq": seq, "events": [] });
    if ended {
        r["exit_code"] = json!(0);
    }
    r
}

fn row(f: &Fake, id: &str) -> Value {
    let settled = f.settled.lock().unwrap().iter().any(|s| s == id);
    json!({ "session": id, "state": if settled { "done" } else { "running" }, "settled": settled,
        "workspace": "hw_1", "branch": format!("arugula/{id}"), "head": "abc1234" })
}

async fn any(State(f): State<Fake>, req: Request) -> Response {
    let (method, at) = (req.method().to_string(), req.uri().to_string());
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
    let b: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    f.calls.lock().unwrap().push((format!("{method} {at}"), b.clone()));
    let path = at.split_once('?').map_or(at.as_str(), |(p, _)| p);
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), segs.as_slice()) {
        ("POST", ["api", "daemon", "watches"]) => {
            let mode = *f.mode.lock().unwrap();
            if mode == Mode::Refuse {
                return (StatusCode::NOT_FOUND, Json(json!({ "error": "no route" }))).into_response();
            }
            let id = format!("wch_{}", f.made.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
            if mode == Mode::Lost {
                *f.ended.lock().unwrap() = true;
            }
            if mode == Mode::Fire {
                // What it watches gets there a moment later, and the watch's
                // message goes to its session.
                let (f, id, session) = (f.clone(), id.clone(), b["session"].as_str().unwrap().to_owned());
                let targets = b["targets"].as_array().cloned().unwrap_or_default();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let what = match targets.first().map(|t| t["kind"].as_str().unwrap()) {
                        Some("run") => {
                            *f.ended.lock().unwrap() = true;
                            format!("run {RUN} succeeded")
                        }
                        _ => {
                            let first = targets[0]["id"].as_str().unwrap().to_owned();
                            f.settled.lock().unwrap().push(first.clone());
                            format!("session {first} done")
                        }
                    };
                    let op = json!({ "t": "op", "id": format!("m-{id}"), "op": { "op": "agents.mention",
                        "room": id, "seq": 0, "author": "Arugula", "text": what,
                        "watch": { "id": id, "session": session } } });
                    f.outbox.lock().unwrap().push(op.to_string());
                });
            }
            Json(json!({ "id": id, "state": "live", "expires_ms": 1 })).into_response()
        }
        ("POST", ["api", "daemon", "watches", "cancel"]) => Json(json!({ "cancelled": [b["id"]] })).into_response(),
        ("GET", ["api", "daemon", "runs", _]) => {
            // A long-poll (since) waits for it to end, or its wait.
            if at.contains("since=") {
                for _ in 0..20 {
                    if *f.ended.lock().unwrap() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Json(run(&f)).into_response()
        }
        ("GET", ["api", "daemon", "runs", _, "log"]) => {
            Json(json!({ "run": RUN, "kept": true, "text": "test result: ok. 3 passed\n", "offset": 0,
                "next_offset": 26, "end": true, "note": null }))
            .into_response()
        }
        ("POST", ["api", "daemon", "sessions", "wait"]) => {
            let ids: Vec<String> = serde_json::from_value(b["sessions"].clone()).unwrap();
            let rows: Vec<Value> = ids.iter().map(|id| row(&f, id)).collect();
            let settled: Vec<&Value> = rows.iter().filter(|r| r["settled"] == true).map(|r| &r["session"]).collect();
            Json(json!({ "settled": settled, "sessions": rows })).into_response()
        }
        ("GET", ["api", "daemon", "sessions", id]) => Json(json!({ "session": row(&f, id),
            "answer": { "text": format!("{id}'s whole answer"), "at": 1, "cut": false } }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no route {method} {path}") }))).into_response(),
    }
}

async fn dial(State(f): State<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        let (mut tx, mut rx) = ws.split();
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                m = rx.next() => {
                    let Some(Ok(Message::Text(t))) = m else { if m.is_none() { break } else { continue } };
                    if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                        f.said.lock().unwrap().push(v);
                    }
                }
                _ = tick.tick() => {
                    let out: Vec<String> = std::mem::take(&mut *f.outbox.lock().unwrap());
                    for o in out {
                        let _ = tx.send(Message::Text(o.into())).await;
                    }
                }
            }
        }
    })
}

fn serve(f: Fake) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let app = Router::new()
            .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/relay/dial", get(dial))
            .fallback(any)
            .with_state(f);
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

fn summary(v: &Value) -> &str {
    v["summary"].as_str().unwrap_or_default()
}

#[test]
fn wait_on_a_run_or_sessions_is_a_watch_underneath() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let d = Daemon::child_in(&[], &[], |state| enroll(state, &url));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let mcp = Mcp::bridge(&d, None);
    let long_polled = |f: &Fake| f.calls.lock().unwrap().iter().any(|(c, _)| c.contains("since="));

    // A run: its watch fires when it ends, then it's read as before.
    let waited = mcp.call("wait", json!({ "run": RUN, "until": "command_end", "timeout": 30 })).unwrap();
    assert_eq!(waited["state"], "succeeded", "{waited}");
    assert!(summary(&waited).starts_with(&format!("{RUN} [succeeded, exit 0]")), "{waited}");
    assert!(summary(&waited).contains("ended.") && summary(&waited).contains("3 passed"), "{waited}");
    let made = f.asked("POST /api/daemon/watches");
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0]["targets"], json!([{ "kind": "run", "id": RUN }]), "{made:?}");
    assert!(made[0]["session"].as_str().unwrap().starts_with("wait_"), "a session of its own: {made:?}");
    assert!(!long_polled(&f), "no long-poll while it watched");
    assert!(f.asked("POST /api/daemon/watches/cancel").is_empty(), "a fired watch isn't cancelled");

    // Agent sessions, any of them: the one that settled read whole, the
    // other still going.
    f.reset(Mode::Fire);
    let waited = mcp.call("wait", json!({ "agent_session": "ses_a ses_b", "until": "done", "timeout": 30 })).unwrap();
    assert_eq!(waited["state"], "settled", "{waited}");
    let s = summary(&waited);
    assert!(s.starts_with("Finished its turn: ses_a.") && s.contains("ses_a's whole answer"), "{waited}");
    assert!(s.contains("ses_a done") && s.contains("on hw_1 · arugula/ses_a at abc1234"), "{waited}");
    assert!(s.contains("Still going (wait with agent_session \"ses_b\" for them):\nses_b running"), "{waited}");
    let made = f.asked("POST /api/daemon/watches");
    assert_eq!(made[0]["targets"], json!([{ "kind": "session", "id": "ses_a" }, { "kind": "session", "id": "ses_b" }]));
    assert_eq!(f.asked("POST /api/daemon/sessions/wait").len(), 1, "read once, after it fired");

    // Not by its time: still running, and its watch is cancelled.
    f.reset(Mode::Never);
    let waited = mcp.call("wait", json!({ "run": RUN, "until": "command_end", "timeout": 1 })).unwrap();
    assert_eq!(waited["state"], "still running", "{waited}");
    assert!(summary(&waited).contains("hasn't ended after 1s: call wait with run again"), "{waited}");
    d.wait_for("its watch cancelled", || f.asked("POST /api/daemon/watches/cancel").len() == 1);
    let made = f.asked("POST /api/daemon/watches");
    let cancelled = f.asked("POST /api/daemon/watches/cancel");
    assert_eq!(cancelled[0]["session"], made[0]["session"], "{cancelled:?}");
    assert!(!long_polled(&f));
    let waited = mcp.call("wait", json!({ "agent_session": "ses_a", "until": "done", "timeout": 1 })).unwrap();
    assert_eq!(waited["state"], "still running", "{waited}");
    assert!(summary(&waited).starts_with("None finished after 1s") && summary(&waited).contains("ses_a running"));
    d.wait_for("its watch cancelled", || f.asked("POST /api/daemon/watches/cancel").len() == 2);

    // Its client goes away: its watch is cancelled.
    f.reset(Mode::Never);
    let waiter = Mcp::bridge(&d, None);
    let gone = waiter.call_within(
        "wait",
        json!({ "run": RUN, "until": "command_end", "timeout": 60 }),
        Duration::from_secs(3),
    );
    assert!(gone.is_none(), "{gone:?}");
    assert_eq!(f.asked("POST /api/daemon/watches").len(), 1);
    drop(waiter);
    d.wait_for("its watch cancelled", || f.asked("POST /api/daemon/watches/cancel").len() == 1);

    // Its message lost on the way: at its time, it's read as ended.
    f.reset(Mode::Lost);
    let waited = mcp.call("wait", json!({ "run": RUN, "until": "command_end", "timeout": 1 })).unwrap();
    assert_eq!(waited["state"], "succeeded", "{waited}");
    assert!(summary(&waited).contains("ended."), "{waited}");

    // A cloud that keeps no watches: long-polled, as before.
    f.reset(Mode::Refuse);
    let flag = f.ended.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        *flag.lock().unwrap() = true;
    });
    let waited = mcp.call("wait", json!({ "run": RUN, "until": "command_end", "timeout": 30 })).unwrap();
    assert_eq!(waited["state"], "succeeded", "{waited}");
    assert!(long_polled(&f), "long-polled");
}
