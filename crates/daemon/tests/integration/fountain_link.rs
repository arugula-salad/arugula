//! Fountain track 2, F3 (#942): a Fountain conversation as an agent block
//! on the native link, against a stub Fountain served here (its REST API
//! and its SSE streams). No `fountain acp` runs, and nothing reaches a real
//! Fountain.
//!
//! What's checked (F3's "done when"): a conversation started in Arugula
//! (with the block's agent, vault, environment, labels and permission
//! policy, by name) prompts and answers, asks and is answered (here, or
//! elsewhere: the card goes either way); the daemon is killed mid-turn, the
//! turn ends and another client's turn runs while it's down, the sandbox
//! parks; started again, the block carries on from its log's cursor (every
//! stream request names it: no `session/load` replay, nothing twice), its
//! prompt answered by the turn that ended while it was away; the sandbox
//! panes say "Parked — Wake to look", Wake wakes it, and the diff, git
//! status, a directory and a file read; a wake that fails retryably sends
//! the prompt again by itself; Interrupt and Terminate. A block saved by a
//! daemon that ran `fountain acp` (no cursor in its log) comes back on the
//! native link with one `session/load`, merged without duplicates, then
//! follows from its cursor. With `--fountain-account-stream` the block's
//! events come from the account stream's fan-out.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd;

use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use agentd::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

const KEY: &str = "ftn_f3_test_key";
const DIFF: &str = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1..2 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n fn a() {}\n-fn b() {}\n+fn b() { todo!() }\n";

#[derive(Default)]
struct Conv {
    status: String,
    sandbox: String,
    /// The turn running, if one is.
    turn: Option<String>,
    turns: Vec<Value>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    /// `(id, conversation, payload)`.
    events: Vec<(u64, String, Value)>,
    convs: BTreeMap<String, Conv>,
    created: Vec<Value>,
    prompts: Vec<Value>,
    answers: Vec<(String, String)>,
    /// Each stream request: its path, `Last-Event-ID`, and whether it waits.
    streams: Vec<(String, Option<u64>, bool)>,
    /// The permission request waiting, `(conversation, turn, id)`.
    asked: Option<(String, String, u64)>,
    /// `flaky` prompts seen (the first one's wake fails).
    flaky: u32,
    reads: Vec<String>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Inner>>);

