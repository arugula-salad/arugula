//! #418: a machine's Docs copied up to the platform, against the real
//! daemon and a fake control that keeps what it's put. Nothing is put, and
//! nothing more marked, until the account's own word for copies comes
//! (native #673): not with it off, not on chat heard from the owner's
//! teams, and not on chat itself, the account's or a team's; a Doc made
//! before the machine joined is copied once the word is there, and only
//! once; a Doc filled
//! section by section is a put or two with its last content, its picture
//! after it, and where its copy is lands in its `meta.json`; a refused put
//! keeps the Doc dirty, and neither it nor another Doc is tried again
//! before a refresh; and what
//! changed while control was out of reach is put after a restart, once
//! control is back; and a Doc deleted here takes its copy down (native
//! #673).

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicUsize, Ordering},
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
    /// Each copy the daemon took down: the machine's Doc id.
    deletes: Arc<Mutex<Vec<String>>>,
    /// Answer a take-down with this status (0: take it).
    refuse_delete: Arc<AtomicU16>,
    refreshes: Arc<AtomicUsize>,
    /// Answer a Doc's put with this status (0: take it).
    refuse: Arc<AtomicU16>,
    /// Out of reach: the refresh and the puts fail.
    down: Arc<AtomicBool>,
    /// What control sends with the flags (native #673): 0 the account's
    /// word for copies off (a put is refused, as the platform refuses it
    /// with chat off too), 1 the word on, 2 chat on, the account's and a
    /// team's, with no word (the platform would take a put), 3 chat heard
    /// from the owner's teams (the platform would take a put).
    chat: Arc<AtomicU8>,
    /// The account the machine is joined to now ("" for the first), and
    /// the account each Doc's copy was made for: as the platform does, a
    /// put for another account's copy is refused (403).
    account: Arc<Mutex<String>>,
    copied_for: Arc<Mutex<std::collections::HashMap<String, String>>>,
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
    let flags = match f.chat.load(Ordering::SeqCst) {
        1 => json!({ "account": { "doc-copies": true }, "team": {} }),
        2 => json!({ "account": { "chat": true }, "team": { "chat": true } }),
        3 => json!({ "account": {}, "team": { "chat-heard": true } }),
        _ => json!({ "account": { "doc-copies": false }, "team": {} }),
    };
    Json(json!({ "certs": [], "revocations": [], "flags": flags })).into_response()
}

async fn put_doc(State(f): State<Fake>, Path(doc): Path<String>, Json(b): Json<Value>) -> Response {
    if f.down.load(Ordering::SeqCst) {
        return unreachable();
    }
    f.puts.lock().unwrap().push((doc.clone(), b));
    let account = f.account.lock().unwrap().clone();
    let owner = f.copied_for.lock().unwrap().get(&doc).cloned();
    if owner.is_some_and(|o| o != account) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "this Doc now sits in a project where its owner may no longer write" })),
        )
            .into_response();
    }
    let refuse = if f.chat.load(Ordering::SeqCst) == 0 { 403 } else { f.refuse.load(Ordering::SeqCst) };
    if refuse == 0 {
        f.copied_for.lock().unwrap().entry(doc.clone()).or_insert(account);
    }
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

