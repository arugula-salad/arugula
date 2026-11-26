//! Machine tokens (aru-141): a channel let in by control's token instead
//! of a device key. The role in the token decides what it may do; a token
//! for another machine, from another key, out of date or used twice is
//! refused; remote input off here makes everyone a viewer and closes a
//! channel that could type; and each channel is in the remote sessions log,
//! which only the owner reads back (Arugula #1094).

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
    /// The account's root and its certificates, the daemon's among them.
    root: String,
    certs: Vec<Cert>,
}

/// A machine joined to a control that publishes the issuer's key.
async fn rig() -> Rig {
    rig_with(None).await
}

/// [`rig`], with `cli` one of the account's devices too (a CLI logged in
/// before machine tokens).
async fn rig_with(cli: Option<&DeviceKeys>) -> Rig {
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
    let mut certs = vec![root, cert.clone()];
    if let Some(k) = cli {
        let mut c = Cert::new(k, "a1", Kind::Cli, "cli");
        c.sign_with(&device);
        certs.push(c);
    }
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
    Rig { d, daemon, control, root: device.id(), certs }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A token as the hub signs one, with `edit` applied to its claims.
fn token(r: &Rig, role: &str, jti: &str, key: &SigningKey, edit: impl FnOnce(&mut Value)) -> String {
    signed(&r.control, &r.daemon.id(), role, jti, key, edit)
}

/// A token from the hub at `iss` for machine `aud`.
fn signed(iss: &str, aud: &str, role: &str, jti: &str, key: &SigningKey, edit: impl FnOnce(&mut Value)) -> String {
    let mut claims = json!({
        "iss": iss, "sub": "acct-b", "aud": aud, "use": USE, "role": role,
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
    ask_json(ws, ch, id, method, path).await.0
}

/// One request over the channel: its status and its body, as JSON (null
/// when it isn't).
async fn ask_json(ws: &mut Ws, ch: &Channel, id: u32, method: &str, path: &str) -> (u16, Value) {
    send_json(ws, ch, id, method, path, None).await
}

/// [`ask_json`], with a JSON body.
async fn send_json(ws: &mut Ws, ch: &Channel, id: u32, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let head = RequestHead {
        method: method.into(),
        path: path.into(),
        content_type: body.as_ref().map(|_| "application/json".into()),
        stream: false,
        headers: Default::default(),
    };
    let body = body.map(|b| serde_json::to_vec(&b).unwrap()).unwrap_or_default();
    for w in ch.seal(&Msg::Request { id, head, body }).unwrap() {
        ws.send(Message::Binary(w.into())).await.unwrap();
    }
    let mut body = Vec::new();
    loop {
        match next(ws, ch).await.expect("an answer") {
            Msg::Response { id: got, head, body: part } if got == id => {
                body.extend(part);
                if !head.more {
                    return (head.status, serde_json::from_slice(&body).unwrap_or(Value::Null));
                }
            }
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

/// Arugula #1002: another machine's token reaches the agents here and
/// nothing else, as a daemon's device key did (#399); `drives` is that too
/// on a machine that isn't hosted. Its calls name the machine that asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_machines_token_reaches_only_the_agents() {
    let r = rig().await;
    let k = issuer();
    for (i, role) in ["agents", "drives"].into_iter().enumerate() {
        let jti = format!("m{i}");
        let (mut ws, ch, said) = open(&r, token(&r, role, &jti, &k, |c| c["from"] = json!("caller-machine"))).await;
        assert!(refused(&said).is_none(), "{role} wasn't let in");
        for (id, path) in [(1, "/api/links"), (2, "/api/panes"), (3, "/api/host")] {
            let (status, body) = ask_json(&mut ws, &ch, id, "GET", path).await;
            assert_eq!(status, 403, "{role} reached {path}");
            assert!(body["error"].as_str().unwrap_or_default().contains("/api/a2a/"), "{body}");
        }
        // The agent catalog answers (off in Labs here: its own 404, not
        // the channel's refusal).
        let (status, body) = ask_json(&mut ws, &ch, 4, "GET", "/api/a2a/agents").await;
        assert_ne!(status, 403, "{role} didn't reach the agents: {body}");
    }
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
async fn a_token_taken_before_a_restart_is_refused_after_it() {
    let mut r = rig().await;
    let k = issuer();
    let t = token(&r, "editor", "before", &k, |_| {});
    let (_ws, _ch, said) = open(&r, t.clone()).await;
    assert!(refused(&said).is_none());
    // `iat` is whole seconds: let the next one start in a later second.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    r.d.stop();
    r.d.start();
    r.d.wait_for("joined", || r.d.get("/api/host")["control_state"]["state"] == "joined");
    // The new daemon never saw it taken, and refuses it anyway.
    let (_ws, _ch, said) = open(&r, t).await;
    let no = refused(&said).expect("a token from before the restart");
    assert_eq!(no.refused, "machine-token");
    assert!(no.message.contains("before this machine started"), "{}", no.message);
    // A new one gets in.
    let (_ws, _ch, said) = open(&r, token(&r, "editor", "after", &k, |_| {})).await;
    assert!(refused(&said).is_none());
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
    // Arugula #1210: the owner still watches their own sessions (this
    // machine is in no team, and nothing is shared), and types into none.
    let pane = r.d.get("/api/panes")[0]["id"].as_u64().unwrap();
    assert_eq!(ask(&mut ws, &ch, 2, "GET", &format!("/api/panes/{pane}/capture")).await, 200);
    let (status, body) =
        send_json(&mut ws, &ch, 3, "POST", &format!("/api/panes/{pane}/send"), Some(json!({ "text": "echo hi" })))
            .await;
    assert_eq!(status, 403, "the owner typed with remote input off: {body}");
    drop(ws);
    // Both are in the log, opened and closed.
    r.d.wait_for("the log's closes", || {
        let log = std::fs::read_to_string(r.d.state.join("remote-sessions.jsonl")).unwrap_or_default();
        let lines: Vec<Value> = log
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|l| l["event"] != "start")
            .collect();
        let opened: Vec<&str> =
            lines.iter().filter(|l| l["event"] == "open").filter_map(|l| l["role"].as_str()).collect();
        let closes = lines.iter().filter(|l| l["event"] == "close").count();
        opened == ["owner", "viewer"]
            && closes == 2
            && lines.iter().all(|l| l["account"] == "acct-b" && l["by"] == "token")
    });
}

/// Arugula #1194: the machine menu reads whether people may type here from
/// `/api/host`, through control too; and the answer Getting started sends
/// as a machine joins is refused through control, owner or not, leaving the
/// switch as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_input_shows_through_control_and_isnt_set_through_it() {
    let r = rig().await;
    let k = issuer();
    let (mut ws, ch, said) = open(&r, token(&r, "owner", "s1", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    let (status, host) = ask_json(&mut ws, &ch, 1, "GET", "/api/host").await;
    assert_eq!(status, 200, "{host}");
    assert_eq!(host["features"]["remote_input"], true);
    for (id, on) in [(2, false), (3, true)] {
        let body = Some(json!({ "remote_input": on }));
        let (status, v) = send_json(&mut ws, &ch, id, "POST", "/api/setup/control", body).await;
        assert_eq!(status, 403, "{v}");
        assert!(v.to_string().contains("on the machine itself"), "{v}");
    }
    assert!(!r.d.state.join("sharing.json").exists(), "nothing written");
    // Turned off on the machine: a new channel (a viewer's now) reads that.
    drop(ws);
    std::fs::write(r.d.state.join("sharing.json"), r#"{ "remote_input": false }"#).unwrap();
    let (mut ws, ch, _) = open(&r, token(&r, "owner", "s2", &k, |_| {})).await;
    let (status, host) = ask_json(&mut ws, &ch, 1, "GET", "/api/host").await;
    assert_eq!(status, 200, "{host}");
    assert_eq!(host["features"]["remote_input"], false);
    let (status, _) =
        send_json(&mut ws, &ch, 2, "POST", "/api/setup/control", Some(json!({ "remote_input": true }))).await;
    assert_eq!(status, 403);
    assert_eq!(std::fs::read_to_string(r.d.state.join("sharing.json")).unwrap(), r#"{ "remote_input": false }"#);
}

/// Remote input off holds for a session shared with someone, too: a guest
/// whose token gives nothing machine-wide types into a session shared with
/// them as an editor while it's on; with it off, their channel that could
/// type closes, and a new one only watches that session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_input_off_makes_a_shared_guest_a_viewer() {
    let r = rig().await;
    let k = issuer();
    let panes = r.d.get("/api/panes");
    let (pane, session) = (panes[0]["id"].as_u64().unwrap(), panes[0]["session"].as_u64().unwrap());
    r.d.post("/api/acl", json!({ "session": session, "principal": "account:acct-b", "role": "editor", "root": "-" }));
    let send = format!("/api/panes/{pane}/send");
    let typed = || Some(json!({ "text": "echo hi" }));
    let (mut open_ws, open_ch, said) = open(&r, token(&r, "guest", "g1", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    // Trusted with the owner's pane (M14), as an editor needs to be to type.
    // (Asked over an owner's channel: this machine's TCP port is control's.)
    let (mut owner_ws, owner_ch, _) = open(&r, token(&r, "owner", "o1", &k, |_| {})).await;
    let op =
        json!({ "type": "pane", "pane": pane, "op": { "op": "grant_trust", "to": "account:acct-b", "minutes": 30 } });
    for w in owner_ch.seal(&Msg::Text(op.to_string())).unwrap() {
        owner_ws.send(Message::Binary(w.into())).await.unwrap();
    }
    let mut id = 0;
    let typed_in = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            id += 1;
            if send_json(&mut open_ws, &open_ch, id, "POST", &send, typed()).await.0 == 200 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    assert!(typed_in.is_ok(), "the share and the trust let them type");
    std::fs::write(r.d.state.join("sharing.json"), r#"{ "remote_input": false }"#).unwrap();
    let closed =
        tokio::time::timeout(Duration::from_secs(8), async { while next(&mut open_ws, &open_ch).await.is_some() {} })
            .await;
    assert!(closed.is_ok(), "a shared guest's channel stayed open with remote input off");
    // A new one watches the shared session and can't type into it.
    let (mut ws, ch, said) = open(&r, token(&r, "guest", "g2", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    let (status, body) = send_json(&mut ws, &ch, 1, "POST", &send, typed()).await;
    assert_eq!(status, 403, "a shared guest typed with remote input off: {body}");
    assert_eq!(ask(&mut ws, &ch, 2, "GET", &format!("/api/panes/{pane}/capture")).await, 200);
    // An editor's token, with the same share, is held to a viewer too.
    let (mut ws, ch, said) = open(&r, token(&r, "editor", "e1", &k, |_| {})).await;
    assert!(refused(&said).is_none());
    let (status, body) = send_json(&mut ws, &ch, 1, "POST", &send, typed()).await;
    assert_eq!(status, 403, "a shared editor typed with remote input off: {body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_remote_sessions_log_is_the_owners_to_read() {
    let r = rig().await;
    let k = issuer();
    // A viewer and an editor come; neither may read who else did.
    let (mut viewer, vch, _) = open(&r, token(&r, "viewer", "l1", &k, |_| {})).await;
    assert_eq!(ask(&mut viewer, &vch, 1, "GET", "/api/remote-sessions").await, 403);
    let (mut editor, ech, _) = open(&r, token(&r, "editor", "l2", &k, |_| {})).await;
    assert_eq!(ask(&mut editor, &ech, 1, "GET", "/api/remote-sessions").await, 403);
    // The viewer leaves.
    drop(viewer);
    r.d.wait_for("the viewer's close", || {
        let log = std::fs::read_to_string(r.d.state.join("remote-sessions.jsonl")).unwrap_or_default();
        log.lines().any(|l| l.contains(r#""event":"close""#))
    });
    // The owner, through control, reads them back: newest first, each open
    // paired with its close, and whether they may type.
    let (mut owner, och, _) = open(&r, token(&r, "owner", "l3", &k, |_| {})).await;
    let (status, v) = ask_json(&mut owner, &och, 1, "GET", "/api/remote-sessions").await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["remote_input"], true);
    let rows: Vec<(&str, &str, &str, bool, bool)> = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["role"].as_str().unwrap(),
                s["name"].as_str().unwrap(),
                s["path"].as_str().unwrap(),
                s["open"].as_bool().unwrap(),
                s["closed_ms"].as_u64().is_some_and(|c| c >= s["opened_ms"].as_u64().unwrap()),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("owner", "bee", "direct", true, false),
            ("editor", "bee", "direct", true, false),
            ("viewer", "bee", "direct", false, true),
        ]
    );
    // The owner at the machine reads the same, and so does the CLI there.
    let here = r.d.get("/api/remote-sessions");
    assert_eq!(here["sessions"].as_array().map(Vec::len), Some(3), "{here}");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_arugulad"))
        .args(["sharing", "log", "--state-dir"])
        .arg(&r.d.state)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("bee") && lines[0].contains("owner") && lines[0].ends_with("open now"), "{text}");
    assert!(lines[2].contains("viewer") && lines[2].contains(", for "), "{text}");
    drop(editor);
}

// ---------------------------------------------------------------- the CLI

/// The CLI, built next to the daemon (cargo builds only this package's
/// binaries for its tests).
fn cli_bin() -> std::path::PathBuf {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = std::process::Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// What the CLI's control does for these tests.
struct Hub {
    /// The issuer the daemon knows (the rig's control).
    iss: String,
    machine: String,
    port: u16,
    /// The Noise key the directory gives (#994), if it gives one.
    noise: Option<String>,
    certs: Vec<Cert>,
    /// Whether it gives machine tokens (a hub without an issuer key: 404).
    tokens: bool,
    given: std::sync::atomic::AtomicUsize,
}

/// A hub for the CLI: its directory, the account's devices and machine
/// tokens, for a session's cookie or a device's signature.
async fn hub(h: Hub) -> (String, std::sync::Arc<Hub>) {
    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::post,
    };
    use std::sync::{Arc, atomic::Ordering};
    fn signed_in(h: &HeaderMap) -> bool {
        h.get("cookie").is_some_and(|c| c == "ilg_session=s3cret") || h.contains_key("x-arugula-auth")
    }
    async fn directory(State(h): State<Arc<Hub>>, hs: HeaderMap) -> Response {
        if !signed_in(&hs) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let mut m = json!({ "id": h.machine, "name": "tokbox", "online": true,
            "urls": [format!("http://127.0.0.1:{}", h.port)] });
        if let Some(n) = &h.noise {
            m["noise"] = json!(n);
        }
        Json(json!({ "daemons": [m] })).into_response()
    }
    async fn devices(State(h): State<Arc<Hub>>) -> Json<Value> {
        Json(json!({ "certs": h.certs, "revocations": [] }))
    }
    async fn token(State(h): State<Arc<Hub>>, hs: HeaderMap, Path(id): Path<String>) -> Response {
        if !signed_in(&hs) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if !h.tokens || id != h.machine {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such machine" }))).into_response();
        }
        let n = h.given.fetch_add(1, Ordering::SeqCst);
        let t = signed(&h.iss, &id, "owner", &format!("cli-{n}"), &issuer(), |_| {});
        Json(json!({ "token": t, "expires": now() + 300, "role": "owner" })).into_response()
    }
    let h = Arc::new(h);
    let app = Router::new()
        .route("/api/directory", get(directory))
        .route("/api/devices", get(devices))
        .route("/api/machines/{id}/token", post(token))
        .with_state(h.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (at, h)
}

/// `arugula --host tokbox <args>` with the config dir `config`, and no
/// local daemon: what it printed, or why it failed.
async fn cli_at(config: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let (config, args) = (config.to_owned(), args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    tokio::task::spawn_blocking(move || {
        let o = arugula_testkit::command(cli_bin())
            .arg("--socket")
            .arg(config.join("no-daemon.sock"))
            .args(["--host", "tokbox"])
            .args(&args)
            .env("XDG_CONFIG_HOME", &config)
            .env_remove("ARUGULA_PANE")
            .output()
            .unwrap();
        let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        if o.status.success() { Ok(out.into_owned()) } else { Err(format!("{out}{err}")) }
    })
    .await
    .unwrap()
}

fn config_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("arugula-cli-tokens-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("arugula")).unwrap();
    dir
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The remote sessions the daemon logged as opened: (by, role).
fn opened(r: &Rig) -> Vec<(String, String)> {
    let log = std::fs::read_to_string(r.d.state.join("remote-sessions.jsonl")).unwrap_or_default();
    log.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|l| l["event"] == "open")
        .map(|l| (l["by"].as_str().unwrap_or("").to_owned(), l["role"].as_str().unwrap_or("").to_owned()))
        .collect()
}

fn hub_for(r: &Rig, noise: Option<String>, tokens: bool) -> Hub {
    Hub {
        iss: r.control.clone(),
        machine: r.daemon.id(),
        port: r.d.port,
        noise,
        certs: r.certs.clone(),
        tokens,
        given: Default::default(),
    }
}

/// #998: a CLI signed in with a session (no key, no certificate) reaches a
/// machine with a machine token in a channel of a key made for it, to the
/// Noise key the directory gives; one token per channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signed_in_cli_reaches_a_machine_with_a_token() {
    let r = rig().await;
    let (at, h) = hub(hub_for(&r, Some(hex(&r.daemon.noise_public)), true)).await;
    let config = config_dir("session");
    let session = json!({ "url": at, "cookie": "ilg_session=s3cret", "account": "acct-b", "login": "bee" });
    std::fs::write(config.join("arugula/cli-session.json"), session.to_string()).unwrap();
    let out = cli_at(&config, &["--json", "ls"]).await.unwrap();
    let panes: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert!(panes[0]["session"].is_u64(), "{panes}");
    assert_eq!(h.given.load(std::sync::atomic::Ordering::SeqCst), 1);
    r.d.wait_for("the token's session in the log", || opened(&r) == [("token".into(), "owner".into())]);
    let _ = std::fs::remove_dir_all(&config);
}

/// A CLI that's a device from before machine tokens asks for a token
/// first, and uses its key only when the hub gives none (404, as one
/// without an issuer key answers). With no Noise key in the directory, the
/// machine's own certificate says it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_cli_uses_its_key_only_when_the_hub_gives_no_tokens() {
    let keys = DeviceKeys::generate();
    let config = config_dir("device");
    keys.save(&config.join("arugula/cli-key")).unwrap();
    for tokens in [false, true] {
        let r = rig_with(Some(&keys)).await;
        let (at, h) = hub(hub_for(&r, None, tokens)).await;
        let cert = r.certs.iter().find(|c| c.device == keys.id()).unwrap();
        let saved = json!({ "url": at, "cert": cert, "trust": { "account": "a1", "root": r.root } });
        std::fs::write(config.join("arugula/cli-control.json"), saved.to_string()).unwrap();
        let out = cli_at(&config, &["--json", "ls"]).await.unwrap();
        let panes: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
        assert!(panes[0]["session"].is_u64(), "{panes}");
        assert_eq!(h.given.load(std::sync::atomic::Ordering::SeqCst), usize::from(tokens));
        let by = if tokens { "token" } else { "device" };
        r.d.wait_for("the channel in the log", || opened(&r).iter().any(|(b, _)| b == by));
        assert!(opened(&r).iter().all(|(b, _)| b == by), "{:?}", opened(&r));
    }
    let _ = std::fs::remove_dir_all(&config);
}
