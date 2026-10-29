//! Waking a hosted machine on connect (M182) over real sockets: the hub,
//! its platform in process with a provider in memory and its own worker,
//! and a hosted box that enrolls, dials the relay and freezes as a paused
//! box does (its socket open, nothing read). Opening it wakes the box once
//! however many open it, a box that doesn't wake says why, and nothing
//! that didn't ask wakes it.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering::SeqCst},
    },
    time::{Duration, Instant},
};

use arugula_control_wire::hosted::EnrollGiven;
use arugula_e2e::{DeviceKeys, now_ms};
use arugula_platform::{
    HubPort as _,
    compute::{self, Create, Created, Paused, Resumed, Sandbox, State, Substrate},
};
use futures_util::{SinkExt as _, StreamExt as _, future::BoxFuture};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use crate::{
    hosted_wire::{PROJECT, ask, delegation, enroll_body},
    routing_wire::{Control, control, person, session, v2},
};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A provider in memory with one box, `sbx1`. A resume is counted and
/// told on `resumed`; with `gone`, the box is gone.
#[derive(Default)]
struct Boxes {
    state: Mutex<Option<State>>,
    resumes: AtomicU32,
    resumed: tokio::sync::Notify,
}

fn ready<T: Send + 'static>(v: compute::Result<T>) -> BoxFuture<'static, compute::Result<T>> {
    Box::pin(async move { v })
}

impl Substrate for Boxes {
    fn create<'a>(&'a self, _: &'a Create) -> BoxFuture<'a, compute::Result<Created>> {
        ready(Err(compute::Error::Refused { status: 400, code: None }))
    }
    fn resume<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, compute::Result<Resumed>> {
        self.resumes.fetch_add(1, SeqCst);
        let mut s = self.state.lock().unwrap();
        let r = match *s {
            None => Err(compute::Error::NotFound),
            Some(State::Paused) => Ok(Resumed::Woke),
            Some(State::Running) => Ok(Resumed::Running),
        };
        if r.is_ok() {
            *s = Some(State::Running);
            self.resumed.notify_waiters();
        }
        ready(r)
    }
    fn pause<'a>(&'a self, _: &'a str) -> BoxFuture<'a, compute::Result<Paused>> {
        ready(Ok(Paused::Paused))
    }
    fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, compute::Result<()>> {
        ready(Ok(()))
    }
    fn status<'a>(&'a self, id: &'a str) -> BoxFuture<'a, compute::Result<Option<Sandbox>>> {
        let state = *self.state.lock().unwrap();
        ready(Ok(state.map(|state| Sandbox {
            id: id.into(),
            template: "tpl".into(),
            state,
            metadata: BTreeMap::new(),
            started_at: String::new(),
            end_at: String::new(),
            pauses_on_timeout: Some(true),
            cpus: None,
            memory_mb: None,
        })))
    }
    fn list<'a>(&'a self, _: &'a BTreeMap<String, String>) -> BoxFuture<'a, compute::Result<Vec<Sandbox>>> {
        ready(Ok(vec![]))
    }
    fn hold<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, compute::Result<()>> {
        ready(Ok(()))
    }
}

struct Rig {
    c: Control,
    boxes: Arc<Boxes>,
    /// The box's keys: its machine's id.
    k: DeviceKeys,
    cookie: String,
}

/// Me, my project delegated, and my hosted workspace `hw_1`, whose box
/// enrolled (not dialed in yet) and is `ready` as the platform last saw.
async fn rig() -> Rig {
    let boxes = Arc::new(Boxes::default());
    *boxes.state.lock().unwrap() = Some(State::Running);
    let sub = boxes.clone();
    let c = control(move |a| a.platform.inner_mut().compute = Some(sub)).await;
    let me = person(&c.app, "me", "m1");
    let cookie = session(&c.app, "m1");
    let d = delegation(&c, "account:m1", &me);
    c.as_person(&cookie, "POST", "/api/hosted/delegations", Some(json!({ "delegation": d }))).await;
    let EnrollGiven::Ready { ticket: Some(t), .. } =
        c.app.hub_port.hosted_enrollment(&ask("account:m1", "hw_1", None)).await.unwrap()
    else {
        panic!("no ticket")
    };
    let k = DeviceKeys::generate();
    assert_eq!(c.post("/api/hosted/enroll", &enroll_body(&k, Some(&t), None)).await.0, 200);
    // The platform's row for it: ready, its box `sbx1`.
    let p = c.app.platform.inner_arc();
    let spec = arugula_platform::hosted::Spec::default();
    p.db.add_hosted_workspace("hw_1", PROJECT, "dev", &spec, "m1", 10, now_ms()).unwrap();
    let mut job = p.db.claim_hosted(now_ms()).unwrap().unwrap();
    job.w.state = "ready".into();
    job.w.machine = Some(k.id());
    job.sandbox = Some("sbx1".into());
    p.db.checkpoint_hosted(&job, None, now_ms() + 3_600_000, now_ms()).unwrap();
    arugula_platform::hosted::worker::start(p);
    Rig { c, boxes, k, cookie }
}

