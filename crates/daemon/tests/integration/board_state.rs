//! An agent block's state goes to control for the board rows that point at
//! it (live status board, step 4; aru-116): needs input, working, idle (with
//! the turn's work, when it left any) and closed, and nothing else about the
//! block. Against a fake control; a machine that joined it with the
//! `boards` flag on. Labs' flag, so a build without Labs has none of this.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::agentd::Daemon;
use serde_json::{Value, json};

/// A fake control: the routes a joined daemon asks, and the block states it
/// is told (the bodies of `PUT /api/daemon/block-state`, in order).
#[derive(Clone, Default)]
struct FakeControl {
    told: Arc<Mutex<Vec<Value>>>,
}

async fn fake_control(f: FakeControl) -> String {
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post, put},
    };
    async fn block_state(State(f): State<FakeControl>, Json(b): Json<Value>) -> Json<Value> {
        f.told.lock().unwrap().push(b);
        Json(json!({}))
    }
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
        .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .route(arugula_control_wire::boards::AGENT_BLOCK_STATE, put(block_state))
        .with_state(f);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

async fn told(f: &FakeControl, pane: u64, state: &str) -> Value {
    let pane = pane.to_string();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found = f
            .told
            .lock()
            .unwrap()
            .iter()
            .find(|b| b["pane"] == pane.as_str() && b["block"]["state"] == state)
            .cloned();
        if let Some(b) = found {
            return b;
        }
        assert!(Instant::now() < deadline, "control was never told {state}: {:?}", f.told.lock().unwrap());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// An agent block's turn ends (idle) and it closes (closed); control hears
/// each, and every body is the state's fields and nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_blocks_state_goes_to_control() {
    let fake = FakeControl::default();
    let url = fake_control(fake.clone()).await;
    let d = Daemon::child_in(&["--wisp-token-file", "/nonexistent"], &[], |state| {
        let keys = arugula_e2e::DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
        let cert = arugula_e2e::Cert::new(&keys, "a1", arugula_e2e::Kind::Daemon, "test");
        let saved = json!({ "url": url, "trust": { "account": "a1", "root": keys.id() }, "cert": cert });
        std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
        arugula_proto::flags::set(state, "boards", true).unwrap();
    });
    let id = d.open("hello");
    let idle = told(&fake, id, "idle").await;
    assert_eq!(idle["pane"], id.to_string());

    d.post(&format!("/api/panes/{id}/close"), json!({}));
    told(&fake, id, "closed").await;
    let allowed = ["state", "branch", "head", "pushed", "uncommitted", "diffstat"];
    for b in fake.told.lock().unwrap().iter() {
        let keys: Vec<_> = b["block"].as_object().unwrap().keys().collect();
        assert!(keys.iter().all(|k| allowed.contains(&k.as_str())), "{b}");
        assert_eq!(b.as_object().unwrap().len(), 2, "a pane and its state, no machine: {b}");
    }
}
