//! An agent block's state goes to control for the board rows that point at
//! it (boards, step 4; aru-116): needs input, working, idle (with
//! the turn's work, when it left any) and closed, and nothing else about the
//! block, with how long ago it was seen. Against a fake control; a machine
//! that joined it.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::agentd::Daemon;
use serde_json::{Value, json};

/// A fake control: the routes a joined daemon asks, and the block states it
/// is told (the bodies of `PUT /api/daemon/block-state`, in order). While
/// `down`, it answers those with a 500, and counts them in `failed`. While
/// `old`, it refuses a body with an age with a 422, as a control from before
/// Arugula #745 does, counted in `failed` too.
#[derive(Clone, Default)]
struct FakeControl {
    told: Arc<Mutex<Vec<Value>>>,
    down: Arc<AtomicBool>,
    old: Arc<AtomicBool>,
    failed: Arc<AtomicUsize>,
    /// How many more of its reports to answer with a 500, and when each
    /// report (failed or not) came.
    fail_next: Arc<AtomicUsize>,
    at: Arc<Mutex<Vec<(Instant, bool)>>>,
}

async fn fake_control(f: FakeControl) -> String {
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, post, put},
    };
    async fn block_state(State(f): State<FakeControl>, Json(b): Json<Value>) -> axum::response::Response {
        use axum::response::IntoResponse;
        let failing = f.fail_next.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok();
        f.at.lock().unwrap().push((Instant::now(), failing));
        if failing || f.down.load(Ordering::SeqCst) {
            f.failed.fetch_add(1, Ordering::SeqCst);
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        if f.old.load(Ordering::SeqCst) && b.get("age_ms").is_some() {
            f.failed.fetch_add(1, Ordering::SeqCst);
            return axum::http::StatusCode::UNPROCESSABLE_ENTITY.into_response();
        }
        f.told.lock().unwrap().push(b);
        Json(json!({})).into_response()
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
    told_within(f, pane, state, Duration::from_secs(30)).await
}

async fn told_within(f: &FakeControl, pane: u64, state: &str, within: Duration) -> Value {
    let pane = pane.to_string();
    let deadline = Instant::now() + within;
    loop {
        let found =
            f.told.lock().unwrap().iter().find(|b| b["pane"] == pane.as_str() && b["block"]["state"] == state).cloned();
        if let Some(b) = found {
            return b;
        }
        assert!(Instant::now() < deadline, "control was never told {state}: {:?}", f.told.lock().unwrap());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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

/// An agent block's turn ends (idle) and it closes (closed); control hears
/// each, and every body is the pane, the state's fields and its age, and
/// nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_blocks_state_goes_to_control() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let id = d.open("hello");
    let idle = told(&fake, id, "idle").await;
    assert_eq!(idle["pane"], id.to_string());

    d.post(&format!("/api/panes/{id}/close"), json!({}));
    told(&fake, id, "closed").await;
    let allowed = ["state", "title", "branch", "head", "pushed", "uncommitted", "diffstat"];
    for b in fake.told.lock().unwrap().iter() {
        let keys: Vec<_> = b["block"].as_object().unwrap().keys().collect();
        assert!(keys.iter().all(|k| allowed.contains(&k.as_str())), "{b}");
        let mut keys: Vec<_> = b.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["age_ms", "block", "pane"], "a pane, its state and its age, no machine: {b}");
        assert!(b["age_ms"].is_u64(), "{b}");
    }
}

/// Control fails while a block closes (Arugula #745): the close is kept
/// through the daemon's wait, past the ticks that used to drop it, and
/// reaches control once it answers again, saying how long ago it was seen.
#[tokio::test(flavor = "multi_thread")]
async fn a_close_during_an_outage_reaches_control_after_it() {
    let fake = FakeControl::default();
    let d = joined(fake_control(fake.clone()).await);
    let id = d.open("hello");
    told(&fake, id, "idle").await;

    fake.down.store(true, Ordering::SeqCst);
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    let deadline = Instant::now() + Duration::from_secs(30);
    while fake.failed.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "the daemon never tried to tell control");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // A few of the daemon's ticks go by while it waits.
    tokio::time::sleep(Duration::from_secs(5)).await;
    fake.down.store(false, Ordering::SeqCst);

    let closed = told_within(&fake, id, "closed", Duration::from_secs(60)).await;
    assert!(closed["age_ms"].as_u64().is_some_and(|ms| ms >= 5_000), "as old as it is: {closed}");
}

/// A control from before Arugula #745 refuses the age: the daemon sends the
/// same report without it, and goes on that way.
#[tokio::test(flavor = "multi_thread")]
async fn a_control_that_refuses_the_age_is_told_without_it() {
    let fake = FakeControl::default();
    fake.old.store(true, Ordering::SeqCst);
    let d = joined(fake_control(fake.clone()).await);
    let id = d.open("hello");
    let idle = told(&fake, id, "idle").await;
    assert_eq!(idle.get("age_ms"), None, "{idle}");
    assert!(fake.failed.load(Ordering::SeqCst) >= 1, "it was tried with the age first");
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    let closed = told(&fake, id, "closed").await;
    assert_eq!(closed.get("age_ms"), None, "{closed}");
}

/// A 5xx for one report: the daemon waits about 30 seconds (its
/// `TROUBLE`) before trying it again, not every tick.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_report_is_tried_again_after_a_pause() {
    let fake = FakeControl::default();
    fake.fail_next.store(1, Ordering::SeqCst);
    let d = joined(fake_control(fake.clone()).await);
    let id = d.open("hello");
    told_within(&fake, id, "idle", Duration::from_secs(60)).await;
    let at = fake.at.lock().unwrap().clone();
    let failed = at.iter().position(|(_, f)| *f).expect("a report failed");
    let next = at.get(failed + 1).expect("and was tried again");
    let waited = next.0 - at[failed].0;
    assert!(waited >= Duration::from_secs(25), "waited {waited:?}, not a pause");
    assert!(waited < Duration::from_secs(45), "waited {waited:?}");
}
