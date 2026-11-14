//! Joining on control's word (aru-141): `arugulad join --token` against
//! control's `/api/join/token`, and a join a signed-in person adds with one
//! click from Getting started. Either saves a join that pins no root: a
//! channel with control's machine token gets in, a device's doesn't, and
//! the machine never asks control for rosters or peers. It asks
//! `/api/daemon/trust` only for control's Labs flags and to say its
//! version (#1016), passing over a control that doesn't answer that yet.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_wire::machine_token::{TokenHello, USE};
use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    cert::join_code,
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, Welcome, prologue},
};
use arugula_testkit::{Daemon, arugulad};
use axum::{
    Json, Router,
    extract::{RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const GOOD: &str = "ajt_good";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// The issuer's key in these tests.
fn issuer() -> SigningKey {
    SigningKey::from_bytes(&[43; 32])
}

/// What the fake control saw.
#[derive(Default)]
struct Seen {
    /// The certificate a join asked with.
    asked: Option<Cert>,
    /// The join token was taken.
    used: bool,
    /// Asked for a roster or peers: never, joined this way.
    pinned_asks: Vec<String>,
    /// How `/api/daemon/trust` answers: none is a control from before
    /// #1016 (a 404); else this status and body.
    trust: Option<(StatusCode, Value)>,
    /// The query each `/api/daemon/trust` came with.
    trust_asks: Vec<String>,
    /// `/api/daemon/push-subs` asks: the refreshes.
    refreshes: usize,
}

type Fake = Arc<Mutex<Seen>>;

fn joined(c: &Cert) -> Value {
    json!({ "account": "a1", "login": "sam", "machine": c.device, "team": null })
}

/// A control with join tokens and one-click approval, as the hub has them.
async fn fake_control(fake: Fake) -> String {
    let jwks = json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1", "alg": "EdDSA", "use": "sig",
        "x": b64(issuer().verifying_key().as_bytes()),
    }]});
    let token = post(|State(f): State<Fake>, Json(b): Json<Value>| async move {
        let mut f = f.lock().unwrap();
        if b["join_token"] != GOOD || f.used {
            return (StatusCode::GONE, Json(json!({ "error": "that join token was used or has expired" })))
                .into_response();
        }
        let c: Cert = serde_json::from_value(b["cert"].clone()).unwrap();
        assert!(b["proof"]["sig"].is_string(), "{b}");
        assert_eq!(c.kind, Kind::Daemon);
        f.used = true;
        f.asked = Some(c.clone());
        Json(joined(&c)).into_response()
    });
    let start = post(|State(f): State<Fake>, Json(b): Json<Value>| async move {
        let c: Cert = serde_json::from_value(b["cert"].clone()).unwrap();
        let code = join_code(&c);
        f.lock().unwrap().asked = Some(c);
        Json(json!({ "code": code, "poll": "p", "expires_in_secs": 120 }))
    });
    // Added by a signed-in person: `approved` and `joined`, nothing else.
    let poll = get(|State(f): State<Fake>| async move {
        let c = f.lock().unwrap().asked.clone().unwrap();
        Json(json!({ "approved": true, "joined": joined(&c) }))
    });
    let pinned = |what: &'static str| {
        get(move |State(f): State<Fake>| async move {
            f.lock().unwrap().pinned_asks.push(what.into());
            StatusCode::INTERNAL_SERVER_ERROR
        })
    };
    let trust = get(|State(f): State<Fake>, RawQuery(q): RawQuery| async move {
        let mut f = f.lock().unwrap();
        f.trust_asks.push(q.unwrap_or_default());
        match f.trust.clone() {
            Some((status, body)) => (status, Json(body)).into_response(),
            None => (StatusCode::NOT_FOUND, Json(json!({ "error": "no such route" }))).into_response(),
        }
    });
    let subs = get(|State(f): State<Fake>| async move {
        f.lock().unwrap().refreshes += 1;
        Json(json!({ "subs": [] }))
    });
    let app = Router::new()
        .route("/control.json", get(|| async { Json(json!({ "control": true, "daemon_auth": 2 })) }))
        .route("/.well-known/jwks.json", get(move || async move { Json(jwks) }))
        .route("/api/join/token", token)
        .route("/api/join", start)
        .route("/api/join/{code}", poll)
        .route("/api/daemon/trust", trust)
        .route("/api/daemon/team", pinned("team"))
        .route("/api/daemon/teams", pinned("teams"))
        .route("/api/daemon/peers", pinned("peers"))
        .route("/api/daemon/push-subs", subs)
        .route("/api/daemon/access", post(|| async { Json(json!({})) }))
        .with_state(fake);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

fn daemon(tag: &str, control: &str, args: &[&str]) -> Daemon {
    let d = arugulad!(tag)
        .no_provider()
        .no_tailscale()
        .args(["--direct-url", "http://127.0.0.1:0", "--control", control])
        .args(args)
        .wait_secs(30)
        .start();
    d.wait_for("a session", || d.get("/api/panes")[0]["session"].is_u64());
    d
}