impl Fake {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }

    fn push(&self, conv: &str, p: Value) {
        self.with(|i| {
            i.next += 1;
            let id = i.next;
            let mut p = p;
            p["conversation_id"] = json!(conv);
            p["ts"] = json!("2026-10-15T00:00:00Z");
            i.events.push((id, conv.to_owned(), p));
        });
    }

    fn stage(&self, conv: &str, stage: &str, state: &str, meta: Value) {
        let turn = meta["turn_id"].clone();
        self.push(
            conv,
            json!({ "kind": "stage", "stream": "", "data": meta.to_string(), "stage": stage, "state": state, "turn_id": turn }),
        );
    }

    fn acp(&self, conv: &str, turn: &str, line: Value) {
        self.push(
            conv,
            json!({ "kind": "output", "stream": "acp", "data": line.to_string(), "stage": null, "state": null, "turn_id": turn }),
        );
    }

    fn say(&self, conv: &str, turn: &str, text: &str) {
        self.acp(
            conv,
            turn,
            json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "sprite-sess",
                "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } } } }),
        );
    }

    /// A turn starts: `crid` is whose prompt it is.
    fn start(&self, conv: &str, crid: Option<&str>, prompt: &str) -> String {
        let t = self.with(|i| {
            let c = i.convs.get_mut(conv).unwrap();
            let t = format!("turn-{}", c.turns.len() + 1);
            c.turns.push(json!({ "id": t, "turn_number": c.turns.len() + 1, "status": "running", "client_request_id": crid, "prompt": prompt }));
            c.turn = Some(t.clone());
            c.status = "running".into();
            t
        });
        self.stage(conv, "turn", "started", json!({ "turn_id": t, "client_request_id": crid, "turn_number": 1 }));
        t
    }

    fn end(&self, conv: &str, how: &str, stop: Option<&str>) {
        let t = self.with(|i| {
            let c = i.convs.get_mut(conv).unwrap();
            c.status = "idle".into();
            c.turn.take()
        });
        let Some(t) = t else { return };
        let mut meta = json!({ "turn_id": t });
        if let Some(s) = stop {
            meta["stop_reason"] = json!(s);
        }
        self.stage(conv, "turn", how, meta);
    }

    fn wake(&self, conv: &str) -> bool {
        let parked = self.with(|i| {
            let c = i.convs.get_mut(conv).unwrap();
            let p = c.sandbox == "suspended";
            c.sandbox = "ready".into();
            p
        });
        if parked {
            self.stage(conv, "reattach", "started", json!({ "sprite_name": "sp-1" }));
            self.stage(conv, "reattach", "done", json!({ "outcome": "no_running_turn" }));
        }
        parked
    }

    fn park(&self, conv: &str) {
        self.with(|i| i.convs.get_mut(conv).unwrap().sandbox = "suspended".into());
        self.stage(conv, "sandbox", "done", json!({ "event": "suspended", "reason": "idle" }));
    }

    /// Someone answered the waiting request (here or elsewhere): the turn
    /// goes on.
    fn answered(&self, option: &str) {
        let Some((conv, turn, id)) = self.with(|i| i.asked.take()) else { return };
        self.stage(&conv, "request", "done", json!({ "request_id": id, "outcome": "answered", "option_id": option }));
        self.say(&conv, &turn, &format!("allowed: {option}"));
        self.end(&conv, "done", Some("end_turn"));
    }

    fn conv_json(&self, id: &str) -> Option<Value> {
        self.with(|i| {
            let c = i.convs.get(id)?;
            Some(json!({ "id": id, "status": c.status, "agent_id": "agent-games", "sandbox_id": "sb-1",
                         "sandbox": { "id": "sb-1", "status": c.sandbox }, "runtime": "claude" }))
        })
    }

    fn after(&self, conv: Option<&str>, after: u64, streams: Option<&str>) -> Vec<(u64, Value)> {
        let want = |p: &Value| match streams {
            None => true,
            Some(s) => s.split(',').any(|w| (p["kind"] == "stage" && w == "stage") || p["stream"] == w),
        };
        self.with(|i| {
            i.events
                .iter()
                .filter(|(id, c, p)| *id > after && conv.is_none_or(|x| x == c) && want(p))
                .map(|(id, _, p)| (*id, p.clone()))
                .collect()
        })
    }
}

