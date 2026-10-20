//! Block origins (M50) over HTTP: which host gets what, that a block's
//! origin never gets control's page, cookies or API, and a block's channel
//! through the relay to a daemon, end to end (the daemon's half as
//! `block_channel.rs` does it, with a fake block behind it).

use std::{net::SocketAddr, sync::Arc, time::Duration};

use arugula_e2e::{
    Cert, DeviceKeys, Kind,
    block::{
        Answer, Expect, Grant, Hello, Refused,
        wire::{self, ReqHead, ResHead},
    },
    channel::{Channel, Initiator, Msg, Responder, prologue},
    mux::Mux,
    now_ms,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, header};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio_tungstenite::tungstenite::{
    Message,
    client::IntoClientRequest,
    protocol::{CloseFrame, frame::coding::CloseCode},
};

use crate::{App, auth::hash, blocks};

const CONTROL: &str = "http://control.test";
const DOMAIN: &str = "blocks.test";
const KEY: &str = "0123456789abcdef0123456789abcdef";
/// The block the fake daemon has.
const BLOCK: &str = "7";

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Control {
    app: Arc<App>,
    base: String,
    http: reqwest::Client,
    _files: Files,
}

/// A stand-in web build, removed when the test ends.
struct Files(std::path::PathBuf);

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Control at `http://control.test` with blocks under `blocks.test`, and a
/// stand-in web build with control's page and the block files.
async fn control(tweak: impl FnOnce(&mut App)) -> Control {
    let files = Files(std::env::temp_dir().join(format!("arugula-blocks-{}", hex::encode(arugula_e2e::random::<8>()))));
    std::fs::create_dir_all(files.0.join("block")).unwrap();
    std::fs::write(files.0.join("index.html"), "<!doctype html><title>Arugula control</title>").unwrap();
    for f in ["boot.js", "sw.js", "shim.js"] {
        std::fs::write(files.0.join("block").join(f), format!("// {f}")).unwrap();
    }
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let mut app = App::for_tests(CONTROL);
    app.cfg.block_domain = Some(DOMAIN.into());
    app.cfg.static_dir = Some(files.0.to_owned());
    tweak(&mut app);
    let app = app.start();
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { app, base, http, _files: files }
}

fn block_host(key: &str) -> String {
    format!("b-{key}.{DOMAIN}")
}

impl Control {
    fn at(&self, host: &str, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base)).header(header::HOST, host)
    }

    fn session(&self, account: &str) -> String {
        let t = crate::auth::token();
        self.app.db.add_session(&hash(&t), account, now_ms(), now_ms() + 3_600_000).unwrap();
        format!("{}={t}", crate::auth::SESSION_COOKIE)
    }

    /// A block's socket to `daemon`'s relay, as its worker opens it.
    async fn relay(&self, key: &str, origin: Option<&str>, daemon: &str) -> Result<Ws, u16> {
        let url = format!("{}/.arugula/relay?d={daemon}", self.base.replace("http://", "ws://"));
        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert("host", block_host(key).parse().unwrap());
        if let Some(o) = origin {
            req.headers_mut().insert("origin", o.parse().unwrap());
        }
        match tokio_tungstenite::connect_async(req).await {
            Ok((ws, _)) => Ok(ws),
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16()),
            Err(e) => panic!("{e}"),
        }
    }
}

fn own_origin(key: &str) -> String {
    format!("http://{}", block_host(key))
}