/// `arugulad join URL --token T`, with the token in the environment as a
/// script would give it: whether it worked, and what it said.
fn join_with(d: &Daemon, url: &str, token: &str) -> (bool, String) {
    let out = arugula_testkit::command(env!("CARGO_BIN_EXE_arugulad"))
        .args(["join", url, "--name", "box", "--state-dir"])
        .arg(&d.state)
        .env("ARUGULA_JOIN_TOKEN", token)
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), said)
}

/// The joined file pins no root: an older daemon, which requires `trust`,
/// refuses it.
fn saved_on_controls_word(d: &Daemon) -> Value {
    let saved: Value = serde_json::from_slice(&std::fs::read(d.state.join("control.json")).unwrap()).unwrap();
    assert!(saved.get("trust").is_none(), "{saved}");
    assert!(saved["by_control"]["at"].is_u64(), "{saved}");
    assert_eq!(saved["cert"]["account"], "a1");
    assert_eq!(saved["login"], "sam");
    saved
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A machine token as the hub signs one.
fn token(control: &str, machine: &str, role: &str, jti: &str) -> String {
    let claims = json!({
        "iss": control, "sub": "a1", "aud": machine, "use": USE, "role": role,
        "name": "sam", "iat": now(), "exp": now() + 300, "jti": jti,
    });
    let h = b64(&serde_json::to_vec(&json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" })).unwrap());
    let p = b64(&serde_json::to_vec(&claims).unwrap());
    let s = b64(&issuer().sign(format!("{h}.{p}").as_bytes()).to_bytes());
    format!("{h}.{p}.{s}")
}

/// A channel at `/e2e` from `key`, with `hello` in message 1: the socket,
/// the channel, and what message 2 said.
async fn open(d: &Daemon, daemon: &DeviceKeys, key: &DeviceKeys, hello: &[u8]) -> (Ws, Channel, Vec<u8>) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/e2e", d.port)).await.unwrap();
    let (init, m1) = Initiator::start_with(key, &daemon.noise_public, &prologue(&daemon.id()), hello).unwrap();
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
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("an answer");
        let Some(Ok(Message::Binary(w))) = m else { panic!("closed before answering") };
        if let Some(Msg::Response { id: got, head, .. }) = ch.open(&w).unwrap()
            && got == id
        {
            return head.status;
        }
    }
}

/// `arugulad join --token`: a bad token saves nothing; a good one joins at
/// once with no root. Then a machine token's channel gets in and a device
/// of the very account it joined doesn't; sharing with an account needs no
/// root; and the machine's refreshes ask control for no certificates.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_join_token_lets_in_tokens_and_no_devices() {
    let fake = Fake::default();
    let control = fake_control(fake.clone()).await;
    let d = daemon("jointoken", &control, &[]);

    let (ok, said) = join_with(&d, &control, "ajt_wrong");
    assert!(!ok && said.contains("join token was used already, has expired"), "{said}");
    assert!(!d.state.join("control.json").exists(), "nothing saved");

    let (ok, said) = join_with(&d, &control, GOOD);
    assert!(ok, "{said}");
    assert!(said.contains("Joined. This machine (box) is in sam's account"), "{said}");
    let saved = saved_on_controls_word(&d);
    let daemon_keys = DeviceKeys::load(&d.state.join("daemon.key")).unwrap();
    assert_eq!(saved["cert"]["device"], daemon_keys.id().as_str());
    assert_eq!(fake.lock().unwrap().asked.as_ref().unwrap().device, daemon_keys.id());
    d.wait_for("joined", || d.get("/api/host")["control_state"]["state"] == "joined");
    assert_eq!(d.get("/api/host")["control_state"]["name"], "sam");

    // Joined already: a second token says so, and nothing changes.
    let (ok, said) = join_with(&d, &control, GOOD);
    assert!(!ok && said.contains("already in sam's account"), "{said}");

    // A machine token gets in, as its role says.
    let hello = TokenHello { machine_token: token(&control, &daemon_keys.id(), "owner", "j1") }.encode();
    let (mut ws, ch, said) = open(&d, &daemon_keys, &DeviceKeys::generate(), &hello).await;
    assert!(Welcome::decode(&said).is_some(), "the token's channel is welcomed");
    assert_eq!(ask(&mut ws, &ch, 1, "GET", "/api/links").await, 200);

    // A device's channel, with no token, doesn't: there's no root here
    // for its certificate to chain to.
    let (_ws, _ch, said) = open(&d, &daemon_keys, &DeviceKeys::generate(), &[]).await;
    assert_eq!(Refusal::decode(&said), Some(Refusal::unknown_device()));

    // Sharing a session with an account needs no root device here.
    let s = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    let (status, why) =
        d.raw("POST", "/api/acl", Some(json!({ "session": s, "principal": "account:b1", "role": "viewer" })));
    assert_eq!(status, 200, "{why}");

    // Refreshes come round (push subscriptions), and never ask for what
    // only a pinned root checks.
    d.wait_for("a refresh", || fake.lock().unwrap().refreshes > 0);
    assert_eq!(fake.lock().unwrap().pinned_asks, Vec::<String>::new());
}

