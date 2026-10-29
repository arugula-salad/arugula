//! Chat through the platform's own routes, with a hub of the test's own
//! ([`TestHub`]): what it asks the hub for a page of rooms, how a roster
//! change reaches it through the change feed, and how it pushes. Control's
//! `chat_wire` runs chat end to end through the hub's gate and in-process
//! port.
//!
//! The test hub and its helpers are the base's `testing::hub` (#314); this
//! re-exports them under their old path (`chat_tests::TestHub`) for the
//! tests that name it so.

// Only re-exports outside its own tests (the `testing` feature).
#![cfg_attr(not(test), allow(unused_imports))]

use std::{sync::Arc, time::Duration};

use arugula_e2e::team::TeamRole;
use serde_json::json;

pub use crate::testing::hub::*;
#[cfg(test)]
use crate::{Platform, hub::PushOptions};

/// Listing rooms asks the hub for a page of rooms at once: one
/// `may_reach` for every pane thread on others' machines, and nothing
/// at all again while nothing changed.
#[tokio::test]
async fn a_page_of_rooms_asks_the_hub_once_each() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    let (st, v) = call(&base, "b", "GET", "/api/chat", None).await;
    assert_eq!(st, 200, "{v}");
    let rooms = v["rooms"].as_array().unwrap();
    assert_eq!(rooms.len(), 1 + 5 * 3, "#general and the threads b reads");
    assert_eq!(rooms.iter().filter(|r| r["kind"] == "pane").count(), 15);
    assert_eq!(rooms[1]["machine_name"], "m0-box");
    for m in ["may_reach", "teams", "daemons", "machines_for"] {
        assert_eq!(hub.calls_of(m), 1, "{m}: one call for 30 threads on 5 machines");
    }
    let before = hub.calls();
    assert_eq!(call(&base, "b", "GET", "/api/chat", None).await.0, 200);
    assert_eq!(hub.calls(), before, "kept: nothing changed");
}

/// A roster change reaches who sees what once the feed is read, and not
/// before (the delay the docs give, `cache::DELAY`).
#[tokio::test]
async fn a_roster_change_reaches_chat_through_the_feed() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    // Chat stays on for b out of t.
    hub.flag("account:b", true);
    crate::cache::catch_up(&app).await.unwrap();
    let general = "/api/chat/rooms/ch_general";
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200);
    hub.team("t", &[("a", TeamRole::Owner)]);
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200, "until the feed is read");
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 404, "b left t");
    // And back.
    hub.team("t", &[("a", TeamRole::Owner), ("b", TeamRole::Viewer)]);
    crate::cache::catch_up(&app).await.unwrap();
    assert_eq!(call(&base, "b", "GET", general, None).await.0, 200);
}

/// A mention goes through the hub's push, to every subscription the
/// account has, with the hub's TTL: what chat's pushes always did.
#[tokio::test]
async fn a_mention_pushes_through_the_hub_as_chat_always_did() {
    let hub = Arc::new(TestHub::default());
    let (app, base) = serve(hub.clone()).await;
    seed(&app, &hub);
    let (st, v) =
        call(&base, "a", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "@b look" }))).await;
    assert_eq!((st, &v["pushed"]), (200, &json!([{ "account": "b", "why": "mention" }])), "{v}");
    let mut pushes = hub.pushes();
    for _ in 0..50 {
        if !pushes.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        pushes = hub.pushes();
    }
    let (to, msg, options) = pushes.first().cloned().expect("pushed");
    assert_eq!((to.as_str(), options), ("b", PushOptions { ttl: None, any_subscription: true }));
    assert_eq!(msg["title"], "a mentioned you in #ch_general");
    assert_eq!(
        (&msg["tag"], &msg["control"], &msg["chat"]["seq"]),
        (&json!("chat-ch_general"), &json!(true), &json!(1))
    );
}