fn authed(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|a| a.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

fn err(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

async fn agents(h: HeaderMap) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    Json(json!({ "data": [{ "id": "agent-other", "name": "other" }, { "id": "agent-games", "name": "games", "runtime": "claude" }] }))
        .into_response()
}

async fn vaults() -> Response {
    Json(json!({ "data": [{ "id": "vault-1", "name": "v1" }] })).into_response()
}

async fn environments() -> Response {
    Json(json!({ "data": [{ "id": "env-1", "name": "e1" }] })).into_response()
}

async fn create(State(f): State<Fake>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    let id = f.with(|i| {
        i.created.push(b.clone());
        let id = format!("conv-{}", i.created.len());
        i.convs.insert(id.clone(), Conv { status: "idle".into(), sandbox: "ready".into(), ..Default::default() });
        id
    });
    f.stage(&id, "provision", "started", json!({}));
    f.stage(&id, "provision", "done", json!({}));
    (StatusCode::CREATED, Json(json!({ "data": f.conv_json(&id) }))).into_response()
}

async fn conversation(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    match f.conv_json(&id) {
        Some(c) => Json(json!({ "data": c })).into_response(),
        None => err(StatusCode::NOT_FOUND, "not_found"),
    }
}

async fn turns(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    let t = f.with(|i| i.convs.get(&id).map(|c| c.turns.clone()).unwrap_or_default());
    Json(json!({ "data": t })).into_response()
}

async fn prompts(State(f): State<Fake>, UrlPath(id): UrlPath<String>, Json(b): Json<Value>) -> Response {
    if f.conv_json(&id).is_none() {
        return err(StatusCode::NOT_FOUND, "not_found");
    }
    f.with(|i| i.prompts.push(b.clone()));
    let text = b["prompt"].as_str().unwrap_or_default().to_owned();
    let crid = b["client_request_id"].as_str().map(str::to_owned);
    if f.with(|i| i.convs[&id].turn.is_some()) {
        return err(StatusCode::CONFLICT, "conversation_busy");
    }
    // The first `flaky` prompt is answered queued, and its wake fails on the
    // stream (retryably).
    if text == "flaky"
        && f.with(|i| {
            i.flaky += 1;
            i.flaky == 1
        })
    {
        f.stage(&id, "wake", "failed", json!({ "reason": "fleet_full", "retryable": true }));
        return Json(json!({ "data": { "status": "queued", "client_request_id": crid } })).into_response();
    }
    f.wake(&id);
    let t = f.start(&id, crid.as_deref(), &text);
    match text.as_str() {
        "ask" => {
            let rid = 41;
            f.acp(
                &id,
                &t,
                json!({ "jsonrpc": "2.0", "id": rid, "method": "session/request_permission", "params": {
                    "sessionId": "sprite-sess",
                    "toolCall": { "toolCallId": "tc-1", "title": "rm -rf build", "kind": "execute",
                                  "rawInput": { "command": "rm -rf build" }, "locations": [{ "path": "/home/sprite/repo/build" }] },
                    "options": [
                        { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                        { "optionId": "reject", "name": "Reject", "kind": "reject_once" },
                    ] } }),
            );
            f.with(|i| i.asked = Some((id.clone(), t, rid)));
        }
        "slow" => f.say(&id, &t, "working on it"),
        _ => {
            f.say(&id, &t, &format!("echo: {text}"));
            f.end(&id, "done", Some("end_turn"));
        }
    }
    Json(json!({ "data": { "status": "accepted", "client_request_id": crid } })).into_response()
}

async fn answer(
    State(f): State<Fake>,
    UrlPath((_id, rid)): UrlPath<(String, String)>,
    Json(b): Json<Value>,
) -> Response {
    let option = b["option_id"].as_str().unwrap_or_default().to_owned();
    if f.with(|i| i.asked.as_ref().is_none_or(|a| a.2.to_string() != rid)) {
        return err(StatusCode::CONFLICT, "already_resolved");
    }
    f.with(|i| i.answers.push((rid, option.clone())));
    f.answered(&option);
    StatusCode::NO_CONTENT.into_response()
}

async fn interrupt(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    if f.with(|i| i.convs[&id].turn.is_none()) {
        return err(StatusCode::CONFLICT, "no_turn_running");
    }
    f.end(&id, "interrupted", None);
    Json(json!({ "data": {} })).into_response()
}

async fn wake(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    let status = if f.wake(&id) { "waking" } else { "awake" };
    Json(json!({ "data": { "status": status } })).into_response()
}

async fn terminate(State(f): State<Fake>, UrlPath(id): UrlPath<String>) -> Response {
    f.with(|i| {
        let c = i.convs.get_mut(&id).unwrap();
        (c.status, c.sandbox) = ("terminated".into(), "destroyed".into());
    });
    f.stage(&id, "terminate", "done", json!({}));
    Json(json!({ "data": {} })).into_response()
}

fn parked(f: &Fake) -> Option<Response> {
    f.with(|i| i.convs.values().any(|c| c.sandbox != "ready")).then(|| err(StatusCode::CONFLICT, "sandbox_not_ready"))
}

async fn sandbox_read(
    State(f): State<Fake>,
    UrlPath((_sb, what)): UrlPath<(String, String)>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    f.with(|i| i.reads.push(what.clone()));
    if let Some(r) = parked(&f) {
        return r;
    }
    let root = "/home/sprite/repo";
    match what.as_str() {
        "diff" => Json(json!({ "data": { "diff": DIFF, "path": root, "repo_root": root, "staged": false, "truncated": false } }))
            .into_response(),
        "git-status" => Json(json!({ "data": { "path": root, "repo_root": root, "branch": "main", "untracked": "all",
            "entries": [{ "path": "src/lib.rs", "index": "unmodified", "worktree": "modified" },
                        { "path": "new.txt", "index": "untracked", "worktree": "untracked" }],
            "truncated": false } }))
        .into_response(),
        "files" => Json(json!({ "data": { "path": q.get("path").cloned().unwrap_or("/home/sprite".into()),
            "entries": [{ "name": "repo", "type": "directory", "size": null }, { "name": "notes.md", "type": "file", "size": 6 }],
            "truncated": false } }))
        .into_response(),
        "file" => match q.get("path").map(String::as_str) {
            Some(p) if p.ends_with("notes.md") => Json(json!({ "data": { "path": "/home/sprite/notes.md",
                "content": "hello\n", "encoding": "utf-8", "size": 6, "truncated": false } }))
            .into_response(),
            _ => err(StatusCode::UNPROCESSABLE_ENTITY, "not_a_file"),
        },
        _ => err(StatusCode::NOT_FOUND, "not_found"),
    }
}

/// SSE, as Fountain frames it: what's after `Last-Event-ID`, then (unless
/// `wait=false`) what comes, until it's been quiet a while (Fountain's idle
/// close, short here, so reconnects happen).
fn sse(f: Fake, conv: Option<String>, after: u64, streams: Option<String>, wait: bool) -> Response {
    let frame = |id: u64, p: &Value| format!("id: {id}\nevent: {}\ndata: {p}\n\n", p["kind"].as_str().unwrap());
    let first: String = std::iter::once(": connected\n\n".to_owned())
        .chain(f.after(conv.as_deref(), after, streams.as_deref()).iter().map(|(id, p)| frame(*id, p)))
        .collect();
    let last = f.after(conv.as_deref(), after, None).last().map_or(after, |e| e.0).max(after);
    if !wait {
        return Response::builder().header("content-type", "text/event-stream").body(Body::from(first)).unwrap();
    }
    let st = (f, conv, streams, last, Instant::now(), Some(first));
    let s = futures_util::stream::unfold(st, move |(f, conv, streams, last, quiet, first)| async move {
        if let Some(c) = first {
            return Some((Ok::<_, Infallible>(c), (f, conv, streams, last, Instant::now(), None)));
        }
        loop {
            tokio::time::sleep(Duration::from_millis(25)).await;
            let all = f.after(conv.as_deref(), last, None);
            if let Some(&(newest, _)) = all.last() {
                let chunk: String =
                    f.after(conv.as_deref(), last, streams.as_deref()).iter().map(|(id, p)| frame(*id, p)).collect();
                return Some((Ok(chunk), (f, conv, streams, newest, Instant::now(), None)));
            }
            if quiet.elapsed() > Duration::from_millis(1500) {
                return None;
            }
        }
    });
    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(s)).unwrap()
}

fn last_event_id(h: &HeaderMap) -> Option<u64> {
    h.get("last-event-id").and_then(|v| v.to_str().ok()?.parse().ok())
}

async fn conv_stream(
    State(f): State<Fake>,
    h: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    if f.conv_json(&id).is_none() {
        return err(StatusCode::NOT_FOUND, "not_found");
    }
    let wait = q.get("wait").is_none_or(|w| w != "false");
    let after = last_event_id(&h);
    f.with(|i| i.streams.push((id.clone(), after, wait)));
    sse(f, Some(id), after.unwrap_or(0), q.get("streams").cloned(), wait)
}

/// The account stream: every conversation's events.
async fn account_stream(State(f): State<Fake>, h: HeaderMap) -> Response {
    if !authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "invalid_api_key");
    }
    let after = last_event_id(&h);
    f.with(|i| i.streams.push(("account".into(), after, true)));
    // With no cursor, only what happens from now on.
    let after = after.unwrap_or_else(|| f.with(|i| i.next));
    sse(f, None, after, None, true)
}