async fn delete_doc(State(f): State<Fake>, Path(doc): Path<String>) -> Response {
    if f.down.load(Ordering::SeqCst) {
        return unreachable();
    }
    f.deletes.lock().unwrap().push(doc.clone());
    match f.refuse_delete.load(Ordering::SeqCst) {
        0 => Json(json!({ "deleted": format!("pdc_{doc}") })).into_response(),
        s => (StatusCode::from_u16(s).unwrap(), Json(json!({ "error": "the owner may no longer write there" })))
            .into_response(),
    }
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
            .route("/api/daemon/docs/{doc}", put(put_doc).delete(delete_doc))
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

    // In control with the word off: not one put, whatever changes, and
    // nothing marked: the first queueing takes every Doc.
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > 0);
    call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": "Written with the word off." }));
    std::thread::sleep(Duration::from_secs(4));
    assert!(f.puts.lock().unwrap().is_empty(), "chat is off: {:?}", f.puts.lock().unwrap());
    // The Doc made before the join waits (a change out of control is kept
    // for the next control); nothing joins it while control says off.
    assert_eq!(dirty(&d), std::slice::from_ref(&old), "only the Doc from before the join waits");
    assert!(!d.state.join("docs/.copied-1").exists());

    // Chat heard from the owner's teams, and then chat itself, the
    // account's and a team's: the platform would take a put each time, and
    // none goes, nothing more is marked, and the library isn't counted as
    // queued. Chat isn't the word.
    for (state, words) in [(3, "Written with chat heard."), (2, "Written with chat on.")] {
        f.chat.store(state, Ordering::SeqCst);
        let refreshed = f.refreshes.load(Ordering::SeqCst);
        d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
        call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": words }));
        std::thread::sleep(Duration::from_secs(4));
        assert!(f.puts.lock().unwrap().is_empty(), "{words}: nothing copied: {:?}", f.puts.lock().unwrap());
        assert_eq!(dirty(&d), std::slice::from_ref(&old), "{words}: nothing more waits to flood out later");
        assert!(!d.state.join("docs/.copied-1").exists());
    }

    // The account's word: the next refresh copies it, once, and notes for
    // whom.
    f.chat.store(1, Ordering::SeqCst);
    d.wait_for("the old Doc's copy", || !f.puts_of(&old).is_empty());
    assert_eq!(bodies(&f.puts_of(&old)[0]), [Some("Written with chat on.".to_owned())]);
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

    // Off again, with chat on: a change isn't put, and waits for the word.
    f.chat.store(2, Ordering::SeqCst);
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    let puts = f.puts_of(&old).len();
    call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": "Written while off." }));
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(f.puts_of(&old).len(), puts, "off: not put");
    assert!(dirty(&d).contains(&old), "it waits: {:?}", dirty(&d));
    f.chat.store(1, Ordering::SeqCst);
    d.wait_for("the change put once the word is back", || {
        f.puts_of(&old).last().is_some_and(|p| bodies(p)[0].as_deref() == Some("Written while off."))
    });

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

    // The one-time copy ran once: the old Doc was put once then, and once
    // for its change made while the word was off.
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("another refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(f.puts_of(&old).len(), 2, "the old Doc was copied once, then once for a change");

    // Out of control, a change is kept for the next control: joined to the
    // same account again (its library queued once already), the change
    // is put.
    d.stop();
    std::fs::remove_file(d.state.join("control.json")).unwrap();
    d.start();
    call(&d, old_block, "doc_fill", json!({ "section": "s1", "markdown": "Changed out of control." }));
    d.wait_for("the change kept while out of control", || dirty(&d).contains(&old));
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the change put once control is back", || {
        f.puts_of(&old).last().is_some_and(|p| bodies(p)[0].as_deref() == Some("Changed out of control."))
    });

    // Deleted here, its copy goes too (native #673), from the tombstone
    // the library wrote, which goes once the copy is down.
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{other}"), None);
    assert_eq!(st, 200, "{v}");
    d.wait_for("the deleted Doc's copy taken down", || f.deletes.lock().unwrap().contains(&other));
    d.wait_for("nothing left to copy", || dirty(&d).is_empty() && !gone(&d).contains(&other));
    assert_eq!(f.deletes.lock().unwrap().len(), 1, "once: {:?}", f.deletes.lock().unwrap());

    // Left control: where each copy was is forgotten (native #673). (The
    // old Doc's was noted again when it was put after the re-join.)
    assert!(meta(&d, &old)["platform"].is_object());
    d.stop();
    std::fs::remove_file(d.state.join("control.json")).unwrap();
    d.start();
    d.wait_for("the copies' places forgotten", || meta(&d, &old).get("platform").is_none());
    assert!(meta(&d, &plan).get("platform").is_none());
}

