//! A Cloud workspace's box is one more machine (Arugula #1448): a chief of
//! staff started there as an agent block (`start_agent` with the
//! chief-of-staff recipe) arms its doorbell watches, ends its turn, and is
//! woken when one fires, as on any machine; `wait` with an event works
//! there too. Against a fake control that enrolls an `arugulad --hosted`
//! box, keeps its watches and fires each a moment later over the relay
//! socket, with a fake-ACP agent block, through `arugula mcp`.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::hosted::{EnrollRequest, Enrollment};
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Trust,
    hosted::{Delegation, HostedCert, HostedKey},
};
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

use super::agentd::{self, Daemon, Mcp};

#[derive(Clone)]
struct Fake {
    url: Arc<Mutex<String>>,
    root: Arc<DeviceKeys>,
    root_cert: Cert,
    key: Arc<HostedKey>,
    delegation: Delegation,
    /// The box's enrollment, once it enrolled.
    enrolled: Arc<Mutex<Option<Enrollment>>>,
    /// Each watch asked for, as it came.
    watches: Arc<Mutex<Vec<Value>>>,
    /// The staff room's lead, as `start_agent` set it.
    leads: Arc<Mutex<Vec<String>>>,
    /// What the daemon said on its relay socket, and what's for it to hear.
    said: Arc<Mutex<Vec<Value>>>,
    outbox: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn new() -> Self {
        let root = DeviceKeys::generate();
        let mut root_cert = Cert::new(&root, "acct1", Kind::Browser, "laptop");
        root_cert.sign_with(&root);
        let key = HostedKey::generate();
        let mut delegation = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        delegation.sign_with(&root);
        Fake {
            url: Default::default(),
            root: Arc::new(root),
            root_cert,
            key: Arc::new(key),
            delegation,
            enrolled: Default::default(),
            watches: Default::default(),
            leads: Default::default(),
            said: Default::default(),
            outbox: Default::default(),
        }
    }
}

async fn enroll(State(f): State<Fake>, Json(b): Json<EnrollRequest>) -> Response {
    let mut cert = HostedCert::new(&f.delegation, "hw_1", "box", &b.noise, &b.sign, arugula_e2e::now_ms());
    f.key.sign(&mut cert);
    let e = Enrollment {
        url: f.url.lock().unwrap().clone(),
        cert,
        delegation: f.delegation.clone(),
        revocations: vec![],
        closed: vec![],
        team: None,
        roster: None,
        certs: Default::default(),
        trust: Some(Trust { account: "acct1".into(), root: f.root.id() }),
        account_certs: vec![f.root_cert.clone()],
        account_revocations: vec![],
        locked: false,
        lock: None,
        legacy_app_children: None,
        passkey_logs: None,
        passkey_origins: None,
        purpose: None,
    };
    *f.enrolled.lock().unwrap() = Some(e.clone());
    Json(e).into_response()
}