#[tokio::test]
async fn a_block_origin_gets_the_bootstrap_page_and_never_controls() {
    let c = control(|_| {}).await;
    c.app.db.account_for("github", "gh-1", "sam", "a1", now_ms()).unwrap();
    let cookie = c.session("a1");
    let b = block_host(KEY);

    // A navigation the worker didn't answer: the bootstrap page, framed only
    // by control, with no inline script.
    let res =
        c.at(&b, reqwest::Method::GET, "/some/page?x=1").header("sec-fetch-mode", "navigate").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let h = res.headers().clone();
    let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    assert!(csp.contains("frame-ancestors http://control.test"), "{csp}");
    assert!(csp.contains("script-src 'self'") && csp.contains("default-src 'none'"), "{csp}");
    assert!(!csp.contains("unsafe"), "{csp}");
    assert!(h.get(header::X_FRAME_OPTIONS).is_none(), "control's DENY would keep it out of control's page");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    let page = res.text().await.unwrap();
    assert!(page.contains(r#"<meta name=arugula-control content="http://control.test">"#), "{page}");
    assert!(page.contains(r#"<script src="/.arugula/boot.js"></script>"#));
    assert!(!page.contains("Arugula control"), "not control's page");

    // Control's files for the block, and the worker's scope is the origin.
    let sw = c.at(&b, reqwest::Method::GET, "/.arugula/sw.js").send().await.unwrap();
    assert_eq!(sw.status(), StatusCode::OK);
    assert_eq!(sw.headers()["service-worker-allowed"], "/");
    assert!(sw.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/javascript"));
    assert_eq!(sw.text().await.unwrap(), "// sw.js");
    for f in ["boot.js", "shim.js"] {
        let r = c.at(&b, reqwest::Method::GET, &format!("/.arugula/{f}")).send().await.unwrap();
        assert_eq!(r.text().await.unwrap(), format!("// {f}"));
    }
    assert_eq!(c.at(&b, reqwest::Method::GET, "/.arugula/nope").send().await.unwrap().status(), 404);

    // An app's own worker gets control's, told where the app's is.
    let app_sw =
        c.at(&b, reqwest::Method::GET, "/app/sw.js?v=2").header("service-worker", "script").send().await.unwrap();
    assert_eq!(
        app_sw.text().await.unwrap(),
        "self.ARUGULA_APP_SW=\"/app/sw.js?v=2\";importScripts(\"/.arugula/sw.js\");\n"
    );

    // Control's own page, API and sign-in aren't here, signed in or not,
    // and nothing sets a cookie.
    for (method, path) in [
        ("GET", "/"),
        ("GET", "/index.html"),
        ("GET", "/control.json"),
        ("GET", "/api/me"),
        ("GET", "/api/devices"),
        ("POST", "/auth/logout"),
        ("GET", "/auth/github"),
        ("GET", "/api/relay/m"),
    ] {
        let r = c.at(&b, method.parse().unwrap(), path).header(header::COOKIE, &cookie).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE, "{method} {path}");
        assert!(r.headers().get(header::SET_COOKIE).is_none(), "{method} {path}");
        assert!(!r.text().await.unwrap().contains("Arugula control"), "{method} {path}");
    }
    // As a navigation too: the bootstrap page, never control's.
    let r =
        c.at(&b, reqwest::Method::GET, "/api/me").header(header::COOKIE, &cookie).header("sec-fetch-mode", "navigate");
    let page = r.send().await.unwrap().text().await.unwrap();
    assert!(page.contains("/.arugula/boot.js") && !page.contains("\"login\""), "{page}");
}

#[tokio::test]
async fn hosts_under_the_block_domain_that_arent_blocks_get_nothing() {
    let c = control(|_| {}).await;
    for host in [DOMAIN.to_owned(), format!("www.{DOMAIN}"), "b-xyz.blocks.test".into(), format!("a.b-{KEY}.{DOMAIN}")]
    {
        for path in ["/", "/control.json", "/.arugula/sw.js"] {
            let r = c.at(&host, reqwest::Method::GET, path).header("sec-fetch-mode", "navigate").send().await.unwrap();
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{host}{path}");
        }
    }
}

#[tokio::test]
async fn controls_own_host_is_as_before_and_says_where_blocks_are() {
    let c = control(|_| {}).await;
    let j: Value =
        c.at("control.test", reqwest::Method::GET, "/control.json").send().await.unwrap().json().await.unwrap();
    assert_eq!(j["block_domain"], DOMAIN);
    let page = c.at("control.test", reqwest::Method::GET, "/").send().await.unwrap();
    assert_eq!(page.headers()[header::X_FRAME_OPTIONS], "DENY");
    // Its page frames blocks' origins (PR 4), on its own scheme: https
    // hosted, http for a local control.
    let csp = page.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    assert!(csp.contains(&format!("frame-src 'self' https: *.{DOMAIN}:*;")), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(page.text().await.unwrap().contains("Arugula control"));
    // Blocks' files aren't control's page's.
    assert_eq!(c.at("control.test", reqwest::Method::GET, "/block/sw.js").send().await.unwrap().status(), 404);
    assert_eq!(c.at("control.test", reqwest::Method::GET, "/.arugula/sw.js").send().await.unwrap().status(), 404);
    // A host that's neither is served as control, as before blocks.
    let j: Value =
        c.at("elsewhere.example", reqwest::Method::GET, "/control.json").send().await.unwrap().json().await.unwrap();
    assert_eq!(j["url"], CONTROL);

    // With no block domain, a block's name is just another host.
    let off = control(|a| a.cfg.block_domain = None).await;
    let j: Value =
        off.at(&block_host(KEY), reqwest::Method::GET, "/control.json").send().await.unwrap().json().await.unwrap();
    assert_eq!(j["control"], true);
    assert!(j.get("block_domain").is_none());
    let page = off.at("control.test", reqwest::Method::GET, "/").send().await.unwrap();
    let csp = page.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    assert!(csp.contains("frame-src 'self' https:;"), "{csp}");
}

// ---- the relay

/// A daemon joined to `account`, as control knows it.
fn daemon(app: &App, account: &str) -> DeviceKeys {
    let keys = DeviceKeys::generate();
    let cert = Cert::new(&keys, account, Kind::Daemon, "geek");
    app.db.put_device(&cert, true, now_ms()).unwrap();
    app.db.put_daemon(account, &cert.device, "geek", &[]).unwrap();
    keys
}

fn v2(keys: &DeviceKeys, method: &str, pq: &str) -> String {
    let ms = now_ms();
    let nonce = hex::encode(arugula_e2e::random::<16>());
    let msg = crate::auth::daemon_auth_message_v2(method, pq, ms, &nonce, b"");
    format!("v2 {} {ms} {nonce} {}", keys.id(), hex::encode(keys.signature(msg.as_bytes())))
}

async fn read_frame(rd: &mut (impl AsyncReadExt + Unpin)) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    rd.read_exact(&mut len).await.ok()?;
    let mut b = vec![0u8; u32::from_be_bytes(len) as usize];
    rd.read_exact(&mut b).await.ok()?;
    Some(b)
}

async fn write_frame(wr: &mut (impl AsyncWriteExt + Unpin), b: &[u8]) {
    let mut f = (b.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(b);
    let _ = wr.write_all(&f).await;
}

/// The daemon's half of a block channel on one relay stream: check the
/// grant (only `device` may sign, block 7 only), answer, then answer each
/// request with its own path.
async fn serve_block(keys: Arc<DeviceKeys>, device: [u8; 32], device_id: String, stream: DuplexStream) {
    let (mut rd, mut wr) = tokio::io::split(stream);
    let Some(m1) = read_frame(&mut rd).await else { return };
    let Ok((responder, remote, payload)) = Responder::read(&keys, &prologue(&keys.id()), &m1) else { return };
    let verdict = Hello::decode(&payload).map_err(|_| Refused::Malformed).and_then(|h| {
        h.grant.check(
            &Expect { daemon: &keys.id(), remote: &remote, now: now_ms() },
            |by| (by == device_id).then_some(device),
            |b| b == BLOCK,
        )
    });
    let answer = match verdict {
        Ok(()) => Answer::ok(),
        Err(why) => Answer::refused(why),
    };
    let (m2, ch) = responder.finish(&answer.encode()).unwrap();
    write_frame(&mut wr, &m2).await;
    if verdict.is_err() {
        let _ = wr.shutdown().await;
        return;
    }
    while let Some(w) = read_frame(&mut rd).await {
        let Ok(Some(Msg::Binary(f))) = ch.open(&w) else { continue };
        let Ok((kind, sid, payload)) = wire::parse(&f) else { return };
        if kind != wire::HEAD {
            continue;
        }
        let head: ReqHead = serde_json::from_slice(payload).unwrap();
        let res = ResHead { status: 200, headers: vec![("content-type".into(), "text/plain".into())] };
        for f in [
            wire::json_frame(wire::RESPONSE, sid, &res),
            wire::frame(wire::DATA, sid, format!("block {BLOCK} at {}", head.path).as_bytes()),
            wire::frame(wire::END, sid, b""),
        ] {
            for w in ch.seal(&Msg::Binary(f)).unwrap() {
                write_frame(&mut wr, &w).await;
            }
        }
    }
}

/// A daemon on the relay, serving block channels on every stream control
/// opens to it.
async fn fake_daemon(c: &Control, keys: DeviceKeys, device: &DeviceKeys) {
    let mut req = format!("{}/api/relay/dial", c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("x-arugula-auth", v2(&keys, "GET", "/api/relay/dial").parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let (accept_tx, mut accept) = tokio::sync::mpsc::unbounded_channel();
    let (mux, mut out) = Mux::new(Some(accept_tx));
    let (mut tx, mut rx) = ws.split();
    tokio::spawn(async move {
        while let Some(f) = out.recv().await {
            if tx.send(Message::Binary(f.into())).await.is_err() {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(Ok(m)) = rx.next().await {
            if let Message::Binary(b) = m
                && mux.handle(&b).is_err()
            {
                break;
            }
        }
    });
    let (keys, sign, id) = (Arc::new(keys), device.sign_public(), device.id());
    let daemon_id = keys.id();
    tokio::spawn(async move {
        while let Some(s) = accept.recv().await {
            tokio::spawn(serve_block(keys.clone(), sign, id.clone(), s));
        }
    });
    for _ in 0..100 {
        if c.app.relay.online(&daemon_id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the daemon didn't get on the relay");
}

/// Opens a block channel over `ws`: message 2's answer, and the channel.
async fn handshake(
    ws: &mut Ws,
    daemon: (&str, &[u8; 32]),
    grant: Grant,
    block_key: &DeviceKeys,
) -> (Result<(), Refused>, Channel) {
    let hello = Hello { grant }.encode();
    let (init, m1) = Initiator::start_with(block_key, daemon.1, &prologue(daemon.0), &hello).unwrap();
    ws.send(Message::Binary(m1.into())).await.unwrap();
    let m2 = loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("message 2") {
            Some(Ok(Message::Binary(b))) => break b,
            Some(Ok(_)) => continue,
            other => panic!("no message 2: {other:?}"),
        }
    };
    let (got, ch) = init.finish(&m2).unwrap();
    (Answer::decode(&got), ch)
}

/// Whether the socket closes within `secs`, and the close frame if any.
async fn closed(ws: &mut Ws, secs: u64) -> Option<Option<CloseFrame>> {
    tokio::time::timeout(Duration::from_secs(secs), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(f))) => return f,
                None | Some(Err(_)) => return None,
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .ok()
}

#[tokio::test]
async fn a_blocks_channel_crosses_the_relay_to_its_daemon() {
    // The owner's client sockets are capped at one: block sockets don't
    // count against them.
    let c = control(|a| a.relay.caps.sockets = 1).await;
    c.app.db.account_for("github", "gh-1", "sam", "a1", now_ms()).unwrap();
    let device = DeviceKeys::generate();
    let keys = daemon(&c.app, "a1");
    let id = keys.id();
    let noise = keys.noise_public;
    fake_daemon(&c, keys, &device).await;
    let origin = own_origin(KEY);

    // Granted: a request goes and comes back through control, which saw
    // only ciphertext.
    let block_key = DeviceKeys::generate();
    let grant = Grant::new(&device, &id, BLOCK, &block_key.noise_public, now_ms() + 60_000);
    let mut ws = c.relay(KEY, Some(&origin), &id).await.unwrap();
    let mut second = c.relay(KEY, Some(&origin), &id).await.expect("past the owner's own socket cap");
    let (answer, ch) = handshake(&mut ws, (&id, &noise), grant.clone(), &block_key).await;
    assert_eq!(answer, Ok(()));
    assert_eq!(c.app.blocks.count(&format!("d:{id}")), 2);
    assert_eq!(c.app.blocks.count(&format!("o:{KEY}")), 2);
    let head = ReqHead { method: "GET".into(), path: "/src/main.ts?t=1".into(), headers: vec![] };
    for f in [wire::json_frame(wire::HEAD, 1, &head), wire::frame(wire::END, 1, b"")] {
        for w in ch.seal(&Msg::Binary(f)).unwrap() {
            ws.send(Message::Binary(w.into())).await.unwrap();
        }
    }
    let mut got = vec![];
    while got.last().is_none_or(|(k, _)| *k != wire::END) {
        let Some(Ok(Message::Binary(w))) = ws.next().await else { panic!("the channel closed") };
        if let Some(Msg::Binary(f)) = ch.open(&w).unwrap() {
            let (kind, sid, payload) = wire::parse(&f).unwrap();
            assert_eq!(sid, 1);
            got.push((kind, payload.to_vec()));
        }
    }
    let res: ResHead = serde_json::from_slice(&got[0].1).unwrap();
    assert_eq!((got[0].0, res.status), (wire::RESPONSE, 200));
    assert_eq!(got[1], (wire::DATA, b"block 7 at /src/main.ts?t=1".to_vec()));

    // Refused (expired): the refusal is readable, then the socket closes.
    let stale = Grant::new(&device, &id, BLOCK, &block_key.noise_public, now_ms() - 1);
    let (answer, _) = handshake(&mut second, (&id, &noise), stale, &block_key).await;
    assert_eq!(answer, Err(Refused::Expired));
    assert!(closed(&mut second, 5).await.is_some(), "a refused channel closes");

    // Closing gives the seats back.
    ws.close(None).await.unwrap();
    drop(ws);
    for _ in 0..100 {
        if c.app.blocks.count(&format!("d:{id}")) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(c.app.blocks.count(&format!("d:{id}")), 0);
    assert_eq!(c.app.blocks.count(&format!("o:{KEY}")), 0);
}

#[tokio::test]
async fn the_relay_says_no_more_about_a_daemon_than_before() {
    let c = control(|_| {}).await;
    c.app.db.account_for("github", "gh-1", "sam", "a1", now_ms()).unwrap();
    let offline = daemon(&c.app, "a1").id();
    let origin = own_origin(KEY);
    // Unknown and known-but-not-connected: the same answer.
    let unknown = c.relay(KEY, Some(&origin), "nosuchdaemon").await.unwrap_err();
    let known = c.relay(KEY, Some(&origin), &offline).await.unwrap_err();
    assert_eq!((unknown, known), (404, 404));
    let body = |d: &str| {
        c.at(&block_host(KEY), reqwest::Method::GET, &format!("/.arugula/relay?d={d}"))
            .header(header::ORIGIN, &origin)
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
    };
    let (a, b) = (body("nosuchdaemon").await.unwrap(), body(&offline).await.unwrap());
    assert_eq!(a.status(), b.status());
    assert_eq!(a.text().await.unwrap(), b.text().await.unwrap());

    // Another site's page (another block's among them), or no Origin at
    // all, can't use a block's relay.
    let other = own_origin("fedcba9876543210fedcba9876543210");
    for o in [Some(other.as_str()), Some("http://control.test"), None] {
        assert_eq!(c.relay(KEY, o, &offline).await.unwrap_err(), 403, "{o:?}");
    }
}

#[tokio::test]
async fn block_sockets_are_capped_and_close_without_a_handshake() {
    let c = control(|a| a.blocks.deadlines.first = Duration::from_millis(300)).await;
    c.app.db.account_for("github", "gh-1", "sam", "a1", now_ms()).unwrap();
    let device = DeviceKeys::generate();
    let keys = daemon(&c.app, "a1");
    let id = keys.id();
    fake_daemon(&c, keys, &device).await;
    let origin = own_origin(KEY);

    // Nothing sent: closed, and why.
    let mut idle = c.relay(KEY, Some(&origin), &id).await.unwrap();
    let f = closed(&mut idle, 5).await.expect("closes").expect("with a reason");
    assert_eq!((f.code, f.reason.as_str()), (CloseCode::Normal, "no handshake"));

    // Per block origin.
    let c = control(|_| {}).await;
    c.app.db.account_for("github", "gh-1", "sam", "a1", now_ms()).unwrap();
    let keys = daemon(&c.app, "a1");
    let id = keys.id();
    fake_daemon(&c, keys, &device).await;
    let mut held = vec![];
    for _ in 0..blocks::PER_ORIGIN {
        held.push(c.relay(KEY, Some(&origin), &id).await.unwrap());
    }
    assert_eq!(c.relay(KEY, Some(&origin), &id).await.unwrap_err(), 429);
    // Another block's origin has room of its own.
    let other = "fedcba9876543210fedcba9876543210";
    held.push(c.relay(other, Some(&own_origin(other)), &id).await.unwrap());
}
