//! A mention of a machine whose agents can't read the room asks its owner
//! (#578): no grant, a card, the owner's grant, delivered once; or "never",
//! and "not granted" for good. The grant and the "never" are the owner's own
//! acts, over their session: no machine makes either.

use std::time::Duration;

use arugula_control_wire::forge::{AgentGrantAsk, DaemonOp, OpAnswer};
use arugula_e2e::now_ms;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{TestHub, call},
    db::{
        chat_mention_status::{GRANT_PENDING, GRANT_PENDING_MS, MentionStatus},
        projects::THREADS_READ,
    },
};
use arugula_platform::PlatformPort as _;

const ROOM: &str = "/api/chat/rooms/ch_general/messages";
const NEVER: &str = "/api/chat/rooms/ch_general/grant-never";

async fn post(base: &str, text: &str) -> (i64, Value) {
    let (st, v) = call(base, "b", "POST", ROOM, Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    (v["seq"].as_i64().unwrap(), v)
}

/// A call signed by a's machine geek, as the hub's gate vouches for it.
async fn as_geek(base: &str, path: &str, body: Value) -> u16 {
    let r = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header("x-test-daemon", "a geek")
        .json(&body)
        .send()
        .await
        .unwrap();
    r.status().as_u16()
}

fn states(app: &crate::Platform) -> Vec<(i64, String)> {
    app.db.chat_mention_statuses("ch_general", 1, 1_000).unwrap().into_iter().map(|s| (s.seq, s.state)).collect()
}

fn mentions(hub: &TestHub) -> Vec<i64> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentMention(a) if m == "geek" => Some(a.seq),
            _ => None,
        })
        .collect()
}

fn asks(hub: &TestHub) -> Vec<AgentGrantAsk> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentGrantAsk(a) if m == "geek" => Some(a),
            _ => None,
        })
        .collect()
}

/// Until `ok` holds, or the test fails with `what`.
async fn until(what: &str, ok: impl Fn() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never: {what}");
}

/// Then a moment for what shouldn't come.
async fn quiet() {
    tokio::time::sleep(Duration::from_millis(250)).await;
}

async fn grant_read(base: &str, t: &str) {
    let (st, g) =
        call(base, "a", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t }))).await;
    assert_eq!(st, 200, "{g}");
}

/// No grant → the sender is told "not granted" and geek's daemon is asked
/// for a card, over the project a grant would be over; not now changes
/// nothing; then the owner's grant delivers the mention, once: a second
/// grant, the same grant again, and a daemon coming back after a restart
/// find nothing to send.
#[tokio::test]
async fn a_grant_delivers_the_mention_once() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (seq, _) = post(&base, "@geek-box are you there\nsecond line").await;

    // The sender reads "not granted" (why, #572, is the agents' post route's).
    assert_eq!(states(&app), [(seq, GRANT_PENDING.to_owned())]);
    let (_, v) = call(&base, "b", "GET", ROOM, None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).unwrap().clone();
    assert_eq!(m["mention_status"][0]["state"], "not_granted", "{m}");

    // The card: the daemon is asked, with what the owner's Grant is for.
    until("the daemon is asked for a card", || !asks(&hub).is_empty()).await;
    let ask = asks(&hub).remove(0);
    assert_eq!(
        (ask.room.as_str(), ask.seq, ask.author.as_str(), ask.line.as_str(), ask.machine.as_str()),
        ("ch_general", seq, "b", "@geek-box are you there", "geek")
    );
    assert_eq!((ask.scope_kind.as_str(), ask.scope_id.as_str()), ("project", t.as_str()));
    assert_eq!(ask.scope_name, "t", "the card names what the grant reaches");
    assert!(mentions(&hub).is_empty(), "nothing reaches a machine that can't read the room");

    // Not now is the daemon's alone (the card is dismissed): nothing here.
    quiet().await;
    assert_eq!(states(&app), [(seq, GRANT_PENDING.to_owned())]);

    // The owner grants: delivered then, once.
    grant_read(&base, &t).await;
    until("the mention is delivered", || mentions(&hub) == [seq]).await;
    until("its status moves on", || states(&app) == [(seq, "delivered".to_owned())]).await;
    let (_, v) = call(&base, "b", "GET", ROOM, None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).unwrap().clone();
    assert_eq!(m["mention_status"][0]["state"], "delivered", "{m}");

    // A second grant, the same one again, and a restart: not again.
    grant_read(&base, &t).await;
    let (st, g) = call(
        &base,
        "a",
        "POST",
        "/api/grants",
        give("geek", crate::db::projects::THREADS_POST, json!({ "kind": "project", "id": t })),
    )
    .await;
    assert_eq!(st, 200, "{g}");
    crate::chat::release_stale(&app).await;
    app.daemon_came("geek").await.unwrap();
    quiet().await;
    assert_eq!(mentions(&hub), [seq], "delivered once");
    assert_eq!(states(&app), [(seq, "delivered".to_owned())]);

    // Heard from now on: the next mention goes straight through, no card.
    let (next, _) = post(&base, "@geek-box and again").await;
    until("the next is delivered", || mentions(&hub) == [seq, next]).await;
    assert_eq!(asks(&hub).len(), 1);
}