/// Two machines on one Postgres (M144): a message posted through one
/// reaches a live stream held by the other, woken by `pg_notify` (well
/// within the catch-up that would find it anyway), and a page that dropped
/// from one resumes on the other from its `Last-Event-ID`.
#[tokio::test]
async fn a_message_posted_on_one_machine_reaches_a_stream_on_the_other() {
    let db = crate::db::Db::test();
    let Some(db_a) = db.second() else { return eprintln!("skipped: two machines need Postgres") };
    let db_b = db.second().unwrap();
    let hub = Arc::new(TestHub::default());
    let (a, base_a) = serve_on(hub.clone(), db_a).await;
    let (b, base_b) = serve_on(hub.clone(), db_b).await;
    crate::chat::start(&a);
    crate::chat::start(&b);
    seed(&a, &hub);

    let on_b = open_live(&base_b, "b", None).await;
    hears(&on_b, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let (st, v) =
        call(&base_a, "a", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "hi" }))).await;
    assert_eq!(st, 200, "{v}");
    let took = hears(&on_b, "\"type\":\"message\",\"room\":\"ch_general\",\"seq\":1", Duration::from_secs(4)).await;
    assert!(took < crate::chat::CATCH_UP, "woken by the notification, not the catch-up ({took:?})");

    // The page drops after that event, a second message is posted through
    // b, and the page comes back through a.
    let text = on_b.lock().unwrap().clone();
    let id = text.lines().filter_map(|l| l.strip_prefix("id: ")).next_back().expect("an event id").to_owned();
    assert!(id.starts_with("c-"), "{id}");
    let (st, _) =
        call(&base_b, "b", "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": "and" }))).await;
    assert_eq!(st, 200);
    let on_a = open_live(&base_a, "b", Some(&id)).await;
    hears(&on_a, "\"type\":\"message\",\"room\":\"ch_general\",\"seq\":2", Duration::from_secs(4)).await;
    assert!(!on_a.lock().unwrap().contains("resync"), "resumed, not resynced: {}", on_a.lock().unwrap());
    assert!(!on_a.lock().unwrap().contains("\"seq\":1}"), "not sent again");
    // An id from before M144 (one process's numbering) resyncs.
    let old = open_live(&base_a, "b", Some("19a2b3c4d5e-7")).await;
    hears(&old, "\"type\":\"resync\"", Duration::from_secs(4)).await;
}

/// Typing (#111) crosses machines as a short-lived `chat_events` row: a
/// ping through one reaches the other's stream that has the room open.
#[tokio::test]
async fn a_typing_ping_on_one_machine_reaches_a_stream_on_the_other() {
    let db = crate::db::Db::test();
    let Some(db_a) = db.second() else { return eprintln!("skipped: two machines need Postgres") };
    let db_b = db.second().unwrap();
    let hub = Arc::new(TestHub::default());
    let (a, base_a) = serve_on(hub.clone(), db_a).await;
    let (b, base_b) = serve_on(hub.clone(), db_b).await;
    crate::chat::start(&a);
    crate::chat::start(&b);
    seed(&a, &hub);

    let on_b = open_live(&base_b, "b", None).await;
    hears(&on_b, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let stream = {
        let text = on_b.lock().unwrap().clone();
        let at = text.find("\"stream\":\"").unwrap() + 10;
        text[at..].split('"').next().unwrap().to_owned()
    };
    let (st, _) =
        call(&base_b, "b", "POST", "/api/chat/live/open", Some(json!({ "stream": stream, "room": "ch_general" })))
            .await;
    assert_eq!(st, 200);
    let (st, _) = call(&base_a, "a", "POST", "/api/chat/rooms/ch_general/typing", None).await;
    assert_eq!(st, 204);
    hears(&on_b, "\"type\":\"typing\"", Duration::from_secs(4)).await;
}

/// One of two machines leads (M144): it alone applies the hub's feed to
/// projects and grants, and when its connection closes the other takes
/// over within a try or two.
#[tokio::test]
async fn one_machine_leads_and_the_other_takes_over() {
    let db = crate::db::Db::test();
    let Some(db_a) = db.second() else { return eprintln!("skipped: two machines need Postgres") };
    let hub = Arc::new(TestHub::default());
    let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
    let a = Platform::new(cfg.clone(), db_a, hub.clone(), reqwest::Client::new(), Default::default());
    let b = Platform::new(cfg, db.second().unwrap(), hub.clone(), reqwest::Client::new(), Default::default());
    assert!(a.is_leader() && !b.is_leader(), "the first to start leads");
    assert!(!crate::projects::follow(&b).await.unwrap(), "only the leader follows");
    assert!(crate::projects::follow(&a).await.unwrap());
    drop(a);
    let started = std::time::Instant::now();
    while !b.is_leader() {
        assert!(started.elapsed() < crate::db::pg::LEADER_TRY * 3, "b never took over");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(crate::projects::follow(&b).await.is_ok());
}
