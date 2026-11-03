//! #418: a machine's Docs copied up to the platform, against the real
//! daemon and a fake control that keeps what it's put. Nothing is put while
//! control says chat is off; a Doc made before the machine joined is copied
//! once it's in with chat on, and only once; a Doc filled
//! section by section is a put or two with its last content, its picture
//! after it, and where its copy is lands in its `meta.json`; a refused put
//! keeps the Doc dirty, and neither it nor another Doc is tried again
//! before a refresh; and what
//! changed while control was out of reach is put after a restart, once
//! control is back.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::{Value, json};

use crate::agentd::*;
use crate::doc_share_ask::enroll;

/// A picture put: the Doc, its name and its bytes.
type AssetPut = (String, String, Vec<u8>);

/// A control that keeps what a daemon puts of its Docs.
#[derive(Clone, Default)]
struct Fake {
    /// Each Doc put: the machine's Doc id and the body.
    puts: Arc<Mutex<Vec<(String, Value)>>>,
    assets: Arc<Mutex<Vec<AssetPut>>>,
    refreshes: Arc<AtomicUsize>,
    /// Answer a Doc's put with this status (0: take it).
    refuse: Arc<AtomicU16>,
    /// Out of reach: the refresh and the puts fail.
    down: Arc<AtomicBool>,
    /// Control's chat setting for the machine's account, sent with the
    /// flags: off, a put is refused, as the platform refuses it.
    chat: Arc<AtomicBool>,
}

impl Fake {
    fn puts_of(&self, doc: &str) -> Vec<Value> {
        self.puts.lock().unwrap().iter().filter(|(d, _)| d == doc).map(|(_, b)| b.clone()).collect()
    }
}

fn unreachable() -> Response {
    (StatusCode::BAD_GATEWAY, "down").into_response()
}

async fn trust(State(f): State<Fake>) -> Response {
    if f.down.load(Ordering::SeqCst) {
        return unreachable();
    }
    f.refreshes.fetch_add(1, Ordering::SeqCst);
    let chat = f.chat.load(Ordering::SeqCst);
    Json(json!({ "certs": [], "revocations": [], "flags": { "account": { "chat": chat }, "team": {} } }))
        .into_response()
}

async fn put_doc(State(f): State<Fake>, Path(doc): Path<String>, Json(b): Json<Value>) -> Response {
    if f.down.load(Ordering::SeqCst) {
        return unreachable();
    }
    f.puts.lock().unwrap().push((doc.clone(), b));
    let refuse = if f.chat.load(Ordering::SeqCst) { f.refuse.load(Ordering::SeqCst) } else { 403 };
    match refuse {
        0 => {
            let rev = f.puts_of(&doc).len();
            Json(json!({ "project": "prj_me", "id": format!("pdc_{doc}"),
                         "link": format!("https://control.test/p/prj_me/docs/pdc_{doc}"), "rev": rev }))
            .into_response()
        }
        s => (
            StatusCode::from_u16(s).unwrap(),
            Json(json!({ "error": "native Docs come with chat, which isn't on for you", "code": "chat_off" })),
        )
            .into_response(),
    }
}

async fn put_asset(State(f): State<Fake>, Path((doc, name)): Path<(String, String)>, body: Bytes) -> Response {
    if f.down.load(Ordering::SeqCst) {
        return unreachable();
    }
    let size = body.len();
    f.assets.lock().unwrap().push((doc, name.clone(), body.to_vec()));
    Json(json!({ "name": name, "mime": "image/png", "size": size, "rev": 1 })).into_response()
}

async fn dial(up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        std::future::pending::<()>().await;
        drop(ws);
    })
}

fn serve(f: Fake) -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let url = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        let info = json!({ "daemon_auth": 2 });
        let app = Router::new()
            .route("/control.json", get(move || async move { Json(info) }))
            .route("/api/daemon/trust", get(trust))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "lex00" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/daemon/access", post(|| async { Json(json!({})) }))
            .route("/api/relay/dial", get(dial))
            .route("/api/daemon/docs/{doc}", put(put_doc))
            .route("/api/daemon/docs/{doc}/assets/{name}", put(put_asset))
            .with_state(f);
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    });
    (rt, url)
}

