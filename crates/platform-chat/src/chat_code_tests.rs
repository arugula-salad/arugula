//! Code is never a mention (#492): inside backticks or a fence, `@person`,
//! `@channel`, a keyword, `@machine`, `@machine/session` and `@arugula`
//! notify nobody, tell no machine and start no session; outside code they
//! all do. The text is stored as written.

use arugula_control_wire::forge::OpAnswer;
use serde_json::{Value, json};

use crate::{
    Platform,
    agents_tests::give,
    chat_tests::{TestHub, call},
    db::projects::{THREADS_POST, THREADS_READ},
    sessions_hydrate_tests::{platform, post_in, sessions},
};

const ROOM: &str = "ch_general";

/// Every kind of mention, as a text with the word alone. `geek-box` is a's
/// machine, which hears the room; `a` asks for "deploy"; `@arugula` starts
/// a session.
const KINDS: [&str; 6] = ["@a", "@channel", "deploy", "@geek-box", "@geek-box/main", "@arugula fix the canary"];

/// b posts into a project with a's machine heard in the room.
async fn setup() -> (std::sync::Arc<TestHub>, std::sync::Arc<Platform>, String, String) {
    let (hub, app, base, t, _u, _keys) = platform(&["https://github.com/o/app"]).await;
    hub.daemon("geek", "a", Some("t"));
    crate::projects::follow(&app).await.unwrap();
    crate::cache::catch_up(&app).await.unwrap();
    for power in [THREADS_READ, THREADS_POST] {
        let scope = json!({ "kind": "project", "id": t });
        let (st, g) = call(&base, "a", "POST", "/api/grants", give("geek", power, scope)).await;
        assert_eq!(st, 200, "{g}");
    }
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (st, v) = call(&base, "a", "PUT", "/api/chat/keywords", Some(json!({ "words": ["deploy"] }))).await;
    assert_eq!(st, 200, "{v}");
    (hub, app, base, t)
}

/// What a post did: the mentions, the unreached, who was pushed.
fn outcome(v: &Value) -> (Value, Value, Value) {
    (v["mentions"].clone(), v["unreached"].clone(), v["pushed"].clone())
}

fn wrapped(text: &str) -> [String; 4] {
    [format!("`{text}`"), format!("see ``{text}`` here"), format!("```\n{text}\n```"), format!("~~~\n{text}\n~~~")]
}

/// Whatever the text asked of the world: machines told, devices pushed,
/// sessions made.
fn did(hub: &TestHub, app: &Platform, t: &str) -> (usize, usize, usize) {
    (hub.ops_asked().len(), hub.pushes().len(), sessions(app, t).len())
}

#[tokio::test]
async fn code_mentions_nobody_and_starts_nothing() {
    let (hub, app, base, t) = setup().await;
    for kind in KINDS {
        for text in wrapped(kind) {
            let v = post_in(&base, ROOM, "b", &text, None).await;
            assert_eq!(outcome(&v), (json!([]), json!([]), json!([])), "{text:?}: {v}");
            assert_eq!(did(&hub, &app, &t), (0, 0, 0), "{text:?} did something");
        }
    }
    // Stored as written.
    let (_, v) = call(&base, "b", "GET", &format!("/api/chat/rooms/{ROOM}/messages"), None).await;
    let texts: Vec<&Value> = v["messages"].as_array().unwrap().iter().map(|m| &m["text"]).collect();
    assert!(texts.contains(&&json!("`@a`")), "{texts:?}");
}

#[tokio::test]
async fn code_in_an_edit_mentions_nobody_either() {
    let (hub, app, base, t) = setup().await;
    let first = post_in(&base, ROOM, "b", "plain", None).await["seq"].as_i64().unwrap();
    for kind in KINDS {
        for text in wrapped(kind) {
            let (st, v) = call(
                &base,
                "b",
                "PATCH",
                &format!("/api/chat/rooms/{ROOM}/messages/{first}"),
                Some(json!({ "text": text })),
            )
            .await;
            assert_eq!(st, 200, "{v}");
            assert_eq!(v["mentions"], json!([]), "{text:?}: {v}");
            assert_eq!(did(&hub, &app, &t), (0, 0, 0), "{text:?} did something");
        }
    }
}

#[tokio::test]
async fn the_same_words_outside_code_still_mention() {
    let (hub, app, base, t) = setup().await;
    let v = post_in(&base, ROOM, "b", "@a hi", None).await;
    assert_eq!(v["mentions"], json!(["a"]), "{v}");
    let v = post_in(&base, ROOM, "b", "@channel hi", None).await;
    assert_eq!(v["mentions"], json!(["a", "x", "y"]), "{v}");
    let v = post_in(&base, ROOM, "b", "the deploy failed", None).await;
    assert_eq!(v["mentions"], json!(["a"]), "{v}");
    assert_eq!(hub.ops_asked().len(), 0);
    for text in ["@geek-box hi", "@geek-box/main hi"] {
        let v = post_in(&base, ROOM, "b", text, None).await;
        assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["machine:geek"]), &json!([])), "{text}: {v}");
    }
    assert_eq!(hub.ops_asked().len(), 2, "both told the machine");
    let v = post_in(&base, ROOM, "b", "@arugula fix the canary", None).await;
    let p = v["seq"].as_u64().unwrap();
    assert!(sessions(&app, &t).iter().any(|s| s.parent_seq == p), "a session was started");

    // An edit adding a mention outside code works it out again.
    let own = post_in(&base, ROOM, "b", "plain", None).await["seq"].as_i64().unwrap();
    let path = format!("/api/chat/rooms/{ROOM}/messages/{own}");
    let (_, v) = call(&base, "b", "PATCH", &path, Some(json!({ "text": "@a" }))).await;
    assert_eq!(v["mentions"], json!(["a"]), "{v}");
}

/// A backtick that never closes isn't code, and a mention in code beside a
/// real one doesn't hide it.
#[tokio::test]
async fn an_unclosed_backtick_is_text_and_a_real_mention_beside_code_counts() {
    let (_hub, _app, base, _t) = setup().await;
    let v = post_in(&base, ROOM, "b", "it costs `5 and @a", None).await;
    assert_eq!(v["mentions"], json!(["a"]), "{v}");
    let v = post_in(&base, ROOM, "b", "```\n@a", None).await;
    assert_eq!(v["mentions"], json!(["a"]), "an unclosed fence: {v}");
    let v = post_in(&base, ROOM, "b", "`@geek-box` but @a", None).await;
    assert_eq!((&v["mentions"], &v["unreached"]), (&json!(["a"]), &json!([])), "{v}");
}
