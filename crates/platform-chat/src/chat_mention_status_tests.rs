//! Where an `@machine` mention got to (#502): what each of the daemon's four
//! steps, an old daemon and an offline one show the sender under their
//! message, who sees it, and an agent's reply in the thread answering it.

use std::time::Duration;

use arugula_control_wire::{
    chat::agent_messages,
    forge::{OpAnswer, Routed, RoutedStep},
};
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{call, hears, open_live},
    db::{
        chat_mention_status::MentionStatus,
        projects::{THREADS_POST, THREADS_READ},
    },
};

const ROOM: &str = "/api/chat/rooms/ch_general/messages";

async fn geek_in_t(base: &str, t: &str) {
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
}

fn done(step: RoutedStep, session: Option<&str>, asked: Option<&str>) -> OpAnswer {
    OpAnswer::Done {
        url: None,
        routed: Some(Routed { step, session: session.map(str::to_owned), asked: asked.map(str::to_owned) }),
    }
}

/// `who` posts `text` (under `parent`, if any); the new message's number.
async fn post(base: &str, who: &str, text: &str, parent: Option<i64>) -> i64 {
    let (st, v) = call(base, who, "POST", ROOM, Some(json!({ "text": text, "parent": parent }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// What `who` sees under message `seq` of the room (or of its thread).
async fn status(base: &str, who: &str, seq: i64, thread: Option<i64>) -> Value {
    let path = match thread {
        Some(t) => format!("{ROOM}?thread={t}"),
        None => ROOM.to_owned(),
    };
    let (_, v) = call(base, who, "GET", &path, None).await;
    let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).cloned().unwrap();
    m.get("mention_status").cloned().unwrap_or(json!([]))
}

/// Every way a daemon can answer shows as its own line, to the poster and
/// to a second person, each told over the stream; a daemon from before the
/// routing is still "delivered".
#[tokio::test]
async fn each_answer_is_its_own_line_for_everyone() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let a_live = open_live(&base, "a", None).await;
    hears(&a_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    let cases = [
        // 1 and 2: delivered, to the session when it said which.
        (done(RoutedStep::Named, Some("slowness"), None), json!("delivered"), json!("slowness"), Value::Null),
        (done(RoutedStep::Thread, Some("views"), None), json!("delivered"), json!("views"), Value::Null),
        // An older daemon says nothing of how: delivered, to no one in particular.
        (OpAnswer::Done { url: None, routed: None }, json!("delivered"), Value::Null, Value::Null),
        // 3: a fallback tells the sender the name missed.
        (
            done(RoutedStep::Room, Some("views"), Some("slowness")),
            json!("fell_back"),
            json!("views"),
            json!("slowness"),
        ),
        // 4: nobody took it.
        (done(RoutedStep::Inbox, None, Some("slowness")), json!("waiting"), Value::Null, json!("slowness")),
        (done(RoutedStep::Inbox, None, None), json!("waiting"), Value::Null, Value::Null),
        // The owner's switch is off, or the daemon doesn't know the op.
        (OpAnswer::Cant { why: "off".into() }, json!("offline"), Value::Null, Value::Null),
        (OpAnswer::Unknown, json!("offline"), Value::Null, Value::Null),
        (OpAnswer::Offline, json!("offline"), Value::Null, Value::Null),
    ];
    for (answer, state, session, asked) in cases {
        hub.answers_ops("geek", answer.clone());
        let seq = post(&base, "b", "@geek-box are you there", None).await;
        for who in ["b", "a"] {
            let got = status(&base, who, seq, None).await;
            assert_eq!(got.as_array().map(Vec::len), Some(1), "{got}");
            // A field with nothing to say is absent.
            let field = |k: &str| got[0].get(k).cloned().unwrap_or(Value::Null);
            assert_eq!(
                (got[0]["machine"].clone(), got[0]["machine_name"].clone(), got[0]["state"].clone()),
                (json!("geek"), json!("geek-box"), state.clone()),
                "{answer:?} as {who} saw it"
            );
            assert_eq!(
                (field("session"), field("asked"), field("reply")),
                (session.clone(), asked.clone(), Value::Null),
                "{answer:?} as {who} saw it"
            );
        }
        hears(&a_live, &format!("\"type\":\"reacted\",\"room\":\"ch_general\",\"seq\":{seq}"), Duration::from_secs(5))
            .await;
    }
}

/// An unanswered offline daemon (the test hub's default) is a line too, and
/// a message that mentions no machine has none.
#[tokio::test]
async fn an_unreachable_machine_is_offline_and_a_plain_message_has_no_line() {
    let (_hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let seq = post(&base, "b", "@geek-box hello", None).await;
    let s = status(&base, "b", seq, None).await;
    assert_eq!((s[0]["state"].clone(), s[0]["machine_name"].clone()), (json!("offline"), json!("geek-box")), "{s}");
    let plain = post(&base, "b", "just people talking", None).await;
    assert_eq!(status(&base, "b", plain, None).await, json!([]));
}

/// The machine's agent replying in the thread answers the mention under it
/// (the reply's number, seen by a second client over the stream), a person's
/// reply or another thread's doesn't, and a mention from over an hour ago
/// stays as it was.
#[tokio::test]
async fn the_agents_reply_in_the_thread_answers_it() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let root = post(&base, "b", "@geek-box/slowness why is it slow", None).await;
    let other = post(&base, "b", "@geek-box something else", None).await;
    // A second mention under the root, from the same person, and a stale one.
    let again = post(&base, "b", "@geek-box ping", Some(root)).await;
    let old = post(&base, "b", "@geek-box long ago", Some(root)).await;
    app.db
        .set_chat_mention_status(&MentionStatus {
            room: "ch_general".into(),
            seq: old,
            machine: "geek".into(),
            machine_name: "geek-box".into(),
            root,
            state: "delivered".into(),
            session: None,
            asked: None,
            reply: None,
            at: 1,
        })
        .unwrap();
    let a_live = open_live(&base, "a", None).await;
    hears(&a_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    // A person answering, and the agent in another thread: nothing yet.
    post(&base, "a", "it's the db", Some(root)).await;
    let path = agent_messages("ch_general");
    let agent = |parent: Option<i64>| {
        let c = reqwest::Client::new();
        let url = format!("{base}{path}");
        async move {
            let r = c
                .post(url)
                .header("x-test-daemon", "a geek")
                .json(&json!({ "text": "looking", "parent": parent }))
                .send()
                .await
                .unwrap();
            r.json::<Value>().await.unwrap()["seq"].as_i64().unwrap()
        }
    };
    agent(Some(other)).await;
    agent(None).await;
    let states = |v: Value| v[0]["state"].as_str().unwrap().to_owned();
    assert_eq!(states(status(&base, "a", root, None).await), "delivered");

    // The agent's reply under the root answers the mentions in that thread.
    let reply = agent(Some(root)).await;
    hears(&a_live, &format!("\"type\":\"reacted\",\"room\":\"ch_general\",\"seq\":{root}"), Duration::from_secs(5))
        .await;
    for who in ["a", "b"] {
        let s = status(&base, who, root, None).await;
        assert_eq!((s[0]["state"].clone(), s[0]["reply"].clone()), (json!("answered"), json!(reply)), "{who}: {s}");
    }
    let s = status(&base, "a", again, Some(root)).await;
    assert_eq!((s[0]["state"].clone(), s[0]["reply"].clone()), (json!("answered"), json!(reply)), "{s}");
    assert_eq!(states(status(&base, "a", old, Some(root)).await), "delivered", "over an hour old");
    assert_eq!(states(status(&base, "a", other, None).await), "answered", "its own thread's reply answered it");
}
