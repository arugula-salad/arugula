//! #1067: an agent asks to share its Doc, and only the owner's tap
//! publishes. Against the real daemon, joined to a fake control that counts
//! what it is sent: the agent's own token (`Scope::Block`) asks with `doc
//! share`, which raises a card on the Doc block and uploads nothing; Share
//! uploads, and a repeat call hands the agent the URL; Not now, a lapse and a
//! failure upload nothing and are said once; nobody but the owner answers
//! the card, by any route; and a restart keeps it.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use axum::{
    Json, Router,
    extract::{State, ws::WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde_json::{Value, json};

use crate::agentd::*;
use crate::mcp::agent_mcp;

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";
const EDITOR: &str = "pal@example.com";
const ONLY_OWNER: &str = "only the owner can share this Doc";

/// A control that keeps nothing and counts what a daemon sends it.
#[derive(Clone, Default)]
struct Fake {
    /// Parts put, commits made and links deleted.
    puts: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
    /// Answer the quota with this status instead of 200 (0: as it is).
    refuse: Arc<AtomicU16>,
    viewer: Arc<Mutex<String>>,
}

async fn quota(State(f): State<Fake>) -> Response {
    match f.refuse.load(Ordering::SeqCst) {
        0 => Json(json!({ "used": 0, "limit": 200 << 20, "links": 0, "max_links": 100, "max_link": 48 << 20 }))
            .into_response(),
        s => (axum::http::StatusCode::from_u16(s).unwrap(), Json(json!({}))).into_response(),
    }
}

async fn part(State(f): State<Fake>) -> Json<Value> {
    f.puts.fetch_add(1, Ordering::SeqCst);
    Json(json!({}))
}

async fn commit(State(f): State<Fake>) -> Json<Value> {
    f.commits.fetch_add(1, Ordering::SeqCst);
    Json(json!({}))
}

async fn dial(up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        drop(ws);
    })
}

fn serve(f: Fake) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        *f.viewer.lock().unwrap() = format!("{at}/d");
        let info = json!({ "daemon_auth": 2, "doc_links": 1, "doc_viewer": format!("{at}/d") });
        let app = Router::new()
            .route("/control.json", get(move || async move { Json(info) }))
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "lex00" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/relay/dial", get(dial))
            .route("/api/doc-links/quota", get(quota))
            .route("/api/doc-links/{id}/v/{ver}/parts/{n}", put(part))
            .route("/api/doc-links/{id}/v/{ver}/commit", post(commit))
            .route("/api/doc-links/{id}", delete(|| async { Json(json!({})) }))
            // What a visitor's browser asks first: the link is served.
            .route("/d/{id}/data/meta", get(|| async { Json(json!({})) }))
            .with_state(f);
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

/// Joined to the fake control, as `join` leaves it.
fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "lex00" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

struct World {
    d: Daemon,
    fake: Fake,
    /// An agent block, which started the Doc.
    agent: u64,
    doc: u64,
    _rt: tokio::runtime::Runtime,
}

impl World {
    /// A daemon with its owner, FRIEND a viewer and EDITOR an editor, the
    /// `docs` flag on, joined to the fake control, and an agent block that
    /// has made a Doc of two sections.
    fn new(env: &[(&str, &str)]) -> World {
        let fake = Fake::default();
        let (rt, url) = serve(fake.clone());
        let args = ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"];
        let d = Daemon::child_in(&args, env, |state| enroll(state, &url));
        arugula_proto::flags::set(&d.state, "docs", true).unwrap();
        let p = &d.get("/api/panes")[0];
        let session = p["session"].as_u64().unwrap();
        d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "viewer" }));
        d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{EDITOR}"), "role": "editor" }));
        let agent = d.open("hello");
        d.wait(agent, "idle");
        let sections = json!([{ "heading": "One", "intent": "first" }, { "heading": "Two", "intent": "second" }]);
        let made =
            agent_mcp(&d, agent, "doc", json!({ "kind": "new", "title": "Q3 plan", "sections": sections })).unwrap();
        let doc = made["block"].as_u64().unwrap();
        agent_mcp(
            &d,
            agent,
            "doc",
            json!({ "kind": "fill", "block": doc, "section": "s1", "markdown": "Secret words." }),
        )
        .unwrap();
        World { d, fake, agent, doc, _rt: rt }
    }

    fn share(&self) -> Result<Value, String> {
        agent_mcp(&self.d, self.agent, "doc", json!({ "kind": "share", "block": self.doc }))
    }

    /// The card on the Doc, if there is one.
    fn card(&self) -> Option<Value> {
        self.d
            .get("/api/panes")
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == self.doc)
            .map(|p| p["ask"].clone())
            .filter(|a| !a.is_null())
    }

    fn wait_card(&self) -> Value {
        let mut card = None;
        self.d.wait_for("the card", || {
            card = self.card();
            card.is_some()
        });
        card.unwrap()
    }

    fn act(&self, action: &str, content: Value) -> Value {
        let card = self.card().expect("a card to answer");
        let body = json!({ "action": action, "pane": self.doc, "id": card["id"], "content": content });
        self.d.post("/api/attention/act", body)
    }

    fn tap_share(&self) -> Value {
        self.act("answer", json!({}))
    }

    fn uploaded(&self) -> (usize, usize) {
        (self.fake.puts.load(Ordering::SeqCst), self.fake.commits.load(Ordering::SeqCst))
    }

    /// Long enough for anything that was going to be uploaded to have been.
    fn settle(&self) {
        std::thread::sleep(Duration::from_millis(600));
    }
}

