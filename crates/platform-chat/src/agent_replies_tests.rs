//! A machine's unread replies, grouped by thread (#580): replies to its
//! agents' posts, the root's first line the daemon is told with each, and a
//! reply leaving the list once a session read it.

use arugula_control_wire::{
    chat::{AGENT_REPLIES, AgentReplies, agent_messages},
    forge::{DaemonOp, OpAnswer, Routed, RoutedStep},
};
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::projects::{THREADS_POST, THREADS_READ},
};

const ROOM: &str = "/api/chat/rooms/ch_general/messages";

/// `who` posts `text` (under `parent`, if any); the new message's number.
async fn post(base: &str, who: &str, text: &str, parent: Option<i64>) -> i64 {
    let (st, v) = call(base, who, "POST", ROOM, Some(json!({ "text": text, "parent": parent }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// The machine `geek`'s agent posts `text` (top-level, or under `parent`).
async fn agent_post(base: &str, text: &str, parent: Option<i64>) -> i64 {
    let r = reqwest::Client::new()
        .post(format!("{base}{}", agent_messages("ch_general")))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "text": text, "parent": parent }))
        .send()
        .await
        .unwrap();
    r.json::<Value>().await.unwrap()["seq"].as_i64().unwrap()
}

async fn replies(base: &str) -> AgentReplies {
    let r = reqwest::Client::new()
        .get(format!("{base}{AGENT_REPLIES}"))
        .header("x-test-daemon", "a geek")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    r.json().await.unwrap()
}

/// The shape of the answer: per thread, its root and its replies' numbers.
fn shape(r: &AgentReplies) -> Vec<(i64, Vec<i64>)> {
    r.threads.iter().map(|t| (t.root.seq, t.replies.iter().map(|m| m.seq).collect())).collect()
}

/// Three replies in two threads come back as two threads, oldest first
/// inside each; one that a session read leaves; a daemon is told the root's
/// first line with each reply.
#[tokio::test]
async fn three_replies_in_two_threads_are_two_groups_and_a_read_one_leaves() {
    let (hub, _app, base, t, _u) = seeded().await;
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(&base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
    hub.answers_ops(
        "geek",
        OpAnswer::Done {
            url: None,
            routed: Some(Routed {
                step: RoutedStep::Thread,
                session: Some("views".into()),
                asked: None,
                queued: false,
                names: vec![],
                silenced: false,
            }),
        },
    );
    assert!(replies(&base).await.threads.is_empty());

    let first = agent_post(&base, "Plan for the beta\nwith a second line", None).await;
    let second = agent_post(&base, "Who owns the deploy?", None).await;
    let a1 = post(&base, "b", "first reply", Some(first)).await;
    let b1 = post(&base, "b", "reply elsewhere", Some(second)).await;
    // Only the reply right after an agent's post is to the agent; a later one
    // reaches its machine by naming it.
    let a2 = post(&base, "b", "@geek-box second reply", Some(first)).await;
    // A reply in a thread no agent of the machine posted in is not its.
    let theirs = post(&base, "a", "a person's thread", None).await;
    post(&base, "b", "reply to a person", Some(theirs)).await;

    // The daemon heard the thread's first line with each reply.
    for _ in 0..100 {
        if hub.ops_asked().len() >= 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let lines: Vec<(i64, Option<String>)> = hub
        .ops_asked()
        .into_iter()
        .filter_map(|(_, op)| match op {
            DaemonOp::AgentMention(m) => Some((m.seq, m.root_line)),
            _ => None,
        })
        .collect();
    assert!(lines.contains(&(a1, Some("Plan for the beta".into()))), "{lines:?}");
    assert!(lines.contains(&(b1, Some("Who owns the deploy?".into()))), "{lines:?}");

    let got = replies(&base).await;
    assert_eq!(shape(&got), [(first, vec![a1, a2]), (second, vec![b1])]);
    assert_eq!(got.threads[0].room, "ch_general");

    // A session read the first reply of the first thread.
    let r = reqwest::Client::new()
        .put(format!("{base}{}", arugula_control_wire::chat::MENTION_STATE))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "room": "ch_general", "seq": a1, "session": "views", "state": "read" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // Threads go by their oldest unread reply: the other thread's now.
    assert_eq!(shape(&replies(&base).await), [(second, vec![b1]), (first, vec![a2])]);
}