/// Never for this room: what waited is "not granted" for good, a grant later
/// doesn't deliver it, and a new mention there raises no card.
#[tokio::test]
async fn never_leaves_the_mention_not_granted() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (first, _) = post(&base, "@geek-box one").await;
    until("asked", || asks(&hub).len() == 1).await;

    let (st, v) = call(&base, "a", "POST", NEVER, Some(json!({ "daemon": "geek" }))).await;
    assert_eq!((st, &v["refused"]), (200, &json!(1)), "{v}");
    assert_eq!(states(&app), [(first, "not_granted".to_owned())]);

    // A new mention: "not granted", and nobody is asked.
    let (second, _) = post(&base, "@geek-box two").await;
    quiet().await;
    assert_eq!(asks(&hub).len(), 1, "no second card");
    assert_eq!(states(&app), [(first, "not_granted".to_owned()), (second, "not_granted".to_owned())]);

    // Granting from settings undoes it for what comes next, not for what
    // was refused.
    grant_read(&base, &t).await;
    quiet().await;
    assert!(mentions(&hub).is_empty(), "{:?}", mentions(&hub));
    assert_eq!(states(&app), [(first, "not_granted".to_owned()), (second, "not_granted".to_owned())]);
    let (third, _) = post(&base, "@geek-box three").await;
    until("heard now", || mentions(&hub) == [third]).await;
}

/// A mention that waited past a week is dropped by the grant, not delivered.
#[tokio::test]
async fn a_mention_older_than_a_week_is_dropped_not_delivered() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (old, _) = post(&base, "@geek-box old").await;
    let (fresh, _) = post(&base, "@geek-box fresh").await;
    until("asked", || asks(&hub).len() == 2).await;
    let mut s: MentionStatus = app.db.chat_mention_statuses("ch_general", old, old).unwrap().remove(0);
    s.at = now_ms() - GRANT_PENDING_MS - 60_000;
    app.db.set_chat_mention_status(&s).unwrap();

    grant_read(&base, &t).await;
    until("the fresh one is delivered", || mentions(&hub) == [fresh]).await;
    until("settled", || states(&app) == [(old, "not_granted".to_owned()), (fresh, "delivered".to_owned())]).await;
}

/// The grant and the "never" are the owner's own act. A machine's own call
/// (daemon auth), on its own behalf or its owner's, is refused and changes
/// nothing; so is another account's, for a machine that isn't theirs.
#[tokio::test]
async fn only_the_owner_grants_or_says_never() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (seq, _) = post(&base, "@geek-box are you there").await;
    until("asked", || asks(&hub).len() == 1).await;

    // The machine, signed as itself: neither route takes it.
    let give_body = give("geek", THREADS_READ, json!({ "kind": "project", "id": t })).unwrap();
    assert!(matches!(as_geek(&base, "/api/grants", give_body).await, 401 | 403));
    assert!(matches!(as_geek(&base, NEVER, json!({ "daemon": "geek" })).await, 401 | 403));
    // Another account can't answer for a's machine.
    let (st, _) = call(&base, "b", "POST", NEVER, Some(json!({ "daemon": "geek" }))).await;
    assert_eq!(st, 404, "not b's machine");
    let (st, _) =
        call(&base, "b", "POST", "/api/grants", give("geek", THREADS_READ, json!({ "kind": "project", "id": t })))
            .await;
    assert_eq!(st, 404, "not b's machine");

    quiet().await;
    assert_eq!(states(&app), [(seq, GRANT_PENDING.to_owned())], "nothing changed");
    assert!(!app.db.chat_grant_never("ch_general", "geek").unwrap());
    assert!(mentions(&hub).is_empty());

    // The owner's own call does both.
    let (st, _) = call(&base, "a", "POST", NEVER, Some(json!({ "daemon": "geek" }))).await;
    assert_eq!(st, 200);
    assert!(app.db.chat_grant_never("ch_general", "geek").unwrap());
}
