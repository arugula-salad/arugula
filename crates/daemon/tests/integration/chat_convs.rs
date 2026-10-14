//! #855: conversations, threads of their own beside the pane and session
//! threads, kept on their home machine.
//!
//! Their people are the record's, not a session's: a watcher on a session
//! posts in a conversation they're a member of, reading from when they were
//! added (or all of it, with history). Only the owner makes, renames and
//! deletes one and adds and takes out members, and only someone who can
//! reach the machine can be added; a member may leave. Each member's state
//! lists the conversation with its unread, a mention pushes them a
//! notification that opens it (no pane), and so does being added, with what
//! being a member means. It all outlives a restart, and search finds it.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::{Duration, Instant};

use crate::agentd::Phone;
use arugula_testkit::arugulad;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const OWNER: &str = "me@example.com";
const WATCHER: &str = "watcher@example.com";
const DRIVER: &str = "driver@example.com";
const STRANGER: &str = "stranger@example.com";

fn daemon(tag: &str) -> arugula_testkit::Daemon {
    arugulad!(tag).labs().no_wisp().args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]).start()
}

fn guest(d: &arugula_testkit::Daemon, who: &str, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let body = body.map(|b| b.to_string());
    let (status, _, text) =
        d.tcp(method, path, &[("tailscale-user-login", who), ("content-type", "application/json")], body.as_deref());
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn owner(d: &arugula_testkit::Daemon, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    guest(d, OWNER, method, path, body)
}

fn texts(v: &Value) -> Vec<String> {
    v["messages"].as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap().to_owned()).collect()
}

