//! A Noise channel to a machine through control's relay. A thread owns the
//! socket: it seals what's queued, in order, and hands back what arrives.
//! Control carries the channel's bytes and can't read them. As the CLI opens
//! one (aru-141, #998): to the Noise key control's directory gives for the
//! machine, with a key made for this channel alone, and a machine token from
//! control (`POST /api/machines/{id}/token`) in its first message, which the
//! machine checks itself.

use std::{
    io::{ErrorKind, Read, Write},
    os::{fd::AsFd, unix::net::UnixStream},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use arugula_control_wire::machine_token::{TokenHello, token_path};
use arugula_e2e::{
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, ResponseHead, prologue},
    keys::DeviceKeys,
};
use arugula_proto::{AttachPane, ClientMsg, Delta, Frame, FrameKind, ServerMsg, State};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, client::IntoClientRequest};

use crate::{
    control::{Control, Machine},
    net::Sock,
};

const HANDSHAKE: Duration = Duration::from_secs(20);

/// What a machine sends.
#[derive(Debug)]
pub enum Event {
    /// The machine's state changed, or first said hello.
    State(Box<State>),
    /// Panes' fields that changed since the last `State` or `Delta`.
    Delta(Box<Delta>),
    /// VT bytes that reproduce the pane: start over from them.
    Snapshot {
        pane: u32,
        data: Vec<u8>,
    },
    Output {
        pane: u32,
        data: Vec<u8>,
    },
    /// What it refused, in its words.
    Said(String),
    /// The channel ended.
    Closed(String),
}

enum Out {
    Send(Msg),
    Ask(u32, Msg, mpsc::Sender<(ResponseHead, Vec<u8>)>),
}

/// An open channel to one machine. Cheap to share; dropping the last one ends
/// its thread.
pub struct Link {
    out: mpsc::Sender<Out>,
    wake: Arc<UnixStream>,
    next: AtomicU32,
    /// What the machine sends, for one reader at a time.
    pub events: std::sync::Mutex<mpsc::Receiver<Event>>,
}

/// The machine's refusal of a channel opened with a machine token, with its
/// code: `machine-token` (the token wasn't taken), or `malformed` (a daemon
/// from before machine tokens).
#[derive(Debug)]
pub struct Refused {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code == "malformed" {
            write!(f, "the machine's arugulad is older than machine tokens (0.70.4): update it there")
        } else {
            write!(f, "the machine refused: {}", self.message)
        }
    }
}

impl std::error::Error for Refused {}

/// A machine token for machine `id`, fresh: the machine takes each once.
fn machine_token(c: &Control, id: &str) -> Result<String> {
    let v = c.post(&token_path(id), &json!({})).context("Arugula cloud gave no token for this machine")?;
    v["token"].as_str().filter(|t| !t.is_empty()).map(str::to_owned).context("Arugula cloud's answer has no token")
}

impl Link {
    /// Through control's relay to the machine `id`.
    pub fn open(c: &Control, id: &str) -> Result<Self> {
        Self::open_notifying(c, id, || {})
    }

    /// The same, calling `notify` from the socket's thread each time an event
    /// is queued: a UI's cue to look, with no polling. A machine that refuses
    /// the token itself gets one more, fresh: it refuses a token issued before
    /// it started, which right after a restart can be a new one when
    /// control's clock is behind the machine's.
    pub fn open_notifying(c: &Control, id: &str, notify: impl Fn() + Send + 'static) -> Result<Self> {
        let m = c.machine(id)?;
        match Self::connect(c, &m) {
            Err(e) if e.downcast_ref::<Refused>().is_some_and(|r| r.code == "machine-token") => {}
            r => return r.and_then(|(ws, ch)| Self::run(ws, ch, notify)),
        }
        let (ws, ch) = Self::connect(c, &m)?;
        Self::run(ws, ch, notify)
    }

