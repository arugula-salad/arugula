//! Fountain track 2, F2 (#942): the account view and attention, against a
//! stub Fountain served here (REST and the account's SSE stream).
//!
//! What's checked, F2's "done when": a conversation started anywhere asks
//! for permission (the stream's `permission_request` block and `request`
//! stage) and Arugula raises a card and a phone push, with no block open for
//! it (the account view opens to hold the card); allowing it from the card
//! answers Fountain with the agent's own option and the turn goes on; a
//! request answered elsewhere (the CLI) clears its card; losing the race
//! (409) clears it and says so; a request replayed after its timeout raises
//! nothing. Then the view: every conversation with live status from the
//! stream, the unread dot and marking read, the label filter (Fountain's
//! `?label=`), a fan-out's tree, opening a conversation as an `agent:
//! fountain` block, and MCP's `list` kind `fountain_conversations`.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentd::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as UrlPath, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

const KEY: &str = "ftn_view_test_key";

/// One event: its number on the wire (none for a signal), its SSE event and
/// its payload.
type Ev = (Option<u64>, String, Value);

#[derive(Default)]
struct Inner {
    events: Vec<Ev>,
    next_id: u64,
    convs: Vec<Value>,
    /// (conversation, request, option) as answered here.
    answers: Vec<(String, String, String)>,
    resolved: HashSet<String>,
    reads: Vec<String>,
    /// The list's query strings.
    lists: Vec<String>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Inner>>);

impl Fake {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }

    fn push(&self, kind: &str, data: Value) {
        self.with(|i| {
            let id = (kind != "conversations").then(|| {
                i.next_id += 1;
                i.next_id
            });
            i.events.push((id, kind.to_owned(), data));
        });
    }

    fn stage(&self, conv: &str, stage: &str, state: &str, meta: Value, ts: &str) {
        self.push(
            "stage",
            json!({ "conversation_id": conv, "kind": "stage", "stage": stage, "state": state,
                    "data": meta.to_string(), "ts": ts, "turn_id": format!("{conv}-t1"), "blocks": [] }),
        );
    }

    /// A conversation asks to run `command`: the transcript's block, then
    /// the request stage, as Fountain sends them.
    fn ask(&self, conv: &str, request: &str, command: &str, ts: &str) {
        let options = json!([
            { "optionId": "allow-o", "name": "Allow", "kind": "allow_once" },
            { "optionId": "always-o", "name": "Always allow", "kind": "allow_always" },
            { "optionId": "reject-o", "name": "Reject", "kind": "reject_once" },
        ]);
        self.push(
            "output",
            json!({ "conversation_id": conv, "kind": "output", "stream": "acp", "data": "{}", "ts": ts,
                    "blocks": [{ "kind": "permission_request", "request_id": request, "name": "Bash",
                                 "summary": command, "options": options }] }),
        );
        let meta = json!({ "request_id": request, "tool": "Bash", "options": options, "timeout_ms": 300_000 });
        self.stage(conv, "request", "started", meta, ts);
    }

    /// Another client answers (the CLI, an editor): first answer wins, and
    /// the stream says it's done.
    fn answer(&self, conv: &str, request: &str, option: &str) -> bool {
        if !self.with(|i| i.resolved.insert(request.to_owned())) {
            return false;
        }
        self.with(|i| i.answers.push((conv.to_owned(), request.to_owned(), option.to_owned())));
        let meta = json!({ "request_id": request, "outcome": "answered", "option_id": option });
        self.stage(conv, "request", "done", meta, &now_iso());
        // The turn goes on, and ends.
        self.push(
            "output",
            json!({ "conversation_id": conv, "kind": "output", "stream": "acp", "data": "{}", "blocks": [] }),
        );
        self.stage(conv, "turn", "done", json!({}), &now_iso());
        true
    }
}

/// Now, as Fountain writes a time.
fn now_iso() -> String {
    iso(std::time::SystemTime::now())
}

fn iso(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn authed(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|a| a.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

fn refused() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "bad key", "reason": "invalid_api_key" }))).into_response()
}

fn sse(ev: &Ev) -> String {
    let id = ev.0.map(|i| format!("id: {i}\n")).unwrap_or_default();
    format!("{id}event: {}\ndata: {}\n\n", ev.1, ev.2)
}