/// The tombstones' ids, as the library keeps them.
fn gone(d: &Daemon) -> Vec<String> {
    std::fs::read_dir(d.state.join("docs/.gone"))
        .map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// Only a tombstone takes a copy down (native #673): a Doc whose meta.json
/// can't be read is tried again, never deleted; a stray tombstone for a Doc
/// that still reads is dropped at a start, nothing taken down; a Doc
/// deleted while control is out of reach is taken down after a restart,
/// once control is back; told to slow down, the take-down waits and isn't
/// counted; a take-down the platform refuses is tried after the next
/// refreshes, three times, while other Docs keep going, then given up, the
/// copy left; and a machine that re-joined under another account is refused
/// on the old account's copies, as the platform refuses it, gives up the
/// same way, and copies its library again for the new account.
#[test]
fn a_copy_comes_down_from_a_tombstone_alone_and_a_refused_take_down_is_given_up() {
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
    d.stop();
    enroll(&d.state, &url);
    f.chat.store(1, Ordering::SeqCst);
    d.start();
    let (plan_block, plan) = new_doc(&d, "Plan", 1);
    let (_, other) = new_doc(&d, "Other", 1);
    d.wait_for("both copied", || !f.puts_of(&plan).is_empty() && !f.puts_of(&other).is_empty());
    d.wait_for("nothing left to copy", || dirty(&d).is_empty());

    // A meta.json that doesn't parse: the Doc stays dirty, nothing is
    // taken down, and once it reads again the put goes.
    f.down.store(true, Ordering::SeqCst);
    call(&d, plan_block, "doc_fill", json!({ "section": "s1", "markdown": "Offline words." }));
    d.wait_for("the Doc dirty on disk", || dirty(&d).contains(&plan));
    let meta_file = d.state.join("docs").join(&plan).join("meta.json");
    let good = std::fs::read(&meta_file).unwrap();
    std::fs::write(&meta_file, b"{ not json").unwrap();
    let puts = f.puts_of(&plan).len();
    f.down.store(false, Ordering::SeqCst);
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    std::thread::sleep(Duration::from_secs(4));
    assert!(f.deletes.lock().unwrap().is_empty(), "an unreadable Doc is never taken for deleted");
    assert_eq!(f.puts_of(&plan).len(), puts, "nor put");
    assert!(dirty(&d).contains(&plan), "it stays dirty: {:?}", dirty(&d));
    std::fs::write(&meta_file, &good).unwrap();
    d.wait_for("the put once it reads again", || {
        f.puts_of(&plan).last().is_some_and(|p| bodies(p)[0].as_deref() == Some("Offline words."))
    });
    d.wait_for("nothing left to copy", || dirty(&d).is_empty());

    // A stray tombstone for a Doc that still reads (a removal that failed
    // before its files went): dropped at the start, nothing taken down.
    d.stop();
    std::fs::create_dir_all(d.state.join("docs/.gone")).unwrap();
    std::fs::write(d.state.join("docs/.gone").join(&plan), b"0").unwrap();
    d.start();
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > 0);
    d.wait_for("the stray tombstone dropped", || !gone(&d).contains(&plan));
    std::thread::sleep(Duration::from_secs(3));
    assert!(f.deletes.lock().unwrap().is_empty(), "a living Doc keeps its copy");

    // Deleted out of reach, and a restart: the tombstone is on disk, and
    // the copy goes once control is back.
    f.down.store(true, Ordering::SeqCst);
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{plan}"), None);
    assert_eq!(st, 200, "{v}");
    assert!(gone(&d).contains(&plan), "the tombstone: {:?}", gone(&d));
    d.stop();
    d.start();
    f.down.store(false, Ordering::SeqCst);
    d.wait_for("the copy taken down once control is back", || f.deletes.lock().unwrap().contains(&plan));
    d.wait_for("the tombstone gone", || !gone(&d).contains(&plan));

    // Told to slow down: the take-down waits for the next refresh, as
    // often as it takes, uncounted, and the tombstone stays until the
    // copy is down.
    let (_, slow) = new_doc(&d, "Slow", 1);
    d.wait_for("the slow Doc copied", || !f.puts_of(&slow).is_empty());
    d.wait_for("nothing left to copy", || dirty(&d).is_empty());
    f.refuse_delete.store(429, Ordering::SeqCst);
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{slow}"), None);
    assert_eq!(st, 200, "{v}");
    d.wait_for("a second take-down after being told to slow down", || {
        f.deletes.lock().unwrap().iter().filter(|x| **x == slow).count() >= 2
    });
    assert!(gone(&d).contains(&slow), "slow down doesn't count: the tombstone stays");
    f.refuse_delete.store(0, Ordering::SeqCst);
    d.wait_for("the slow Doc's tombstone gone", || !gone(&d).contains(&slow));

    // Copies off: a deleted Doc's copy still goes.
    let (_, off) = new_doc(&d, "Off", 1);
    d.wait_for("the Off Doc copied", || !f.puts_of(&off).is_empty());
    f.chat.store(0, Ordering::SeqCst);
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{off}"), None);
    assert_eq!(st, 200, "{v}");
    d.wait_for("the copy taken down with copies off", || f.deletes.lock().unwrap().contains(&off));
    d.wait_for("its tombstone gone", || !gone(&d).contains(&off));
    f.chat.store(1, Ordering::SeqCst);
    let refreshed = f.refreshes.load(Ordering::SeqCst);
    d.wait_for("a refresh", || f.refreshes.load(Ordering::SeqCst) > refreshed);

    // Refused (the owner may no longer write where the copy sits): tried
    // after the next refreshes, three times, other Docs still going, then
    // given up and the copy left.
    f.refuse_delete.store(403, Ordering::SeqCst);
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{other}"), None);
    assert_eq!(st, 200, "{v}");
    d.wait_for("the first refused take-down", || {
        f.deletes.lock().unwrap().iter().filter(|x| **x == other).count() >= 1
    });
    let (third_block, third) = new_doc(&d, "Third", 1);
    call(&d, third_block, "doc_fill", json!({ "section": "s1", "markdown": "Meanwhile." }));
    d.wait_for("another Doc still copied meanwhile", || {
        f.puts_of(&third).last().is_some_and(|p| bodies(p)[0].as_deref() == Some("Meanwhile."))
    });
    d.wait_for("the take-down given up after three tries", || {
        f.deletes.lock().unwrap().iter().filter(|x| **x == other).count() >= 3 && !gone(&d).contains(&other)
    });
    std::thread::sleep(Duration::from_secs(12));
    assert_eq!(f.deletes.lock().unwrap().iter().filter(|x| **x == other).count(), 3, "no more tries");
    assert!(dirty(&d).is_empty(), "{:?}", dirty(&d));
    f.refuse_delete.store(0, Ordering::SeqCst);

    // Re-joined under another account: the platform refuses the new
    // account on the old one's copies (403), its puts as its take-downs. A
    // Doc deleted meanwhile is given up after three refusals; a Doc copied
    // for the old account is refused for the new one and stays dirty; the
    // library is queued again for the new account.
    let (_, kept) = new_doc(&d, "Kept", 1);
    d.wait_for("the kept Doc copied", || !f.puts_of(&kept).is_empty());
    f.down.store(true, Ordering::SeqCst);
    let (st, v) = d.raw("DELETE", &format!("/api/docs/{third}"), None);
    assert_eq!(st, 200, "{v}");
    d.stop();
    enroll_as(&d.state, &url, "b2");
    *f.account.lock().unwrap() = "b2".into();
    let before = f.puts_of(&kept).len();
    f.refuse_delete.store(403, Ordering::SeqCst);
    d.start();
    f.down.store(false, Ordering::SeqCst);
    d.wait_for("the library queued again", || {
        std::fs::read_to_string(d.state.join("docs/.copied-1")).is_ok_and(|a| a.trim() == "b2")
    });
    d.wait_for("the kept Doc tried for the new account", || f.puts_of(&kept).len() > before);
    // Three refusals are three refreshes, slower after a start that found
    // control down, and a refused put pauses the rest until the next one.
    arugula_testkit::wait_for("the old account's copy given up after three refusals", Duration::from_secs(300), || {
        f.deletes.lock().unwrap().iter().filter(|x| **x == third).count() >= 3 && !gone(&d).contains(&third)
    });
    assert!(gone(&d).is_empty(), "{:?}", gone(&d));
    assert!(dirty(&d).contains(&kept), "the old account's copy isn't the new one's to write: {:?}", dirty(&d));
}

/// [`enroll`], as account `account`.
fn enroll_as(state: &std::path::Path, control: &str, account: &str) {
    use arugula_e2e::{Cert, DeviceKeys, Kind};
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, account, Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": account, "root": keys.id() }, "cert": cert, "login": "lex00" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}