    /// The relay's WebSocket and the handshake on it.
    fn connect(c: &Control, m: &Machine) -> Result<(WebSocket<Sock>, Channel)> {
        let path = format!("/api/relay/c/{}", m.id);
        let scheme = if c.site.tls { "wss" } else { "ws" };
        let mut req = format!("{scheme}://{}{path}", c.site.authority).into_client_request()?;
        let h = req.headers_mut();
        for (k, v) in c.auth() {
            h.insert(k, v.parse()?);
        }
        // A hosted machine that's asleep is woken (M182): the app opens a
        // machine only when someone asks it to.
        h.insert("x-arugula-wake", "1".parse()?);
        let sock = c.site.connect()?;
        let (mut ws, _) = tungstenite::client(req, sock).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r)) => {
                let body = r.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default().into_owned();
                let said = serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_owned))
                    .unwrap_or(body);
                anyhow!("the relay refused ({}): {said}", r.status())
            }
            tungstenite::HandshakeError::Failure(e) => anyhow::Error::from(e).context("the relay's websocket"),
            tungstenite::HandshakeError::Interrupted(_) => anyhow!("the relay's websocket was interrupted"),
        })?;
        // Asked for only now that the socket is there: the machine takes each
        // token once.
        let hello = TokenHello { machine_token: machine_token(c, &m.id)? }.encode();
        let (init, m1) = Initiator::start_with(&DeviceKeys::generate(), &m.noise, &prologue(&m.id), &hello)?;
        ws.send(Message::Binary(m1.into()))?;
        let began = std::time::Instant::now();
        let m2 = loop {
            match ws.read() {
                Ok(Message::Binary(b)) => break b,
                Ok(Message::Close(_)) => bail!("{} closed the channel during the handshake", m.name),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                        && began.elapsed() < HANDSHAKE => {}
                Err(e) => return Err(anyhow::Error::from(e).context("no answer to the handshake")),
            }
        };
        let (said, channel) = init.finish(&m2)?;
        if let Some(r) = Refusal::decode(&said) {
            return Err(Refused { code: r.refused, message: r.message }.into());
        }
        ws.get_ref().nonblocking()?;
        Ok((ws, channel))
    }

    /// The socket's thread, from an open channel.
    fn run(ws: WebSocket<Sock>, channel: Channel, notify: impl Fn() + Send + 'static) -> Result<Self> {
        let (out, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let (wake, woken) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        woken.set_nonblocking(true)?;
        std::thread::spawn(move || {
            let why = io(ws, channel, rx, woken, &events_tx, &notify)
                .map_or_else(|e| format!("{e:#}"), |()| "closed".to_owned());
            let _ = events_tx.send(Event::Closed(why));
            notify();
        });
        Ok(Self { out, wake: Arc::new(wake), next: AtomicU32::new(1), events: std::sync::Mutex::new(events) })
    }

    fn put(&self, o: Out) -> Result<()> {
        self.out.send(o).map_err(|_| anyhow!("the channel closed"))?;
        let _ = (&*self.wake).write(&[1]);
        Ok(())
    }

    fn send(&self, m: &ClientMsg) -> Result<()> {
        self.put(Out::Send(Msg::Text(serde_json::to_string(m)?)))
    }

    /// Receive `pane`, sized to the view: its snapshot and then its output
    /// come as [`Event`]s.
    pub fn attach(&self, tab: u32, pane: u32, cols: u16, rows: u16) -> Result<()> {
        self.send(&ClientMsg::View { tab, cols, rows, zoom: Some(pane), claim: true, typed: false, asked: false })?;
        self.send(&ClientMsg::Attach {
            panes: vec![AttachPane::new(pane, None)],
            zstd: false,
            acks: false,
            kitty_keys: false,
        })
    }

    /// The view's size in cells, when it changes.
    pub fn view(&self, tab: u32, pane: u32, cols: u16, rows: u16) -> Result<()> {
        self.send(&ClientMsg::View { tab, cols, rows, zoom: Some(pane), claim: true, typed: false, asked: false })
    }

    /// Watch `tab` whole, at the size it is (`cols`x`rows`, its own): a view
    /// that claims nothing, so nobody's panes resize, and `panes` attached.
    pub fn watch(&self, tab: u32, panes: Vec<u32>, cols: u16, rows: u16) -> Result<()> {
        self.send(&ClientMsg::View { tab, cols, rows, zoom: None, claim: false, typed: false, asked: false })?;
        let panes = panes.into_iter().map(|p| AttachPane::new(p, None)).collect();
        self.send(&ClientMsg::Attach { panes, zstd: false, acks: false, kitty_keys: false })
    }

    /// Receive `panes` at the size each already is, from any tab, whatever
    /// is in view: a view that claims nothing, so nobody's panes resize (a
    /// card's panes at the Overview's Work level, Arugula #1307).
    pub fn attach_panes(&self, panes: Vec<u32>) -> Result<()> {
        let panes = panes.into_iter().map(|p| AttachPane::new(p, None)).collect();
        self.send(&ClientMsg::Attach { panes, zstd: false, acks: false, kitty_keys: false })
    }

    /// Stop receiving `panes`.
    pub fn detach(&self, panes: Vec<u32>) -> Result<()> {
        self.send(&ClientMsg::Detach { panes })
    }

    pub fn input(&self, pane: u32, data: &[u8]) -> Result<()> {
        let f = Frame { kind: FrameKind::Input, pane, offset: 0, data: data.to_vec() };
        self.put(Out::Send(Msg::Binary(f.encode())))
    }

    /// One request to the machine's own API, over the channel: its status
    /// and its answer, JSON as itself and anything else (a capture's text) as
    /// a string.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>, wait: Duration) -> Result<(u16, Value)> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let bytes = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
        let head = RequestHead {
            method: method.into(),
            path: path.into(),
            content_type: body.map(|_| "application/json".to_owned()),
            stream: false,
            headers: Default::default(),
        };
        let (tx, rx) = mpsc::channel();
        self.put(Out::Ask(id, Msg::Request { id, head, body: bytes }, tx))?;
        let (head, got) =
            rx.recv_timeout(wait).map_err(|_| anyhow!("no answer from the machine to {method} {path}"))?;
        // JSON as itself; any other answer (a pane's text) as a string.
        let v = serde_json::from_slice(&got).unwrap_or_else(|_| {
            if got.is_empty() { Value::Null } else { Value::String(String::from_utf8_lossy(&got).into_owned()) }
        });
        Ok((head.status, v))
    }
}

