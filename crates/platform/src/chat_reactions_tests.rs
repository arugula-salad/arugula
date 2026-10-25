//! Emoji reactions (#104): who may react and who sees it, adding and
//! taking back twice, an agent's apart from its owner's, the stream, and a
//! deleted message's.

use std::time::Duration;

use arugula_control_wire::chat::agent_reactions;
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{call, hears, open_live},
    db::projects::{THREADS_POST, THREADS_READ},
    grants::AGENT_UNGRANTED,
};

const ROOM: &str = "/api/chat/rooms/ch_general";

fn on(seq: i64) -> String {
    format!("{ROOM}/messages/{seq}/reactions")
}

fn react(emoji: &str) -> Option<Value> {
    Some(json!({ "emoji": emoji }))
}

/// `DELETE`'s emoji goes in the query, percent-encoded.
fn off(seq: i64, emoji: &str) -> String {
    let enc: String = emoji.bytes().map(|b| format!("%{b:02X}")).collect();
    format!("{}?emoji={enc}", on(seq))
}

async fn geek(base: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .request(method.parse().unwrap(), format!("{base}{path}"))
        .header("x-test-daemon", "a geek");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

async fn say(base: &str, who: &str, text: &str) -> i64 {
    let (st, v) = call(base, who, "POST", &format!("{ROOM}/messages"), Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// The reactions on message `seq` as `who` reads the room.
async fn seen(base: &str, who: &str, seq: i64) -> Value {
    let (st, v) = call(base, who, "GET", &format!("{ROOM}/messages"), None).await;
    assert_eq!(st, 200, "{v}");
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).unwrap().clone();
    m.get("reactions").cloned().unwrap_or(json!([]))
}

/// Two people react to one message and each sees both, their own marked;
/// doing it twice or taking back what isn't there is no error; a room's
/// stream is told, and nobody outside the room reacts or hears.
#[tokio::test]
async fn people_react_and_see_each_other() {
    let (hub, _app, base, _t, _u) = seeded().await;
    let seq = say(&base, "a", "ship it").await;
    let b_live = open_live(&base, "b", None).await;
    hears(&b_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    let (st, m) = call(&base, "a", "POST", &on(seq), react("👍")).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["reactions"][0]["emoji"], "👍");
    assert_eq!((m["reactions"][0]["count"].clone(), m["reactions"][0]["mine"].clone()), (json!(1), json!(true)));
    hears(&b_live, &format!("\"type\":\"reacted\",\"room\":\"ch_general\",\"seq\":{seq}"), Duration::from_secs(5))
        .await;

    // The same again changes nothing (and says nothing).
    let (st, again) = call(&base, "a", "POST", &on(seq), react("👍")).await;
    assert_eq!((st, &again["reactions"]), (200, &m["reactions"]));
    let (st, _) = call(&base, "b", "POST", &on(seq), react("👍")).await;
    assert_eq!(st, 200);
    call(&base, "b", "POST", &on(seq), react("🎉")).await;

    let for_a = seen(&base, "a", seq).await;
    let for_b = seen(&base, "b", seq).await;
    assert_eq!(for_a[0]["count"], 2, "{for_a}");
    assert_eq!(
        for_a[0]["who"].as_array().unwrap().iter().map(|w| w["account"].as_str().unwrap()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!((&for_a[0]["mine"], &for_a[1]["mine"]), (&json!(true), &json!(false)), "{for_a}");
    assert_eq!((&for_b[0]["mine"], &for_b[1]["mine"]), (&json!(true), &json!(true)), "{for_b}");
    assert_eq!((&for_a[1]["emoji"], &for_a[1]["count"]), (&json!("🎉"), &json!(1)), "in the order first made");

    // Taking back is for your own, and twice is fine.
    let (st, m) = call(&base, "a", "DELETE", &off(seq, "👍"), None).await;
    assert_eq!(
        (st, m["reactions"][0]["count"].clone(), m["reactions"][0]["mine"].clone()),
        (200, json!(1), json!(false))
    );
    let (st, _) = call(&base, "a", "DELETE", &off(seq, "👍"), None).await;
    assert_eq!(st, 200);
    let (_, m) = call(&base, "b", "DELETE", &off(seq, "🎉"), None).await;
    assert_eq!(m["reactions"].as_array().map(Vec::len), Some(1), "only the 👍 of b's is left: {m}");

    // Emoji, not words; a message that isn't there; a room you aren't in.
    for bad in ["", "hi", "👍 👍", "a👍"] {
        let (st, v) = call(&base, "a", "POST", &on(seq), react(bad)).await;
        assert_eq!(st, 400, "{bad:?}: {v}");
    }
    let (st, _) = call(&base, "a", "POST", &on(99), react("👍")).await;
    assert_eq!(st, 404);
    let (st, _) = call(&base, "a", "POST", "/api/chat/rooms/dm_bx/messages/1/reactions", react("👍")).await;
    assert_eq!(st, 404, "a isn't in b and x's DM");
    let (st, _) = call(&base, "c", "POST", &on(seq), react("👍")).await;
    assert_ne!(st, 200, "c isn't in the team");
    assert_eq!(seen(&base, "b", seq).await.as_array().map(Vec::len), Some(1));
    assert!(hub.pushes().is_empty(), "reactions push nobody: {:?}", hub.pushes());
}

/// A message holds only so many kinds of emoji.
#[tokio::test]
async fn a_message_holds_twenty_kinds() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let seq = say(&base, "a", "party").await;
    let all = "😀😃😄😁😆😅🤣😂🙂🙃😉😊😇🥰😍🤩😘😗😚😙";
    assert_eq!(all.chars().count(), 20);
    for e in all.chars() {
        let (st, v) = call(&base, "a", "POST", &on(seq), react(&e.to_string())).await;
        assert_eq!(st, 200, "{e}: {v}");
    }
    let (st, _) = call(&base, "a", "POST", &on(seq), react("🥲")).await;
    assert_eq!(st, 409);
    let (st, _) = call(&base, "b", "POST", &on(seq), react("😀")).await;
    assert_eq!(st, 200, "joining one that's there is not a new kind");
}

/// An agent reacts as "<owner>'s agent on <machine>" with `threads.post`
/// (not with only `threads.read`), apart from its owner's own; and sees
/// its own as its own.
#[tokio::test]
async fn an_agent_reacts_with_threads_post_apart_from_its_owner() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let seq = say(&base, "b", "can you look?").await;
    let path = agent_reactions("ch_general", seq);

    let (st, v) = geek(&base, "POST", &path, react("👀")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "{v}");
    let scope = json!({ "kind": "project", "id": t });
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;
    let (st, v) = geek(&base, "POST", &path, react("👀")).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "reading isn't reacting: {v}");
    let (st, g) = call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, scope)).await;
    assert_eq!(st, 200, "{g}");

    let (st, m) = geek(&base, "POST", &path, react("👀")).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["reactions"][0]["mine"], true, "its own, to it");
    call(&base, "a", "POST", &on(seq), react("👀")).await;

    let for_b = seen(&base, "b", seq).await;
    assert_eq!(for_b[0]["count"], 2, "the owner's and the agent's are two: {for_b}");
    let who = for_b[0]["who"].as_array().unwrap();
    assert_eq!(
        (&who[0]["agent"], &who[0]["account"]),
        (&json!({ "machine": "geek", "machine_name": "geek-box" }), &json!("a"))
    );
    assert_eq!(who[1]["agent"], Value::Null);
    let for_a = seen(&base, "a", seq).await;
    assert_eq!(for_a[0]["mine"], true);
    // The agent reads them, its own marked, not the owner's.
    let (_, v) = geek(&base, "GET", "/api/daemon/chat/rooms/ch_general/messages", None).await;
    assert_eq!(v["messages"][0]["reactions"][0]["mine"], true, "{v}");

    // Taking back the agent's leaves the owner's.
    let (st, m) = geek(&base, "DELETE", &format!("{path}?emoji=%F0%9F%91%80"), None).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!((&m["reactions"][0]["count"], &m["reactions"][0]["mine"]), (&json!(1), &json!(false)));
    assert_eq!(seen(&base, "a", seq).await[0]["mine"], true);
}

/// Deleting a message drops its reactions; none can be added after.
#[tokio::test]
async fn deleting_a_message_drops_its_reactions() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let seq = say(&base, "b", "oops").await;
    call(&base, "a", "POST", &on(seq), react("😂")).await;
    let (st, _) = call(&base, "b", "DELETE", &format!("{ROOM}/messages/{seq}"), None).await;
    assert_eq!(st, 200);
    assert_eq!(seen(&base, "a", seq).await, json!([]));
    assert!(app.db.chat_reactions("ch_general", seq, seq).unwrap().is_empty(), "gone from the table, not hidden");
    let (st, _) = call(&base, "a", "POST", &on(seq), react("😂")).await;
    assert_eq!(st, 409);
    // Nothing to take back is still fine.
    let (st, _) = call(&base, "a", "DELETE", &off(seq, "😂"), None).await;
    assert_eq!(st, 200);
}

/// An archived channel takes no more reactions.
#[tokio::test]
async fn an_archived_channel_is_read_only() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let seq = say(&base, "a", "old news").await;
    app.db.archive_chat_room("ch_general", Some(1)).unwrap();
    let (st, v) = call(&base, "b", "POST", &on(seq), react("👍")).await;
    assert_eq!(st, 409, "{v}");
}