/// Getting started's Cloud step (#1003): a signed-in person adds the
/// machine with one click, the poll says where it went, and the daemon
/// saves it at once, with no fingerprint to confirm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_click_approval_joins_with_nothing_to_confirm() {
    let fake = Fake::default();
    let control = fake_control(fake.clone()).await;
    let d = daemon("joinclick", &control, &[]);

    let started = d.post("/api/setup/control", json!({ "url": control }));
    assert!(started["pending"]["approve"].as_str().is_some_and(|a| a.starts_with(&control)), "{started}");
    d.wait_for("joined", || d.get("/api/host")["control_state"]["state"] == "joined");
    let setup = d.get("/api/setup?part=control")["control"].clone();
    assert_eq!(setup["joined"], control.as_str(), "{setup}");
    assert!(setup.get("confirm").is_none() && setup.get("error").is_none(), "{setup}");
    saved_on_controls_word(&d);

    let daemon_keys = DeviceKeys::load(&d.state.join("daemon.key")).unwrap();
    let hello = TokenHello { machine_token: token(&control, &daemon_keys.id(), "viewer", "c1") }.encode();
    let (_ws, _ch, said) = open(&d, &daemon_keys, &DeviceKeys::generate(), &hello).await;
    assert!(Welcome::decode(&said).is_some(), "the token's channel is welcomed");
    d.wait_for("a refresh", || fake.lock().unwrap().refreshes > 0);
    assert_eq!(fake.lock().unwrap().pinned_asks, Vec::<String>::new());
}

/// #1016: joined on control's word, a refresh still says the machine's
/// version, OS and arch and takes control's Labs flags (chat's on/off with
/// them) from `/api/daemon/trust`, and nothing else of the answer. A
/// control that doesn't answer that yet (a 404) is passed over: still
/// joined, not dropped. A 410 there is a removal, as on any machine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_joined_machine_still_takes_controls_flags() {
    let fake = Fake::default();
    let control = fake_control(fake.clone()).await;
    // No relay: it looks at control every ten seconds.
    let d = daemon("joinflags", &control, &["--no-relay"]);
    let (ok, said) = join_with(&d, &control, GOOD);
    assert!(ok, "{said}");
    let state = || d.get("/api/host")["control_state"].clone();
    d.wait_for("joined", || state()["state"] == "joined");

    // A control from before: no answer there, and the rest goes on.
    d.wait_for("a refresh", || fake.lock().unwrap().refreshes > 0);
    let asked = fake.lock().unwrap().trust_asks.clone();
    assert!(
        asked.iter().any(|q| q.contains("version=") && q.contains(&format!("os={}", std::env::consts::OS))),
        "{asked:?}"
    );
    assert_eq!(state()["state"], "joined", "{}", state());
    assert!(!d.state.join("flags.control.json").exists());

    // Control answers, with flags and (ignored) certificates of its own.
    let (stranger, mut cert) = {
        let k = DeviceKeys::generate();
        let c = Cert::new(&k, "a1", Kind::Browser, "theirs");
        (k, c)
    };
    cert.sign_with(&stranger);
    fake.lock().unwrap().trust = Some((
        StatusCode::OK,
        json!({ "certs": [cert], "revocations": [], "flags": { "account": { "studio": true }, "team": { "chat": true } } }),
    ));
    d.wait_for("control's flags", || {
        let f: Value = std::fs::read(d.state.join("flags.control.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        f["team"]["chat"] == true && f["account"]["studio"] == true
    });
    let saved: Value = serde_json::from_slice(&std::fs::read(d.state.join("control.json")).unwrap()).unwrap();
    assert!(saved.get("certs").is_none_or(|c| c.as_array().is_some_and(Vec::is_empty)), "{saved}");
    let (_ws, _ch, said) = open(&d, &DeviceKeys::load(&d.state.join("daemon.key")).unwrap(), &stranger, &[]).await;
    assert_eq!(Refusal::decode(&said), Some(Refusal::unknown_device()), "a certificate in the answer lets nobody in");
    assert_eq!(fake.lock().unwrap().pinned_asks, Vec::<String>::new());

    // Removed: dropped, as any machine would be.
    fake.lock().unwrap().trust =
        Some((StatusCode::GONE, json!({ "error": "this machine's key was removed", "at": 1, "by": "laptop" })));
    d.wait_for("dropped", || state()["state"] == "dropped");
}