async fn trust(State(f): State<Fake>) -> Response {
    match f.enrolled.lock().unwrap().clone() {
        Some(e) => Json(e).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// The staff room of `prj_1`, `dm_staff`, led by `lead` if it has one.
fn room(lead: Option<&str>) -> Value {
    let mut r = json!({
        "id": "dm_staff", "kind": "dm", "name": "Chief of staff", "project": "prj_1", "staff": true,
        "last": 0, "powers": ["threads.read", "threads.post"], "writable": true,
    });
    if let Some(session) = lead {
        r["lead"] = json!({ "machine": "box", "machine_name": "box", "session": session });
    }
    r
}

/// A watch: made, and a moment later its message goes to its session, as
/// control's matcher sends one that fired.
async fn watch(State(f): State<Fake>, Json(b): Json<Value>) -> Json<Value> {
    let id = {
        let mut w = f.watches.lock().unwrap();
        w.push(b.clone());
        format!("wch_{}", w.len())
    };
    let (f2, id2, session) = (f.clone(), id.clone(), b["session"].as_str().unwrap_or_default().to_owned());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let op = json!({ "t": "op", "id": format!("m-{id2}"), "op": { "op": "agents.mention",
            "room": id2, "seq": 0, "author": "Arugula", "text": "board.closed: board brd_1 closed",
            "watch": { "id": id2, "session": session } } });
        f2.outbox.lock().unwrap().push(op.to_string());
    });
    Json(json!({ "id": id, "state": "live", "expires_ms": arugula_e2e::now_ms() + 86_400_000 }))
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

async fn other(req: Request) -> Response {
    let said = format!("no route {} {}", req.method(), req.uri());
    (StatusCode::NOT_FOUND, Json(json!({ "error": said }))).into_response()
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    use arugula_control_wire::chat::{agent_room_lead, agent_staff_room};
    rt.block_on(async {
        let home = json!({ "project": "prj_1", "name": "Launch crew", "home": "native" });
        let app = Router::new()
            .route(
                "/control.json",
                get(|State(f): State<Fake>| async move {
                    Json(json!({ "control": true, "daemon_auth": 2, "url": f.url.lock().unwrap().clone() }))
                }),
            )
            .route("/api/hosted/enroll", post(enroll))
            .route("/api/hosted/trust", get(trust))
            .route("/api/hosted/work", get(|| async { Json(json!({ "items": [], "revokes": [], "sessions": [] })) }))
            // Each agent turn asks the platform whose key pays (M185).
            .route("/api/hosted/turns", post(|| async { Json(json!({ "turn": "trn_1", "pays": "platform" })) }))
            .route("/api/hosted/turns/{turn}/end", post(|| async { Json(json!({ "ended": true })) }))
            .route("/api/relay/dial", get(dial))
            .route(arugula_control_wire::issues::AGENT_ISSUE_HOME, get(move || async move { Json(home) }))
            .route(&agent_staff_room("prj_1"), post(|| async { Json(room(None)) }))
            .route(
                &agent_room_lead("dm_staff"),
                post(|State(f): State<Fake>, Json(b): Json<Value>| async move {
                    let session = b["session"].as_str().unwrap_or_default().to_owned();
                    f.leads.lock().unwrap().push(session.clone());
                    Json(room(Some(&session)))
                }),
            )
            .route(arugula_control_wire::watch::AGENT_WATCHES, post(watch))
            .fallback(other)
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

fn summary(v: &Value) -> &str {
    v["summary"].as_str().unwrap_or_default()
}

#[test]
fn a_chief_of_staff_on_a_cloud_workspace_arms_a_watch_and_is_woken() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::new();
    let url = serve(&rt, f.clone());
    let env = [
        ("ARUGULA_HOSTED_TICKET", "ticket-1"),
        ("ARUGULA_CONTROL_URL", url.as_str()),
        // No `runner` user here, nor session users: the daemon's own.
        ("ARUGULA_HOSTED_RUN_USER", ""),
        ("ARUGULA_HOSTED_SESSION_USERS", ""),
    ];
    let d = Daemon::child_in(&["--hosted"], &env, |state| {
        // The box is set up already.
        let ready = json!({ "status": "ready", "reported": true }).to_string();
        std::fs::write(state.join("hosted-setup.json"), ready).unwrap();
    });
    d.wait_for("the box enrolled", || f.enrolled.lock().unwrap().is_some());
    d.wait_for("the box on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    // The chief of staff, as an agent block on the box, leading the staff
    // room. The box takes a moment to know its project.
    let owner = Mcp::bridge(&d, None);
    let chief = json!({
        "agent": "claude", "command": format!("{} {}", agentd::PYTHON, agentd::fake()),
        "recipe": "chief-of-staff", "prompt": "brief me",
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let started = loop {
        match owner.call("start_agent", chief.clone()) {
            Ok(r) => break r,
            Err(e) => assert!(std::time::Instant::now() < deadline, "never started: {e}"),
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    let block = started["block"].as_u64().unwrap_or_else(|| panic!("{started}"));
    assert_eq!(started["chief_of_staff"]["lead"], true, "{started}");
    d.wait(block, "idle");

    // It arms a doorbell from its own pane, as its recipe says, and ends
    // its turn.
    let mcp = Mcp::bridge(&d, Some(block));
    let made = mcp
        .call("watch", json!({ "kind": "create", "event": "board.closed", "expires": "7d", "note": "brief them" }))
        .unwrap();
    let id = made["id"].as_str().unwrap_or_else(|| panic!("{made}")).to_owned();
    assert!(summary(&made).contains("End your turn now"), "{made}");
    let asked = f.watches.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0]["event"], "board.closed", "{asked:?}");
    let sid = d.state(block)["session_id"].as_str().unwrap().to_owned();
    assert_eq!(asked[0]["session"], sid, "the block's own session: {asked:?}");

    // It fires: the chief's next prompt is the watch's message.
    d.wait_for("the chief woken", || d.state(block).to_string().contains(&format!("Your watch {id}")));
    assert!(d.state(block).to_string().contains("board brd_1 closed"), "{}", d.state(block));

    // `wait` with an event, too: its own short-lived watch, whose message
    // is the call's answer.
    let waited = mcp.call("wait", json!({ "event": "board.closed", "timeout": 30 })).unwrap();
    assert!(summary(&waited).contains("board brd_1 closed"), "{waited}");
    let asked = f.watches.lock().unwrap().clone();
    assert!(asked[1]["session"].as_str().unwrap().starts_with("wait_"), "{asked:?}");
}