fn said(v: &Value) -> &str {
    v["status"].as_str().unwrap_or_default()
}

/// The ask, the owner's Share, and the URL for the agent that asked.
#[test]
fn an_agent_asks_and_the_owners_tap_publishes_and_the_repeat_call_gets_the_url() {
    let w = World::new(&[]);
    let r = w.share().unwrap();
    assert_eq!(said(&r), "waiting", "{r}");

    // A card on the Doc itself, naming the title and the agent that asks.
    let card = w.wait_card();
    assert_eq!(card["message"], "Share 'Q3 plan' with a link anyone can open?", "{card}");
    assert_eq!(card["source"], "doc", "{card}");
    let detail = card["schema"]["description"].as_str().unwrap();
    assert!(detail.contains("without an account") && detail.contains("unshare it from the Doc's menu"), "{detail}");
    assert!(w.d.state(w.doc)["shared"].is_null());

    // Asking again while it waits is no second card, and uploads nothing.
    let again = w.share().unwrap();
    assert_eq!(said(&again), "waiting", "{again}");
    assert_eq!(w.card().unwrap()["id"], card["id"]);
    w.settle();
    assert_eq!(w.uploaded(), (0, 0), "nothing is uploaded until the owner taps");

    // The owner taps Share: the upload, as the owner.
    let done = w.tap_share();
    assert_eq!(done["results"][0]["ok"], true, "{done}");
    w.d.wait_for("the Doc shared", || w.d.state(w.doc)["shared"].is_object());
    let (puts, commits) = w.uploaded();
    assert!(puts >= 1 && commits == 1, "{puts} parts, {commits} commits");
    assert!(w.card().is_none(), "the card is gone");

    // The repeat call is the one place the agent is given the link.
    let got = w.share().unwrap();
    assert_eq!(said(&got), "shared", "{got}");
    let url = got["url"].as_str().unwrap();
    assert!(url.contains("/d/") && url.contains("#k="), "{url}");
    assert_eq!(w.share().unwrap()["url"], got["url"], "and again: no new card");
    assert!(w.card().is_none());
    assert_eq!(w.uploaded(), (puts, 1));

    // The key is in the store and in that answer, nowhere else the Doc or
    // the machine keeps: not its state, its folder, its config, or the
    // files beside the store. (The agent's own transcript holds what it was
    // told, as it must.)
    let key = url.split("#k=").nth(1).unwrap();
    assert!(!w.d.state(w.doc).to_string().contains(key), "the key is in the Doc's state");
    assert!(!w.d.get("/api/panes").to_string().contains(key), "the key is in the panes");
    let mut at = vec![w.d.state.join(format!("blocks/{}", w.doc)), w.d.state.clone()];
    let doc_dir = at[0].clone();
    while let Some(dir) = at.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if dir == doc_dir {
                    at.push(path);
                }
            } else if kind.is_file()
                && let Ok(text) = std::fs::read_to_string(&path)
                && path.file_name().is_some_and(|n| n != "doc-links.json")
            {
                assert!(!text.contains(key), "the key is in {}", path.display());
            }
        }
    }
    assert!(std::fs::read_to_string(w.d.state.join("doc-links.json")).unwrap().contains(key));
}

