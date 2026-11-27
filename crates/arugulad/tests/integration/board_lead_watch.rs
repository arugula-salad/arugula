//! Arugula #1530: a session that becomes a board's lead, by `lead_board` or
//! by making the board, gets a standing watch on `board.wait.changed` for
//! it, through the path `watch create` takes. A second `lead_board` makes
//! none, and `release` cancels it. Against a fake control and a real daemon
//! whose agent block is the fake ACP agent.
#![cfg(unix)]

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Request, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp, PYTHON, fake};
use super::mention_agent_block::enroll;

#[derive(Clone, Default)]
struct Fake {
    /// Every call: method and path with its query, and its body.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// The watches that are live, as control lists them.
    live: Arc<Mutex<Vec<Value>>>,
}

impl Fake {
    /// The bodies of the calls to exactly `what`, as "METHOD /path".
    fn asked(&self, what: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(c, _)| c == what).map(|(_, b)| b.clone()).collect()
    }
}

fn board_view() -> Value {
    let lane = |id: &str| json!({ "id": format!("lane_{id}"), "rev": 0, "title": id, "kind": "lane", "rows": [] });
    json!({
        "board": { "id": "brd_1", "project": "prj_1", "title": "Launch", "created_by": "a1", "created_at": 1,
                   "updated_at": 1, "progress": { "done": 0, "total": 0, "proven": 0 } },
        "groups": [],
        "zones": [lane("next"), lane("doing"), lane("review"), lane("done")],
    })
}

async fn any(State(f): State<Fake>, req: Request) -> Response {
    let (method, at) = (req.method().to_string(), req.uri().to_string());
    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
    let b: Value = serde_json::from_slice(&bytes).unwrap_or_default();
    f.calls.lock().unwrap().push((format!("{method} {at}"), b.clone()));
    let path = at.split_once('?').map_or(at.as_str(), |(p, _)| p);
    match (method.as_str(), path) {
        ("POST", "/api/daemon/boards/brd_1/lead") | ("POST", "/api/daemon/projects/prj_1/boards") => {
            Json(board_view()).into_response()
        }
        ("POST", "/api/daemon/watches") => {
            let n = f.asked("POST /api/daemon/watches").len() - 1;
            let id = format!("wch_{n}");
            // As control words a standing event watch's target.
            let target = format!("event {} {}", b["event"].as_str().unwrap(), b["match"]);
            f.live.lock().unwrap().push(json!({ "id": id, "session": b["session"], "targets": [target],
                "state": "live", "created_ms": 1, "expires_ms": 2, "every_ms": 300_000 }));
            Json(json!({ "id": id, "state": "live", "expires_ms": 1 })).into_response()
        }
        ("GET", "/api/daemon/watches") => Json(json!({ "watches": *f.live.lock().unwrap() })).into_response(),
        ("POST", "/api/daemon/watches/cancel") => {
            let id = b["id"].as_str().unwrap().to_owned();
            f.live.lock().unwrap().retain(|w| w["id"] != id.as_str());
            Json(json!({ "cancelled": [id] })).into_response()
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no route {method} {path}") }))).into_response(),
    }
}

fn serve(f: Fake) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let home = json!({ "project": "prj_1", "name": "Launch crew", "home": "native" });
        let app = Router::new()
            .route("/control.json", get(|| async { Json(json!({ "daemon_auth": 2 })) }))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route(arugula_control_wire::issues::AGENT_ISSUE_HOME, get(move || async move { Json(home) }))
            .route("/api/relay/dial", get(|up: WebSocketUpgrade| async { up.on_upgrade(|_| async {}) }))
            .fallback(any)
            .with_state(f);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

fn summary(v: &Value) -> &str {
    v["summary"].as_str().unwrap_or_default()
}

#[test]
fn a_boards_lead_gets_a_standing_watch_on_what_its_board_waits_on() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let d = Daemon::child_in(&[], &[], |state| enroll(state, &url));
    let config = json!({ "agent": "acp", "command": [PYTHON, fake()], "title": "Lead", "prompt": "hello" });
    let lead = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(lead, "idle");
    let mcp = Mcp::bridge(&d, Some(lead));
    let watches = |f: &Fake| f.asked("POST /api/daemon/watches").len();

    // Making a board leads it, and the lead gets the watch.
    let made =
        mcp.call("board_write", json!({ "kind": "create_board", "project": "prj_1", "title": "Launch" })).unwrap();
    assert!(summary(&made).contains("You'll hear when work this board waits on moves."), "{made}");
    assert_eq!(watches(&f), 1);
    let w = f.asked("POST /api/daemon/watches")[0].clone();
    assert_eq!(w["event"], "board.wait.changed", "{w}");
    assert_eq!(w["match"], json!({ "board": "brd_1" }), "{w}");
    assert_eq!((&w["standing"], &w["note"]), (&json!(true), &json!("work this board waits on moved")), "{w}");
    let hours = (w["expires_in_ms"].as_u64().unwrap()) / 3_600_000;
    assert_eq!(hours, 24 * 7, "the 7-day maximum: {w}");

    // Taking the lead again: the watch is live, so none is made.
    let led = mcp.call("board_write", json!({ "kind": "lead_board", "board": "brd_1" })).unwrap();
    assert!(summary(&led).contains("a watch is live already"), "{led}");
    assert_eq!(watches(&f), 1, "no second one");

    // Letting go cancels it, and a new lead makes it again.
    let freed = mcp.call("board_write", json!({ "kind": "lead_board", "board": "brd_1", "release": true })).unwrap();
    assert!(summary(&freed).contains("Cancelled this session's watch"), "{freed}");
    assert_eq!(f.asked("POST /api/daemon/watches/cancel")[0]["id"], "wch_0");
    assert!(f.live.lock().unwrap().is_empty());
    let led = mcp.call("board_write", json!({ "kind": "lead_board", "board": "brd_1" })).unwrap();
    assert!(summary(&led).contains("You'll hear when work this board waits on moves."), "{led}");
    assert_eq!(watches(&f), 2);
}
