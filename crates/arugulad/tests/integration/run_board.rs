//! Arugula #1416, "Run this board": the chief of staff asks its person to
//! run a board (`ask_approval` `run_board`), the person's Approve reaches the
//! grant's lead machine as `agents.approved`, which starts the board's lead
//! there as a ladder start (a start ticket under the grant control just
//! wrote) and makes it the board's lead; a post on the board's thread then
//! reaches it. Revoked, the grant refuses the next start. Against a fake
//! control (it takes the card, plays the person's Approve on the relay
//! socket, signs start tickets while the grant is live, and sends the
//! board's thread to its lead) and a real daemon whose agents are the fake
//! ACP agent.
#![cfg(unix)]

use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use arugula_control_wire::ladder::START_USE;
use arugula_e2e::DeviceKeys;
use axum::{
    Json, Router,
    extract::{
        Path as UrlPath, Request, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp, PYTHON, fake};
use super::mention_agent_block::enroll;

const BOARD: &str = "brd_1";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn issuer() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A JWT as the hub signs one.
fn jwt(claims: &Value) -> String {
    let h = b64(&serde_json::to_vec(&json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" })).unwrap());
    let p = b64(&serde_json::to_vec(claims).unwrap());
    let s = b64(&issuer().sign(format!("{h}.{p}").as_bytes()).to_bytes());
    format!("{h}.{p}.{s}")
}

fn signer(h: &HeaderMap) -> String {
    let a = h.get("x-arugula-auth").and_then(|v| v.to_str().ok()).unwrap_or_default();
    a.split(' ').nth(1).unwrap_or_default().to_owned()
}

#[derive(Clone, Default)]
struct Fake {
    url: Arc<Mutex<String>>,
    /// Every call but the relay's: method and path, and its body.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// What the daemon said on its relay socket, and what's for it to hear.
    said: Arc<Mutex<Vec<Value>>>,
    outbox: Arc<Mutex<Vec<String>>>,
    /// The board's grant is live: written by the person's Approve, and not
    /// revoked since.
    granted: Arc<AtomicBool>,
    issued: Arc<Mutex<Vec<Value>>>,
    leads: Arc<Mutex<Vec<Value>>>,
}

impl Fake {
    fn asked(&self, what: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(c, _)| c.starts_with(what)).map(|(_, b)| b.clone()).collect()
    }

    /// The daemon's answer to op `id`, once it gives one.
    fn answer(&self, id: &str) -> Option<Value> {
        self.said.lock().unwrap().iter().find(|v| v["t"] == "op.done" && v["id"] == id).map(|v| v["answer"].clone())
    }

    fn send(&self, op: Value) {
        self.outbox.lock().unwrap().push(op.to_string());
    }
}

async fn any(State(f): State<Fake>, req: Request) -> Response {
    let (method, at) = (req.method().to_string(), req.uri().to_string());
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
    let b: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    f.calls.lock().unwrap().push((format!("{method} {at}"), b.clone()));
    let path = at.split_once('?').map_or(at.as_str(), |(p, _)| p);
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), segs.as_slice()) {
        ("POST", ["api", "daemon", "projects", "prj_1", "proposals"]) => {
            let n = f.asked("POST /api/daemon/projects/prj_1/proposals").len() - 1;
            Json(json!({ "ask": format!("prp_{n}"), "room": "dm_staff", "seq": 40 + n })).into_response()
        }
        ("PUT", ["api", "daemon", "chat", "mention-state"]) => Json(json!({})).into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no route {method} {path}") }))).into_response(),
    }
}

async fn jwks() -> Json<Value> {
    Json(json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1", "alg": "EdDSA", "use": "sig",
        "x": b64(issuer().verifying_key().as_bytes()),
    }]}))
}

