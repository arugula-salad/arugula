//! M50 (#150): a block through a block channel, as control's block origin
//! reaches it. A device's channel learns the daemon's clock; a one-off key
//! with a grant that device signed opens one block, whose requests and
//! WebSockets are answered by the block listener's proxy; grants that
//! don't hold are refused in message 2; and closing the block closes its
//! channel. A daemon with no block flags opens port blocks, and serves them
//! over block channels, while its control has a block domain (PR 4), and
//! not otherwise.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{collections::HashMap, net::TcpListener, time::Duration};

use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    block::{
        Answer, Grant, Hello, Refused,
        wire::{self, ReqHead, ResHead, WsOpen},
    },
    channel::{Channel, Initiator, Msg, Welcome, prologue},
};
use arugula_testkit::{Daemon, arugulad};
use axum::{
    Json, Router,
    extract::ws::{Message as AxMessage, WebSocketUpgrade},
    http::HeaderMap,
    response::{Html, Redirect},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A control that trusts `device` (the account's root) and this daemon,
/// serving block origins under `block_domain` if it's given.
async fn fake_control(certs: Vec<Cert>, block_domain: Option<&'static str>) -> String {
    let trust = json!({ "certs": certs, "revocations": [] });
    let info = match block_domain {
        Some(d) => json!({ "control": true, "daemon_auth": 2, "block_domain": d }),
        None => json!({ "control": true, "daemon_auth": 2 }),
    };
    let app = Router::new()
        .route("/control.json", get(move || async move { Json(info) }))
        .route("/api/daemon/trust", get(move || async move { Json(trust) }))
        .route("/api/daemon/peers", get(|| async { Json(json!({})) }))
        .route("/api/daemon/push-subs", get(|| async { Json(json!({ "subs": [] })) }))
        .route("/api/daemon/access", post(|| async { Json(json!({})) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    at
}

async fn dev_server(port: u16) {
    fn heads(h: &HeaderMap) -> HashMap<String, String> {
        h.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_owned())).collect()
    }
    let app = Router::new()
        .route("/", get(|| async { Html("<html><head><title>Dev app</title></head><body>hello</body></html>") }))
        .route("/headers", get(|h: HeaderMap| async move { Json(heads(&h)) }))
        .route("/echo", post(|body: String| async move { body }))
        .route("/away", get(move || async move { Redirect::temporary(&format!("http://localhost:{port}/there")) }))
        .route(
            "/hmr",
            get(|h: HeaderMap, ws: WebSocketUpgrade| async move {
                let first = serde_json::to_string(&heads(&h)).unwrap();
                ws.on_upgrade(|mut s| async move {
                    let _ = s.send(AxMessage::Text(first.into())).await;
                    while let Some(Ok(m)) = s.recv().await {
                        if s.send(m).await.is_err() {
                            break;
                        }
                    }
                })
            }),
        );
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
}

struct Rig {
    d: Daemon,
    device: DeviceKeys,
    daemon: DeviceKeys,
    control: String,
}

async fn rig() -> Rig {
    rig_with(true, None).await
}

/// A joined daemon, with `--block-listen` or none, its control serving
/// block origins under `block_domain` or not.
async fn rig_with(listen: bool, block_domain: Option<&'static str>) -> Rig {
    let device = DeviceKeys::generate();
    let mut root = Cert::new(&device, "a1", Kind::Browser, "laptop");
    root.sign_with(&device);
    let d = arugulad!("blockchan");
    let d = if listen { d.block_listen() } else { d };
    let mut d = d.no_wisp().no_tailscale().args(["--direct-url", "http://127.0.0.1:0"]).wait_secs(30).start();
    let daemon = DeviceKeys::load_or_create(&d.state.join("daemon.key")).unwrap();
    let mut cert = Cert::new(&daemon, "a1", Kind::Daemon, "test");
    cert.sign_with(&device);
    let control = fake_control(vec![root, cert.clone()], block_domain).await;
    let saved =
        json!({ "url": control, "trust": { "account": "a1", "root": device.id() }, "cert": cert, "login": "me" });
    std::fs::write(d.state.join("control.json"), saved.to_string()).unwrap();
    d.stop();
    d.start();
    d.wait_for("joined", || d.get("/api/host")["control_state"]["state"] == "joined");
    Rig { d, device, daemon, control }
}

/// A channel to the daemon at `/e2e` with `keys`, message 1 carrying
/// `payload`: the socket, the channel, and message 2's payload. `None` if
/// the daemon hung up instead of answering.
async fn open(r: &Rig, keys: &DeviceKeys, payload: &[u8]) -> Option<(Ws, Channel, Vec<u8>)> {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/e2e", r.d.port)).await.unwrap();
    let (init, m1) = Initiator::start_with(keys, &r.daemon.noise_public, &prologue(&r.daemon.id()), payload).unwrap();
    ws.send(Message::Binary(m1.into())).await.unwrap();
    let m2 = loop {
        match tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("message 2") {
            Some(Ok(Message::Binary(b))) => break b,
            Some(Ok(_)) => continue,
            _ => return None,
        }
    };
    let (got, ch) = init.finish(&m2).unwrap();
    Some((ws, ch, got))
}

async fn send(ws: &mut Ws, ch: &Channel, f: Vec<u8>) {
    for w in ch.seal(&Msg::Binary(f)).unwrap() {
        ws.send(Message::Binary(w.into())).await.unwrap();
    }
}

/// The next frame, or `None` once the channel closed.
async fn next(ws: &mut Ws, ch: &Channel) -> Option<(u8, u32, Vec<u8>)> {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a frame");
        let Some(Ok(Message::Binary(w))) = m else { return None };
        if let Some(Msg::Binary(f)) = ch.open(&w).unwrap() {
            let (k, s, p) = wire::parse(&f).unwrap();
            return Some((k, s, p.to_vec()));
        }
    }
}

/// One request on stream `sid`: its head and whole body.
async fn fetch(ws: &mut Ws, ch: &Channel, sid: u32, head: ReqHead, body: &[u8]) -> (ResHead, Vec<u8>) {
    send(ws, ch, wire::json_frame(wire::HEAD, sid, &head)).await;
    if !body.is_empty() {
        send(ws, ch, wire::frame(wire::DATA, sid, body)).await;
    }
    send(ws, ch, wire::frame(wire::END, sid, &[])).await;
    let (mut res, mut out) = (None, Vec::new());
    loop {
        let (k, s, p) = next(ws, ch).await.expect("an answer");
        assert_eq!(s, sid);
        match k {
            wire::RESPONSE => res = Some(serde_json::from_slice(&p).unwrap()),
            wire::DATA => out.extend_from_slice(&p),
            wire::END => return (res.expect("a head first"), out),
            wire::FAILED => panic!("failed: {}", String::from_utf8_lossy(&p)),
            k => panic!("unexpected frame {k}"),
        }
    }
}

fn header<'a>(h: &'a ResHead, name: &str) -> Vec<&'a str> {
    h.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).collect()
}

