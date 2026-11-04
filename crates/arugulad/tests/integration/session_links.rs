//! A read-only link to a session (`POST /api/links`), through to its end:
//! the owner lists and revokes it, its viewer's channel closes at once and
//! won't reopen, control hears there's nothing left to let through, and
//! nobody but the owner can do any of it.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, Welcome, prologue},
};
use arugula_testkit::{Daemon, arugulad};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Every `links_until` the machine told control, in order.
type Told = Arc<Mutex<Vec<Value>>>;

/// A control that trusts this device (the account's root) and this daemon,
/// and keeps what it's told about links.
async fn fake_control(certs: Vec<Cert>, told: Told) -> String {
    let trust = json!({ "certs": certs, "revocations": [] });
    let info = json!({ "control": true, "daemon_auth": 2 });
    let access = post(|State(told): State<Told>, Json(b): Json<Value>| async move {
        told.lock().unwrap().push(b["links_until"].clone());
        Json(json!({}))
    });
    let app = Router::new()
        .route("/control.json", get(move || async move { Json(info) }))
        .route("/api/daemon/trust", get(move || async move { Json(trust) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", access)
        .with_state(told);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

struct Rig {
    d: Daemon,
    daemon: DeviceKeys,
    told: Told,
}

/// A machine joined to control, with a session.
async fn rig() -> Rig {
    let device = DeviceKeys::generate();
    let mut root = Cert::new(&device, "a1", Kind::Browser, "laptop");
    root.sign_with(&device);
    let mut d =
        arugulad!("links").no_wisp().no_tailscale().args(["--direct-url", "http://127.0.0.1:0"]).wait_secs(30).start();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&daemon, "a1", Kind::Daemon, "test");
    cert.sign_with(&device);
    let certs = vec![root, cert.clone()];
    let told = Told::default();
    let control = fake_control(certs.clone(), told.clone()).await;
    let saved = json!({
        "url": control,
        "trust": { "account": "a1", "root": device.id() },
        "cert": cert,
        "certs": certs,
        "login": "me",
    });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.stop();
    d.start();
    d.wait_for("joined", || d.get("/api/host")["control_state"]["state"] == "joined");
    d.wait_for("a session", || d.get("/api/panes")[0]["session"].is_u64());
    Rig { d, daemon, told }
}

fn session(d: &Daemon) -> u64 {
    d.get("/api/panes")[0]["session"].as_u64().unwrap()
}

/// A link to `session` with `key`'s public half: its id, as the list has it.
fn mint(d: &Daemon, session: u64, key: &DeviceKeys) -> String {
    let made =
        d.post("/api/links", json!({ "session": session, "key": hex::encode(key.noise_public), "ttl_secs": 3600 }));
    made["link"].as_str().unwrap().strip_prefix("link:").unwrap().to_owned()
}

/// A channel at `/e2e` with this link's key: the socket, the channel, and
/// what message 2 said.
async fn open(r: &Rig, key: &DeviceKeys) -> (Ws, Channel, Vec<u8>) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/e2e", r.d.port)).await.unwrap();
    let (init, m1) = Initiator::start_with(key, &r.daemon.noise_public, &prologue(&r.daemon.id()), &[]).unwrap();
    ws.send(Message::Binary(m1.into())).await.unwrap();
    let m2 = loop {
        match tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("message 2") {
            Some(Ok(Message::Binary(b))) => break b,
            Some(Ok(_)) => continue,
            other => panic!("hung up instead of answering: {other:?}"),
        }
    };
    let (said, ch) = init.finish(&m2).unwrap();
    (ws, ch, said)
}

/// The next whole message, or `None` once the channel closed.
async fn next(ws: &mut Ws, ch: &Channel) -> Option<Msg> {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("the channel stays quiet");
        let Some(Ok(Message::Binary(w))) = m else { return None };
        if let Some(m) = ch.open(&w).unwrap() {
            return Some(m);
        }
    }
}

/// One request over the channel: its status.
async fn ask(ws: &mut Ws, ch: &Channel, id: u32, method: &str, path: &str) -> u16 {
    let head = RequestHead {
        method: method.into(),
        path: path.into(),
        content_type: None,
        stream: false,
        headers: Default::default(),
    };
    for w in ch.seal(&Msg::Request { id, head, body: vec![] }).unwrap() {
        ws.send(Message::Binary(w.into())).await.unwrap();
    }
    loop {
        match next(ws, ch).await.expect("an answer") {
            Msg::Response { id: got, head, .. } if got == id => return head.status,
            _ => continue,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_a_link_removes_it_and_closes_its_channel_at_once() {
    let r = rig().await;
    let s = session(&r.d);
    let (key, other) = (DeviceKeys::generate(), DeviceKeys::generate());
    let id = mint(&r.d, s, &key);

    // Listed for its session, with nothing that would open it.
    let listed = r.d.get(&format!("/api/links?session={s}"));
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["id"], id.as_str());
    assert_eq!(listed[0]["session"], s);
    assert!(listed[0]["expires_ms"].as_u64().unwrap() > listed[0]["made_ms"].as_u64().unwrap());
    assert!(listed[0].get("key").is_none(), "{listed}");
    assert!(!listed.to_string().contains(&hex::encode(key.noise_public)), "{listed}");
    assert_eq!(r.d.get(&format!("/api/links?session={}", s + 1000)), json!([]));
    assert_eq!(r.d.get("/api/links").as_array().unwrap().len(), 1);
    // The ACL's route doesn't take a link: this is what ends one.
    let (status, why) =
        r.d.raw("POST", "/api/acl", Some(json!({ "session": s, "principal": format!("link:{id}"), "role": null })));
    assert_eq!(status, 400, "{why}");
    assert!(why.contains("arugula links revoke"), "{why}");

    // A live viewer, who can't do any of this themselves.
    let (mut ws, ch, said) = open(&r, &key).await;
    assert!(Welcome::decode(&said).is_some(), "welcomed");
    assert_eq!(ask(&mut ws, &ch, 1, "GET", "/api/links").await, 403);
    assert_eq!(ask(&mut ws, &ch, 2, "DELETE", &format!("/api/links/{id}")).await, 403);
    assert_eq!(r.d.get("/api/links").as_array().unwrap().len(), 1);

    // Control knows there's a link to let viewers through to.
    r.d.wait_for("control told of the link", || r.told.lock().unwrap().last().is_some_and(|u| u.is_u64()));

    // Another link stays when this one goes; so does control's word.
    let other_id = mint(&r.d, s, &other);
    let (mut other_ws, other_ch, _) = open(&r, &other).await;

    let before = std::time::Instant::now();
    assert_eq!(r.d.raw("DELETE", &format!("/api/links/{id}"), None).0, 200);
    let mut told_why = false;
    while let Some(m) = next(&mut ws, &ch).await {
        if let Msg::Text(t) = m {
            told_why |= t.contains("your access was removed");
        }
    }
    assert!(before.elapsed() < Duration::from_secs(5), "closed in {:?}", before.elapsed());
    assert!(told_why, "the viewer hears why");
    let grants = r.d.get("/api/acl")["grants"].clone();
    assert!(grants.as_array().unwrap().iter().all(|g| g["principal"] != format!("link:{id}").as_str()), "{grants}");
    let listed = r.d.get("/api/links");
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["id"], other_id.as_str());
    assert_eq!(ask(&mut other_ws, &other_ch, 1, "GET", "/api/host").await, 200, "the other link's viewer stays");

    // It won't reopen, and it's gone only once.
    let (_ws, _, said) = open(&r, &key).await;
    assert_eq!(Refusal::decode(&said), Some(Refusal::unknown_device()));
    assert_eq!(r.d.raw("DELETE", &format!("/api/links/{id}"), None).0, 404);

    // With the last one gone, control hears there's nothing to let through.
    assert_eq!(r.d.raw("DELETE", &format!("/api/links/{other_id}"), None).0, 200);
    while next(&mut other_ws, &other_ch).await.is_some() {}
    r.d.wait_for("control told there's nothing left", || r.told.lock().unwrap().last().is_some_and(Value::is_null));
    assert_eq!(r.d.get("/api/links"), json!([]));

    // The log says so.
    let audit = r.d.get("/api/acl")["audit"].clone();
    let lines: Vec<&str> = audit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["principal"] == format!("link:{id}").as_str())
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(lines, ["link", "revoke"]);
}

/// Only the machine's owner: someone a session is shared with, whatever
/// their role, neither reads a session's links nor ends one.
#[test]
fn only_the_owner_lists_and_revokes_links() {
    const FRIEND: &str = "friend@example.com";
    let d = arugulad!("links-owner")
        .args(["--owner", "me@example.com", "--tailscale-socket", "/nonexistent/sock"])
        .wait_secs(10)
        .start();
    d.wait_for("a session", || d.get("/api/panes")[0]["session"].is_u64());
    let s = session(&d);
    let id = mint(&d, s, &DeviceKeys::generate());
    for role in ["viewer", "editor", "owner"] {
        d.post("/api/acl", json!({ "session": s, "principal": format!("tailnet:{FRIEND}"), "role": role }));
        for (method, path) in [
            ("GET", "/api/links".to_owned()),
            ("GET", format!("/api/links?session={s}")),
            ("DELETE", format!("/api/links/{id}")),
        ] {
            let (status, _, why) = d.tcp(method, &path, &[("tailscale-user-login", FRIEND)], None);
            assert_eq!(status, 403, "{role}: {method} {path}: {why}");
        }
    }
    assert_eq!(d.get("/api/links").as_array().unwrap().len(), 1, "still there");
    // The owner over the tailnet, as on the socket.
    let (status, _, why) =
        d.tcp("DELETE", &format!("/api/links/{id}"), &[("tailscale-user-login", "me@example.com")], None);
    assert_eq!(status, 200, "{why}");
    assert_eq!(d.get("/api/links"), json!([]));
}
