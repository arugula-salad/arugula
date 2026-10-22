//! Public previews (M187) over HTTP: a stranger's request to
//! `<id>.<preview domain>` reaches the hosted machine's port through
//! control and the machine's relay socket, with a grant signed with
//! control's hosted key; what crosses and what doesn't; a port turned off
//! (by the platform, or refused by the machine); a paused box; and a
//! WebSocket. The machine's half is a fake that checks the grant and asks
//! the platform which ports are public, as `public_preview.rs` does
//! through `GET /api/hosted/previews`.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use arugula_e2e::{
    DeviceKeys,
    block::{
        Answer, Expect, Refused,
        wire::{self, ReqHead, ResHead},
    },
    channel::{Msg, Responder, prologue},
    mux::Mux,
    now_ms,
    preview::PublicHello,
};
use arugula_platform::PlatformPort as _;
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, header};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use crate::App;

const CONTROL: &str = "http://control.test";
const DOMAIN: &str = "previews.test";
const PROJECT: &str = "prj_0123456789abcdef0123456789abcdef";
const ID: &str = "0123456789abcdef0123456789abcdef";

struct Control {
    app: Arc<App>,
    base: String,
    http: reqwest::Client,
}

async fn control() -> Control {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let mut app = App::for_tests(CONTROL);
    app.cfg.preview_domain = Some(DOMAIN.into());
    app.platform.inner_mut().hosted.preview_domain = Some(DOMAIN.into());
    let app = app.start();
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Control { app, base, http }
}

impl Control {
    fn get(&self, host: &str, path: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}{path}", self.base)).header(header::HOST, host)
    }

    /// A hosted machine of `workspace` in [`PROJECT`], enrolled at the hub
    /// and `ready` at the platform.
    fn machine(&self, workspace: &str) -> Arc<DeviceKeys> {
        let keys = DeviceKeys::generate();
        let d = arugula_e2e::hosted::Delegation {
            project: PROJECT.into(),
            owner: "team:00aa11bb22cc33dd".into(),
            key: self.app.hosted_key.public(),
            at: now_ms(),
            by: String::new(),
            sig: String::new(),
        };
        let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
        let cert = arugula_e2e::hosted::HostedCert::new(&d, workspace, "box", &noise, &sign, now_ms());
        self.app.db.add_hosted(&cert, now_ms()).unwrap();
        let p = self.app.platform.inner();
        let spec = arugula_platform::hosted::Spec::default();
        p.db.add_hosted_workspace(workspace, PROJECT, workspace, &spec, "alice", 10, now_ms()).unwrap();
        let mut job = p.db.claim_hosted(now_ms()).unwrap().unwrap();
        job.w.state = "ready".into();
        job.w.machine = Some(keys.id());
        p.db.checkpoint_hosted(&job, None, now_ms() + 3_600_000, now_ms()).unwrap();
        Arc::new(keys)
    }
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