/// The account stream: what's after `Last-Event-ID`, then whatever comes,
/// as it comes.
async fn stream(State(f): State<Fake>, h: HeaderMap) -> Response {
    if !authed(&h) {
        return refused();
    }
    let after = h.get("last-event-id").and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    let (backlog, from) = f.with(|i| {
        let backlog: String =
            i.events.iter().filter(|e| after.is_some_and(|a| e.0.is_some_and(|id| id > a))).map(sse).collect();
        (backlog, i.events.len())
    });
    let first = futures_util::stream::once(async move { Ok::<_, std::io::Error>(format!(": connected\n\n{backlog}")) });
    let rest = futures_util::stream::unfold((f, from, 0u32), |(f, seen, ticks)| async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let (chunk, now) = f.with(|i| (i.events[seen..].iter().map(sse).collect::<String>(), i.events.len()));
        let chunk = if chunk.is_empty() && ticks % 30 == 29 { ": heartbeat\n\n".to_owned() } else { chunk };
        Some((Ok::<_, std::io::Error>(chunk), (f, now, ticks + 1)))
    });
    use futures_util::StreamExt;
    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(first.chain(rest))).unwrap()
}

async fn conversations(State(f): State<Fake>, h: HeaderMap, RawQuery(q): RawQuery) -> Response {
    if !authed(&h) {
        return refused();
    }
    let q = q.unwrap_or_default();
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(q.as_bytes()).into_owned().collect();
    let labels: Vec<(String, String)> = pairs
        .iter()
        .filter(|(k, _)| k == "label")
        .filter_map(|(_, v)| v.split_once(':').map(|(a, b)| (a.to_owned(), b.to_owned())))
        .collect();
    let limit = pairs.iter().find(|(k, _)| k == "limit").and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(1000);
    let data: Vec<Value> = f.with(|i| {
        i.lists.push(q.clone());
        i.convs
            .iter()
            .filter(|c| labels.iter().all(|(k, v)| c["labels"][k] == v.as_str()))
            .take(limit)
            .cloned()
            .collect()
    });
    Json(json!({ "data": data })).into_response()
}

async fn conversation(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if !authed(&h) {
        return refused();
    }
    match f.with(|i| i.convs.iter().find(|c| c["id"] == id.as_str()).cloned()) {
        Some(c) => Json(json!({ "data": c })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response(),
    }
}

async fn read(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if !authed(&h) {
        return refused();
    }
    f.with(|i| {
        i.reads.push(id.clone());
        if let Some(c) = i.convs.iter_mut().find(|c| c["id"] == id.as_str()) {
            c["unread"] = json!(false);
        }
    });
    f.push("conversations", json!({ "reason": "changed" }));
    StatusCode::NO_CONTENT.into_response()
}

async fn tree(State(f): State<Fake>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    if !authed(&h) {
        return refused();
    }
    let nodes: Vec<Value> = f.with(|i| {
        i.convs
            .iter()
            .filter(|c| c["id"] == id.as_str() || c["parent_conversation_id"] == id.as_str())
            .map(|c| json!({ "id": c["id"], "parent_id": c["parent_conversation_id"], "status": c["status"], "source": "api" }))
            .collect()
    });
    Json(json!({ "data": nodes })).into_response()
}

async fn answer(
    State(f): State<Fake>,
    h: HeaderMap,
    UrlPath((id, request)): UrlPath<(String, String)>,
    Json(b): Json<Value>,
) -> Response {
    if !authed(&h) {
        return refused();
    }
    let option = b["option_id"].as_str().unwrap_or_default().to_owned();
    if f.answer(&id, &request, &option) {
        Json(json!({ "ok": true })).into_response()
    } else {
        (StatusCode::CONFLICT, Json(json!({ "error": "permission_request_resolved" }))).into_response()
    }
}

async fn agents(h: HeaderMap) -> Response {
    if !authed(&h) {
        return refused();
    }
    Json(
        json!({ "data": [{ "id": "a1", "name": "games", "runtime": "claude", "model": "anthropic/claude-sonnet-4" }] }),
    )
    .into_response()
}

async fn environments(h: HeaderMap) -> Response {
    if !authed(&h) {
        return refused();
    }
    Json(json!({ "data": [] })).into_response()
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
        f.with(|i| {
            i.convs = vec![
                json!({ "id": "c1-flaky", "status": "idle", "agent_id": "a1", "title": "fix the flaky test",
                        "unread": true, "labels": { "env": "prod" }, "runtime": "claude", "turn_count": 2 }),
                json!({ "id": "c2-child", "status": "idle", "agent_id": "a1", "title": "check the logs",
                        "parent_conversation_id": "c1-flaky", "labels": { "env": "dev" }, "runtime": "claude" }),
                json!({ "id": "c3-cli", "status": "idle", "agent_id": "a1", "title": null,
                        "labels": { "team": "a" }, "runtime": "claude" }),
            ];
        });
        let origin = rt.block_on(async {
            let api = Router::new()
                .route("/api/events/stream", get(stream))
                .route("/api/conversations", get(conversations))
                .route("/api/conversations/{id}", get(conversation))
                .route("/api/conversations/{id}/read", post(read))
                .route("/api/conversations/{id}/tree", get(tree))
                .route("/api/conversations/{id}/requests/{request}", post(answer))
                .route("/api/agents", get(agents))
                .route("/api/environments", get(environments))
                .with_state(f.clone());
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = format!("http://{}", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, api).await.unwrap() });
            at
        });
        Self { _rt: rt, f, origin }
    }

    /// A daemon that follows the account (`--fountain-account-stream`), with
    /// the key in `$FOUNTAIN_API_KEY` (the login it falls back to).
    fn daemon(&self, dir: &Scratch, stream: bool) -> Daemon {
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let home = home.display().to_string();
        let mut args = vec!["--wisp-token-file", "/nonexistent", "--tailscale-socket", "/nonexistent/sock"];
        if stream {
            args.push("--fountain-account-stream");
        }
        let d = Daemon::child_env(
            &args,
            &[
                ("HOME", home.as_str()),
                ("FOUNTAIN_API_KEY", KEY),
                ("FOUNTAIN_BASE_URL", self.origin.as_str()),
                ("ARUGULA_FOUNTAIN_STREAM_RETRY_MS", "50"),
                ("ARUGULA_FOUNTAIN_RELIST_MS", "100"),
                ("ARUGULA_FOUNTAIN_POLL_MS", "600000"),
            ],
        );
        if stream {
            d.wait_for("the account stream", || d.get("/api/fountain/login")["stream"]["connected"] == true);
        }
        d
    }
}