impl Rig {
    /// The box dials the relay, as its daemon does: the socket, unread
    /// (frozen, as a paused box's is) until something polls it.
    async fn dial(&self) -> Ws {
        let was = self.c.app.relay.mux_of(&self.k.id()).map(|(_, g)| g);
        let mut req =
            format!("{}/api/relay/dial", self.c.base.replace("http://", "ws://")).into_client_request().unwrap();
        req.headers_mut().insert("x-arugula-auth", v2(&self.k, "GET", "/api/relay/dial", b"").parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req).await.expect("the box dials the relay");
        for _ in 0..100 {
            if self.c.app.relay.mux_of(&self.k.id()).is_some_and(|(_, g)| Some(g) != was) {
                return ws;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the box never came onto the relay");
    }

    /// An open of the box's machine through `/api/relay/c`: its upgrade's
    /// status, and what an answer that isn't one says.
    async fn open(&self, wake: bool) -> (u16, Option<String>, Value) {
        let mut req = format!("{}/api/relay/c/{}", self.c.base.replace("http://", "ws://"), self.k.id())
            .into_client_request()
            .unwrap();
        req.headers_mut().insert("cookie", self.cookie.parse().unwrap());
        req.headers_mut().insert("origin", self.c.base.parse().unwrap());
        if wake {
            req.headers_mut().insert(crate::wake::HEADER, "1".parse().unwrap());
        }
        match tokio_tungstenite::connect_async(req).await {
            Ok((_, r)) => (r.status().as_u16(), None, Value::Null),
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => {
                let code = r.headers().get(crate::wake::HEADER).map(|v| v.to_str().unwrap().to_owned());
                let body = r.body().as_ref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default();
                (r.status().as_u16(), code, body)
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Keep reading a box's socket, as a daemon that's up does: it answers
/// pings.
fn up(mut ws: Ws) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move { while let Some(Ok(_)) = ws.next().await {} })
}

/// The box paused (its socket open, frozen). Three opens at once, asking
/// to wake it: one resume, and once the box's daemon dials in again every
/// open goes through. An open that doesn't ask hears it's asleep, and
/// doesn't wake it.
#[tokio::test]
async fn opening_a_sleeping_hosted_machine_wakes_it_once() {
    let r = rig().await;
    let frozen = r.dial().await;
    *r.boxes.state.lock().unwrap() = Some(State::Paused);
    // Quiet past the time it's taken as up without asking.
    tokio::time::sleep(Duration::from_millis(2_100)).await;

    // The box's daemon dials in again as it resumes (M180).
    let redial = async {
        r.boxes.resumed.notified().await;
        r.dial().await
    };
    let started = Instant::now();
    let opens = futures_util::future::join_all((0..3).map(|_| r.open(true)));
    let (opened, woke) = tokio::join!(opens, redial);
    println!("three opens went through {} ms after they began", started.elapsed().as_millis());
    for o in &opened {
        assert_eq!(o.0, 101, "{o:?}");
    }
    assert_eq!(r.boxes.resumes.load(SeqCst), 1, "one resume");
    drop(frozen);

    // Up and answering: an open goes through without asking the provider.
    let live = up(woke);
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    assert_eq!(r.open(true).await.0, 101);
    assert_eq!(r.boxes.resumes.load(SeqCst), 1, "a box that's up isn't woken");
    live.abort();

    // Paused again, opened by something that didn't ask to wake it.
    let _frozen = r.dial().await;
    *r.boxes.state.lock().unwrap() = Some(State::Paused);
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    let (status, code, body) = r.open(false).await;
    assert_eq!((status, code.as_deref()), (503, Some("asleep")), "{body}");
    assert_eq!(r.boxes.resumes.load(SeqCst), 1, "not woken");
    assert!(r.c.app.relay.mux_of(&r.k.id()).is_none(), "its frozen socket was hung up on");
}

/// Through a page's shared socket: `OPEN_WAKE` to a box that's gone hears
/// `waking`, then `failed` with why (code `gone`), then the close; an
/// `OPEN` of a sleeping one hears `asleep`.
#[tokio::test]
async fn a_page_hears_waking_and_why_a_box_didnt_wake() {
    let r = rig().await;
    let mut req = format!("{}/api/relay/m", r.c.base.replace("http://", "ws://")).into_client_request().unwrap();
    req.headers_mut().insert("cookie", r.cookie.parse().unwrap());
    req.headers_mut().insert("origin", r.c.base.parse().unwrap());
    let (mut page, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let frame = |kind: u8, chan: u32, payload: &[u8]| {
        let mut f = vec![kind];
        f.extend_from_slice(&chan.to_be_bytes());
        f.extend_from_slice(payload);
        Message::Binary(f.into())
    };
    let id = r.k.id();
    async fn next(page: &mut Ws) -> (u8, u32, Vec<u8>) {
        loop {
            match tokio::time::timeout(Duration::from_secs(30), page.next()).await.expect("an answer").unwrap().unwrap()
            {
                Message::Binary(b) => return (b[0], u32::from_be_bytes(b[1..5].try_into().unwrap()), b[5..].to_vec()),
                _ => continue,
            }
        }
    }

    // Not connected, not asked to wake: asleep.
    page.send(frame(1, 7, id.as_bytes())).await.unwrap();
    let (kind, chan, said) = next(&mut page).await;
    assert_eq!((kind, chan), (6, 7));
    assert_eq!(serde_json::from_slice::<Value>(&said).unwrap(), json!({ "state": "asleep" }));
    assert_eq!(next(&mut page).await.0, 3, "then the close");

    // Gone at the provider: waking, then failed (gone), then the close.
    *r.boxes.state.lock().unwrap() = None;
    page.send(frame(5, 8, id.as_bytes())).await.unwrap();
    let (kind, _, said) = next(&mut page).await;
    assert_eq!((kind, serde_json::from_slice::<Value>(&said).unwrap()), (6, json!({ "state": "waking" })));
    let (kind, _, said) = next(&mut page).await;
    let said: Value = serde_json::from_slice(&said).unwrap();
    assert_eq!((kind, said["state"].as_str(), said["code"].as_str()), (6, Some("failed"), Some("gone")), "{said}");
    assert!(said["why"].as_str().unwrap().contains("Rebuild"), "{said}");
    let (kind, _, why) = next(&mut page).await;
    assert_eq!(kind, 3);
    assert_eq!(String::from_utf8(why).unwrap(), said["why"].as_str().unwrap());
    assert_eq!(r.boxes.resumes.load(SeqCst), 1);
}

/// The CLI's open (`/api/relay/c` with `x-arugula-wake: 1`) of a paused
/// box whose old socket comes back to life as the box resumes, as E2B's
/// can, and whose daemon dials in afresh a while later (M180's redial on
/// resume, here 900 ms on). The open waits for the new connection and its
/// stream goes there, never onto the old socket the daemon is about to
/// drop.
#[tokio::test]
async fn a_cli_open_of_a_frozen_hosted_machine_waits_for_the_redial() {
    let r = rig().await;
    let frozen = r.dial().await;
    *r.boxes.state.lock().unwrap() = Some(State::Paused);
    tokio::time::sleep(Duration::from_millis(600)).await;

    // As the box resumes: the old socket reads again (it answers pings,
    // and says what it was sent), and the daemon dials in afresh later.
    let (old_tx, mut old_rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let (new_tx, mut new_rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let resume = async {
        r.boxes.resumed.notified().await;
        let mut old = frozen;
        let revived = tokio::spawn(async move {
            while let Some(Ok(m)) = old.next().await {
                if let Message::Binary(b) = m {
                    let _ = old_tx.send(b[0]);
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(900)).await;
        let mut fresh = r.dial().await;
        tokio::spawn(async move {
            while let Some(Ok(m)) = fresh.next().await {
                if let Message::Binary(b) = m {
                    let _ = new_tx.send(b[0]);
                }
            }
        });
        Some(revived)
    };
    let started = Instant::now();
    let (opened, revived) = tokio::join!(r.open(true), resume);
    println!("the CLI's open went through {} ms after it began", started.elapsed().as_millis());
    assert_eq!(opened.0, 101, "{opened:?}");
    assert_eq!(r.boxes.resumes.load(SeqCst), 1);
    let first = tokio::time::timeout(Duration::from_secs(5), new_rx.recv()).await.expect("a stream on the new socket");
    assert_eq!(first, Some(1), "the open's stream, on the daemon's new connection");
    assert!(old_rx.try_recv().is_err(), "nothing opened on the old socket");
    // The hub hung up on the old socket.
    tokio::time::timeout(Duration::from_secs(5), revived.unwrap()).await.expect("the old socket closed").unwrap();
}
