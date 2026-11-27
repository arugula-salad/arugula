//! Arugula #1417: the chief of staff on a machine asks its person to approve
//! a start or a close on a card (`ask_approval`), the person's Approve
//! reaches this machine as `agents.approved`, which starts an agent block on
//! the issue (or closes blocks) here, and the card's `ask.answered` wakes the
//! chief of staff through its watch, or its answer does with no watch
//! (Arugula #1465). Against a fake control (it takes the
//! card, plays the person's Approve on the relay socket, and fires the
//! watch) and a real daemon whose agents are the fake ACP agent.
#![cfg(unix)]

use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
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

use super::agentd::{Daemon, Mcp, PYTHON, entries, fake};
use super::mention_agent_block::enroll;

#[derive(Clone, Default)]
struct Fake {
    /// Every call but the relay's: method and path, and its body.
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// What the daemon said on its relay socket, and what's for it to hear.
    said: Arc<Mutex<Vec<Value>>>,
    outbox: Arc<Mutex<Vec<String>>>,
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
        ("POST", ["api", "daemon", "projects", "prj_1", "proposals", ask, "cancel"]) => {
            Json(json!({ "ask": ask })).into_response()
        }
        ("POST", ["api", "daemon", "watches"]) => {
            let n = f.asked("POST /api/daemon/watches").len() - 1;
            Json(json!({ "id": format!("wch_{n}"), "state": "live", "expires_ms": 1 })).into_response()
        }
        ("PUT", ["api", "daemon", "chat", "mention-state"]) => Json(json!({})).into_response(),
        ("GET", ["api", "daemon", "boards", "brd_1"]) => Json(board_view()).into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no route {method} {path}") }))).into_response(),
    }
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

/// Board `brd_1` with one row, on #12, as the platform sends it.
fn board_view() -> Value {
    let lane = |id: &str, rows: Value| json!({ "id": format!("lane_{id}"), "rev": 0, "title": id, "kind": "lane", "rows": rows });
    json!({
        "board": { "id": "brd_1", "project": "prj_1", "title": "Launch", "created_by": "a1", "created_at": 1,
                   "updated_at": 1, "progress": { "done": 0, "total": 1, "proven": 0 } },
        "groups": [],
        "zones": [
            lane("next", json!([{ "id": "row_1", "rev": 1, "pointer": { "kind": "issue", "id": "iss_12" },
                "note": "", "status": "none", "lane": "next", "attachments": [],
                "live": { "state": "open", "kind": "native", "number": 12, "title": "" },
                "created_at": 1, "updated_at": 1 }])),
            lane("doing", json!([])),
            lane("review", json!([])),
            lane("done", json!([])),
        ],
    })
}

/// A board ask as a control from before Arugula #1465 reads it: its JSON
/// extractor refuses a field it doesn't know, with axum's own answer.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct OldAsk {
    who: String,
    question: String,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    recommend: Option<String>,
}