/// What a person's client is sent when it connects: its `threads` and
/// `convs`.
async fn seen(d: &arugula_testkit::Daemon, who: &str) -> (Value, Value) {
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(format!(
        "ws://127.0.0.1:{}/ws",
        d.port
    ))
    .unwrap();
    req.headers_mut().insert("tailscale-user-login", who.parse().unwrap());
    let (mut ws, _) = connect_async(req).await.unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await else {
            break;
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "hello" {
            return (v["state"]["threads"].clone(), v["state"]["convs"].clone());
        }
    }
    panic!("no hello for {who}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_conversation_has_its_own_members_floors_and_rules() {
    let mut d = daemon("convs");
    let panes = d.get("/api/panes");
    let (pane, session) = (panes[0]["id"].as_u64().unwrap(), panes[0]["session"].as_u64().unwrap());
    // Both reach this machine: one watches the session, one drives it.
    for (who, role) in [(WATCHER, "viewer"), (DRIVER, "editor")] {
        d.post(
            "/api/acl",
            json!({ "session": session, "principal": format!("tailnet:{who}"), "role": role, "history": true }),
        );
    }
    let watcher_phone = Phone::bind();
    let (s, v) = guest(&d, WATCHER, "POST", "/api/push/subscribe", Some(watcher_phone.subscription()));
    assert_eq!(s, 200, "{v}");

    // Only the machine's owner makes one.
    let (s, v) = guest(&d, DRIVER, "POST", "/api/convs", Some(json!({ "title": "mine" })));
    assert_eq!(s, 403, "{v}");
    let (s, v) = owner(&d, "POST", "/api/convs", Some(json!({ "title": " " })));
    assert_eq!(s, 400, "{v}");
    let c = d.post("/api/convs", json!({ "title": "calc tests" }));
    let id = c["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("c-") && id.len() == 34, "{c}");
    assert_eq!((c["owner"].as_str(), c["members"].as_array().map(Vec::len)), (Some("owner"), Some(0)));
    let key = format!("conv-{id}");
    let thread = format!("/api/threads/{key}");

    // Before they're in it, nobody else reads or posts there.
    d.post(&thread, json!({ "text": "before anyone" }));
    let (s, _) = guest(&d, WATCHER, "GET", &thread, None);
    assert_eq!(s, 404);
    let (s, _) = guest(&d, DRIVER, "POST", &thread, Some(json!({ "text": "hi" })));
    assert_eq!(s, 404);

    // Adding: the owner's, someone who can reach the machine, named as an
    // `@` names them or by id; nobody twice.
    let members = format!("/api/convs/{id}/members");
    let (s, v) = guest(&d, DRIVER, "POST", &members, Some(json!({ "who": "watcher" })));
    assert_eq!(s, 403, "{v}");
    let (s, v) = owner(&d, "POST", &members, Some(json!({ "who": STRANGER })));
    assert_eq!(s, 404, "{v}");
    assert!(v["error"].as_str().unwrap().contains("can reach this machine"), "{v}");
    let c = d.post(&members, json!({ "who": "@watcher" }));
    assert_eq!(c["members"][0]["who"], format!("tailnet:{WATCHER}"));
    assert!(c["members"][0].get("history").is_none(), "from now on: {c}");
    let c = d.post(&members, json!({ "who": format!("tailnet:{DRIVER}"), "history": true }));
    assert_eq!(c["members"][1]["history"], true);
    let (s, _) = owner(&d, "POST", &members, Some(json!({ "who": "watcher" })));
    assert_eq!(s, 409);

    // Being added tells them, opening the conversation, with what being a
    // member means.
    let push = watcher_phone.next();
    assert_eq!((push["thread"].as_str(), push["pane"].as_u64()), (Some(key.as_str()), Some(0)), "{push}");
    assert_eq!(push["body"], arugula_proto::CONV_MEMBERS_SEE);
    assert!(push["title"].as_str().unwrap().ends_with("added you to calc tests"), "{push}");

    // A watcher on the session is an editor here, from when they joined; the
    // one added with history reads it all.
    let (s, v) = guest(&d, WATCHER, "POST", &thread, Some(json!({ "text": "watching, and talking" })));
    assert_eq!(s, 200, "{v}");
    let (_, v) = guest(&d, WATCHER, "GET", &thread, None);
    assert_eq!(texts(&v), ["watching, and talking"]);
    let (_, v) = guest(&d, DRIVER, "GET", &thread, None);
    assert_eq!(texts(&v), ["before anyone", "watching, and talking"]);
    // The session's own threads are as they were: the watcher reads the
    // pane's, and still can't post there.
    let (s, _) = guest(&d, WATCHER, "POST", &format!("/api/threads/pane-{pane}"), Some(json!({ "text": "no" })));
    assert_eq!(s, 403);

    // A mention of a member notifies them, opening the conversation.
    let r = d.post(&thread, json!({ "text": "@watcher can you look?" }));
    assert_eq!(r["message"]["mentions"], json!([format!("tailnet:{WATCHER}")]));
    let push = watcher_phone.next();
    assert_eq!((push["thread"].as_str(), push["pane"].as_u64()), (Some(key.as_str()), Some(0)), "{push}");
    assert_eq!(push["tag"], format!("thread-{key}"));

    // Each member's state lists it, with their unread.
    let (threads, convs) = seen(&d, WATCHER).await;
    assert_eq!(convs.as_array().unwrap().iter().map(|c| c["id"].clone()).collect::<Vec<_>>(), [json!(id)]);
    let t = threads.as_array().unwrap().iter().find(|t| t["target"]["conv"] == id.as_str()).cloned().unwrap();
    assert_eq!((t["unread"].as_u64(), t["mention"].as_bool()), (Some(1), Some(true)), "{threads}");

    // Renaming is the owner's.
    let (s, _) = guest(&d, DRIVER, "POST", &format!("/api/convs/{id}/rename"), Some(json!({ "title": "ours" })));
    assert_eq!(s, 403);
    let c = d.post(&format!("/api/convs/{id}/rename"), json!({ "title": "calc, again" }));
    assert_eq!(c["title"], "calc, again");

    // A member leaves; another can't take someone out.
    let remove = format!("/api/convs/{id}/members/remove");
    let (s, _) = guest(&d, DRIVER, "POST", &remove, Some(json!({ "who": format!("tailnet:{WATCHER}") })));
    assert_eq!(s, 403);
    let (s, v) = guest(&d, WATCHER, "POST", &remove, Some(json!({ "who": format!("tailnet:{WATCHER}") })));
    assert_eq!(s, 200, "{v}");
    let (s, _) = guest(&d, WATCHER, "GET", &thread, None);
    assert_eq!(s, 404, "someone who left reads nothing");
    let (threads, convs) = seen(&d, WATCHER).await;
    assert!(convs.is_null(), "{convs}");
    assert!(!threads.to_string().contains(&key), "{threads}");

    // It all outlives a restart, and search finds what was said.
    std::thread::sleep(Duration::from_secs(3));
    d.stop();
    d.start();
    let (_, v) = guest(&d, DRIVER, "GET", &thread, None);
    assert_eq!(texts(&v).len(), 3, "{v}");
    let r = d.post(&thread, json!({ "text": "after the restart" }));
    assert_eq!(r["message"]["id"], 4, "numbering carries on");
    let hits = d.get("/api/search?re=restart");
    assert!(hits.as_array().unwrap().iter().any(|h| h["thread"] == key), "{hits}");

    // Deleting is the owner's, and takes the timeline with it.
    let (s, _) = guest(&d, DRIVER, "DELETE", &format!("/api/convs/{id}"), None);
    assert_eq!(s, 403);
    let (s, _) = owner(&d, "DELETE", &format!("/api/convs/{id}"), None);
    assert_eq!(s, 200);
    let (s, _) = owner(&d, "GET", &thread, None);
    assert_eq!(s, 404);
    let (s, _) = owner(&d, "POST", &format!("/api/convs/{id}/rename"), Some(json!({ "title": "x" })));
    assert_eq!(s, 404);
    // Not an id at all.
    let (s, _) = owner(&d, "POST", "/api/convs/c-nope/rename", Some(json!({ "title": "x" })));
    assert_eq!(s, 404);
}

/// #889: the owner is called by their name, not their tailnet login's email
/// (as control calls them, and as the page shows them), in what they post,
/// the conversation they make and the grants they give, and an `@` of it
/// still reaches them. A link here keeps its grant's epoch exactly, not
/// the one a browser sent (it rounds nanoseconds).
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_is_named_short_and_a_link_keeps_its_epoch() {
    let d = daemon("convs-name");
    let panes = d.get("/api/panes");
    let (pane, session) = (panes[0]["id"].as_u64().unwrap(), panes[0]["session"].as_u64().unwrap());
    let epoch = panes[0]["epoch"].as_u64().unwrap();
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{WATCHER}"), "role": "editor" }));

    let (s, c) = owner(&d, "POST", "/api/convs", Some(json!({ "title": "names" })));
    assert_eq!(s, 200, "{c}");
    assert_eq!(c["owner_name"], "me", "{c}");
    let id = c["id"].as_str().unwrap().to_owned();
    let thread = format!("/api/threads/conv-{id}");
    let (s, v) = owner(&d, "POST", &thread, Some(json!({ "text": "hello" })));
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["message"]["name"], "me", "{v}");

    // An `@` of the short name reaches the owner.
    d.post(&format!("/api/convs/{id}/members"), json!({ "who": format!("tailnet:{WATCHER}") }));
    let (s, v) = guest(&d, WATCHER, "POST", &thread, Some(json!({ "text": "@me are you there?" })));
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["message"]["mentions"], json!(["owner"]), "{v}");

    let (s, g) = owner(&d, "POST", &format!("/api/panes/{pane}/conv-grants"), Some(json!({ "conv": id })));
    assert_eq!(s, 200, "{g}");
    assert_eq!((g["by_name"].as_str(), g["epoch"].as_u64()), (Some("me"), Some(epoch)), "{g}");
    // A browser's rounded epoch doesn't replace the grant's.
    let c = d.post(&format!("/api/convs/{id}/links"), json!({ "pane": pane, "epoch": epoch / 1000 * 1000 + 1 }));
    assert_eq!(c["links"][0]["epoch"].as_u64(), Some(epoch), "{c}");
}

/// With chat off, conversations are refused like every thread route.
#[test]
fn conversations_follow_the_chat_flag() {
    let d =
        arugulad!("convs-flag").no_wisp().args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]).start();
    let (s, v) = owner(&d, "POST", "/api/convs", Some(json!({ "title": "x" })));
    assert_eq!(s, 404, "{v}");
    assert!(v["error"].as_str().unwrap().contains("arugulad flags chat on"), "{v}");
}