/// Not now (the option, or skipping the card) uploads nothing and is said
/// once; asking again after it raises a new card.
#[test]
fn not_now_uploads_nothing_and_is_said_once() {
    let w = World::new(&[]);

    // Not now (the card's decline).
    assert_eq!(said(&w.share().unwrap()), "waiting");
    w.wait_card();
    w.act("deny", json!({}));
    let r = w.share().unwrap();
    assert_eq!(said(&r), "declined", "{r}");
    assert!(r.get("url").is_none(), "{r}");
    assert!(w.card().is_none());

    // ...said once: the next call asks again (declined by skipping it too).
    assert_eq!(said(&w.share().unwrap()), "waiting");
    w.wait_card();
    let skipped = w.act("deny", json!({}));
    assert_eq!(skipped["results"][0]["ok"], true, "{skipped}");
    assert_eq!(said(&w.share().unwrap()), "declined");

    // ...and asking after a decline raises a new card.
    assert_eq!(said(&w.share().unwrap()), "waiting");
    assert!(w.wait_card()["id"].is_string());
    w.act("deny", json!({}));
    assert_eq!(said(&w.share().unwrap()), "declined");
    w.settle();
    assert_eq!(w.uploaded(), (0, 0), "none of it uploaded anything");
    assert!(w.d.state(w.doc)["shared"].is_null());
}

/// A card nobody answers lapses after its time (a few seconds here, a day
/// really): it goes, it uploads nothing, and the next call says so once.
#[test]
fn a_card_nobody_answers_lapses_and_uploads_nothing() {
    let w = World::new(&[("ARUGULA_DOC_SHARE_TTL_MS", "2500")]);
    assert_eq!(said(&w.share().unwrap()), "waiting");
    w.wait_card();
    w.d.wait_for("the lapse", || w.card().is_none());
    let r = w.share().unwrap();
    assert_eq!(said(&r), "lapsed", "{r}");
    assert!(w.card().is_none());
    w.settle();
    assert_eq!(w.uploaded(), (0, 0), "a lapse uploaded nothing");
    assert!(w.d.state(w.doc)["shared"].is_null());
    // Said once: asking again raises a new card, which lapses in its turn.
    assert_eq!(said(&w.share().unwrap()), "waiting");
    w.wait_card();
    w.d.wait_for("the second lapse", || w.card().is_none());
    assert_eq!(said(&w.share().unwrap()), "lapsed");
    assert_eq!(w.uploaded(), (0, 0));
}

/// The failure's reason is the one line, once.
#[test]
fn a_failed_share_says_why_once() {
    let w = World::new(&[]);
    w.fake.refuse.store(503, Ordering::SeqCst);
    assert_eq!(said(&w.share().unwrap()), "waiting");
    w.wait_card();
    w.tap_share();
    let mut failed = None;
    w.d.wait_for("the failure", || {
        let r = w.share().unwrap();
        if said(&r) == "failed" {
            failed = Some(r);
        }
        failed.is_some()
    });
    let failed = failed.unwrap();
    assert_eq!(failed["why"], "control said 503: try again", "{failed}");
    assert!(!failed.to_string().contains("#k="));
    assert_eq!(w.uploaded(), (0, 0));
}

/// Only the owner answers, by any route: the agent that asked (over MCP,
/// and on the owner's CLI), a viewer and an editor. The card stays, and
/// nothing is uploaded.
#[test]
fn nobody_but_the_owner_answers_the_card_by_any_route() {
    let w = World::new(&[]);
    assert_eq!(said(&w.share().unwrap()), "waiting");
    let card = w.wait_card();
    let id = &card["id"];
    let bearer = w.d.bearer();
    let agent = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    let doc = w.doc;
    let answer = json!({ "id": id, "content": {} });

    // The agent's own MCP, `agent_respond` (it drives its own Doc): to
    // answer the card, or to skip it.
    for action in [json!({ "action": "answer", "answers": {} }), json!({ "action": "skip" })] {
        let mut args = json!({ "pane": doc });
        args.as_object_mut().unwrap().extend(action.as_object().unwrap().clone());
        let e = agent_mcp(&w.d, w.agent, "agent_respond", args).unwrap_err();
        assert!(e.contains(ONLY_OWNER), "{e}");
    }

    // `arugula call %N answer|decline|approve|deny` with the agent header,
    // and the act route with it.
    for method in ["answer", "decline", "approve", "deny", "terminal"] {
        let (status, _, text) =
            w.d.tcp("POST", &format!("/api/blocks/{doc}/call/{method}"), &agent, Some(&answer.to_string()));
        assert_eq!(status, 403, "{method}: {text}");
        assert!(text.contains(ONLY_OWNER), "{method}: {text}");
    }
    let act = json!({ "action": "answer", "pane": doc, "id": id, "content": {} }).to_string();
    let (status, _, text) = w.d.tcp("POST", "/api/attention/act", &agent, Some(&act));
    assert_eq!(status, 403, "{text}");
    assert!(text.contains(ONLY_OWNER), "{text}");

    // A viewer and an editor, by both routes.
    for who in [FRIEND, EDITOR] {
        let (status, text) = w.d.raw_as(who, "POST", "/api/attention/act", Some(serde_json::from_str(&act).unwrap()));
        assert_eq!(status, 403, "{who} act: {text}");
        for method in ["answer", "decline", "approve", "deny"] {
            let (status, text) =
                w.d.raw_as(who, "POST", &format!("/api/blocks/{doc}/call/{method}"), Some(answer.clone()));
            assert_eq!(status, 403, "{who} {method}: {text}");
        }
    }

    // An agent can't take the card away, nor put a card of its own on the
    // Doc (which would push the owner's off): the Doc shows only its own.
    let withdraw = json!({ "id": id }).to_string();
    let (status, _, text) = w.d.tcp("POST", &format!("/api/panes/{doc}/ask/withdraw"), &agent, Some(&withdraw));
    assert_eq!(status, 403, "{text}");
    let own = json!({ "questions": [{ "question": "Share it?", "options": [] }] }).to_string();
    let (status, _, text) = w.d.tcp("POST", &format!("/api/panes/{doc}/ask"), &agent, Some(&own));
    assert_eq!(status, 403, "{text}");

    let permit = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }).to_string();
    let (status, _, text) = w.d.tcp("POST", &format!("/api/panes/{doc}/permit"), &agent, Some(&permit));
    assert_eq!(status, 403, "{text}");

    // None of it changed anything: the same card, nothing uploaded, and the
    // ask still waiting.
    w.settle();
    assert_eq!(w.card().unwrap()["id"], *id);
    assert_eq!(w.uploaded(), (0, 0), "nobody but the owner published");
    assert!(w.d.state(doc)["shared"].is_null());
    assert_eq!(said(&w.share().unwrap()), "waiting");

    // The owner still can.
    let done = w.tap_share();
    assert_eq!(done["results"][0]["ok"], true, "{done}");
    w.d.wait_for("the Doc shared", || w.d.state(doc)["shared"].is_object());
}