fn fountain_blocks(d: &Daemon) -> Vec<Value> {
    d.get("/api/panes").as_array().unwrap().iter().filter(|p| p["type"] == "fountain").cloned().collect()
}

fn pane(d: &Daemon, id: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap_or_default()
}

fn asking(d: &Daemon, block: u64) -> Vec<String> {
    d.state(block)["requests"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn a_permission_request_from_any_conversation_is_a_card_and_a_push() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-attention");
    let d = fz.daemon(&dir, true);
    let phone = Phone::subscribe(&d);
    assert!(fountain_blocks(&d).is_empty(), "no view open");

    // `fountain run` in a terminal: its conversation asks to run a command.
    fz.f.stage("c1-flaky", "turn", "started", json!({}), &now_iso());
    fz.f.ask("c1-flaky", "0.r1", "rm -rf build", &now_iso());
    let push = phone.needs_you();
    assert_eq!(push["approve"]["id"], "0.r1", "{push}");
    assert!(push["body"].as_str().unwrap().contains("rm -rf build"), "{push}");
    assert_eq!(push["reason"]["actions"], json!(["allow", "deny", "dismiss"]), "{push}");
    // The account view opened to hold the card, in a session of its own.
    let views = fountain_blocks(&d);
    assert_eq!(views.len(), 1, "{views:?}");
    let block = views[0]["id"].as_u64().unwrap();
    assert_eq!(push["pane"], block);
    let p = pane(&d, block);
    assert_eq!(p["attention"], "needs_input", "{p}");
    assert_eq!((p["reason"]["kind"].as_str(), p["reason"]["ask"]["id"].as_str()), (Some("ask"), Some("0.r1")), "{p}");
    assert!(p["reason"]["headline"].as_str().unwrap().contains("fix the flaky test wants to run rm -rf build"), "{p}");
    d.wait_for("the list", || d.state(block)["conversations"].as_array().is_some_and(|c| c.len() == 3));
    let st = d.state(block);
    assert_eq!(st["card"], "0.r1");
    assert_eq!(st["requests"][0]["conversation"], "fix the flaky test", "{st}");
    let row = st["conversations"].as_array().unwrap().iter().find(|r| r["id"] == "c1-flaky").cloned().unwrap();
    assert_eq!(row["asking"], 1, "{row}");

    // Allowed from the card (as the phone's Allow does): Fountain is sent
    // the agent's own option, and the turn goes on and ends.
    let r = d.post("/api/attention/act", json!({ "action": "allow", "pane": block, "id": "0.r1" }));
    assert_eq!(r["results"][0]["ok"], true, "{r}");
    assert_eq!(fz.f.with(|i| i.answers.clone()), [("c1-flaky".into(), "0.r1".into(), "allow-o".into())]);
    d.wait_for("the card to close", || pane(&d, block)["attention"] != "needs_input");
    assert!(asking(&d, block).is_empty());

    // Another, answered elsewhere (the CLI): its card clears, and Arugula
    // sent nothing.
    fz.f.ask("c3-cli", "0.r2", "git push --force", &now_iso());
    let push = phone.needs_you();
    assert_eq!(push["approve"]["id"], "0.r2", "{push}");
    d.wait_for("the second card", || pane(&d, block)["reason"]["ask"]["id"] == "0.r2");
    assert!(fz.f.answer("c3-cli", "0.r2", "reject-o"));
    d.wait_for("the card to clear", || pane(&d, block)["attention"] != "needs_input");
    assert!(asking(&d, block).is_empty());
    assert_eq!(fz.f.with(|i| i.answers.len()), 2, "Arugula answered nothing");

    // Two at once: the card shows the newest (the one just pushed), the
    // view lists both, and the phone's Allow for the older one still works.
    fz.f.ask("c1-flaky", "0.r3", "make test", &now_iso());
    assert_eq!(phone.needs_you()["approve"]["id"], "0.r3");
    fz.f.ask("c3-cli", "0.r4", "npm publish", &now_iso());
    assert_eq!(phone.needs_you()["approve"]["id"], "0.r4", "a push for each");
    d.wait_for("both", || asking(&d, block).len() == 2);
    assert_eq!(pane(&d, block)["reason"]["ask"]["id"], "0.r4");
    let r = d.post("/api/attention/act", json!({ "action": "deny", "pane": block, "id": "0.r3" }));
    assert_eq!(r["results"][0]["ok"], true, "{r}");
    assert_eq!(fz.f.with(|i| i.answers.last().cloned().unwrap().2), "reject-o");
    d.wait_for("one left", || asking(&d, block) == ["0.r4"]);
    assert_eq!(pane(&d, block)["attention"], "needs_input");

    // Losing the race: someone else answered and the stream hasn't said so
    // yet. Fountain's 409 closes the card, saying why.
    fz.f.with(|i| i.resolved.insert("0.r4".into()));
    let (status, body) =
        d.raw("POST", "/api/attention/act", Some(json!({ "action": "allow", "pane": block, "id": "0.r4" })));
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("already settled"), "{body}");
    d.wait_for("the card to close", || pane(&d, block)["attention"] != "needs_input");
    assert!(asking(&d, block).is_empty());

    // A request replayed after its timeout raises nothing.
    let old = iso(std::time::SystemTime::now() - Duration::from_secs(600));
    fz.f.ask("c3-cli", "0.r5", "sleep", &old);
    assert!(phone.quiet(1500), "no push for a request Fountain gave up on");
    assert!(asking(&d, block).is_empty());
    assert_eq!(fountain_blocks(&d).len(), 1, "no second view");
}

