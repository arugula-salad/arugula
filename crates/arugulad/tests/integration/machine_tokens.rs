//! Machine tokens (aru-141): a channel let in by control's token instead
//! of a device key. The role in the token decides what it may do; a token
//! for another machine, from another key, out of date or used twice is
//! refused; remote input off here makes everyone a viewer and closes a
//! channel that could type; and each channel is in the remote sessions log.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::Duration;

use arugula_control_wire::machine_token::{TokenHello, USE};
use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, Welcome, prologue},
};
use arugula_testkit::{Daemon, arugulad};
use axum::{Json, Router, routing::get};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// The issuer's key in these tests.
fn issuer() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}

/// A control that publishes the issuer's key and trusts this daemon.
async fn fake_control(certs: Vec<Cert>) -> String {
    let trust = json!({ "certs": certs, "revocations": [] });
    let jwks = json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1", "alg": "EdDSA", "use": "sig",
        "x": b64(issuer().verifying_key().as_bytes()),
    }]});
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "control": true, "daemon_auth": 2 })) }))
        .route("/.well-known/jwks.json", get(move || async move { Json(jwks) }))
        .route("/api/daemon/trust", get(move || async move { Json(trust) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", axum::routing::post(|| async { Json(json!({})) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

struct Rig {
    d: Daemon,
    daemon: DeviceKeys,
    control: String,
}

/// A machine joined to a control that publishes the issuer's key.
async fn rig() -> Rig {
    let device = DeviceKeys::generate();
    let mut root = Cert::new(&device, "a1", Kind::Browser, "laptop");
    root.sign_with(&device);
    let mut d = arugulad!("tokens")
        .no_provider()
        .no_tailscale()
        .args(["--direct-url", "http://127.0.0.1:0"])
        .wait_secs(30)
        .start();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&daemon, "a1", Kind::Daemon, "test");
    cert.sign_with(&device);
    let certs = vec![root, cert.clone()];
    let control = fake_control(certs.clone()).await;
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
    Rig { d, daemon, control }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A token as the hub signs one, with `edit` applied to its claims.
fn token(r: &Rig, role: &str, jti: &str, key: &SigningKey, edit: impl FnOnce(&mut Value)) -> String {
    let mut claims = json!({
        "iss": r.control, "sub": "acct-b", "aud": r.daemon.id(), "use": USE, "role": role,
        "name": "bee", "iat": now(), "exp": now() + 300, "jti": jti,
    });
    edit(&mut claims);
    let h = b64(&serde_json::to_vec(&json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" })).unwrap());
    let p = b64(&serde_json::to_vec(&claims).unwrap());
    let s = b64(&key.sign(format!("{h}.{p}").as_bytes()).to_bytes());
    format!("{h}.{p}.{s}")
}

/// A channel at `/e2e` presenting `token`, with a key made for it: the
/// socket, the channel, and what message 2 said.
async fn open(r: &Rig, token: String) -> (Ws, Channel, Vec<u8>) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/e2e", r.d.port)).await.unwrap();
    let key = DeviceKeys::generate();
    let hello = TokenHello { machine_token: token }.encode();
    let (init, m1) = Initiator::start_with(&key, &r.daemon.noise_public, &prologue(&r.daemon.id()), &hello).unwrap();
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

/// What message 2 refused, or None for a welcome.
fn refused(said: &[u8]) -> Option<Refusal> {
    match Refusal::decode(said) {
        Some(r) => Some(r),
        None => {
            assert!(serde_json::from_slice::<Welcome>(said).is_ok(), "neither a welcome nor a refusal");
            None
        }
    }
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
async fn a_token_lets_in_its_role_and_nothing_more() {
    let r = rig().await;
    let k = issuer();
    // The owner lists the read-only links, which only an owner may.
    let (mut ws, ch, said) = open(&r, token(&r, "owner", "t1", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    assert_eq!(ask(&mut ws, &ch, 1, "GET", "/api/links").await, 200);
    // A viewer gets in and can't.
    let (mut ws, ch, said) = open(&r, token(&r, "viewer", "t2", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    assert_eq!(ask(&mut ws, &ch, 1, "GET", "/api/links").await, 403);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_token_is_refused_and_says_why() {
    let r = rig().await;
    let k = issuer();
    let cases: Vec<(&str, String)> = vec![
        ("another machine", token(&r, "owner", "b1", &k, |c| c["aud"] = json!("someone-else"))),
        ("another key", token(&r, "owner", "b2", &SigningKey::from_bytes(&[9; 32]), |_| {})),
        ("another issuer", token(&r, "owner", "b3", &k, |c| c["iss"] = json!("https://elsewhere.example"))),
        (
            "out of date",
            token(&r, "owner", "b4", &k, |c| {
                c["iat"] = json!(now() - 900);
                c["exp"] = json!(now() - 600);
            }),
        ),
        (
            "an ID token",
            token(&r, "owner", "b5", &k, |c| {
                c.as_object_mut().unwrap().remove("use");
            }),
        ),
    ];
    for (what, t) in cases {
        let (_ws, _ch, said) = open(&r, t).await;
        let no = refused(&said).unwrap_or_else(|| panic!("{what} was let in"));
        assert_eq!(no.refused, "machine-token", "{what}");
    }
    // A good token works once.
    let t = token(&r, "editor", "once", &k, |_| {});
    let (_ws, _ch, said) = open(&r, t.clone()).await;
    assert!(refused(&said).is_none());
    let (_ws, _ch, said) = open(&r, t).await;
    assert_eq!(refused(&said).expect("a replay").message, "already used");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_input_off_makes_everyone_a_viewer() {
    let r = rig().await;
    let k = issuer();
    let (mut open_ws, open_ch, _) = open(&r, token(&r, "owner", "r1", &k, |_| {})).await;
    assert_eq!(ask(&mut open_ws, &open_ch, 1, "GET", "/api/links").await, 200);
    std::fs::write(r.d.state.join("sharing.json"), r#"{ "remote_input": false }"#).unwrap();
    // The owner's open channel closes within a few seconds.
    let closed =
        tokio::time::timeout(Duration::from_secs(8), async { while next(&mut open_ws, &open_ch).await.is_some() {} })
            .await;
    assert!(closed.is_ok(), "a channel that could type stayed open with remote input off");
    // A new one is a viewer's.
    let (mut ws, ch, said) = open(&r, token(&r, "owner", "r2", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    assert_eq!(ask(&mut ws, &ch, 1, "GET", "/api/links").await, 403);
    drop(ws);
    // Both are in the log, opened and closed.
    r.d.wait_for("the log's closes", || {
        let log = std::fs::read_to_string(r.d.state.join("remote-sessions.jsonl")).unwrap_or_default();
        let lines: Vec<Value> = log.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let opened: Vec<&str> =
            lines.iter().filter(|l| l["event"] == "open").filter_map(|l| l["role"].as_str()).collect();
        let closes = lines.iter().filter(|l| l["event"] == "close").count();
        opened == ["owner", "viewer"]
            && closes == 2
            && lines.iter().all(|l| l["account"] == "acct-b" && l["by"] == "token")
    });
}
