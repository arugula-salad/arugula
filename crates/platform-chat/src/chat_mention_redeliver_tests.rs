//! A mention is told after the post returns, and sent again once when its
//! machine connects after being offline (#419).

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use arugula_control_wire::forge::{DaemonOp, OpAnswer};
use serde_json::json;

use crate::{
    agents_tests::{give, seeded},
    chat_tests::{TestHub, call},
    db::{
        chat_mention_status::{MentionStatus, REDELIVER_MAX},
        projects::{THREADS_POST, THREADS_READ},
    },
};
use arugula_platform::PlatformPort as _;

const ROOM: &str = "/api/chat/rooms/ch_general/messages";

async fn geek_in_t(base: &str, t: &str) {
    for power in [THREADS_POST, THREADS_READ] {
        let (st, g) =
            call(base, "a", "POST", "/api/grants", give("geek", power, json!({ "kind": "project", "id": t }))).await;
        assert_eq!(st, 200, "{g}");
    }
}

async fn post(base: &str, text: &str) -> i64 {
    let (st, v) = call(base, "b", "POST", ROOM, Some(json!({ "text": text }))).await;
    assert_eq!(st, 200, "{v}");
    v["seq"].as_i64().unwrap()
}

fn states(app: &crate::Platform) -> Vec<(i64, String)> {
    app.db.chat_mention_statuses("ch_general", 1, 1_000).unwrap().into_iter().map(|s| (s.seq, s.state)).collect()
}

/// Until no mention is being sent.
async fn settled(app: &crate::Platform) {
    for _ in 0..200 {
        if states(app).iter().all(|(_, s)| s != "sending") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("still sending: {:?}", states(app));
}

/// The machine connects: the platform sends what it missed in the
/// background, so wait for `want` asks of geek, then for them to settle,
/// then a moment for any that shouldn't come.
async fn came(app: &crate::Platform, hub: &TestHub, want: usize) {
    app.daemon_came("geek").await.unwrap();
    for _ in 0..200 {
        if asked(hub) >= want {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    settled(app).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked(hub), want);
}

fn asked(hub: &TestHub) -> usize {
    hub.ops_asked().iter().filter(|(m, op)| m == "geek" && matches!(op, DaemonOp::AgentMention(_))).count()
}

/// The post doesn't wait for a slow daemon: it returns at once with the
/// line `sending`, and the line becomes `delivered` when the answer comes.
#[tokio::test]
async fn a_slow_daemon_does_not_hold_the_post() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    hub.on_ops(Arc::new(|_, _| {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            OpAnswer::Done { url: None, routed: None }
        })
    }));
    let at = Instant::now();
    let seq = post(&base, "@geek-box are you there").await;
    assert!(at.elapsed() < Duration::from_millis(500), "the post waited {:?}", at.elapsed());
    assert_eq!(states(&app), [(seq, "sending".to_owned())], "shown at once");
    let (_, v) = call(&base, "b", "GET", ROOM, None).await;
    let line = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).unwrap()["mention_status"][0].clone();
    assert_eq!(line["state"], "sending", "{line}");

    settled(&app).await;
    assert_eq!(states(&app), [(seq, "delivered".to_owned())]);
}

/// A mention for an offline machine is told once when it connects, however
/// often it connects, and two connects at once don't tell it twice.
#[tokio::test]
async fn a_missed_mention_is_sent_once_when_the_machine_connects() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let seq = post(&base, "@geek-box are you there").await;
    settled(&app).await;
    assert_eq!(states(&app), [(seq, "offline".to_owned())]);
    assert_eq!(asked(&hub), 1);

    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    let (a, b) = tokio::join!(app.daemon_came("geek"), app.daemon_came("geek"));
    a.unwrap();
    b.unwrap();
    for _ in 0..200 {
        if asked(&hub) >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    settled(&app).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked(&hub), 2, "the daemon got the missed mention once");
    assert_eq!(states(&app), [(seq, "delivered".to_owned())]);
    let last = hub.ops_asked().last().unwrap().1.clone();
    let DaemonOp::AgentMention(m) = &last else { panic!() };
    assert_eq!((m.seq, m.text.as_str(), m.author.as_str()), (seq, "@geek-box are you there", "b"));

    came(&app, &hub, 2).await;
    came(&app, &hub, 2).await;
}

