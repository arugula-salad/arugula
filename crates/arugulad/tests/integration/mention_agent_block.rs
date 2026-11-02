//! #611: an `@machine/name` mention that names an agent block reaches it as
//! its next prompt, between turns, and its turn reports #540's `working`
//! then `read`. Against a fake control (the relay socket sends the
//! `agents.mention` op and takes the daemon's answer and its turn reports)
//! and a real daemon with a fake-ACP agent block.

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
    routing::{get, post, put},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Fake {
    /// What the daemon said on its relay socket.
    said: Arc<Mutex<Vec<Value>>>,
    /// Asks for the relay socket to send.
    outbox: Arc<Mutex<Vec<String>>>,
    /// The turn reports it PUT, in order.
    states: Arc<Mutex<Vec<Value>>>,
}

async fn dial(State(f): State<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        let (mut tx, mut rx) = ws.split();
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
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

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let state = f.clone();
        let app = Router::new()
            .route(
                "/control.json",
                get(|| async { Json(json!({ "control": true, "daemon_auth": 2, "url": "http://127.0.0.1" })) }),
            )
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/relay/dial", get(dial))
            .route(
                "/api/daemon/chat/mention-state",
                put(|State(f): State<Fake>, Json(b): Json<Value>| async move {
                    f.states.lock().unwrap().push(b);
                    Json(json!({}))
                }),
            )
            .with_state(state);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
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

/// `@machine/worker slowly`, message `seq` of room `ch_x`.
fn mention(id: &str, seq: i64) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "ch_x", "room_name": "general",
        "seq": seq, "author": "Ada", "text": "slowly", "session": "worker" } })
    .to_string()
}

fn answer(f: &Fake, id: &str) -> Option<Value> {
    f.said.lock().unwrap().iter().find(|v| v["t"] == "op.done" && v["id"] == id).map(|v| v["answer"].clone())
}

fn states(f: &Fake) -> Vec<(i64, String)> {
    let all = f.states.lock().unwrap();
    all.iter().map(|s| (s["seq"].as_i64().unwrap(), s["state"].as_str().unwrap().to_owned())).collect()
}

#[test]
fn a_mention_naming_an_agent_block_is_its_next_prompt_and_its_turn_is_working_then_read() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let sessions = std::env::temp_dir().join(format!("ilg-mab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sessions);
    std::fs::create_dir_all(&sessions).unwrap();
    let mut d = arugulad!("mentionblock")
        .no_wisp()
        .no_tailscale()
        .env("FAKE_ACP_DIR", &sessions)
        .env_remove("CLAUDE_CONFIG_DIR")
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();

    // A block called "Worker", in a turn of three seconds. (Not "slow" and a
    // cancel: stopping a turn drops what is queued behind it.)
    let fake = format!("{}/../daemon/tests/fake_acp.py", env!("CARGO_MANIFEST_DIR"));
    let config = json!({ "agent": "acp", "command": ["python3", fake], "cwd": sessions, "title": "Worker",
                         "prompt": "stream 20 150" });
    let block = d.post("/api/blocks", json!({ "type": "agent", "config": config }))["block"].as_u64().unwrap();
    d.wait_for("its turn", || d.get(&format!("/api/blocks/{block}"))["state"]["status"] == "working");
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    // The mention names it: it is handed over, though it waits for the turn
    // in progress (D4), and nothing is said of a turn that hasn't started.
    f.outbox.lock().unwrap().push(mention("m1", 7));
    d.wait_for("the daemon's answer", || answer(&f, "m1").is_some());
    let a = answer(&f, "m1").unwrap();
    assert_eq!(a["answer"], "done", "{a}");
    assert_eq!(a["routed"]["step"], "named", "{a}");
    assert_eq!(a["routed"]["session"], "Worker", "{a}");
    assert!(a["routed"].get("queued").is_none(), "a block's queue reads it: {a}");
    assert!(states(&f).is_empty(), "{:?}", states(&f));

    // The first turn ends by itself, the mention's starts: working, and not
    // read.
    d.wait_for("working", || states(&f) == [(7, "working".into())]);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(states(&f), [(7, "working".into())], "read waits for the turn to end");

    // Its turn ends: read, once.
    d.post(&format!("/api/blocks/{block}/call/cancel"), json!({}));
    d.wait_for("read", || states(&f) == [(7, "working".into()), (7, "read".into())]);
    let all = f.states.lock().unwrap().clone();
    assert_eq!(all[0]["room"], "ch_x");
    assert_eq!(all[0]["session"], "Worker");

    drop(d);
    let _ = std::fs::remove_dir_all(&sessions);
}