fn call(d: &Daemon, block: u64, method: &str, args: Value) -> Value {
    d.post(&format!("/api/blocks/{block}/call/{method}"), args)
}

/// A Doc block with an outline of `n` sections, and its Doc's id.
fn new_doc(d: &Daemon, title: &str, n: usize) -> (u64, String) {
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let block = d
        .post("/api/blocks", json!({ "type": "doc", "config": { "title": title }, "split": pane, "local": true }))
        ["block"]
        .as_u64()
        .unwrap();
    let sections: Vec<Value> = (1..=n).map(|i| json!({ "heading": format!("Part {i}"), "intent": "" })).collect();
    call(d, block, "doc_outline", json!({ "title": title, "sections": sections }));
    let rows = d.get("/api/docs");
    let id = rows.as_array().unwrap().iter().find(|r| r["open"] == block).unwrap()["id"].as_str().unwrap().to_owned();
    (block, id)
}

fn bodies(put: &Value) -> Vec<Option<String>> {
    put["sections"].as_array().unwrap().iter().map(|s| s["body"].as_str().map(str::to_owned)).collect()
}

fn dirty(d: &Daemon) -> Vec<String> {
    std::fs::read(d.state.join("docs/.copies.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn meta(d: &Daemon, id: &str) -> Value {
    serde_json::from_slice(&std::fs::read(d.state.join("docs").join(id).join("meta.json")).unwrap()).unwrap()
}

#[test]
fn a_machines_docs_are_copied_up_coalesced_kept_dirty_when_refused_and_drained_once_back() {
    let f = Fake::default();
    let (_rt, url) = serve(f.clone());
    let args = [
        "--owner",
        "me@example.com",
        "--tailscale-socket",
        "/nonexistent/sock",
        "--wisp-token-file",
        "/nonexistent",
        "--no-relay",
    ];
    let mut d = Daemon::child_in(&args, &[], |_| {});

    // A Doc from before the machine joined control: nothing goes anywhere.
    let (old_block, old) = new_doc(&d, "Old", 1);
    call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": "From before." }));
    d.stop();
    enroll(&d.state, &url);
    d.start();

    // In control with chat off: not one put, whatever changes.
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > 0);
    call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": "Written with chat off." }));
    std::thread::sleep(Duration::from_secs(4));
    assert!(f.puts.lock().unwrap().is_empty(), "chat is off: {:?}", f.puts.lock().unwrap());
    assert!(!d.state.join("docs/.copied-1").exists());

    // Chat on: the next refresh copies it, once, and notes for whom.
    f.chat.store(true, Ordering::SeqCst);
    d.wait_for("the old Doc's copy", || !f.puts_of(&old).is_empty());
    assert_eq!(bodies(&f.puts_of(&old)[0]), [Some("Written with chat off.".to_owned())]);
    assert_eq!(f.puts_of(&old)[0]["title"], "Old");
    assert_eq!(std::fs::read_to_string(d.state.join("docs/.copied-1")).unwrap(), "a1");

    // Filled section by section, quickly: a put or two, the last with
    // everything.
    let (plan_block, plan) = new_doc(&d, "Plan", 3);
    for n in 1..=3 {
        call(&d, plan_block, "doc_fill", json!({ "section": format!("s{n}"), "markdown": format!("Words of s{n}.") }));
        std::thread::sleep(Duration::from_millis(200));
    }
    let done = vec![Some("Words of s1.".into()), Some("Words of s2.".into()), Some("Words of s3.".into())];
    d.wait_for("the filled Doc's copy", || f.puts_of(&plan).last().is_some_and(|p| bodies(p) == done));
    std::thread::sleep(Duration::from_secs(3));
    let n = f.puts_of(&plan).len();
    assert!((1..=2).contains(&n), "a fill of three sections is a put or two, not {n}");

    // A picture goes after its Doc, and where the copy is lands in meta.json.
    use base64::Engine;
    let png = png();
    call(
        &d,
        plan_block,
        "doc_asset_put",
        json!({ "name": "dot.png", "data": base64::engine::general_purpose::STANDARD.encode(&png) }),
    );
    d.wait_for("the picture's copy", || {
        f.assets.lock().unwrap().iter().any(|(doc, name, _)| *doc == plan && name == "dot.png")
    });
    assert_eq!(f.assets.lock().unwrap().last().unwrap().2, png);
    d.wait_for("where the copy is, in meta.json", || meta(&d, &plan)["platform"]["assets"]["dot.png"].is_u64());
    let on = &meta(&d, &plan)["platform"];
    assert_eq!((on["project"].as_str(), on["id"].as_str()), (Some("prj_me"), Some(format!("pdc_{plan}").as_str())));
    assert!(on["link"].as_str().unwrap().ends_with(&format!("/docs/pdc_{plan}")), "{on}");
    d.wait_for("nothing left to copy", || dirty(&d).is_empty());

    // Refused (the owner may no longer write where it is): the Doc stays
    // dirty, on disk, and neither it nor another Doc is put before the next
    // refresh.
    let (other_block, other) = new_doc(&d, "Other", 1);
    d.wait_for("the other Doc's copy", || !f.puts_of(&other).is_empty());
    f.refuse.store(403, Ordering::SeqCst);
    // Just after a refresh, so the next (10 s on) is after the checks.
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    std::thread::sleep(Duration::from_millis(300));
    let before = f.puts_of(&plan).len();
    call(&d, plan_block, "doc_fill", json!({ "section": "s1", "markdown": "Refused words." }));
    d.wait_for("the refused put", || f.puts_of(&plan).len() > before);
    let (tried, refreshed) = (f.puts_of(&plan).len(), f.refreshes.load(Ordering::SeqCst));
    let others = f.puts_of(&other).len();
    call(&d, other_block, "doc_fill", json!({ "section": "s1", "markdown": "Meanwhile." }));
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(f.refreshes.load(Ordering::SeqCst), refreshed, "no refresh in the window");
    assert_eq!(f.puts_of(&other).len(), others, "a refusal pauses every Doc until a refresh");
    std::thread::sleep(Duration::from_secs(2));
    let (again, refreshes) = (f.puts_of(&plan).len() - tried, f.refreshes.load(Ordering::SeqCst) - refreshed);
    assert!(again <= refreshes, "a refusal waits for a refresh, it doesn't spin: {again} puts, {refreshes} refreshes");
    assert!(dirty(&d).contains(&plan), "the refused Doc is kept dirty: {:?}", dirty(&d));

    // Taken again (chat on): the next refresh drains it.
    f.refuse.store(0, Ordering::SeqCst);
    d.wait_for("the refused Doc's copy after a refresh", || {
        f.puts_of(&plan).last().is_some_and(|p| bodies(p)[0].as_deref() == Some("Refused words."))
            && !dirty(&d).contains(&plan)
    });

    // Out of reach, and a restart: what changed is still dirty, and goes
    // once control is back.
    f.down.store(true, Ordering::SeqCst);
    call(&d, plan_block, "doc_fill", json!({ "section": "s2", "markdown": "Written offline." }));
    d.wait_for("the Doc dirty on disk", || dirty(&d).contains(&plan));
    d.stop();
    d.start();
    f.down.store(false, Ordering::SeqCst);
    d.wait_for("the offline change's copy", || {
        f.puts_of(&plan).last().is_some_and(|p| bodies(p)[1].as_deref() == Some("Written offline."))
    });
    d.wait_for("nothing left to copy", || dirty(&d).is_empty());

    // The one-time copy ran once: the old Doc, unchanged, was put once.
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("another refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(f.puts_of(&old).len(), 1, "the old Doc was copied once");
}
