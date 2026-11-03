//! #551: a mention that no session takes raises a card that belongs to no
//! pane, and it reaches the owner's client over the wire: in the `State` of a
//! summary connection (the swarm's and the phone's), then in a delta. The
//! owner dismissing it clears it, and the client sees that too (the inbox
//! read, `read_thread`, is `agent_mentions`' unit test). Against a fake
//! control (its relay socket sends the `agents.mention` op) and a real
//! daemon.
// `Daemon::stop` is Unix only.
#![cfg(unix)]

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_e2e::{Cert, DeviceKeys, Kind};
use arugula_proto::{ClientMsg, Notice};
use arugula_testkit::arugulad;
use axum::{
    Json, Router,
    extract::{
        State as AxState,
        ws::{Message, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite};

#[derive(Clone, Default)]
struct Fake {
    /// What the daemon said on its relay socket.
    said: Arc<Mutex<Vec<Value>>>,
    /// Asks for the relay socket to send.
    outbox: Arc<Mutex<Vec<String>>>,
}

async fn dial(AxState(f): AxState<Fake>, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(|ws| async move {
        let (mut tx, mut rx) = ws.split();
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                m = rx.next() => {
                    let Some(Ok(Message::Text(t))) = m else { if m.is_none() { break } else { continue } };
                    if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                        f.said.lock().unwrap().push(v);
                    }
                }
                _ = tick.tick() => {
                    let out: Vec<String> = std::mem::take(&mut *f.outbox.lock().unwrap());
                    for o in out {
                        let _ = tx.send(Message::Text(o.into())).await;
                    }
                }
            }
        }
    })
}

fn serve(rt: &tokio::runtime::Runtime, f: Fake) -> String {
    rt.block_on(async {
        let app = Router::new()
            .route(
                "/control.json",
                get(|| async { Json(json!({ "control": true, "daemon_auth": 2, "url": "http://127.0.0.1" })) }),
            )
            .route("/api/daemon/trust", get(|| async { Json(json!({ "certs": [], "revocations": [] })) }))
            .route("/api/daemon/peers", get(|| async { Json(json!({ "a1": { "name": "jake" } })) }))
            .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
            .route("/api/relay/dial", get(dial))
            .with_state(f);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    })
}

fn enroll(state: &std::path::Path, control: &str) {
    let keys = DeviceKeys::load_or_create(&state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&keys, "a1", Kind::Daemon, "test");
    cert.sign_with(&keys);
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": keys.id() }, "cert": cert, "login": "jake" });
    std::fs::write(state.join("control.json"), saved.to_string()).unwrap();
}

/// `@machine` in room `ch_x`, no session named: nothing took it.
fn mention(id: &str, seq: i64, text: &str) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "ch_x", "room_name": "agents",
        "seq": seq, "author": "Ada", "text": text } })
    .to_string()
}

fn answer(f: &Fake, id: &str) -> Option<Value> {
    f.said.lock().unwrap().iter().find(|v| v["t"] == "op.done" && v["id"] == id).map(|v| v["answer"].clone())
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The notices out of a `State` or a `Delta` (a summary connection's panes
/// leave fields out, so these are read as JSON): `None` when it has none to
/// say, as a delta that changed nothing about them.
fn notices_of(v: &Value) -> Option<Vec<Notice>> {
    let body = match v["type"].as_str()? {
        "hello" | "state" => &v["state"],
        "delta" => &v["delta"],
        _ => return None,
    };
    match body.get("notices") {
        Some(n) => Some(serde_json::from_value(n.clone()).unwrap()),
        // A State leaves out an empty list; a Delta says an empty one.
        None if v["type"] != "delta" => Some(vec![]),
        None => None,
    }
}

/// Messages until `ok` holds of the client's notices, applying each `State`
/// and `Delta` as the web client does.
async fn until_notices(ws: &mut Ws, held: &mut Vec<Notice>, what: &str, ok: impl Fn(&[Notice]) -> bool) {
    let until = Instant::now() + Duration::from_secs(15);
    while !ok(held) {
        let left = until.saturating_duration_since(Instant::now());
        let Ok(Some(Ok(tungstenite::Message::Text(t)))) = tokio::time::timeout(left, ws.next()).await else {
            panic!("timed out waiting for {what}; the client had {held:?}");
        };
        if let Some(n) = notices_of(&serde_json::from_str(&t).unwrap()) {
            *held = n;
        }
    }
}

/// A summary connection (the swarm's, the phone's) and the notices in its
/// first `State`.
async fn summary(d: &arugula_testkit::Daemon) -> (Ws, Vec<Notice>) {
    let (mut ws, _) = connect_async(d.ws("/ws")).await.unwrap();
    let sub = serde_json::to_string(&ClientMsg::Subscribe { summary: true }).unwrap();
    ws.send(tungstenite::Message::Text(sub.into())).await.unwrap();
    // Its first State is its own, whatever was sent before it.
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let Ok(Some(Ok(tungstenite::Message::Text(t)))) = tokio::time::timeout(left, ws.next()).await else {
            panic!("no state");
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "state" {
            return (ws, notices_of(&v).unwrap());
        }
    }
}

#[test]
fn an_inbox_mention_reaches_the_owners_swarm_over_the_wire_and_dismissing_it_clears_it() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let mut d = arugulad!("mentioncard").no_wisp().no_tailscale().wait_secs(30).start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));

    rt.block_on(async {
        // The swarm's (summary) connection is open before the mention comes.
        let (mut ws, mut held) = summary(&d).await;
        assert!(held.is_empty());

        f.outbox.lock().unwrap().push(mention("m1", 7, "\nare you there?\nsecond line"));
        let until = Instant::now() + Duration::from_secs(15);
        while answer(&f, "m1").is_none() {
            assert!(Instant::now() < until, "no answer");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(answer(&f, "m1").unwrap()["routed"]["step"], "inbox");
        until_notices(&mut ws, &mut held, "the card", |n| n.len() == 1).await;
        assert_eq!(held[0].key, "mention:ch_x");
        assert_eq!(held[0].headline, "Ada mentioned this machine in #agents: are you there?");
        assert_eq!((held[0].count, held[0].room.as_deref()), (1, Some("ch_x")));

        // A second mention in the room is the same card, counted.
        f.outbox.lock().unwrap().push(mention("m2", 8, "again"));
        until_notices(&mut ws, &mut held, "the count", |n| n.first().is_some_and(|n| n.count == 2)).await;
        assert_eq!(held.len(), 1);

        // A client that connects later has it in its first State.
        let (_late, first) = summary(&d).await;
        assert_eq!(first.len(), 1);

        // The owner dismisses it: it is gone, over the wire, for every client.
        let msg = serde_json::to_string(&ClientMsg::DismissNotice { key: "mention:ch_x".into() }).unwrap();
        ws.send(tungstenite::Message::Text(msg.into())).await.unwrap();
        until_notices(&mut ws, &mut held, "the dismissal", |n| n.is_empty()).await;
        let (_again, first) = summary(&d).await;
        assert!(first.is_empty());

        // Mentioned again, it comes back.
        f.outbox.lock().unwrap().push(mention("m3", 9, "third"));
        until_notices(&mut ws, &mut held, "the card again", |n| n.len() == 1).await;
        assert_eq!(held[0].count, 1, "a dismissed card starts over");
    });
    drop(d);
}
