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
        routed: Some(Routed {
            step,
            session: session.map(str::to_owned),
            asked: asked.map(str::to_owned),
            queued: false,
            names: vec![],
        }),
    }
}

/// A session that has the mention in its queue, its hook not waiting (#571).
fn queued(step: RoutedStep, session: &str, asked: Option<&str>) -> OpAnswer {
    match done(step, Some(session), asked) {
        OpAnswer::Done { url, routed: Some(r) } => OpAnswer::Done { url, routed: Some(Routed { queued: true, ..r }) },
        other => other,
    }
}

/// A mention that named a session none of the daemon's is called (#576).
fn no_such(asked: &str, names: &[&str]) -> OpAnswer {
    OpAnswer::Done {
        url: None,
        routed: Some(Routed {
            step: RoutedStep::Inbox,
            session: None,
            asked: Some(asked.into()),
            queued: false,
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }),
    }
}

/// `who` posts `text` (under `parent`, if any); the new message's number.
async fn post(base: &str, who: &str, text: &str, parent: Option<i64>) -> i64 {
    let (st, v) = call(base, who, "POST", ROOM, Some(json!({ "text": text, "parent": parent }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// What `who` sees under message `seq` of the room (or of its thread), once
/// no line is `sending` (the daemons are told after the post returns, #419).
async fn status(base: &str, who: &str, seq: i64, thread: Option<i64>) -> Value {
    let path = match thread {
        Some(t) => format!("{ROOM}?thread={t}"),
        None => ROOM.to_owned(),
    };
    let mut seen = json!([]);
    for _ in 0..100 {
        let (_, v) = call(base, who, "GET", &path, None).await;
        let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).cloned().unwrap();
        seen = m.get("mention_status").cloned().unwrap_or(json!([]));
        if !seen.as_array().unwrap().iter().any(|l| l["state"] == "sending") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    seen
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
        // Taken, but only queued: its hooks aren't listening (#571). Said so
        // even when it also fell back from a name.
        (queued(RoutedStep::Named, "slowness", None), json!("queued"), json!("slowness"), Value::Null),
        (queued(RoutedStep::Room, "views", Some("slowness")), json!("queued"), json!("views"), json!("slowness")),
        // 3: a fallback tells the sender the name missed.
        (
            done(RoutedStep::Room, Some("views"), Some("slowness")),
            json!("fell_back"),
            json!("views"),
            json!("slowness"),
        ),
        // 4: nobody took it, and no session was named: it waits.
        (done(RoutedStep::Inbox, None, None), json!("waiting"), Value::Null, Value::Null),
        // 4: it named a session that isn't there: not "waiting" (#576).
        (no_such("slowness", &["views", "deploy"]), json!("no_such_session"), Value::Null, json!("slowness")),
        (no_such("slowness", &[]), json!("no_such_session"), Value::Null, json!("slowness")),
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
            let names = match &answer {
                OpAnswer::Done { routed: Some(r), .. } if !r.names.is_empty() => json!(r.names),
                _ => Value::Null,
            };
            assert_eq!(field("names"), names, "{answer:?} as {who} saw it");
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

/// The post's own answer carries the statuses known when it answers (#571):
/// the machine's line is there, `sending` or already settled (which one is
/// a race), and a post that mentions no machine has none. The outcome is
/// what the message shows once it settles.
#[tokio::test]
async fn a_post_answers_with_the_statuses_known_so_far() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", queued(RoutedStep::Named, "slowness", None));
    let (st, v) = call(&base, "b", "POST", ROOM, Some(json!({ "text": "@geek-box/slowness hi" }))).await;
    assert_eq!(st, 200, "{v}");
    let known = v["mention_status"].as_array().unwrap();
    assert_eq!(known.len(), 1, "{v}");
    assert_eq!((known[0]["machine"].clone(), known[0]["machine_name"].clone()), (json!("geek"), json!("geek-box")));
    assert!(["sending", "queued"].contains(&known[0]["state"].as_str().unwrap()), "{v}");
    let seq = v["seq"].as_i64().unwrap();
    assert_eq!(status(&base, "b", seq, None).await[0]["state"], "queued");
    let (_, plain) = call(&base, "b", "POST", ROOM, Some(json!({ "text": "no machines here" }))).await;
    assert!(plain.get("mention_status").is_none(), "{plain}");
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
            names: vec![],
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

/// The daemon's word on a delivered mention's turn (#540), as `machine`
/// (`a geek` and the like) signs it.
async fn turn(base: &str, daemon: &str, seq: i64, state: &str) -> (u16, Value) {
    turn_as(base, daemon, seq, json!(state)).await
}

/// [`turn`], with the state as the wire has it: `{"dropped": "stopped"}`.
async fn turn_as(base: &str, daemon: &str, seq: i64, state: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .put(format!("{base}{}", arugula_control_wire::chat::MENTION_STATE))
        .header("x-test-daemon", daemon)
        .json(&json!({ "room": "ch_general", "seq": seq, "session": "slowness", "state": state }))
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or_default())
}

/// A delivered mention goes `working`, then `read`, each seen by a second
/// client over the stream; a machine with no status for the message is
/// refused and changes nothing.
#[tokio::test]
async fn the_daemon_says_working_then_read_and_a_second_client_sees_it() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.daemon("lab", "a", Some("t"));
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let seq = post(&base, "b", "@geek-box/slowness why is it slow", None).await;
    assert_eq!(status(&base, "b", seq, None).await[0]["state"], "delivered");
    let a_live = open_live(&base, "a", None).await;
    hears(&a_live, "\"type\":\"hello\"", Duration::from_secs(5)).await;

    for state in ["working", "read"] {
        assert_eq!(turn(&base, "a geek", seq, state).await.0, 200);
        hears(&a_live, &format!("\"type\":\"reacted\",\"room\":\"ch_general\",\"seq\":{seq}"), Duration::from_secs(5))
            .await;
        for who in ["a", "b"] {
            let s = status(&base, who, seq, None).await;
            assert_eq!((s[0]["state"].clone(), s[0]["session"].clone()), (json!(state), json!("slowness")), "{who}");
        }
    }
    // A machine the message didn't mention has no status for it.
    let (st, v) = turn(&base, "a lab", seq, "working").await;
    assert_eq!(st, 404, "{v}");
    let (st, _) = turn(&base, "a geek", seq + 100, "working").await;
    assert_eq!(st, 404, "no such message");
    assert_eq!(status(&base, "b", seq, None).await[0]["state"], "read");
}

/// The trap (#540): the agent replies during its turn, so `answered` comes
/// before the turn's `read`, which must not turn it back. `working` after
/// `read` doesn't either, and a mention no session took has no turn.
#[tokio::test]
async fn a_late_read_never_undoes_answered() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let seq = post(&base, "b", "@geek-box/slowness why is it slow", None).await;
    let c = reqwest::Client::new();
    let reply = c
        .post(format!("{base}{}", agent_messages("ch_general")))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "text": "the db", "parent": seq }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["seq"]
        .as_i64()
        .unwrap();
    // The reply came first. Then the turn's own word.
    for state in ["working", "read"] {
        assert_eq!(turn(&base, "a geek", seq, state).await.0, 200, "{state} is accepted, and changes nothing");
        let s = status(&base, "b", seq, None).await;
        assert_eq!((s[0]["state"].clone(), s[0]["reply"].clone()), (json!("answered"), json!(reply)), "{state}");
    }

    // `read` stays `read` against a late `working`.
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let next = post(&base, "b", "@geek-box/slowness and now", None).await;
    status(&base, "b", next, None).await;
    for state in ["read", "working"] {
        turn(&base, "a geek", next, state).await;
    }
    assert_eq!(status(&base, "b", next, None).await[0]["state"], "read");

    // Nobody took it: it waits in the inbox, and has no turn.
    hub.answers_ops("geek", done(RoutedStep::Inbox, None, None));
    let waiting = post(&base, "b", "@geek-box anyone", None).await;
    assert_eq!(status(&base, "b", waiting, None).await[0]["state"], "waiting");
    assert_eq!(turn(&base, "a geek", waiting, "working").await.0, 200);
    assert_eq!(status(&base, "b", waiting, None).await[0]["state"], "waiting");
}

/// The trap (#613): `dropped` is final, and only goes forward. It lands
/// after `working` (a turn cut off) or on a mention that never got a turn
/// (a stop cleared the queue), shows its reason to every reader, and a
/// late `read` or `working` from a stale turn can't undo it. It never
/// replaces `answered` or `read`, nor a mention no session took.
#[tokio::test]
async fn a_dropped_mention_stays_dropped_and_never_replaces_an_answer() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let stopped = json!({ "dropped": "stopped" });
    let exited = json!({ "dropped": "exited" });
    let dropped = |s: &Value| (s[0]["state"].clone(), s[0]["why"].clone());

    // A turn cut off: working, then dropped; the stale turn's `read` and
    // `working`, and a second drop with another reason, change nothing.
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let cut = post(&base, "b", "@geek-box/slowness first", None).await;
    assert_eq!(turn(&base, "a geek", cut, "working").await.0, 200);
    assert_eq!(turn_as(&base, "a geek", cut, exited.clone()).await.0, 200);
    assert_eq!(dropped(&status(&base, "b", cut, None).await), (json!("dropped"), json!("exited")));
    for late in [json!("read"), json!("working"), stopped.clone()] {
        assert_eq!(turn_as(&base, "a geek", cut, late.clone()).await.0, 200, "{late}");
        assert_eq!(dropped(&status(&base, "b", cut, None).await), (json!("dropped"), json!("exited")), "{late}");
    }

    // Never got a turn: dropped from delivered.
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let waiting = post(&base, "b", "@geek-box/slowness second", None).await;
    assert_eq!(turn_as(&base, "a geek", waiting, stopped.clone()).await.0, 200);
    assert_eq!(dropped(&status(&base, "b", waiting, None).await), (json!("dropped"), json!("stopped")));

    // The agent answered first: dropped doesn't replace it. Nor `read`.
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let answered = post(&base, "b", "@geek-box/slowness third", None).await;
    let reply = reqwest::Client::new()
        .post(format!("{base}{}", agent_messages("ch_general")))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "text": "the db", "parent": answered }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["seq"]
        .as_i64()
        .unwrap();
    assert_eq!(turn_as(&base, "a geek", answered, stopped.clone()).await.0, 200);
    let s = status(&base, "b", answered, None).await;
    assert_eq!((s[0]["state"].clone(), s[0]["reply"].clone()), (json!("answered"), json!(reply)));

    // Its turn ended (`read`): a drop after it is for a turn that is over.
    hub.answers_ops("geek", done(RoutedStep::Named, Some("slowness"), None));
    let read = post(&base, "b", "@geek-box/slowness fourth", None).await;
    turn(&base, "a geek", read, "read").await;
    turn_as(&base, "a geek", read, exited.clone()).await;
    assert_eq!(status(&base, "b", read, None).await[0]["state"], "read");

    // Nobody took it: it has no turn to drop.
    hub.answers_ops("geek", done(RoutedStep::Inbox, None, None));
    let inbox = post(&base, "b", "@geek-box anyone", None).await;
    turn_as(&base, "a geek", inbox, stopped).await;
    assert_eq!(status(&base, "b", inbox, None).await[0]["state"], "waiting");
}
