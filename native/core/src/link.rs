//! A Noise channel to a machine through control's relay. A thread owns the
//! socket: it seals what's queued, in order, and hands back what arrives.
//! Control carries the channel's bytes and can't read them; the machine's
//! certificate is checked back to the account's root first.

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
use arugula_e2e::{
    Cert, Kind,
    cert::request_auth,
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, ResponseHead, prologue},
};
use arugula_proto::{AttachPane, ClientMsg, Frame, FrameKind, ServerMsg, State};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use serde_json::Value;
use tungstenite::{Message, WebSocket, client::IntoClientRequest};

use crate::{control::Control, net::Sock};

const HANDSHAKE: Duration = Duration::from_secs(20);

/// What a machine sends.
#[derive(Debug)]
pub enum Event {
    /// The machine's state changed, or first said hello.
    State(Box<State>),
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

/// A machine's certificate, by the account's own devices: `id` has to chain
/// back to the root this device pinned, whatever the directory says.
fn machine_cert(c: &Control, id: &str) -> Result<Cert> {
    let trusted = c.trusted()?;
    ensure_device(&trusted, &c.cert.device)?;
    trusted
        .get(id)
        .filter(|m| m.kind == Kind::Daemon)
        .cloned()
        .with_context(|| format!("{id} isn't a machine this account trusts"))
}

fn ensure_device(trusted: &arugula_e2e::cert::Trusted, me: &str) -> Result<()> {
    if trusted.get(me).is_none() {
        bail!("this device isn't one of the account's any more (removed?): pair it again");
    }
    Ok(())
}

impl Link {
    /// Through control's relay to the machine `id`.
    pub fn open(c: &Control, id: &str) -> Result<Self> {
        Self::open_notifying(c, id, || {})
    }

    /// The same, calling `notify` from the socket's thread each time an event
    /// is queued: a UI's cue to look, with no polling.
    pub fn open_notifying(c: &Control, id: &str, notify: impl Fn() + Send + 'static) -> Result<Self> {
        let cert = machine_cert(c, id)?;
        let path = format!("/api/relay/c/{id}");
        let auth = request_auth(&c.keys, "GET", &path, b"");
        let scheme = if c.site.tls { "wss" } else { "ws" };
        let mut req = format!("{scheme}://{}{path}", c.site.authority).into_client_request()?;
        let h = req.headers_mut();
        h.insert("x-arugula-auth", auth.parse()?);
        h.insert("origin", c.url.parse()?);
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
        let (init, m1) = Initiator::start(&c.keys, &cert.noise_key(), &prologue(&cert.device))?;
        ws.send(Message::Binary(m1.into()))?;
        let began = std::time::Instant::now();
        let m2 = loop {
            match ws.read() {
                Ok(Message::Binary(b)) => break b,
                Ok(Message::Close(_)) => bail!("closed during the handshake (not an approved device?)"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                        && began.elapsed() < HANDSHAKE => {}
                Err(e) => return Err(anyhow::Error::from(e).context("no answer to the handshake")),
            }
        };
        let (said, channel) = init.finish(&m2)?;
        if let Some(r) = Refusal::decode(&said) {
            bail!("the machine refused: {}", r.message);
        }
        ws.get_ref().nonblocking()?;
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

    /// Stop receiving `panes`.
    pub fn detach(&self, panes: Vec<u32>) -> Result<()> {
        self.send(&ClientMsg::Detach { panes })
    }

    pub fn input(&self, pane: u32, data: &[u8]) -> Result<()> {
        let f = Frame { kind: FrameKind::Input, pane, offset: 0, data: data.to_vec() };
        self.put(Out::Send(Msg::Binary(f.encode())))
    }

    /// One request to the machine's own API, over the channel.
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
        Ok((head.status, serde_json::from_slice(&got).unwrap_or(Value::Null)))
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
        ServerMsg::Error { message, .. } => Some(Event::Said(message)),
        _ => None,
    }
}
