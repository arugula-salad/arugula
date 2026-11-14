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
