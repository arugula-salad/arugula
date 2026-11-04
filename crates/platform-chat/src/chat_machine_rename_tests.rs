//! A renamed machine (Arugula #500): its new name is what chat shows and
//! what `@machine` mentions it by, and the name it had still reaches it.

use std::time::Duration;

use arugula_control_wire::forge::{DaemonOp, OpAnswer, Routed, RoutedStep};
use serde_json::{Value, json};

use crate::{
    agents_tests::{give, seeded},
    chat_tests::call,
    db::projects::{THREADS_POST, THREADS_READ},
};

const ROOM: &str = "/api/chat/rooms/ch_general/messages";

async fn in_t(base: &str, t: &str, machine: &str) {
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(base, "a", "POST", "/api/grants", give(machine, power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
}

fn named(session: &str) -> OpAnswer {
    OpAnswer::Done {
        url: None,
        routed: Some(Routed {
            step: RoutedStep::Named,
            session: Some(session.into()),
            asked: None,
            queued: false,
            names: vec![],
            silenced: false,
        }),
    }
}

async fn post(base: &str, text: &str) -> i64 {
    let (st, v) = call(base, "b", "POST", ROOM, Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

/// What `b` sees under message `seq` once nothing is `sending`.
async fn status(base: &str, seq: i64) -> Value {
    let mut seen = json!([]);
    for _ in 0..100 {
        let (_, v) = call(base, "b", "GET", ROOM, None).await;
        let m = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).cloned().unwrap();
        seen = m.get("mention_status").cloned().unwrap_or(json!([]));
        if !seen.as_array().unwrap().iter().any(|l| l["state"] == "sending") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    seen
}

/// The machine renamed in the hub, and the platform's cache told, as the
/// change feed does within `cache::DELAY`.
async fn rename(hub: &crate::testing::TestHub, app: &crate::Platform, id: &str, name: &str) {
    hub.rename_daemon(id, name);
    crate::cache::catch_up(app).await.unwrap();
}

fn told(hub: &crate::testing::TestHub) -> Vec<(String, Option<String>)> {
    hub.ops_asked()
        .into_iter()
        .filter_map(|(m, op)| match op {
            DaemonOp::AgentMention(a) => Some((m, a.session)),
            _ => None,
        })
        .collect()
}

/// The trap: `@new/x` and `@old/x` both reach the machine, with the session
/// each names; the statuses of an old message say the new name; the picker
/// lists the new name alone.
#[tokio::test]
async fn the_new_name_and_the_old_one_reach_the_same_machine() {
    let (hub, app, base, t, _u) = seeded().await;
    in_t(&base, &t, "geek").await;
    hub.answers_ops("geek", named("x"));
    let before = post(&base, "@geek-box/x before").await;
    assert_eq!(status(&base, before).await[0]["machine_name"], "geek-box");

    rename(&hub, &app, "geek", "Mini").await;
    let new = post(&base, "@mini/x after").await;
    let old = post(&base, "@geek-box/x by the old name").await;
    for seq in [new, old] {
        let s = status(&base, seq).await;
        assert_eq!((s[0]["machine"].clone(), s[0]["state"].clone()), (json!("geek"), json!("delivered")), "{s}");
        assert_eq!(s[0]["machine_name"], "Mini", "{s}");
    }
    // The message sent before the rename shows the name now, not then.
    assert_eq!(status(&base, before).await[0]["machine_name"], "Mini");
    assert_eq!(told(&hub), vec![("geek".to_owned(), Some("x".to_owned())); 3]);

    // Neither word is a stranger's: nothing is told for a name no machine had.
    let none = post(&base, "@nobody/x hi").await;
    assert_eq!(status(&base, none).await, json!([]));
    assert_eq!(told(&hub).len(), 3);

    // The picker lists the machine by its new name only.
    let (_, room) = call(&base, "b", "GET", "/api/chat/rooms/ch_general", None).await;
    let names: Vec<&str> =
        room["agents"].as_array().unwrap().iter().filter_map(|a| a["machine_name"].as_str()).collect();
    assert_eq!(names, ["Mini"], "{room}");
    assert!(!room.to_string().contains("aliases"), "an old name isn't offered: {room}");
}

/// A name another machine of the team goes by now is that machine's, even if
/// the first machine went by it once.
#[tokio::test]
async fn a_name_in_use_now_goes_to_its_machine_not_to_the_one_that_had_it() {
    let (hub, app, base, t, _u) = seeded().await;
    hub.daemon("lab", "a", Some("t"));
    in_t(&base, &t, "geek").await;
    in_t(&base, &t, "lab").await;
    hub.answers_ops("geek", named("x"));
    hub.answers_ops("lab", named("x"));

    rename(&hub, &app, "geek", "mini").await;
    rename(&hub, &app, "lab", "geek-box").await;
    let seq = post(&base, "@geek-box/x hello").await;
    let s = status(&base, seq).await;
    assert_eq!(s.as_array().map(Vec::len), Some(1), "{s}");
    assert_eq!(s[0]["machine"], "lab", "{s}");
    assert_eq!(told(&hub), vec![("lab".to_owned(), Some("x".to_owned()))]);
}