async fn old_ask(
    State(f): State<Fake>,
    body: Result<Json<OldAsk>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let ok = body.is_ok();
    f.calls.lock().unwrap().push((format!("POST /api/daemon/boards/brd_1/rows/row_1/ask {ok}"), json!(ok)));
    match body {
        Ok(_) => Json(board_view()).into_response(),
        Err(e) => e.into_response(),
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
            .route("/api/relay/dial", get(dial))
            .route("/api/daemon/boards/brd_1/rows/row_1/ask", post(old_ask))
            .fallback(any)
            .with_state(f);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

/// Runs `git ARGS` in `dir`, as a test identity.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// What the fake agent in block `id` was prompted with, every turn's.
fn prompts(d: &Daemon, id: u64) -> String {
    let Some(sid) = d.state(id)["session_id"].as_str().map(str::to_owned) else { return String::new() };
    std::fs::read_to_string(d.sessions.join(format!("prompt-{sid}.json"))).unwrap_or_default()
}

fn pane(d: &Daemon, id: u64) -> Option<Value> {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).cloned()
}

#[test]
fn the_chief_of_staff_asks_the_person_approves_and_an_agent_starts_on_the_issue() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    // A clone whose origin is a bare repo: the chief of staff works in it.
    let tmp = std::env::temp_dir().join(format!("arugula-cards-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let tmp = tmp.canonicalize().unwrap();
    git(&tmp, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&tmp, &["init", "-q", "-b", "main", "work"]);
    let work = tmp.join("work");
    git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&work, &["remote", "add", "origin", tmp.join("origin.git").to_str().unwrap()]);
    git(&work, &["push", "-q", "origin", "main"]);
    // Claude Code, as an approved start runs it, is the fake agent.
    let agents = tmp.join("agents");
    let bin = agents.join("claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let acp = bin.join("claude-agent-acp");
    std::fs::write(&acp, format!("#!/bin/sh\nexec {PYTHON} {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&acp, std::fs::Permissions::from_mode(0o755)).unwrap();

    let agents_dir = agents.display().to_string();
    let d = Daemon::child_in(&[], &[("ARUGULA_AGENTS_DIR", &agents_dir)], |state| enroll(state, &url));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let ops = f.said.lock().unwrap().iter().find(|v| v["t"] == "ops").cloned().unwrap();
    assert!(ops["ops"].as_array().unwrap().contains(&json!("agents.approved")), "{ops}");

    // The chief of staff: an agent block in the clone.
    let config = json!({ "agent": "acp", "command": [PYTHON, fake()], "cwd": work, "title": "Chief of staff",
        "prompt": "hello" });
    let chief = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(chief, "idle");
    let mcp = Mcp::bridge(&d, Some(chief));

    // It asks: a card, which it watches. It does nothing itself.
    let before = d.get("/api/panes").as_array().unwrap().len();
    let asked = mcp
        .call("ask_approval", json!({ "action": "start", "issue": "#12", "why": "it's next", "project": "prj_1" }))
        .unwrap();
    assert_eq!(asked["ask"], "prp_0", "{asked}");
    assert!(asked["summary"].as_str().unwrap().contains("no watch needed"), "{asked}");
    let card = &f.asked("POST /api/daemon/projects/prj_1/proposals")[0];
    assert_eq!(
        card["proposal"],
        json!({ "action": "start", "number": 12, "prompt": null, "repo": null,
        "why": "it's next" })
    );
    assert_eq!((&card["session"], &card["pane"]), (&json!("Chief of staff"), &json!(chief)), "{card}");
    assert_eq!(d.get("/api/panes").as_array().unwrap().len(), before, "nothing started");
    let watched =
        mcp.call("watch", json!({ "kind": "create", "event": "ask.answered", "match": { "ask": "prp_0" } })).unwrap();
    assert_eq!(watched["id"], "wch_0", "{watched}");
    let watch = f.asked("POST /api/daemon/watches")[0].clone();
    assert_eq!((&watch["event"], &watch["match"]["ask"]), (&json!("ask.answered"), &json!("prp_0")), "{watch}");
    // Refused here, before control: what isn't the card's.
    let why = mcp.call("ask_approval", json!({ "action": "close", "blocks": [chief], "why": "x" })).unwrap_err();
    assert!(why.contains("your own block"), "{why}");
    let why = mcp.call("ask_approval", json!({ "action": "start", "issue": "#12", "why": "" })).unwrap_err();
    assert!(why.contains("say why"), "{why}");

    // The person approves on the card: control asks this machine.
    f.send(json!({ "t": "op", "id": "approve-1", "op": { "op": "agents.approved", "ask": "prp_0",
        "project": "prj_1", "action": "start", "session": "Chief of staff", "pane": chief,
        "issue": { "id": "iss_12", "number": 12, "title": "Fix the login" } } }));
    d.wait_for("its answer", || f.answer("approve-1").is_some());
    let a = f.answer("approve-1").unwrap();
    assert_eq!(a["answer"], "approved", "{a}");
    let block = a["block"].as_u64().unwrap();
    assert!(a["said"].as_str().unwrap().starts_with(&format!("started agent block %{block} on #12")), "{a}");

    // An agent block on the issue, in a worktree of the clone on its branch.
    let p = pane(&d, block).expect("the new block");
    assert_eq!(p["session_name"], "issue-12", "a tab of its own: {p}");
    // Arugula #1518: in the card's project, though its clone is in none.
    assert_eq!(p["session_project"]["id"], "prj_1", "the card's project: {p}");
    let b = d.get(&format!("/api/blocks/{block}"));
    let config = &b["state"];
    assert_eq!((&config["agent"], &config["title"]), (&json!("claude"), &json!("#12 Fix the login")), "{b}");
    assert_eq!(config["work"], json!({ "issue": "#12", "project": "prj_1" }), "{b}");
    assert_eq!(config["user_settings"], true, "{b}");
    let cwd = config["cwd"].as_str().unwrap();
    assert_eq!(config["worktree"], cwd, "{b}");
    assert!(Path::new(cwd).is_dir() && cwd != work.to_str().unwrap(), "{cwd}");
    assert_eq!(git(Path::new(cwd), &["branch", "--show-current"]), "feat/12-fix-the-login");
    d.wait(block, "idle");
    assert!(prompts(&d, block).contains("Work on #12, Fix the login."), "{}", prompts(&d, block));
    // Arugula #1440: told first that it leads #12's thread.
    assert!(prompts(&d, block).contains("You lead that issue's thread"), "{}", prompts(&d, block));

    // The card's answer reaches the chief of staff: its watch fires.
    let session = watch["session"].as_str().unwrap();
    f.send(json!({ "t": "op", "id": "m-wch_0", "op": { "op": "agents.mention", "room": "wch_0", "seq": 0,
        "author": "Arugula", "text": format!("ask.answered: prp_0 approved, started %{block}"),
        "watch": { "id": "wch_0", "session": session } } }));
    d.wait_for("the chief of staff woken", || prompts(&d, chief).contains("ask.answered: prp_0 approved"));

    // A close: asked, approved, and the block goes.
    let asked = mcp
        .call(
            "ask_approval",
            json!({ "action": "close", "blocks": [format!("%{block}")],
        "why": "#12 is done", "project": "prj_1" }),
        )
        .unwrap();
    assert_eq!(asked["ask"], "prp_1", "{asked}");
    let card = &f.asked("POST /api/daemon/projects/prj_1/proposals")[1];
    assert_eq!(card["proposal"]["blocks"], json!([block]), "{card}");
    assert_eq!(card["blocks"][0]["id"], format!("%{block}"), "{card}");
    assert_eq!(card["blocks"][0]["title"], "#12 Fix the login", "{card}");
    assert!(pane(&d, block).is_some(), "not closed by asking");
    f.send(json!({ "t": "op", "id": "approve-2", "op": { "op": "agents.approved", "ask": "prp_1",
        "project": "prj_1", "action": "close", "session": "Chief of staff", "pane": chief, "blocks": [block] } }));
    d.wait_for("its answer", || f.answer("approve-2").is_some());
    let a = f.answer("approve-2").unwrap();
    assert_eq!(a["answer"], "approved", "{a}");
    assert!(a["said"].as_str().unwrap().starts_with(&format!("closed %{block}")), "{a}");
    assert!(pane(&d, block).is_none(), "closed");

    // Not an agent block: nothing is closed.
    let shell = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    f.send(json!({ "t": "op", "id": "approve-3", "op": { "op": "agents.approved", "ask": "prp_2",
        "project": "prj_1", "action": "close", "session": "Chief of staff", "pane": chief, "blocks": [shell] } }));
    d.wait_for("its answer", || f.answer("approve-3").is_some());
    let a = f.answer("approve-3").unwrap();
    assert_eq!(a["answer"], "failed", "{a}");
    assert!(pane(&d, shell).is_some());

    // Arugula #1518: a card in another project than the owner's default
    // (prj_1, here) puts its tab in its own.
    f.send(json!({ "t": "op", "id": "approve-5", "op": { "op": "agents.approved", "ask": "prp_4",
        "project": "prj_2", "action": "start", "session": "Chief of staff", "pane": chief,
        "issue": { "id": "iss_14", "number": 14, "title": "Elsewhere" } } }));
    d.wait_for("its answer", || f.answer("approve-5").is_some());
    let a = f.answer("approve-5").unwrap();
    assert_eq!(a["answer"], "approved", "{a}");
    let p = pane(&d, a["block"].as_u64().unwrap()).expect("the new block");
    assert_eq!(p["session_project"]["id"], "prj_2", "the card's project, not the default: {p}");

    // The chief of staff is gone (moved, Arugula #1340): its card's start
    // fails, and nothing opens in some other directory.
    drop(mcp);
    d.post(&format!("/api/panes/{chief}/close"), json!({}));
    d.wait_for("closed", || pane(&d, chief).is_none());
    let before = d.get("/api/panes").as_array().unwrap().len();
    f.send(json!({ "t": "op", "id": "approve-4", "op": { "op": "agents.approved", "ask": "prp_3",
        "project": "prj_1", "action": "start", "session": "Chief of staff", "pane": chief,
        "issue": { "id": "iss_13", "number": 13, "title": "Another" } } }));
    d.wait_for("its answer", || f.answer("approve-4").is_some());
    let a = f.answer("approve-4").unwrap();
    assert_eq!(a["answer"], "failed", "{a}");
    assert!(a["error"].as_str().unwrap().contains("isn't running on"), "{a}");
    assert_eq!(d.get("/api/panes").as_array().unwrap().len(), before, "nothing started");

    drop(d);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Arugula #1465: a Claude Code chief of staff's card names its
/// conversation, and the answer control tells it, tagged with the card's
/// `prp_…`, wakes it with no watch: the person's answer and note, framed as
/// theirs, once.
#[test]
fn a_cards_answer_wakes_the_conversation_that_asked_with_no_watch() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let tmp = std::env::temp_dir().join(format!("arugula-answer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let bin = tmp.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let acp = bin.join("claude-agent-acp");
    std::fs::write(&acp, format!("#!/bin/sh\nexec {PYTHON} {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&acp, std::fs::Permissions::from_mode(0o755)).unwrap();
    let agents_dir = tmp.join("agents").display().to_string();
    let d = Daemon::child_in(&[], &[("ARUGULA_AGENTS_DIR", &agents_dir)], |state| enroll(state, &url));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    let config = json!({ "agent": "claude", "cwd": tmp, "title": "Chief of staff", "prompt": "hello" });
    let chief = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(chief, "idle");
    let session = d.state(chief)["session_id"].as_str().expect("its session").to_owned();
    let mcp = Mcp::bridge(&d, Some(chief));
    let asked = mcp
        .call("ask_approval", json!({ "action": "start", "issue": "#12", "why": "it's next", "project": "prj_1" }))
        .unwrap();
    assert_eq!(asked["ask"], "prp_0", "{asked}");
    let card = &f.asked("POST /api/daemon/projects/prj_1/proposals")[0];
    assert_eq!(card["conversation"], json!({ "agent": "claude", "id": session }), "{card}");
    assert!(f.asked("POST /api/daemon/watches").is_empty(), "no watch");

    // The person answers: control tells the conversation that asked.
    let said = "Ada approved your card prp_0 (start an agent on #12 Fix the login): started agent block %9.\nAda's note: \"go\"";
    let op = |id: &str| {
        json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "prp_0", "room_name": "answer",
        "seq": 0, "author": "Arugula", "text": said, "watch": { "id": "prp_0", "session": session } } })
    };
    f.send(op("m-1"));
    d.wait_for("its answer", || f.answer("m-1").is_some());
    assert_eq!(f.answer("m-1").unwrap()["routed"]["step"], "named");
    d.wait_for("the chief of staff woken", || prompts(&d, chief).contains("Your ask prp_0 was answered"));
    let p = prompts(&d, chief);
    assert!(p.contains("started agent block %9") && p.contains("Ada's note"), "{p}");
    // Sent again (control lost the daemon's answer): told once.
    d.wait(chief, "idle");
    f.send(op("m-2"));
    d.wait_for("its answer", || f.answer("m-2").is_some());
    assert_eq!(prompts(&d, chief).matches("Your ask prp_0 was answered").count(), 1, "once");

    // The done-when, on a machine: it adds a to-do with no watch, ends its
    // turn, and Done wakes it with the note; the to-do leaves its list.
    let added = mcp
        .call("todo", json!({ "kind": "add", "what": "Run the migration", "why": "it needs your prod credentials" }))
        .unwrap();
    assert_eq!(added["todo"], "prp_1", "{added}");
    assert!(added["summary"].as_str().unwrap().contains("no watch needed"), "{added}");
    let card = &f.asked("POST /api/daemon/projects/prj_1/proposals")[1];
    assert_eq!(card["conversation"], json!({ "agent": "claude", "id": session }), "{card}");
    assert!(f.asked("POST /api/daemon/watches").is_empty(), "no watch");
    let listed = mcp.call("todo", json!({ "kind": "list" })).unwrap();
    assert!(listed["summary"].as_str().unwrap().contains("prp_1"), "{listed}");
    d.wait(chief, "idle");
    let done = "Your to-do \"Run the migration\" (prp_1) is done: Ada did it.\nAda's note: \"14,203 rows\"";
    f.send(json!({ "t": "op", "id": "m-3", "op": { "op": "agents.mention", "room": "prp_1", "room_name": "answer",
        "seq": 0, "author": "Arugula", "text": done, "watch": { "id": "prp_1", "session": session } } }));
    d.wait_for("woken with the note", || prompts(&d, chief).contains("14,203 rows"));
    let listed = mcp.call("todo", json!({ "kind": "list" })).unwrap();
    assert!(!listed["summary"].as_str().unwrap().contains("prp_1"), "answered, it leaves the list: {listed}");

    // A control from before #1465 refuses a board ask's conversation: the
    // daemon asks again without it, and the ask goes in.
    let asked = mcp
        .call(
            "board_write",
            json!({ "kind": "ask", "board": "brd_1", "item": "#12", "who": "acc", "question": "Ship?" }),
        )
        .unwrap();
    assert!(asked["summary"].as_str().unwrap().contains("Asked on #12."), "{asked}");
    let tries: Vec<Value> = f.asked("POST /api/daemon/boards/brd_1/rows/row_1/ask");
    assert_eq!(tries, [json!(false), json!(true)], "refused with its conversation, taken without");

    drop(mcp);
    drop(d);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The reason pane `id` shows, while it needs input.
fn needs(d: &Daemon, id: u64) -> Option<Value> {
    pane(d, id).filter(|p| p["attention"] == "needs_input").map(|p| p["reason"].clone())
}

/// Arugula #1464's done-when: an agent block on a machine adds a to-do, and
/// its pane waits on its person for it (unpushed: the card was) until the
/// person's answer reaches this daemon, which wakes the agent with it. A
/// card does the same, and a cancel ends the wait.
#[test]
fn an_agent_blocks_to_do_is_its_pane_waiting_on_you_until_the_answer_reaches_the_daemon() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let tmp = std::env::temp_dir().join(format!("arugula-waits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let bin = tmp.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let acp = bin.join("claude-agent-acp");
    std::fs::write(&acp, format!("#!/bin/sh\nexec {PYTHON} {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&acp, std::fs::Permissions::from_mode(0o755)).unwrap();
    let agents_dir = tmp.join("agents").display().to_string();
    let d = Daemon::child_in(&[], &[("ARUGULA_AGENTS_DIR", &agents_dir)], |state| enroll(state, &url));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    let config = json!({ "agent": "claude", "cwd": tmp, "title": "Worker", "prompt": "hello" });
    let worker = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(worker, "idle");
    let session = d.state(worker)["session_id"].as_str().expect("its session").to_owned();
    assert_eq!(needs(&d, worker), None, "nothing waits yet");
    let mcp = Mcp::bridge(&d, Some(worker));

    // It adds a to-do: its pane waits on its person, under its what.
    let added = mcp
        .call("todo", json!({ "kind": "add", "what": "Run the migration", "why": "it needs your prod credentials" }))
        .unwrap();
    assert_eq!(added["todo"], "prp_0", "{added}");
    d.wait_for("the pane waits on you", || needs(&d, worker).is_some());
    let r = needs(&d, worker).unwrap();
    assert_eq!(r["kind"], "input", "{r}");
    assert_eq!(r["headline"], "Waits on you: Run the migration", "{r}");
    assert!(r["bundle"].as_str().unwrap().starts_with(arugula_proto::WAITS_BUNDLE), "{r}");

    // A card too: one more.
    let asked = mcp
        .call("ask_approval", json!({ "action": "start", "issue": "#12", "why": "it's next", "project": "prj_1" }))
        .unwrap();
    assert_eq!(asked["ask"], "prp_1", "{asked}");
    d.wait_for("both wait", || {
        needs(&d, worker).is_some_and(|r| r["headline"] == "Waits on you: Run the migration (+1 more)")
    });

    // The owner answers the to-do: control tells this daemon, which wakes
    // the agent with the note; only the card waits now.
    let done = "Your to-do \"Run the migration\" (prp_0) is done: Ada did it.\nAda's note: \"14,203 rows\"";
    f.send(json!({ "t": "op", "id": "m-1", "op": { "op": "agents.mention", "room": "prp_0", "room_name": "answer",
        "seq": 0, "author": "Arugula", "text": done, "watch": { "id": "prp_0", "session": session } } }));
    d.wait_for("its answer", || f.answer("m-1").is_some());
    assert_eq!(f.answer("m-1").unwrap()["routed"]["step"], "named");
    d.wait_for("woken with the note", || prompts(&d, worker).contains("14,203 rows"));
    d.wait(worker, "idle");
    d.wait_for("the card waits", || {
        needs(&d, worker).is_some_and(|r| r["headline"] == "Waits on you: approve starting an agent on #12")
    });

    // Another to-do, cancelled: it waits no more.
    let added = mcp.call("todo", json!({ "kind": "add", "what": "Rotate the key", "why": "it's yours" })).unwrap();
    assert_eq!(added["todo"], "prp_2", "{added}");
    d.wait_for("it waits too", || {
        needs(&d, worker).is_some_and(|r| r["headline"] == "Waits on you: approve starting an agent on #12 (+1 more)")
    });
    mcp.call("todo", json!({ "kind": "cancel", "id": "prp_2" })).unwrap();
    d.wait_for("cancelled", || {
        needs(&d, worker).is_some_and(|r| r["headline"] == "Waits on you: approve starting an agent on #12")
    });

    // The card is answered too: nothing waits, and the pane is the agent's
    // own again.
    let said = "Ada denied your card prp_1 (start an agent on #12 Fix the login).";
    f.send(json!({ "t": "op", "id": "m-2", "op": { "op": "agents.mention", "room": "prp_1", "room_name": "answer",
        "seq": 0, "author": "Arugula", "text": said, "watch": { "id": "prp_1", "session": session } } }));
    d.wait_for("its answer", || f.answer("m-2").is_some());
    d.wait_for("woken", || prompts(&d, worker).contains("denied your card prp_1"));
    d.wait(worker, "idle");
    d.wait_for("nothing waits", || {
        needs(&d, worker).is_none_or(|r| !r["bundle"].as_str().unwrap_or_default().starts_with("waits:"))
    });

    drop(mcp);
    drop(d);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Arugula #1466, the done-when: a to-do whose Claude Code session has
/// exited (its transcript is on this machine, in no pane) is marked Done,
/// and its answer opens an agent block on that conversation, continued,
/// with the note as its first message. A Can't starts nothing.
#[test]
fn done_on_an_ended_session_continues_its_conversation_with_the_note() {
    const ID: &str = "66666666-2222-4333-8444-555555555555";
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let tmp = std::env::temp_dir().join(format!("arugula-ended-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (claude, work) = (tmp.join("claude"), tmp.join("work"));
    std::fs::create_dir_all(claude.join("sessions")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let bin = tmp.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let acp = bin.join("claude-agent-acp");
    std::fs::write(&acp, format!("#!/bin/sh\nexec {PYTHON} {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&acp, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The conversation that added the to-do, and then exited.
    let cwd = work.display().to_string();
    let projects = claude.join("projects").join(cwd.replace(['/', '.'], "-"));
    std::fs::create_dir_all(&projects).unwrap();
    let line = |t: &str, uuid: &str, parent: Option<&str>, message: Value| {
        json!({ "type": t, "uuid": uuid, "parentUuid": parent, "sessionId": ID, "cwd": cwd, "entrypoint": "cli",
                "version": "2.1.288", "timestamp": "2026-11-26T20:00:00.000Z", "isSidechain": false,
                "message": message })
        .to_string()
    };
    let transcript = [
        line("user", "u1", None, json!({ "role": "user", "content": "migrate the users table" })),
        line(
            "assistant",
            "a1",
            Some("u1"),
            json!({ "id": "m1", "role": "assistant", "model": "claude-haiku-4-5-20251001",
                    "content": [{ "type": "text", "text": "I added a to-do for you to run it." }] }),
        ),
    ];
    std::fs::write(projects.join(format!("{ID}.jsonl")), transcript.join("\n") + "\n").unwrap();

    let env = [
        ("CLAUDE_CONFIG_DIR", claude.display().to_string()),
        ("ARUGULA_AGENTS_DIR", tmp.join("agents").display().to_string()),
    ];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let d = Daemon::child_in(&[], &env, |state| enroll(state, &url));
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let panes = || d.get("/api/panes").as_array().unwrap().len();
    let before = panes();
    let answer = |id: &str, ask: &str, text: &str, first: Option<&str>| {
        let mut watch = json!({ "id": ask, "session": ID });
        if let Some(p) = first {
            watch["first_prompt"] = json!(p);
        }
        json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": ask, "room_name": "answer",
            "seq": 0, "author": "Arugula", "text": text, "watch": watch } })
    };

    // Can't: kept on its card, and the conversation isn't started again.
    f.send(answer("m-cant", "prp_0", "Ada can't do your to-do \"Rotate the key\" (prp_0).", None));
    d.wait_for("its answer", || f.answer("m-cant").is_some());
    assert_eq!(f.answer("m-cant").unwrap()["routed"]["step"], "away");
    assert_eq!(panes(), before, "nothing opened");

    // Done, with a note: an agent block on the conversation, continued with
    // the note as its first message.
    let done = "Your to-do \"Run the migration\" (prp_1) is done: Ada did it.\nAda's note: \"14,203 rows\"";
    f.send(answer("m-done", "prp_1", done, Some("14,203 rows")));
    d.wait_for("its answer", || f.answer("m-done").is_some());
    assert_eq!(f.answer("m-done").unwrap()["routed"]["step"], "named", "{:?}", f.answer("m-done"));
    let listed = d.get("/api/conversations");
    let row = listed["conversations"].as_array().unwrap().iter().find(|c| c["id"] == ID).cloned();
    let block = row.and_then(|r| r["block"].as_u64()).expect("an agent block has the conversation");
    assert_eq!(panes(), before + 1);
    d.wait(block, "idle");
    let s = d.state(block);
    assert_eq!((&s["session_id"], &s["import"]["continued"]), (&json!(ID), &json!(true)), "{s}");
    let first: Value = serde_json::from_str(&prompts(&d, block)).unwrap();
    assert_eq!(first, json!([{ "type": "text", "text": "14,203 rows" }]), "the note alone");
    let said: Vec<String> = entries(&s).iter().filter_map(|e| e["text"].as_str().map(str::to_owned)).collect();
    let at = said.iter().position(|t| t == "Continued in Arugula").expect("continued");
    assert_eq!(said[at + 1..].iter().find(|t| !t.is_empty()).map(String::as_str), Some("14,203 rows"), "{said:?}");

    // Sent again (control lost the answer): it's in that block now, told once.
    f.send(answer("m-again", "prp_1", done, Some("14,203 rows")));
    d.wait_for("its answer", || f.answer("m-again").is_some());
    assert_eq!(panes(), before + 1, "one block");
    assert_eq!(entries(&d.state(block)).iter().filter(|e| e["text"] == "14,203 rows").count(), 1, "once");

    drop(d);
    let _ = std::fs::remove_dir_all(&tmp);
}