/// Offline again at the connect: `missed`, and not tried a third time.
#[tokio::test]
async fn a_mention_that_fails_again_is_missed() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let seq = post(&base, "@geek-box hello").await;
    settled(&app).await;
    came(&app, &hub, 2).await;
    assert_eq!(states(&app), vec![(seq, "missed".to_owned())]);
    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    came(&app, &hub, 2).await;
    let (_, v) = call(&base, "b", "GET", ROOM, None).await;
    let line = v["messages"].as_array().unwrap().iter().find(|m| m["seq"] == seq).unwrap()["mention_status"][0].clone();
    assert_eq!(line["state"], "offline", "{line}");
}

/// Only the last day's, at most [`REDELIVER_MAX`], oldest first, and only
/// the machine that connected's.
#[tokio::test]
async fn only_recent_mentions_up_to_the_cap_are_sent() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let mut seqs = vec![];
    for i in 0..REDELIVER_MAX + 3 {
        seqs.push(post(&base, &format!("@geek-box number {i}")).await);
    }
    settled(&app).await;
    // The first is 25 hours old; another machine has one too.
    let stale = app.db.chat_mention_statuses("ch_general", seqs[0], seqs[0]).unwrap().remove(0);
    let day = 25 * 60 * 60 * 1000;
    app.db.set_chat_mention_status(&MentionStatus { at: stale.at - day, ..stale }).unwrap();
    let other = app.db.chat_mention_statuses("ch_general", seqs[1], seqs[1]).unwrap().remove(0);
    app.db.set_chat_mention_status(&MentionStatus { machine: "other".into(), ..other }).unwrap();
    let before = asked(&hub);

    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    came(&app, &hub, before + REDELIVER_MAX).await;
    let got = states(&app);
    let state = |seq: i64| got.iter().find(|(s, _)| *s == seq).map(|(_, s)| s.as_str()).unwrap();
    assert_eq!(state(seqs[0]), "offline", "25 hours old");
    // Oldest first: the first 20 of the rest, not the last 3.
    let rest: Vec<i64> = seqs[1..].to_vec();
    assert!(rest[..REDELIVER_MAX].iter().all(|s| state(*s) == "delivered"), "{got:?}");
    assert!(rest[REDELIVER_MAX..].iter().all(|s| state(*s) == "offline"), "{got:?}");

    came(&app, &hub, before + rest.len()).await;
    assert!(
        app.db
            .chat_mention_statuses("ch_general", seqs[1], seqs[1])
            .unwrap()
            .iter()
            .any(|s| s.machine == "other" && s.state == "offline")
    );
}

/// A mention left `sending` by a restart is sent at the next connect; one
/// that is only just being sent isn't taken from the task sending it. And
/// the sweep after a restart makes the lost ones `offline`, and sends those
/// of a machine already connected.
#[tokio::test]
async fn a_mention_lost_while_sending_is_sent_again_and_a_fresh_one_is_not() {
    let (hub, app, base, t, _u) = seeded().await;
    geek_in_t(&base, &t).await;
    let lost = post(&base, "@geek-box lost in a restart").await;
    let fresh = post(&base, "@geek-box being sent right now").await;
    settled(&app).await;
    let before = asked(&hub);
    let set = |seq: i64, age_ms: u64| {
        let s = app.db.chat_mention_statuses("ch_general", seq, seq).unwrap().remove(0);
        let at = arugula_e2e::now_ms() - age_ms;
        app.db.set_chat_mention_status(&MentionStatus { state: "sending".into(), at, ..s }).unwrap();
    };
    set(lost, 3 * 60 * 1000);
    set(fresh, 1_000);

    hub.answers_ops("geek", OpAnswer::Done { url: None, routed: None });
    // A stale one and a fresh one: the stale is sent, the fresh isn't.
    app.daemon_came("geek").await.unwrap();
    for _ in 0..200 {
        if asked(&hub) > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked(&hub), before + 1);
    let got = states(&app);
    let state = |seq: i64| got.iter().find(|(s, _)| *s == seq).map(|(_, s)| s.as_str()).unwrap();
    assert_eq!((state(lost), state(fresh)), ("delivered", "sending"), "{got:?}");

    // The sweep: lost ones go back to offline (and, geek not online in the
    // test hub, wait for its connect).
    set(lost, 3 * 60 * 1000);
    crate::chat::release_stale(&app).await;
    let got = states(&app);
    let state = |seq: i64| got.iter().find(|(s, _)| *s == seq).map(|(_, s)| s.as_str()).unwrap();
    assert_eq!((state(lost), state(fresh)), ("offline", "sending"), "{got:?}");
}