struct Fountain {
    _rt: tokio::runtime::Runtime,
    f: Fake,
    origin: String,
}

impl Fountain {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let f = Fake::default();
        let origin = rt.block_on(async {
            let api = Router::new()
                .route("/api/agents", get(agents))
                .route("/api/vaults", get(vaults))
                .route("/api/environments", get(environments))
                .route("/api/conversations", post(create))
                .route("/api/conversations/{id}", get(conversation))
                .route("/api/conversations/{id}/turns", get(turns))
                .route("/api/conversations/{id}/prompts", post(prompts))
                .route("/api/conversations/{id}/requests/{rid}", post(answer))
                .route("/api/conversations/{id}/interrupt", post(interrupt))
                .route("/api/conversations/{id}/wake", post(wake))
                .route("/api/conversations/{id}/terminate", post(terminate))
                .route("/api/conversations/{id}/stream", get(conv_stream))
                .route("/api/events/stream", get(account_stream))
                .route("/api/sandboxes/{sb}/{what}", get(sandbox_read))
                .with_state(f.clone());
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = format!("http://{}", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, api).await.unwrap() });
            at
        });
        Self { _rt: rt, f, origin }
    }

    fn daemon(&self, dir: &Scratch, args: &[&str]) -> Daemon {
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let home = home.display().to_string();
        let mut all = vec!["--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"];
        all.extend_from_slice(args);
        Daemon::child_labs_env(
            &all,
            &[
                ("HOME", home.as_str()),
                ("FOUNTAIN_API_KEY", KEY),
                ("FOUNTAIN_BASE_URL", self.origin.as_str()),
                ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
                ("ARUGULA_FOUNTAIN_STREAM_IDLE_MS", "5000"),
            ],
        )
    }

    /// Stream requests made since `from`.
    fn streams_since(&self, from: usize) -> Vec<(String, Option<u64>, bool)> {
        self.f.with(|i| i.streams[from..].to_vec())
    }
}

