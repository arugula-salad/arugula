//! A mention says it's from the machine's owner (#665) only when the owner
//! posted it themselves: a teammate's post, and an agent's (the owner's own
//! agents' too), never do, whether told at once or sent again later.

use std::time::Duration;

use arugula_control_wire::{
    chat::agent_messages,
    forge::{AgentMention, DaemonOp, OpAnswer},
};
use serde_json::json;

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{TestHub, call},
    db::projects::{THREADS_POST, THREADS_READ},
};
use arugula_platform::PlatformPort as _;

const ROOM: &str = "/api/chat/rooms/ch_general/messages";

/// `geek` (account `a`'s machine) may read and post in the room.
async fn geek_in_t(base: &str, t: &str) {
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
}

/// `who` posts `text`; the new message's number.
async fn post(base: &str, who: &str, text: &str) -> i64 {
    let (st, v) = call(base, who, "POST", ROOM, Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// What geek's daemon was told, each time, oldest first.
fn told(hub: &TestHub) -> Vec<AgentMention> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(machine, op)| match op {
            DaemonOp::AgentMention(m) if machine == "geek" => Some(m),
            _ => None,
        })
        .collect()
}

/// Until geek's daemon was told `n` mentions.
async fn told_at_least(hub: &TestHub, n: usize) -> Vec<AgentMention> {
    for _ in 0..200 {
        let got = told(hub);
        if got.len() >= n {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("told {:?}", told(hub).iter().map(|m| m.seq).collect::<Vec<_>>());
}

fn from_owner(told: &[AgentMention], seq: i64) -> Vec<bool> {
    told.iter().filter(|m| m.seq == seq).map(|m| m.from_owner).collect()
}

/// The owner's own post is the owner's; a teammate's, and the owner's agent's
/// mention of its own machine (#575), are not.
#[tokio::test]
async fn only_the_owners_own_post_is_told_as_the_owners() {
    let (hub, _app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });

    let owners = post(&base, "a", "@geek-box how about you take it").await;
    let teammates = post(&base, "b", "@geek-box or you could").await;
    let r = reqwest::Client::new()
        .post(format!("{base}{}", agent_messages("ch_general")))
        .header("x-test-daemon", "a geek")
        .json(&json!({ "text": "@geek-box over to another session" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let agents = r.json::<serde_json::Value>().await.unwrap()["seq"].as_i64().unwrap();

    let got = told_at_least(&hub, 3).await;
    assert_eq!(from_owner(&got, owners), [true], "the owner's post");
    assert_eq!(from_owner(&got, teammates), [false], "a teammate's post");
    assert_eq!(from_owner(&got, agents), [false], "the owner's agent's post");
    // On the wire, only the owner's says so.
    let wire = |m: &AgentMention| serde_json::to_value(m).unwrap().get("from_owner").cloned();
    let by = |seq| got.iter().find(|m| m.seq == seq).unwrap();
    assert_eq!(wire(by(owners)), Some(json!(true)));
    assert_eq!(wire(by(teammates)), None);
}

/// A mention sent again when the machine connects (#419) says the same: the
/// owner's is the owner's, a teammate's is not.
#[tokio::test]
async fn a_mention_sent_again_keeps_whose_it_is() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    // Offline: each is told once, and missed.
    let owners = post(&base, "a", "@geek-box are you there").await;
    let teammates = post(&base, "b", "@geek-box hello?").await;
    told_at_least(&hub, 2).await;

    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    app.daemon_came("geek").await.unwrap();
    let got = told_at_least(&hub, 4).await;
    assert_eq!(from_owner(&got, owners), [true, true]);
    assert_eq!(from_owner(&got, teammates), [false, false]);
}