fn get_req(path: &str, headers: &[(&str, &str)]) -> ReqHead {
    ReqHead {
        method: "GET".into(),
        path: path.into(),
        headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_block_through_its_channel() {
    let r = rig().await;
    let dev_port = free_port();
    dev_server(dev_port).await;

    // The device's own channel says the daemon's clock in message 2.
    let (_ws, _, welcome) = open(&r, &r.device, &[]).await.expect("the device's channel");
    let now = Welcome::decode(&welcome).expect("a welcome").now;
    let mine = arugula_e2e::now_ms();
    assert!(now.abs_diff(mine) < 10_000, "daemon {now}, here {mine}");

    let (status, body) =
        r.d.raw("POST", "/api/blocks", Some(json!({"type": "browser", "config": {"port": dev_port, "path": "/"}})));
    assert_eq!(status, 200, "{body}");
    let id = serde_json::from_str::<Value>(&body).unwrap()["block"].as_u64().unwrap();
    let state = r.d.get(&format!("/api/blocks/{id}"))["state"].clone();
    let site_key = state["site_key"].as_str().expect("a site key").to_owned();
    assert_eq!(site_key.len(), 32);
    let block_origin = format!("https://b-{site_key}.blocks.example");

    // A one-off key and a grant for this block, by the daemon's clock.
    let key = DeviceKeys::generate();
    let grant = |block: &str, expires: u64, by: &DeviceKeys, key: &DeviceKeys| {
        Hello { grant: Grant::new(by, &r.daemon.id(), block, &key.noise_public, expires) }.encode()
    };
    let (mut ws, ch, answer) = open(&r, &key, &grant(&id.to_string(), now + 60_000, &r.device, &key)).await.unwrap();
    assert_eq!(Answer::decode(&answer), Ok(()));

    // A page, framed only by control's.
    let (res, page) =
        fetch(&mut ws, &ch, 1, get_req("/", &[("sec-fetch-mode", "navigate"), ("sec-fetch-dest", "iframe")]), b"")
            .await;
    assert_eq!(res.status, 200);
    assert!(String::from_utf8_lossy(&page).contains("hello"));
    assert_eq!(header(&res, "content-security-policy"), [format!("frame-ancestors 'self' {}", r.control).as_str()]);

    // What the port sees: a local request, uncompressed.
    let (_, seen) = fetch(
        &mut ws,
        &ch,
        3,
        get_req(
            "/headers",
            &[
                ("origin", &block_origin),
                ("referer", &format!("{block_origin}/somewhere")),
                ("accept-encoding", "gzip, br"),
                ("cookie", "session=1"),
            ],
        ),
        b"",
    )
    .await;
    let seen: HashMap<String, String> = serde_json::from_slice(&seen).unwrap();
    assert_eq!(seen["host"], format!("localhost:{dev_port}"));
    assert_eq!(seen["origin"], format!("http://localhost:{dev_port}"));
    assert_eq!(seen["referer"], format!("http://localhost:{dev_port}/somewhere"));
    assert_eq!(seen["accept-encoding"], "identity");
    assert_eq!(seen["cookie"], "session=1");

    // A body goes there; a redirect to the port stays on the page's origin.
    let post = ReqHead { method: "POST".into(), path: "/echo".into(), headers: vec![] };
    let (res, echo) = fetch(&mut ws, &ch, 5, post, b"some body").await;
    assert_eq!((res.status, echo.as_slice()), (200, &b"some body"[..]));
    let (res, _) = fetch(&mut ws, &ch, 7, get_req("/away", &[]), b"").await;
    assert_eq!(res.status, 307);
    assert_eq!(header(&res, "location"), ["/there"]);

    // Hot reload's socket, on the same channel.
    send(&mut ws, &ch, wire::json_frame(wire::OPEN, 9, &WsOpen { path: "/hmr".into(), protocols: vec![] })).await;
    let (k, _, _) = next(&mut ws, &ch).await.unwrap();
    assert_eq!(k, wire::ACCEPTED);
    let (k, _, first) = next(&mut ws, &ch).await.unwrap();
    assert_eq!(k, wire::MESSAGE);
    let upgrade: HashMap<String, String> = serde_json::from_slice(&first[1..]).unwrap();
    assert_eq!(upgrade["host"], format!("localhost:{dev_port}"));
    send(&mut ws, &ch, wire::frame(wire::MESSAGE, 9, b"\x00update")).await;
    let (k, s, echo) = next(&mut ws, &ch).await.unwrap();
    assert_eq!((k, s, echo.as_slice()), (wire::MESSAGE, 9, &b"\x00update"[..]));

    // Grants that don't hold are refused in message 2, readably, and the
    // channel closes.
    let stranger = DeviceKeys::generate();
    let other = DeviceKeys::generate();
    let block = id.to_string();
    for (payload, want) in [
        (grant(&block, now - 1, &r.device, &other), Refused::Expired),
        (grant(&block, now + 60 * 60_000, &r.device, &other), Refused::TooLong),
        (grant(&block, now + 60_000, &stranger, &other), Refused::Untrusted),
        (grant("999", now + 60_000, &r.device, &other), Refused::NoBlock),
        // Someone else's grant: the key it names isn't theirs.
        (grant(&block, now + 60_000, &r.device, &key), Refused::OtherKey),
        (b"{}".to_vec(), Refused::Malformed),
    ] {
        let (mut ws, _, got) = open(&r, &other, &payload).await.expect("an answer");
        assert_eq!(Answer::decode(&got), Err(want));
        assert!(!matches!(ws.next().await, Some(Ok(Message::Binary(_)))), "{want:?}: closed");
    }

    // Closing the block closes its channel.
    let (status, body) = r.d.raw("POST", &format!("/api/panes/{id}/close"), Some(json!({})));
    assert_eq!(status, 200, "{body}");
    assert_eq!(next(&mut ws, &ch).await, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_daemon_with_no_block_flags_shows_blocks_through_control_only() {
    let dev_port = free_port();
    dev_server(dev_port).await;
    let open_port = |r: &Rig| {
        r.d.raw("POST", "/api/blocks", Some(json!({"type": "browser", "config": {"port": dev_port, "path": "/"}})))
    };

    // Joined to a control with no block domain: nowhere to show a port.
    let r = rig_with(false, None).await;
    let features = r.d.get("/api/host")["features"].clone();
    assert_eq!(
        (features["blocks"].as_bool(), features["block_channels"].as_bool()),
        (Some(false), Some(false)),
        "{features}"
    );
    let (status, body) = open_port(&r);
    assert_ne!(status, 200, "{body}");
    assert!(body.contains("off here"), "{body}");
    drop(r);

    // A control with a block domain: its page shows them, the daemon's own
    // page doesn't offer them.
    let r = rig_with(false, Some("blocks.example")).await;
    r.d.wait_for("blocks through control", || r.d.get("/api/host")["features"]["block_channels"] == true);
    assert_eq!(r.d.get("/api/host")["features"]["blocks"], false);
    let (status, body) = open_port(&r);
    assert_eq!(status, 200, "{body}");
    let id = serde_json::from_str::<Value>(&body).unwrap()["block"].as_u64().unwrap();
    let state = r.d.get(&format!("/api/blocks/{id}"))["state"].clone();
    // No name of its own: a path, and the key of its origin on control's.
    assert_eq!(state["src"], "/", "{state}");
    assert_eq!(state["site_key"].as_str().map(str::len), Some(32), "{state}");

    let (_ws, _, welcome) = open(&r, &r.device, &[]).await.expect("the device's channel");
    let now = Welcome::decode(&welcome).expect("a welcome").now;
    let key = DeviceKeys::generate();
    let hello =
        Hello { grant: Grant::new(&r.device, &r.daemon.id(), &id.to_string(), &key.noise_public, now + 60_000) };
    let (mut ws, ch, answer) = open(&r, &key, &hello.encode()).await.unwrap();
    assert_eq!(Answer::decode(&answer), Ok(()));
    let (res, page) =
        fetch(&mut ws, &ch, 1, get_req("/", &[("sec-fetch-mode", "navigate"), ("sec-fetch-dest", "iframe")]), b"")
            .await;
    assert_eq!(res.status, 200);
    assert!(String::from_utf8_lossy(&page).contains("hello"));
}
