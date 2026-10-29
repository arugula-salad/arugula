//! A channel's topic and pinned messages (#119): who may set or pin, what
//! the room and its stream say, system lines that are never unread,
//! searched or pushed, a deleted message's pin, and an agent's.

use std::time::Duration;

use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{call, hears, open_live},
    db::projects::{THREADS_POST, THREADS_READ},
    grants::AGENT_UNGRANTED,
};

const ROOM: &str = "/api/chat/rooms/ch_general";

fn pin_of(seq: i64) -> String {
    format!("{ROOM}/messages/{seq}/pin")
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

async fn room(base: &str, who: &str) -> Value {
    let (st, v) = call(base, who, "GET", ROOM, None).await;
    assert_eq!(st, 200, "{v}");
    v["room"].clone()
}

async fn messages(base: &str, who: &str) -> Vec<Value> {
    let (_, v) = call(base, who, "GET", &format!("{ROOM}/messages"), None).await;
    v["messages"].as_array().unwrap().clone()
}

/// A member sets the topic: everyone sees it in the room and the list, a
/// system line says who, nobody has it unread or is pushed, and search
/// doesn't find it. The same again says nothing.
#[tokio::test]
async fn a_member_sets_the_topic_and_the_room_says_so() {
    let (hub, _app, base, _t, _u) = seeded().await;
    let a_live = open_live(&base, "a", None).await;
    hears(&a_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    let put = json!({ "topic": "deploys  at\nnoon @a", "description": "Where we ship." });
    let (st, r) = call(&base, "b", "PUT", &format!("{ROOM}/topic"), Some(put.clone())).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!((&r["topic"], &r["description"]), (&json!("deploys at noon @a"), &json!("Where we ship.")));
    hears(&a_live, "\"type\":\"room\",\"room\":\"ch_general\"", Duration::from_secs(5)).await;
    hears(&a_live, "\"type\":\"message\",\"room\":\"ch_general\"", Duration::from_secs(5)).await;

    let a = room(&base, "a").await;
    assert_eq!(
        (&a["topic"], &a["topic_writable"], &a["unread"]),
        (&json!("deploys at noon @a"), &json!(true), &json!(0))
    );
    let (_, list) = call(&base, "a", "GET", "/api/chat", None).await;
    let g = list["rooms"].as_array().unwrap().iter().find(|r| r["id"] == "ch_general").unwrap().clone();
    assert_eq!((&g["topic"], &g["unread"], &g["mentions"]), (&json!("deploys at noon @a"), &json!(0), &json!(0)));

    let ms = messages(&base, "a").await;
    assert_eq!(ms.len(), 2, "one line for the topic, one for the description: {ms:?}");
    assert_eq!(
        (&ms[0]["system"]["kind"], &ms[0]["author"], &ms[0]["text"]),
        (&json!("topic"), &json!("b"), &json!("deploys at noon @a"))
    );
    assert_eq!((&ms[1]["system"]["kind"], &ms[1]["text"]), (&json!("description"), &json!("")));
    assert!(hub.pushes().is_empty(), "a system line pushes nobody");

    let (_, found) = call(&base, "a", "GET", "/api/chat/search?q=noon", None).await;
    assert_eq!(found["found"], json!([]), "system lines aren't searched");

    // Nothing changed, nothing said; a part left out stays; "" clears.
    call(&base, "b", "PUT", &format!("{ROOM}/topic"), Some(put)).await;
    assert_eq!(messages(&base, "a").await.len(), 2);
    let (_, r) = call(&base, "a", "PUT", &format!("{ROOM}/topic"), Some(json!({ "topic": "" }))).await;
    assert_eq!((&r["topic"], &r["description"]), (&json!(""), &json!("Where we ship.")));
    assert_eq!(messages(&base, "a").await.len(), 3);
}

/// Who may: any member who may post, not a stranger, not a DM; sizes; a
/// team can leave it to its owners.
#[tokio::test]
async fn who_sets_a_topic() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let path = format!("{ROOM}/topic");
    let (st, _) = call(&base, "c", "PUT", &path, Some(json!({ "topic": "mine" }))).await;
    assert_eq!(st, 403, "not in a team with chat");
    let (st, _) = call(&base, "a", "PUT", "/api/chat/rooms/dm_ab/topic", Some(json!({ "topic": "us" }))).await;
    assert_eq!(st, 400, "a channel has one");
    let (st, _) = call(&base, "a", "PUT", &path, Some(json!({ "topic": "x".repeat(251) }))).await;
    assert_eq!(st, 413);
    let (st, _) = call(&base, "a", "PUT", &path, Some(json!({ "description": "x".repeat(1001) }))).await;
    assert_eq!(st, 413);

    // Owners only: b is told so, and isn't offered the field.
    let (st, s) = call(
        &base,
        "a",
        "PUT",
        "/api/chat/teams/t/settings",
        Some(json!({ "broadcast_owners": false, "topic_owners": true })),
    )
    .await;
    assert_eq!((st, &s["topic_owners"]), (200, &json!(true)), "{s}");
    let (st, v) = call(&base, "b", "PUT", &path, Some(json!({ "topic": "mine" }))).await;
    assert_eq!(st, 403, "{v}");
    assert_eq!(room(&base, "b").await["topic_writable"], false);
    assert_eq!(room(&base, "a").await["topic_writable"], true);
    let (st, _) = call(&base, "a", "PUT", &path, Some(json!({ "topic": "ours" }))).await;
    assert_eq!(st, 200);

    // An archived channel is read-only.
    app.db.archive_chat_room("ch_general", Some(1)).unwrap();
    let (st, _) = call(&base, "a", "PUT", &path, Some(json!({ "topic": "late" }))).await;
    assert_eq!(st, 409);
}

/// Two pins seen live by another member, listed newest first with their
/// messages; the header's count; system lines that name the message; pinning
/// twice says nothing; unpinning does.
#[tokio::test]
async fn pins_are_live_listed_and_said() {
    let (_hub, _app, base, _t, _u) = seeded().await;
    let first = say(&base, "a", "the runbook is in the wiki").await;
    let second = say(&base, "b", "freeze starts friday").await;
    let b_live = open_live(&base, "b", None).await;
    hears(&b_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    let (st, m) = call(&base, "a", "PUT", &pin_of(first), None).await;
    assert_eq!((st, &m["pinned"]), (200, &json!(true)), "{m}");
    hears(&b_live, "\"type\":\"pinned\",\"room\":\"ch_general\"", Duration::from_secs(5)).await;
    let (st, _) = call(&base, "b", "PUT", &pin_of(second), None).await;
    assert_eq!(st, 200, "any member who may post pins, the message's author or not");
    call(&base, "b", "PUT", &pin_of(second), None).await;

    assert_eq!(room(&base, "b").await["pins"], 2);
    let (st, p) = call(&base, "b", "GET", &format!("{ROOM}/pins"), None).await;
    assert_eq!(st, 200, "{p}");
    let seqs: Vec<i64> = p["pins"].as_array().unwrap().iter().map(|x| x["message"]["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, [second, first], "the newest pin first");
    assert_eq!((&p["pins"][0]["by"], &p["pins"][0]["by_name"]), (&json!("b"), &json!("b")));
    assert_eq!(p["pins"][1]["message"]["text"], "the runbook is in the wiki");

    let ms = messages(&base, "a").await;
    let lines: Vec<&Value> = ms.iter().filter(|m| m["system"].is_object()).collect();
    assert_eq!(lines.len(), 2, "pinning twice said it once");
    assert_eq!(
        (&lines[0]["system"]["kind"], &lines[0]["system"]["target"], &lines[0]["author"]),
        (&json!("pin"), &json!(first), &json!("a"))
    );
    assert_eq!(room(&base, "a").await["unread"], 1, "b's message is unread; the system lines are not");

    let (st, m) = call(&base, "a", "DELETE", &pin_of(first), None).await;
    assert_eq!((st, &m["pinned"]), (200, &json!(false)), "{m}");
    call(&base, "a", "DELETE", &pin_of(first), None).await;
    assert_eq!(room(&base, "a").await["pins"], 1);
    let ms = messages(&base, "a").await;
    assert_eq!(ms.iter().filter(|m| m["system"]["kind"] == "unpin").count(), 1);
    assert_eq!(ms.iter().find(|m| m["seq"] == second).unwrap()["pinned"], true);
}

/// A stranger doesn't pin or list; what isn't a message to pin isn't
/// pinned; a system line is not edited, deleted or reacted to; an archive is
/// read-only; the pin limit.
#[tokio::test]
async fn what_may_be_pinned() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let seq = say(&base, "a", "hello").await;
    // c is in no team that has chat on: refused, not told it exists.
    assert_eq!(call(&base, "c", "PUT", &pin_of(seq), None).await.0, 403);
    assert_eq!(call(&base, "c", "GET", &format!("{ROOM}/pins"), None).await.0, 403);
    assert_eq!(call(&base, "a", "PUT", &pin_of(99), None).await.0, 404);
    call(&base, "a", "PUT", &pin_of(seq), None).await;

    let line = messages(&base, "a").await.into_iter().find(|m| m["system"].is_object()).unwrap();
    let ls = line["seq"].as_i64().unwrap();
    assert_eq!(call(&base, "a", "PUT", &pin_of(ls), None).await.0, 409, "a note isn't pinned");
    let path = format!("{ROOM}/messages/{ls}");
    assert_eq!(call(&base, "a", "PATCH", &path, Some(json!({ "text": "x" }))).await.0, 403);
    assert_eq!(call(&base, "a", "DELETE", &path, None).await.0, 403);
    assert_eq!(call(&base, "b", "POST", &format!("{path}/reactions"), Some(json!({ "emoji": "👍" }))).await.0, 409);

    for i in 0..49 {
        let s = say(&base, "a", &format!("m{i}")).await;
        assert_eq!(call(&base, "a", "PUT", &pin_of(s), None).await.0, 200);
    }
    let one_more = say(&base, "a", "one more").await;
    assert_eq!(call(&base, "a", "PUT", &pin_of(one_more), None).await.0, 409, "at most 50");
    assert_eq!(call(&base, "a", "PUT", &pin_of(seq), None).await.0, 200, "what is pinned still is");

    app.db.archive_chat_room("ch_general", Some(1)).unwrap();
    assert_eq!(call(&base, "b", "DELETE", &pin_of(seq), None).await.0, 409);
}

/// A deleted message loses its pin, and open pages hear of it.
#[tokio::test]
async fn deleting_a_message_unpins_it() {
    let (_hub, app, base, _t, _u) = seeded().await;
    let seq = say(&base, "b", "oops").await;
    call(&base, "a", "PUT", &pin_of(seq), None).await;
    let a_live = open_live(&base, "a", None).await;
    hears(&a_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;
    let (st, _) = call(&base, "b", "DELETE", &format!("{ROOM}/messages/{seq}"), None).await;
    assert_eq!(st, 200);
    hears(&a_live, "\"type\":\"pinned\",\"room\":\"ch_general\"", Duration::from_secs(5)).await;
    assert!(app.db.chat_pins("ch_general").unwrap().is_empty(), "gone from the table, not hidden");
    assert_eq!(room(&base, "a").await["pins"], 0);
    let (_, p) = call(&base, "a", "GET", &format!("{ROOM}/pins"), None).await;
    assert_eq!(p["pins"], json!([]));
}

/// An agent reads the topic and pins, and pins with `threads.post` as its
/// owner, labelled with the machine.
#[tokio::test]
async fn an_agent_reads_the_topic_and_pins() {
    let (_hub, _app, base, t, _u) = seeded().await;
    let seq = say(&base, "b", "read me").await;
    call(&base, "b", "PUT", &format!("{ROOM}/topic"), Some(json!({ "topic": "ship friday" }))).await;
    call(&base, "b", "PUT", &pin_of(seq), None).await;
    let scope = json!({ "kind": "project", "id": t });
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_READ, scope.clone())).await;

    let (st, v) = geek(&base, "GET", "/api/daemon/chat/rooms/ch_general/messages", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((&v["room"]["topic"], &v["pins"][0]["message"]["seq"]), (&json!("ship friday"), &json!(seq)));
    assert!(v["messages"].as_array().unwrap().iter().any(|m| m["system"]["kind"] == "topic"));

    let path = wire_pin(seq);
    let (st, v) = geek(&base, "DELETE", &path, None).await;
    assert_eq!((st, v["reason"].as_str()), (403, Some(AGENT_UNGRANTED)), "reading isn't pinning: {v}");
    call(&base, "a", "POST", "/api/grants", give("geek", THREADS_POST, scope)).await;
    let (st, m) = geek(&base, "DELETE", &path, None).await;
    assert_eq!((st, &m["pinned"]), (200, &json!(false)), "{m}");
    let (st, m) = geek(&base, "PUT", &path, None).await;
    assert_eq!((st, &m["pinned"]), (200, &json!(true)), "{m}");
    let line = messages(&base, "b").await.into_iter().rev().find(|m| m["system"]["kind"] == "pin").unwrap();
    assert_eq!(
        (&line["author"], &line["agent"]["machine"], &line["agent"]["machine_name"]),
        (&json!("a"), &json!("geek"), &json!("geek-box"))
    );
}

fn wire_pin(seq: i64) -> String {
    arugula_control_wire::chat::agent_pin("ch_general", seq)
}