/// The socket's whole life.
fn io(
    mut ws: WebSocket<Sock>,
    channel: Channel,
    rx: mpsc::Receiver<Out>,
    mut woken: UnixStream,
    events: &mpsc::Sender<Event>,
    notify: &dyn Fn(),
) -> Result<()> {
    let mut waiting = std::collections::HashMap::new();
    loop {
        {
            let sock = ws.get_ref();
            // SAFETY: the fd is the socket `ws` owns, open for this borrow.
            let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(sock.fd()) };
            let mut fds = [PollFd::new(fd, PollFlags::POLLIN), PollFd::new(woken.as_fd(), PollFlags::POLLIN)];
            poll(&mut fds, PollTimeout::from(1000u16)).context("waiting on the socket")?;
        }
        let mut sink = [0u8; 64];
        while matches!(woken.read(&mut sink), Ok(n) if n > 0) {}
        loop {
            match rx.try_recv() {
                Ok(Out::Send(m)) => seal(&mut ws, &channel, &m)?,
                Ok(Out::Ask(id, m, tx)) => {
                    waiting.insert(id, tx);
                    seal(&mut ws, &channel, &m)?;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // Every Link is gone.
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        loop {
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    let Some(m) = channel.open(&b)? else { continue };
                    match m {
                        Msg::Text(t) => {
                            if let Some(e) = server_event(&t) {
                                let _ = events.send(e);
                                notify();
                            }
                        }
                        Msg::Binary(b) => {
                            if let Ok(f) = Frame::decode(&b) {
                                let e = match f.kind {
                                    FrameKind::Snapshot => Event::Snapshot { pane: f.pane, data: f.data },
                                    FrameKind::Output => Event::Output { pane: f.pane, data: f.data },
                                    _ => continue,
                                };
                                let _ = events.send(e);
                                notify();
                            }
                        }
                        Msg::Response { id, head, body } => {
                            if let Some(tx) = waiting.remove(&id) {
                                let _ = tx.send((head, body));
                            }
                        }
                        Msg::Request { .. } => {}
                    }
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn seal(ws: &mut WebSocket<Sock>, channel: &Channel, m: &Msg) -> Result<()> {
    for wire in channel.seal(m)? {
        match ws.write(Message::Binary(wire.into())) {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn server_event(t: &str) -> Option<Event> {
    match serde_json::from_str(t).ok()? {
        ServerMsg::Hello { state, .. } | ServerMsg::State { state } => Some(Event::State(Box::new(state))),
        ServerMsg::Delta { delta } => Some(Event::Delta(Box::new(delta))),
        ServerMsg::Error { message, .. } => Some(Event::Said(message)),
        _ => None,
    }
}