/// Wait until `f` holds for the block's state, saying what the state was
/// if it never does.
fn until(d: &Daemon, b: u64, what: &str, mut f: impl FnMut(&Value) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let st = d.state(b);
        if f(&st) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {st}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn texts(st: &Value, kind: &str) -> Vec<String> {
    entries(st).iter().filter(|e| e["type"] == kind).filter_map(|e| e["text"].as_str().map(str::to_owned)).collect()
}

fn count(st: &Value, kind: &str, text: &str) -> usize {
    texts(st, kind).iter().filter(|t| t.as_str() == text).count()
}

fn open_conversation(d: &Daemon) -> u64 {
    d.open_with(json!({ "type": "agent", "local": true, "config": {
        "agent": "fountain", "fountain_agent": "games", "vault": "v1", "environment": "e1",
        "labels": { "team": "arugula" } } }))
}

fn send(d: &Daemon, block: u64, text: &str) {
    d.call(block, "send", json!({ "text": text }));
}

fn line(d: &Daemon, block: u64) -> Value {
    d.state(block)["fountain"]["line"].clone()
}

/// F3's "done when", end to end.
#[test]
fn a_conversation_survives_close_restart_park_and_wake() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-link");
    let mut d = fz.daemon(&dir, &[]);

    // Started here: the agent, vault and environment by name; the labels;
    // approvals come to the block.
    let b = open_conversation(&d);
    d.wait_for("the conversation", || d.state(b)["status"] == "ready");
    let created = fz.f.with(|i| i.created.clone());
    assert_eq!(created.len(), 1);
    let c = &created[0];
    assert_eq!(
        (c["agent_id"].as_str(), c["vault_id"].as_str(), c["environment_id"].as_str()),
        (Some("agent-games"), Some("vault-1"), Some("env-1")),
        "{c}"
    );
    assert_eq!(c["labels"], json!({ "team": "arugula" }));
    assert_eq!(c["permission_policy"], json!({ "default": "ask" }));
    let st = d.state(b);
    assert_eq!(st["session_id"], "conv-1");
    assert_eq!(st["server"]["name"], "fountain");
    d.wait_for("its status", || line(&d, b) == "idle·awake");

    // A turn: its updates, its end.
    send(&d, b, "hello");
    assert_eq!(d.wait(b, "idle"), "done", "{}", d.state(b));
    assert_eq!(count(&d.state(b), "agent", "echo: hello"), 1);
    let p = fz.f.with(|i| i.prompts[0].clone());
    assert!(p["client_request_id"].as_str().unwrap().starts_with("arugula-"), "{p}");

    // Asked, and answered here.
    send(&d, b, "ask");
    d.wait_for("the card", || d.state(b)["pending"].as_array().is_some_and(|p| !p.is_empty()));
    let st = d.state(b);
    assert_eq!(st["pending"][0]["id"], "41");
    assert_eq!(st["pending"][0]["title"], "rm -rf build");
    d.call(b, "approve", json!({ "id": "41" }));
    assert_eq!(d.wait(b, "idle"), "done");
    assert_eq!(fz.f.with(|i| i.answers.clone()), [("41".to_owned(), "allow".to_owned())]);
    assert!(texts(&d.state(b), "agent").contains(&"allowed: allow".to_owned()));
    // Asked, and answered elsewhere (the phone, the CLI): the card goes.
    send(&d, b, "ask");
    d.wait_for("the second card", || d.state(b)["pending"].as_array().is_some_and(|p| !p.is_empty()));
    fz.f.answered("reject");
    d.wait_for("the card gone", || d.state(b)["pending"].as_array().is_some_and(|p| p.is_empty()));
    assert_eq!(d.wait(b, "idle"), "done");
    assert_eq!(fz.f.with(|i| i.answers.len()), 1, "nothing sent for an answer given elsewhere");

    // Killed mid-turn ("closed": the daemon gone, no chance to save more).
    send(&d, b, "slow");
    d.wait_for("the slow turn", || texts(&d.state(b), "agent").contains(&"working on it".to_owned()));
    let entries_before = entries(&d.state(b)).len();
    d.kill();
    // Meanwhile: the turn ends; another client's turn runs; it parks.
    fz.f.say("conv-1", "turn-4", "finished slow");
    fz.f.end("conv-1", "done", Some("end_turn"));
    fz.f.start("conv-1", Some("cli-1"), "from the terminal");
    fz.f.say("conv-1", "turn-5", "from the CLI");
    fz.f.end("conv-1", "done", Some("end_turn"));
    fz.f.park("conv-1");
    let streams_from = fz.f.with(|i| i.streams.len());
    d.start();
    d.wait_for("the block back", || d.raw("GET", &format!("/api/blocks/{b}"), None).0 == 200);
    // The prompt it sent before is answered by the turn that ended while
    // it was away.
    assert_eq!(d.wait(b, "idle"), "done", "{}", d.state(b));
    until(&d, b, "the other client's turn", |st| texts(st, "agent").contains(&"from the CLI".to_owned()));
    d.wait_for("parked", || line(&d, b) == "idle·parked");
    let st = d.state(b);
    // One turn across the restart: one message, said once.
    for (t, n) in [("echo: hello", 1), ("working on itfinished slow", 1), ("from the CLI", 1)] {
        assert_eq!(count(&st, "agent", t), n, "{t}: {st}");
    }
    assert!(texts(&st, "note").iter().any(|n| n.contains("wasn't started here")), "{st}");
    assert!(entries(&st).len() > entries_before);
    assert!(st["last_stop"] == "end_turn" && st["error"].is_null(), "{st}");
    // No replay: every stream request after the restart names the cursor,
    // and none drains the history.
    let after = fz.streams_since(streams_from);
    assert!(!after.is_empty());
    assert!(after.iter().all(|(_, c, wait)| c.is_some_and(|c| c > 0) && *wait), "{after:?}");
    assert_eq!(fz.f.with(|i| i.created.len()), 1, "no new conversation");

    // The sandbox panes: parked, they say so (a read never wakes it).
    let diff = d.open_with(json!({ "type": "diff", "config": { "fountain": { "conversation": "conv-1" } } }));
    d.wait_for("the diff to say it's parked", || d.state(diff)["parked"] == true);
    assert_eq!(d.state(diff)["error"], "Parked — Wake to look");
    let reads = fz.f.with(|i| i.reads.len());
    // Wake, from the block's header.
    let w = d.call(b, "wake", json!({}));
    assert_eq!(w["status"], "waking");
    d.wait_for("awake", || line(&d, b) == "idle·awake");
    assert!(fz.f.with(|i| i.reads.len()) >= reads);
    let (code, body) = d.raw("POST", &format!("/api/blocks/{diff}/call/refresh"), Some(json!({})));
    assert_eq!(code, 200, "{body}");
    let st = d.state(diff);
    assert_eq!(st["parked"], Value::Null, "{st}");
    let files: Vec<(String, String)> = st["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["path"].as_str().unwrap().to_owned(), f["status"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(files, [("src/lib.rs".into(), "modified".into()), ("new.txt".into(), "untracked".into())], "{st}");
    assert_eq!(st["repo"], "/home/sprite/repo");
    let (_, text) = d.raw("GET", &format!("/api/panes/{diff}/capture"), None);
    assert!(text.contains("+fn b() { todo!() }"), "{text}");
    // Files: the home, then a file.
    let fb = d.open_with(json!({ "type": "file", "config": { "path": "", "fountain": { "conversation": "conv-1" } } }));
    d.wait_for("the listing", || d.state(fb)["entries"].is_array());
    let names: Vec<String> =
        d.state(fb)["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["repo", "notes.md"]);
    d.call(fb, "open", json!({ "path": "/home/sprite/notes.md" }));
    let st = d.state(fb);
    assert_eq!((st["text"].as_str(), st["name"].as_str()), (Some("hello\n"), Some("notes.md")), "{st}");
    assert!(st["entries"].is_null());

    // On it goes, here.
    send(&d, b, "again");
    assert_eq!(d.wait(b, "idle"), "done");
    assert_eq!(count(&d.state(b), "agent", "echo: again"), 1);

    // A wake that fails retryably: the prompt goes again by itself.
    send(&d, b, "flaky");
    d.wait_for("the retry", || count(&d.state(b), "agent", "echo: flaky") == 1);
    assert_eq!(fz.f.with(|i| i.flaky), 2);
    assert_eq!(count(&d.state(b), "user", "flaky"), 1, "{}", d.state(b));

    // Interrupt.
    send(&d, b, "slow");
    d.wait_for("the turn", || line(&d, b) == "running");
    d.call(b, "interrupt", json!({}));
    d.wait_for("cancelled", || d.state(b)["last_stop"] == "cancelled");

    // Terminate: the disk goes.
    d.call(b, "terminate", json!({}));
    d.wait_for("terminated", || line(&d, b) == "terminated");
    assert!(texts(&d.state(b), "note").iter().any(|t| t.contains("Terminated on Fountain")));

    // Once more, across a plain restart: still nothing replayed.
    let streams_from = fz.f.with(|i| i.streams.len());
    let n = entries(&d.state(b)).len();
    d.stop();
    d.start();
    d.wait_for("the block back", || d.raw("GET", &format!("/api/blocks/{b}"), None).0 == 200);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(entries(&d.state(b)).len(), n);
    assert!(fz.streams_since(streams_from).iter().all(|(_, c, w)| c.is_some() && *w));
}

/// A block saved by a daemon that ran `fountain acp` (its log has no
/// cursor) comes back on the native link: one `session/load`, its replay
/// merged without duplicates, then a cursor, and no more replays.
#[test]
fn a_block_saved_before_f3_restores_onto_the_native_link() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-link-old");
    let mut d = fz.daemon(&dir, &[]);
    let b = open_conversation(&d);
    d.wait_for("the conversation", || d.state(b)["status"] == "ready");
    send(&d, b, "hello");
    assert_eq!(d.wait(b, "idle"), "done");
    d.stop();
    // What an older daemon's log looks like: the same frames, with no
    // Fountain event ids on them (and no status notifications).
    let blk = d.state.join("blocks").join(b.to_string());
    for e in std::fs::read_dir(&blk).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("seg-") && name.ends_with(".log")) {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap();
        let mut out = String::new();
        for l in text.lines() {
            let Ok(mut v) = serde_json::from_str::<Value>(l) else {
                out.push_str(l);
                out.push('\n');
                continue;
            };
            if v["m"]["method"] == "_fountain/status" {
                continue;
            }
            if let Some(m) = v["m"].as_object_mut() {
                m.remove("_arugula");
            }
            out.push_str(&v.to_string());
            out.push('\n');
        }
        std::fs::write(e.path(), out).unwrap();
    }
    // While it was down, more happened.
    fz.f.start("conv-1", Some("cli-1"), "from the terminal");
    fz.f.say("conv-1", "turn-2", "while it was away");
    fz.f.end("conv-1", "done", Some("end_turn"));
    let from = fz.f.with(|i| i.streams.len());
    d.start();
    d.wait_for("the block back", || d.raw("GET", &format!("/api/blocks/{b}"), None).0 == 200);
    until(&d, b, "loaded", |st| texts(st, "agent").contains(&"while it was away".to_owned()));
    d.wait_for("ready", || d.state(b)["status"] == "ready");
    let st = d.state(b);
    assert_eq!(count(&st, "agent", "echo: hello"), 1, "merged, not duplicated: {st}");
    assert_eq!(count(&st, "user", "hello"), 1, "{st}");
    let s = fz.streams_since(from);
    assert_eq!(s.iter().filter(|(_, _, wait)| !wait).count(), 1, "one replay: {s:?}");
    assert!(s.iter().any(|(_, c, w)| *w && c.is_some_and(|c| c > 0)), "then followed from the cursor: {s:?}");
    send(&d, b, "after");
    assert_eq!(d.wait(b, "idle"), "done");
    assert_eq!(count(&d.state(b), "agent", "echo: after"), 1);
    // From now on it has a cursor: a restart replays nothing.
    let from = fz.f.with(|i| i.streams.len());
    d.stop();
    d.start();
    d.wait_for("the block back", || d.raw("GET", &format!("/api/blocks/{b}"), None).0 == 200);
    d.wait_for("following", || !fz.streams_since(from).is_empty());
    assert!(fz.streams_since(from).iter().all(|(_, c, w)| *w && c.is_some()));
    assert_eq!(count(&d.state(b), "agent", "echo: hello"), 1);
}