/// A lead's start ticket, while the board's grant is live: as control
/// refuses one with no grant, else for the machine that asks.
async fn start_ticket(
    State(f): State<Fake>,
    h: HeaderMap,
    UrlPath(board): UrlPath<String>,
    Json(a): Json<Value>,
) -> Response {
    if !f.granted.load(Ordering::SeqCst) {
        let why = "this board has no grant to start agents: its person gives one with \"Run this board\"";
        return (StatusCode::FORBIDDEN, Json(json!({ "error": why, "reason": "no-grant" }))).into_response();
    }
    let iat = now();
    let claims = json!({
        "iss": *f.url.lock().unwrap(), "sub": "a1", "aud": a["to"], "use": START_USE, "from": signer(&h),
        "board": board, "project": "prj_1", "grant": "grt_1", "rung": a["rung"], "issue": a["issue"],
        "recipe": a["recipe"], "iat": iat, "exp": iat + 60, "jti": hex::encode(arugula_e2e::random::<16>()),
    });
    f.issued.lock().unwrap().push(claims.clone());
    Json(json!({ "ticket": jwt(&claims), "expires": iat + 60 })).into_response()
}

async fn lead(State(f): State<Fake>, h: HeaderMap, Json(mut a): Json<Value>) -> Json<Value> {
    a["signer"] = json!(signer(&h));
    f.leads.lock().unwrap().push(a);
    Json(json!({}))
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

fn serve(f: Fake) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let home = json!({ "project": "prj_1", "name": "Launch crew", "home": "native" });
        let app = Router::new()
            .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
            .route("/.well-known/jwks.json", get(jwks))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route(arugula_control_wire::issues::AGENT_ISSUE_HOME, get(move || async move { Json(home) }))
            .route("/api/daemon/boards/{board}/start-ticket", post(start_ticket))
            .route("/api/daemon/boards/{board}/lead", post(lead))
            .route("/api/relay/dial", get(dial))
            .fallback(any)
            .with_state(f.clone());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.url.lock().unwrap() = at.clone();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

/// What the fake agent in block `id` was prompted with, every turn's.
fn prompts(d: &Daemon, id: u64) -> String {
    let Some(sid) = d.state(id)["session_id"].as_str().map(str::to_owned) else { return String::new() };
    std::fs::read_to_string(d.sessions.join(format!("prompt-{sid}.json"))).unwrap_or_default()
}

#[test]
fn run_this_board_starts_its_lead_which_hears_the_boards_thread_and_a_revoked_grant_starts_nothing() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let tmp = std::env::temp_dir().join(format!("arugula-run-board-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Claude Code, as a ladder start runs it, is the fake agent.
    let agents = tmp.join("agents");
    let bin = agents.join("claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let acp = bin.join("claude-agent-acp");
    std::fs::write(&acp, format!("#!/bin/sh\nexec {PYTHON} {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&acp, std::fs::Permissions::from_mode(0o755)).unwrap();

    let agents_dir = agents.display().to_string();
    let here = Arc::new(Mutex::new(String::new()));
    let me = here.clone();
    let d = Daemon::child_in(&[], &[("ARUGULA_AGENTS_DIR", &agents_dir)], move |state| {
        enroll(state, &url);
        *me.lock().unwrap() = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap().id();
    });
    let here = here.lock().unwrap().clone();
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    // The chief of staff: an agent block.
    let config = json!({ "agent": "acp", "command": [PYTHON, fake()], "cwd": tmp, "title": "Chief of staff",
        "prompt": "hello" });
    let chief = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(chief, "idle");
    let mcp = Mcp::bridge(&d, Some(chief));

    // With no grant, nobody starts the board's lead.
    let why = mcp.call("start_agent", json!({ "board": BOARD, "prompt": "Lead it" })).unwrap_err();
    assert!(why.contains("Run this board"), "{why}");

    // It asks to run the board: a card, and nothing starts.
    let before = d.get("/api/panes").as_array().unwrap().len();
    let asked = mcp
        .call(
            "ask_approval",
            json!({ "action": "run_board", "board": BOARD, "cap": 2, "machines": ["mini"],
                "why": "its plan is ready", "project": "prj_1" }),
        )
        .unwrap();
    assert_eq!(asked["ask"], "prp_0", "{asked}");
    let card = &f.asked("POST /api/daemon/projects/prj_1/proposals")[0];
    assert_eq!(
        card["proposal"],
        json!({ "action": "run_board", "board": BOARD, "machines": ["mini"], "cap": 2, "why": "its plan is ready" })
    );
    assert_eq!((&card["session"], &card["pane"]), (&json!("Chief of staff"), &json!(chief)), "{card}");
    assert_eq!(d.get("/api/panes").as_array().unwrap().len(), before, "nothing started");
    // What isn't a run_board's is refused here, before control.
    let why = mcp.call("ask_approval", json!({ "action": "start", "issue": "#1", "cap": 2, "why": "x" })).unwrap_err();
    assert!(why.contains("go with run_board"), "{why}");

    // The person approves: control writes the grant and asks the grant's
    // lead machine (this one) to start the lead.
    f.granted.store(true, Ordering::SeqCst);
    f.send(json!({ "t": "op", "id": "approve-1", "op": { "op": "agents.approved", "ask": "prp_0",
        "project": "prj_1", "action": "run_board", "session": "Chief of staff", "pane": chief, "board": BOARD,
        "board_title": "Launch" } }));
    d.wait_for("its answer", || f.answer("approve-1").is_some());
    let a = f.answer("approve-1").unwrap();
    assert_eq!(a["answer"], "approved", "{a}");
    let lead_block = a["block"].as_u64().unwrap();
    let said = a["said"].as_str().unwrap();
    assert!(said.starts_with(&format!("started the board's lead %{lead_block}")), "{a}");
    assert!(said.contains("it leads the board"), "{a}");

    // Under a start ticket for this machine's lead, and the board's lead now.
    let issued = f.issued.lock().unwrap().clone();
    assert_eq!(issued.len(), 1, "{issued:?}");
    assert_eq!((&issued[0]["rung"], &issued[0]["aud"]), (&json!("lead"), &json!(here)), "{issued:?}");
    let leads = f.leads.lock().unwrap().clone();
    assert_eq!(leads.len(), 1, "{leads:?}");
    assert_eq!((&leads[0]["pane"], &leads[0]["signer"]), (&json!(lead_block), &json!(here)), "{leads:?}");
    // It leads by its block's title, the name a post finds it by.
    assert_eq!(leads[0]["session"], "Lead · Launch", "{leads:?}");
    let session = leads[0]["session"].as_str().unwrap().to_owned();
    let b = d.get(&format!("/api/blocks/{lead_block}"));
    assert_eq!(b["state"]["work"]["ladder"]["rung"], "lead", "{b}");
    assert_eq!(b["state"]["title"], "Lead · Launch", "a title, so its session's posts find it: {b}");
    assert_eq!(b["state"]["work"]["ladder"]["by_session"], "Chief of staff", "{b}");
    // It works without a card for each command, as the board-lead recipe's
    // issue agents do (Arugula #1440).
    assert_eq!((&b["state"]["permission_mode"], &b["state"]["user_settings"]), (&json!("auto"), &json!(true)), "{b}");
    let allow = ["Bash", "Read", "Edit", "Write", "Glob", "Grep", "WebFetch"].map(|t| json!({ "tool": t }));
    assert_eq!(b["state"]["allow"], json!(allow), "{b}");
    d.wait(lead_block, "idle");
    assert!(prompts(&d, lead_block).contains("You lead the board brd_1"), "{}", prompts(&d, lead_block));

    // A post on the board's thread reaches its lead.
    f.send(json!({ "t": "op", "id": "m-1", "op": { "op": "agents.mention", "room": "ch_board", "target": BOARD,
        "room_name": "Launch", "seq": 7, "author": "Jake", "text": "How far along is it?", "session": session } }));
    d.wait_for("the lead hears the board's thread", || prompts(&d, lead_block).contains("How far along is it?"));

    // Revoked: the lead's next start is refused, in control's words.
    f.granted.store(false, Ordering::SeqCst);
    let why = mcp.call("start_agent", json!({ "board": BOARD, "prompt": "Lead it again" })).unwrap_err();
    assert!(why.contains("no grant"), "{why}");
    f.send(json!({ "t": "op", "id": "approve-2", "op": { "op": "agents.approved", "ask": "prp_1",
        "project": "prj_1", "action": "run_board", "session": "Chief of staff", "pane": chief, "board": BOARD } }));
    d.wait_for("its answer", || f.answer("approve-2").is_some());
    let a = f.answer("approve-2").unwrap();
    assert_eq!(a["answer"], "failed", "{a}");
    assert_eq!(f.issued.lock().unwrap().len(), 1, "nothing more signed");

    drop(mcp);
    drop(d);
    let _ = std::fs::remove_dir_all(&tmp);
}