/// The card is kept in the Doc's config: a restart raises it again, and the
/// owner's tap then shares.
#[test]
fn a_restart_keeps_the_pending_card() {
    let mut w = World::new(&[]);
    assert_eq!(said(&w.share().unwrap()), "waiting");
    let card = w.wait_card();
    w.d.stop();
    w.d.start();
    let back = w.wait_card();
    assert_eq!(back["message"], card["message"]);
    assert_eq!(back["at_ms"], card["at_ms"], "the same ask");
    w.settle();
    assert_eq!(w.uploaded(), (0, 0));
    w.tap_share();
    w.d.wait_for("the Doc shared", || w.d.state(w.doc)["shared"].is_object());
    assert_eq!(w.uploaded().1, 1);
}

/// The agent drives only a Doc it may; the flag hides and refuses it; and
/// only a Doc is asked about.
#[test]
fn share_follows_drive_the_flag_and_the_kind() {
    let w = World::new(&[]);
    // Another agent in its tab didn't start this Doc.
    let b = agent_mcp(
        &w.d,
        w.agent,
        "start_agent",
        json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "hello" }),
    )
    .unwrap()["block"]
        .as_u64()
        .unwrap();
    w.d.wait(b, "idle");
    let e = agent_mcp(&w.d, b, "doc", json!({ "kind": "share", "block": w.doc })).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    w.settle();
    assert!(w.card().is_none(), "no card was raised");

    // Only a Doc.
    let file = w.d.sessions.join("notes.txt");
    std::fs::write(&file, "x").unwrap();
    let shown =
        agent_mcp(&w.d, w.agent, "show", json!({ "kind": "file", "path": file })).unwrap()["block"].as_u64().unwrap();
    let e = agent_mcp(&w.d, w.agent, "doc", json!({ "kind": "share", "block": shown })).unwrap_err();
    assert!(e.contains("isn't a Doc"), "{e}");

    // The owner's CLI can't raise the card as an agent either: it isn't a
    // block method for anyone but the tool.
    let (status, text) = w.d.raw("POST", &format!("/api/blocks/{}/call/doc_share_ask", w.doc), Some(json!({})));
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("doc tool's share"), "{text}");
    for who in [FRIEND, EDITOR] {
        let (status, text) =
            w.d.raw_as(who, "POST", &format!("/api/blocks/{}/call/doc_share_ask", w.doc), Some(json!({})));
        assert_eq!(status, 403, "{who}: {text}");
    }

    // Off: the kind is refused with how to turn it on (a build without
    // Labs has no flags to turn off).
    #[cfg(feature = "labs")]
    {
        arugula_proto::flags::set(&w.d.state, "docs", false).unwrap();
        let e = agent_mcp(&w.d, w.agent, "doc", json!({ "kind": "share", "block": w.doc }));
        assert!(e.is_err(), "{e:?}");
        assert!(w.card().is_none());
    }
}