#[test]
fn the_account_view_lists_filters_and_opens() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-view");
    let d = fz.daemon(&dir, true);
    let block =
        d.post("/api/blocks", json!({ "type": "fountain", "config": {}, "local": true }))["block"].as_u64().unwrap();
    d.wait_for("the list", || d.state(block)["conversations"].as_array().is_some_and(|c| c.len() == 3));
    let st = d.state(block);
    assert_eq!((st["tab"].as_str(), st["key_from"].as_str()), (Some("conversations"), Some("env")), "{st}");
    assert_eq!(st["error"], Value::Null, "{st}");
    assert_eq!(st["total"], 1, "the agents too");
    assert!(st["stream"] == true && st["live"] == true, "{st}");
    let rows = st["conversations"].as_array().unwrap();
    assert_eq!(rows[0]["agent"], "games", "{st}");
    assert_eq!((rows[0]["unread"].as_bool(), rows[0]["children"].as_u64()), (Some(true), Some(1)));
    assert!(fz.f.with(|i| i.lists[0].contains("limit=100")), "a page: {:?}", fz.f.with(|i| i.lists.clone()));
    let text = d.raw("GET", &format!("/api/panes/{block}/capture"), None).1;
    assert!(text.contains("3 conversations, live"), "{text}");
    assert!(text.contains("● fix the flaky test [idle] games +1 started {env:prod} c1-flaky"), "{text}");
    assert!(text.contains("1 agents"), "{text}");

    // Live status from the stream, without reading the list.
    let lists = fz.f.with(|i| i.lists.len());
    fz.f.stage("c3-cli", "turn", "started", json!({}), &now_iso());
    let live = |d: &Daemon| {
        d.state(block)["conversations"].as_array().unwrap().iter().find(|r| r["id"] == "c3-cli").unwrap()["live"]
            .clone()
    };
    d.wait_for("running", || live(&d) == "running");
    assert_eq!(fz.f.with(|i| i.lists.len()), lists, "a stage event changes the row in place");
    // A signal reads the list again.
    fz.f.push("conversations", json!({ "reason": "changed" }));
    d.wait_for("a read", || fz.f.with(|i| i.lists.len()) > lists);

    // The label filter is Fountain's, and kept.
    let out = d.call(block, "labels", json!({ "labels": ["env:prod"] }));
    assert_eq!(out["shown"], 1, "{out}");
    assert!(fz.f.with(|i| i.lists.last().unwrap().contains("label=env%3Aprod")));
    assert_eq!(d.state(block)["labels"], json!(["env:prod"]));
    let (status, body) = d.raw("POST", &format!("/api/blocks/{block}/call/labels"), Some(json!({ "labels": ["x"] })));
    assert_eq!(status, 400, "{body}");
    d.call(block, "labels", json!({ "labels": [] }));
    assert_eq!(d.state(block)["conversations"].as_array().unwrap().len(), 3);

    // A fan-out's tree.
    let t = d.call(block, "tree", json!({ "id": "c1-flaky" }));
    let ids: Vec<&str> = t["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["c1-flaky", "c2-child"], "{t}");
    assert_eq!(t["nodes"][1]["depth"], 1);
    assert_eq!(d.state(block)["tree"]["nodes"][1]["title"], "check the logs");

    // Opening one: today's `agent: fountain` block beside the view, loading
    // and following that conversation; it's read now.
    let out = d.call(block, "open_conversation", json!({ "id": "c1-flaky" }));
    let agent = out["block"].as_u64().unwrap();
    let layout = || -> Value {
        let l: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap_or_default())
            .unwrap_or_default();
        l["panes"][agent.to_string()]["config"].clone()
    };
    d.wait_for("the agent's config saved", || layout()["agent"] == "fountain");
    let config = layout();
    assert_eq!(
        (config["agent"].as_str(), config["fountain_agent"].as_str(), config["session_id"].as_str()),
        (Some("fountain"), Some("a1"), Some("c1-flaky")),
        "{config}"
    );
    assert_eq!(config["follow"], true, "never a new conversation: {config}");
    assert_eq!(pane(&d, agent)["tab"], pane(&d, block)["tab"], "beside the view");
    assert_eq!(fz.f.with(|i| i.reads.clone()), ["c1-flaky"]);
    let unread = |d: &Daemon| {
        d.state(block)["conversations"].as_array().unwrap().iter().find(|r| r["id"] == "c1-flaky").unwrap()["unread"]
            .clone()
    };
    assert_eq!(unread(&d), false);
    let (status, _) =
        d.raw("POST", &format!("/api/blocks/{block}/call/open_conversation"), Some(json!({ "id": "nope" })));
    assert_eq!(status, 400);

    // MCP: the account's conversations, read-only.
    let m = Mcp::bridge(&d, None);
    let r = m.call("list", json!({ "kind": "fountain_conversations", "labels": ["team:a"] })).unwrap();
    let convs = r["conversations"].as_array().unwrap();
    assert_eq!(convs.len(), 1, "{r}");
    assert_eq!(convs[0]["id"], "c3-cli");
    assert!(r["summary"].as_str().unwrap().starts_with("1 conversations on"), "{r}");
}

/// A daemon that doesn't follow the account stream reads the list, but
/// raises no card: only the one that does pushes.
#[test]
fn only_the_daemon_following_the_account_raises_cards() {
    let fz = Fountain::start();
    let dir = Scratch::new("fountain-not-home");
    let d = fz.daemon(&dir, false);
    let phone = Phone::subscribe(&d);
    let block =
        d.post("/api/blocks", json!({ "type": "fountain", "config": {}, "local": true }))["block"].as_u64().unwrap();
    d.wait_for("the list", || d.state(block)["conversations"].as_array().is_some_and(|c| c.len() == 3));
    assert_eq!(d.state(block)["stream"], false);
    fz.f.ask("c1-flaky", "0.r1", "rm -rf build", &now_iso());
    assert!(phone.quiet(1500));
    assert!(asking(&d, block).is_empty());
    assert_ne!(pane(&d, block)["attention"], "needs_input");
}