/// With the account stream on this daemon, a block's events come from its
/// fan-out (after catching up from the cursor), not a stream of its own.
#[test]
fn with_the_account_stream_a_block_follows_its_fan_out() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-link-hub");
    let mut d = fz.daemon(&dir, &["--fountain-account-stream"]);
    d.wait_for("the account stream", || fz.f.with(|i| i.streams.iter().any(|s| s.0 == "account")));
    let b = open_conversation(&d);
    d.wait_for("the conversation", || d.state(b)["status"] == "ready");
    send(&d, b, "hello");
    assert_eq!(d.wait(b, "idle"), "done");
    let s = fz.f.with(|i| i.streams.clone());
    assert!(s.iter().filter(|x| x.0 == "conv-1").all(|x| !x.2), "only catch-up reads, no stream of its own: {s:?}");
    d.kill();
    fz.f.start("conv-1", Some("cli-1"), "from the terminal");
    fz.f.say("conv-1", "turn-2", "while it was away");
    fz.f.end("conv-1", "done", Some("end_turn"));
    d.start();
    until(&d, b, "caught up", |st| texts(st, "agent").contains(&"while it was away".to_owned()));
    send(&d, b, "after");
    assert_eq!(d.wait(b, "idle"), "done");
    let st = d.state(b);
    for t in ["echo: hello", "while it was away", "echo: after"] {
        assert_eq!(count(&st, "agent", t), 1, "{t}: {st}");
    }
}