/// The machine on the relay, serving public previews on every stream.
async fn fake_machine(c: &Control, keys: Arc<DeviceKeys>, workspace: &str) {
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
    let id = keys.id();
    let (app, workspace) = (c.app.clone(), workspace.to_owned());
    tokio::spawn(async move {
        while let Some(s) = accept.recv().await {
            tokio::spawn(serve(app.clone(), keys.clone(), workspace.clone(), s));
        }
    });
    for _ in 0..100 {
        if c.app.relay.online(&id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the machine didn't get on the relay");
}

/// The machine's half: the grant with control's hosted key, the port
/// public now; then each request answered with what it saw, and a
/// WebSocket echoed.
async fn serve(app: Arc<App>, keys: Arc<DeviceKeys>, workspace: String, stream: DuplexStream) {
    let (mut rd, mut wr) = tokio::io::split(stream);
    let Some(m1) = read_frame(&mut rd).await else { return };
    let Ok((responder, remote, payload)) = Responder::read(&keys, &prologue(&keys.id()), &m1) else { return };
    let public = app.platform.public_ports(&workspace).await.unwrap();
    let key = app.hosted_key.public();
    let verdict = PublicHello::decode(&payload).map_err(|_| Refused::Malformed).and_then(|h| {
        h.public.check(&Expect { daemon: &keys.id(), remote: &remote, now: now_ms() }, Some(&key), |port, id| {
            public.iter().any(|(p, i)| *p == port && i == id)
        })
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
        let frames = match kind {
            wire::HEAD => {
                let head: ReqHead = serde_json::from_slice(payload).unwrap();
                let names: Vec<&str> = head.headers.iter().map(|(k, _)| k.as_str()).collect();
                let res = ResHead {
                    status: 200,
                    headers: vec![
                        ("content-type".into(), "text/plain".into()),
                        ("set-cookie".into(), "s=1; Path=/; Domain=previews.test".into()),
                    ],
                };
                vec![
                    wire::json_frame(wire::RESPONSE, sid, &res),
                    wire::frame(
                        wire::DATA,
                        sid,
                        format!("{} {} [{}]", head.method, head.path, names.join(",")).as_bytes(),
                    ),
                    wire::frame(wire::END, sid, b""),
                ]
            }
            wire::OPEN => vec![wire::json_frame(wire::ACCEPTED, sid, &serde_json::json!({ "protocol": "" }))],
            wire::MESSAGE => vec![wire::frame(wire::MESSAGE, sid, payload)],
            _ => vec![],
        };
        for f in frames {
            for w in ch.seal(&Msg::Binary(f)).unwrap() {
                write_frame(&mut wr, &w).await;
            }
        }
    }
}

fn host(id: &str) -> String {
    format!("{id}.{DOMAIN}")
}

/// A stranger, signed out, opens a public port; turned off at the
/// platform, it's gone once control's cache lets go, and the machine
/// refuses it even while control's cache still says public.
#[tokio::test]
async fn a_stranger_opens_a_public_port() {
    let c = control().await;
    let keys = c.machine("hw_1");
    let p = c.app.platform.inner().clone();
    p.db.make_port_public("hw_1", 5173, "alice", ID, now_ms() + 60_000, now_ms()).unwrap();
    fake_machine(&c, keys.clone(), "hw_1").await;

    let res = c
        .get(&host(ID), "/hello?x=1")
        .header("fly-client-ip", "203.0.113.9")
        .header("x-forwarded-for", "203.0.113.9")
        .header(header::COOKIE, "s=1")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::SET_COOKIE], "s=1; Path=/", "host-only: no Domain");
    assert_eq!(res.headers()["x-robots-tag"], "noindex");
    assert!(res.headers().get(header::X_FRAME_OPTIONS).is_none(), "none of control's headers");
    let body = res.text().await.unwrap();
    assert!(body.starts_with("GET /hello?x=1 ["), "{body}");
    assert!(body.contains("cookie"), "the preview's own cookies cross: {body}");
    for gone in ["fly-client-ip", "x-forwarded-for", "host"] {
        assert!(!body.contains(gone), "{gone} crossed: {body}");
    }
    // Traffic is metered to the project.
    let day = crate::day(now_ms());
    for _ in 0..100 {
        if c.app.db.relay_bytes(PROJECT, &day).unwrap() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(c.app.db.relay_bytes(PROJECT, &day).unwrap() > 0);

    // The machine's own question, signed as the machine.
    let path = arugula_control_wire::hosted::HOSTED_PREVIEWS;
    let got: Value = c
        .http
        .get(format!("{}{path}", c.base))
        .header("x-arugula-auth", v2(&keys, "GET", path))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(got["ports"][0]["port"], 5173);
    assert_eq!(got["ports"][0]["preview"], ID);

    // Control's own routes aren't on a preview host, nor is anything for
    // an id that isn't public or a name that isn't an id.
    for (h, path) in [(host(ID), "/control.json"), (host(ID), "/api/me")] {
        let r = c.get(&h, path).send().await.unwrap();
        assert!(!r.text().await.unwrap().contains("\"control\""), "{h}{path}");
    }
    for h in [host(&"f".repeat(32)), DOMAIN.to_owned(), format!("www.{DOMAIN}")] {
        let r = c.get(&h, "/").send().await.unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{h}");
        assert!(r.headers().get(header::SET_COOKIE).is_none());
    }

    // Turned off: control still has it cached as public, but the machine
    // asks the platform and refuses; control then says it isn't public.
    assert!(p.db.make_port_private("hw_1", 5173).unwrap());
    let r = c.get(&host(ID), "/").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert!(r.text().await.unwrap().contains("No preview here"));
    // And once control's cache lets go, it doesn't reach the machine at all.
    c.app.previews.forget(ID);
    assert_eq!(c.get(&host(ID), "/").send().await.unwrap().status(), StatusCode::NOT_FOUND);
}

/// A paused box (its daemon not on the relay) isn't woken: the stranger
/// hears it's asleep.
#[tokio::test]
async fn a_paused_box_says_asleep() {
    let c = control().await;
    let _keys = c.machine("hw_2");
    let p = c.app.platform.inner().clone();
    p.db.make_port_public("hw_2", 3000, "alice", ID, now_ms() + 60_000, now_ms()).unwrap();
    let r = c.get(&host(ID), "/").send().await.unwrap();
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.headers()[header::RETRY_AFTER], "30");
    assert!(r.text().await.unwrap().contains("asleep"));
}

/// A WebSocket to a public port (hot reload) crosses both ways.
#[tokio::test]
async fn a_websocket_crosses() {
    let c = control().await;
    let keys = c.machine("hw_3");
    let p = c.app.platform.inner().clone();
    p.db.make_port_public("hw_3", 5173, "alice", ID, now_ms() + 60_000, now_ms()).unwrap();
    fake_machine(&c, keys, "hw_3").await;
    let mut req = format!("{}/ws", c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("host", host(ID).parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::Text("ping".into())).await.unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(got, Message::Text("ping".into()));
}
