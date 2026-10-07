//! Client fixtures (#200), the daemon's end: recording what a client and a
//! daemon said to each other ([`Session`]), and replaying a fixture's client
//! side against a daemon and checking its answers ([`replay`]). The format,
//! normalizing and matching are `arugula_proto::fixture`; `docs/testing.md`
//! has the rest.
//!
//! ```ignore
//! let d = arugula_testkit::fixture::daemon(bin, "rec").record("crates/proto/fixtures").start();
//! let s = d.fixture("run-capture", "arugula run, then capture");
//! let pane = s.post("/api/run", json!({"command": "echo hi"}))["pane"].as_u64().unwrap();
//! s.get(&format!("/api/panes/{pane}/wait?until=exit"));
//! s.finish();
//! ```

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{BufRead, BufReader, ErrorKind, Read, Write},
    net::{Shutdown, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use arugula_proto::{
    Frame, FrameKind, PaneId,
    fixture::{self, Event, Fixture, Header, Normalize, Response, Side, What},
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{self, Message, WebSocket};

use crate::{Builder, Daemon};

/// How long a step waits for the daemon.
const STEP: Duration = Duration::from_secs(20);

/// A daemon set up the way every fixture is recorded and replayed:
/// `bash --norc --noprofile` with `PS1='$ '` (and no macOS bash banner), a
/// `$HOME` of its own in its state dir (made when it starts), no wisp or
/// tailscaled, not Claude Code's IDE, and no update check. Its `$HOME`, the
/// state dir, ports and token come out of a fixture as placeholders, so a
/// replay finds the same fixture.
pub fn daemon(bin: impl Into<PathBuf>, tag: &str) -> Builder {
    Builder::new(bin, tag)
        .no_wisp()
        .no_tailscale()
        .args(["--no-claude-ide", "--no-update-check"])
        .env("PS1", "$ ")
        .env("BASH_SILENCE_DEPRECATION_WARNING", "1")
        .env_remove("ARUGULA_PANE")
        .home_in_state()
}

/// Wait until the first pane's shell has drawn its prompt, so what it says
/// on starting (its cwd, its first prompt) is over before a session starts.
pub fn ready(d: &Daemon) {
    d.wait_for("the first prompt", || d.raw("GET", "/api/panes/1/capture", None).1.contains('$'));
}

/// The placeholders for one daemon: its state dir, `$HOME`, socket, the
/// temp dir, its ports, its token, and this machine's name and user.
fn normalizer(d: &Daemon) -> Normalize {
    let mut n = Normalize::new();
    let both = |n: &mut Normalize, p: &Path, ph: &str| {
        n.sub(p.display().to_string(), ph);
        if let Ok(c) = p.canonicalize() {
            n.sub(c.display().to_string(), ph);
        }
    };
    both(&mut n, &d.state, "<state>");
    both(&mut n, &d.home(), "<home>");
    both(&mut n, &d.sock(), "<sock>");
    let tmp = std::env::temp_dir();
    both(&mut n, Path::new(tmp.to_str().unwrap_or_default().trim_end_matches('/')), "<tmp>");
    for port in [d.port, d.block_port].into_iter().filter(|p| *p != 0) {
        n.sub(format!("127.0.0.1:{port}"), "127.0.0.1:<port>");
        n.sub(format!("localhost:{port}"), "localhost:<port>");
    }
    n.sub(d.token(), "<token>");
    if let Some(h) = hostname().filter(|h| h.len() >= 4) {
        if let Some((short, _)) = h.split_once('.').filter(|(s, _)| s.len() >= 4) {
            n.sub(short, "<hostname>");
        }
        n.sub(h, "<hostname>");
    }
    if let Some(u) = std::env::var("USER").ok().filter(|u| u.len() >= 3) {
        n.sub(u, "<user>");
    }
    n
}

fn hostname() -> Option<String> {
    let out = std::process::Command::new("hostname").output().ok()?;
    let h = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!h.is_empty()).then_some(h)
}

/// What's recorded so far, shared by a session's connections.
struct Rec {
    start: Instant,
    norm: Normalize,
    events: Vec<Event>,
}

impl Rec {
    /// Record `what`, normalized: the daemon's values teach the normalizer
    /// the session names first.
    fn push(&mut self, c: u32, from: Side, mut what: What) {
        let n = &mut self.norm;
        match &mut what {
            What::Request { path, body, .. } => {
                *path = n.string(path);
                if let Some(b) = body {
                    n.value(b);
                }
            }
            What::Response(r) => {
                if let Some(b) = &mut r.body {
                    n.learn(b);
                    n.value(b);
                }
                if let Some(t) = &mut r.text {
                    *t = n.string(t);
                }
            }
            What::Line(v) | What::Msg(v) => {
                if from == Side::Daemon {
                    n.learn(v);
                }
                n.value(v);
            }
            What::Open(p) => *p = n.string(p),
            What::Frame(_) | What::Close(_) => {}
        }
        let t = self.start.elapsed().as_millis() as u64;
        self.events.push(Event { t, c, from, what });
    }
}

/// One recorded session with a daemon: requests over its TCP port with the
/// local token, as a client on this machine makes them, and WebSockets.
/// Everything that crosses the wire is recorded; [`Session::finish`]
/// writes it to the builder's [`Builder::record`] dir as
/// `<name>.jsonl`. Without a record dir it's only a client.
pub struct Session<'d> {
    d: &'d Daemon,
    header: Header,
    rec: Arc<Mutex<Rec>>,
    next: Arc<AtomicU32>,
}

impl Daemon {
    /// Start recording a session named `name` (its file name), saying what
    /// it does in `about`.
    pub fn fixture(&self, name: &str, about: &str) -> Session<'_> {
        let header = Header { fixture: name.into(), version: env!("CARGO_PKG_VERSION").into(), about: about.into() };
        let rec = Rec { start: Instant::now(), norm: normalizer(self), events: vec![] };
        Session { d: self, header, rec: Arc::new(Mutex::new(rec)), next: Arc::new(AtomicU32::new(1)) }
    }

    /// Its `$HOME`: the builder's, or the test's.
    pub fn home(&self) -> PathBuf {
        let set = self.b.env.iter().rev().find(|(k, _)| k == "HOME").and_then(|(_, v)| v.clone());
        set.or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default()
    }
}

impl Session<'_> {
    fn conn(&self) -> u32 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn push(&self, c: u32, from: Side, what: What) {
        self.rec.lock().unwrap().push(c, from, what);
    }

    /// One request: its status and body (JSON, or the text as a string).
    pub fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let c = self.conn();
        self.push(c, Side::Client, What::Request { method: method.into(), path: path.into(), body: body.clone() });
        let mut res = send(self.d.port, &self.d.bearer(), method, path, body.as_ref()).expect("the request");
        let bytes = res.body().expect("the answer");
        let (json, text) = match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) if res.content_type.contains("json") => (Some(v), None),
            _ => (None, Some(String::from_utf8_lossy(&bytes).into_owned())),
        };
        let out = json.clone().unwrap_or_else(|| Value::String(text.clone().unwrap_or_default()));
        let r = Response { status: res.status, content_type: res.content_type, body: json, text, stream: false };
        self.push(c, Side::Daemon, What::Response(r));
        (res.status, out)
    }

    /// GET, which must answer 200.
    pub fn get(&self, path: &str) -> Value {
        let (status, v) = self.request("GET", path, None);
        assert_eq!(status, 200, "{path}: {v}");
        v
    }

    /// POST JSON, which must answer 200.
    pub fn post(&self, path: &str, body: Value) -> Value {
        let (status, v) = self.request("POST", path, Some(body));
        assert_eq!(status, 200, "{path}: {v}");
        v
    }

    /// A GET whose answer streams lines (`/api/events?follow=1`).
    pub fn stream(&self, path: &str) -> Lines {
        let c = self.conn();
        self.push(c, Side::Client, What::Request { method: "GET".into(), path: path.into(), body: None });
        let mut res = send(self.d.port, &self.d.bearer(), "GET", path, None).expect("the request");
        assert_eq!(res.status, 200, "{path}");
        let r = Response {
            status: res.status,
            content_type: res.content_type.clone(),
            body: None,
            text: None,
            stream: true,
        };
        self.push(c, Side::Daemon, What::Response(r));
        let sock = res.r.get_ref().try_clone().unwrap();
        let (tx, rx) = mpsc::channel();
        let rec = self.rec.clone();
        // Lines are recorded as they come, so they land in order with
        // whatever the test does meanwhile.
        std::thread::spawn(move || {
            while let Some(line) = res.line() {
                let v: Value = serde_json::from_str(&line).unwrap_or(Value::String(line));
                rec.lock().unwrap().push(c, Side::Daemon, What::Line(v.clone()));
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Lines { c, rx, sock, rec: self.rec.clone() }
    }

    /// A WebSocket at `/ws`.
    pub fn ws(&self) -> Ws {
        let c = self.conn();
        self.push(c, Side::Client, What::Open("/ws".into()));
        let stream = TcpStream::connect(("127.0.0.1", self.d.port)).unwrap();
        let (ws, _) = tungstenite::client(self.d.ws("/ws"), stream).expect("the WebSocket");
        ws.get_ref().set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        Ws { c, ws, rec: self.rec.clone(), queue: VecDeque::new(), shift: HashMap::new(), text: HashMap::new() }
    }

    /// Write the fixture, if this daemon records: its path.
    pub fn finish(self) -> Option<PathBuf> {
        let dir = self.d.b.record.clone()?;
        let events = std::mem::take(&mut self.rec.lock().unwrap().events);
        let f = Fixture { header: self.header, events };
        Some(f.save(&dir).expect("writing the fixture"))
    }
}

/// A streamed answer's lines.
pub struct Lines {
    c: u32,
    rx: mpsc::Receiver<Value>,
    sock: TcpStream,
    rec: Arc<Mutex<Rec>>,
}

impl Lines {
    /// The next line, waiting up to `timeout`.
    pub fn next(&self, timeout: Duration) -> Option<Value> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Lines until one `f` likes, which it returns; panics after 20 s.
    pub fn until(&self, what: &str, mut f: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + STEP;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let v = self.next(left).unwrap_or_else(|| panic!("timed out waiting for {what}"));
            if f(&v) {
                return v;
            }
        }
    }

    /// Stop reading: the client closes the connection.
    pub fn close(self) {
        self.rec.lock().unwrap().push(self.c, Side::Client, What::Close(true));
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

/// What came in on a [`Ws`].
#[derive(Debug, Clone)]
pub enum WsIn {
    Msg(Value),
    Frame(Frame),
}

/// A WebSocket to the daemon, recorded.
pub struct Ws {
    c: u32,
    ws: WebSocket<TcpStream>,
    rec: Arc<Mutex<Rec>>,
    /// Read and recorded, not yet handed to the test.
    queue: VecDeque<WsIn>,
    shift: HashMap<PaneId, i64>,
    /// Each pane's output so far, as plain text.
    text: HashMap<PaneId, Vec<u8>>,
}

impl Ws {
    /// Read what has arrived (up to `timeout` for the first), recording it.
    fn fill(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::String(t.to_string()));
                    self.rec.lock().unwrap().push(self.c, Side::Daemon, What::Msg(v.clone()));
                    self.queue.push_back(WsIn::Msg(v));
                }
                Ok(Message::Binary(b)) => {
                    let Ok(f) = Frame::decode(&b) else { continue };
                    let mut rec = self.rec.lock().unwrap();
                    let r = rec.norm.frame(&f, &mut self.shift);
                    rec.push(self.c, Side::Daemon, What::Frame(r));
                    drop(rec);
                    if matches!(f.kind, FrameKind::Output | FrameKind::Snapshot) {
                        self.text.entry(f.pane).or_default().extend_from_slice(&f.data);
                    }
                    self.queue.push_back(WsIn::Frame(f));
                }
                Ok(Message::Close(_)) => panic!("the daemon closed the WebSocket"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    if Instant::now() >= deadline || !self.queue.is_empty() {
                        return;
                    }
                }
                Err(e) => panic!("the WebSocket: {e}"),
            }
        }
    }

    /// Send a control message. What arrived before it is recorded first.
    pub fn send(&mut self, msg: Value) {
        self.fill(Duration::ZERO);
        self.rec.lock().unwrap().push(self.c, Side::Client, What::Msg(msg.clone()));
        self.ws.send(Message::Text(msg.to_string().into())).unwrap();
    }

    /// Type into a pane.
    pub fn input(&mut self, pane: PaneId, data: &[u8]) {
        self.fill(Duration::ZERO);
        let f = Frame { kind: FrameKind::Input, pane, offset: 0, data: data.to_vec() };
        let r = self.rec.lock().unwrap().norm.frame(&f, &mut HashMap::new());
        self.rec.lock().unwrap().push(self.c, Side::Client, What::Frame(r));
        self.ws.send(Message::Binary(f.encode().into())).unwrap();
    }

    /// The next thing that came, waiting up to `timeout`.
    pub fn recv(&mut self, timeout: Duration) -> Option<WsIn> {
        if self.queue.is_empty() {
            self.fill(timeout);
        }
        self.queue.pop_front()
    }

    /// Messages until one `f` likes, which it returns (frames go by);
    /// panics after 20 s.
    pub fn until(&mut self, what: &str, mut f: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + STEP;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out waiting for {what}");
            if let Some(WsIn::Msg(v)) = self.recv(left.min(Duration::from_millis(200)))
                && f(&v)
            {
                return v;
            }
        }
    }

    /// `ping`, and wait for its `pong`: everything sent before has been
    /// handled.
    pub fn ping(&mut self, id: u64) {
        self.send(json!({"type": "ping", "id": id}));
        self.until("pong", |m| m["type"] == "pong" && m["id"] == id);
    }

    /// Pane output so far, as plain text.
    pub fn output(&self, pane: PaneId) -> String {
        fixture::clean(self.text.get(&pane).map_or(&[][..], |t| t))
    }

    /// Wait until `pane`'s output has `text` `n` times.
    pub fn wait_output(&mut self, pane: PaneId, text: &str, n: usize) {
        let deadline = Instant::now() + STEP;
        while self.output(pane).matches(text).count() < n {
            assert!(Instant::now() < deadline, "timed out waiting for {text:?} in %{pane}: {:?}", self.output(pane));
            self.recv(Duration::from_millis(100));
        }
    }

    /// Close it, from the client's side.
    pub fn close(mut self) {
        self.fill(Duration::ZERO);
        self.rec.lock().unwrap().push(self.c, Side::Client, What::Close(true));
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

/// An HTTP answer being read.
struct Res {
    status: u16,
    content_type: String,
    chunked: bool,
    len: Option<usize>,
    r: BufReader<TcpStream>,
    /// Of a chunked body: read, not yet a whole line.
    partial: Vec<u8>,
}

impl Res {
    fn chunk(&mut self) -> Option<Vec<u8>> {
        let mut line = String::new();
        self.r.read_line(&mut line).ok()?;
        let n = usize::from_str_radix(line.trim(), 16).ok()?;
        if n == 0 {
            return None;
        }
        let mut buf = vec![0; n + 2];
        self.r.read_exact(&mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    }

    fn body(&mut self) -> std::io::Result<Vec<u8>> {
        let mut out = vec![];
        if self.chunked {
            while let Some(c) = self.chunk() {
                out.extend(c);
            }
        } else if let Some(n) = self.len {
            out.resize(n, 0);
            self.r.read_exact(&mut out)?;
        } else {
            self.r.read_to_end(&mut out)?;
        }
        Ok(out)
    }

    /// The next line of a streamed body, without its newline.
    fn line(&mut self) -> Option<String> {
        loop {
            if let Some(i) = self.partial.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.partial.drain(..=i).collect();
                let line = String::from_utf8_lossy(&line[..i]).trim_end_matches('\r').to_owned();
                if line.is_empty() {
                    continue;
                }
                return Some(line);
            }
            let more = if self.chunked {
                self.chunk()?
            } else {
                let mut buf = [0; 4096];
                let n = self.r.read(&mut buf).ok().filter(|n| *n > 0)?;
                buf[..n].to_vec()
            };
            self.partial.extend(more);
        }
    }
}

/// One request over TCP with the local token; the answer's head read.
fn send(port: u16, bearer: &str, method: &str, path: &str, body: Option<&Value>) -> std::io::Result<Res> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {bearer}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !body.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
    }
    req.push_str("\r\n");
    req.push_str(&body);
    s.write_all(req.as_bytes())?;
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let status = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut content_type, mut chunked, mut len) = (String::new(), false, None);
    loop {
        line.clear();
        r.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "content-type" => content_type = v.to_owned(),
            "transfer-encoding" => chunked |= v.eq_ignore_ascii_case("chunked"),
            "content-length" => len = v.parse().ok(),
            _ => {}
        }
    }
    Ok(Res { status, content_type, chunked, len, r, partial: vec![] })
}

/// Message types a daemon sends as often as things change, not once per
/// step: a recorded one is checked against the next of its type that has
/// come, if any has, but a run that sent fewer isn't wrong.
const SOFT: [&str; 4] = ["state", "delta", "block", "notice"];

/// One of the fixture's connections, replayed.
enum Conn {
    Http { rx: mpsc::Receiver<Got>, sock: TcpStream, lines: VecDeque<Value> },
    Ws { ws: Box<WebSocket<TcpStream>>, msgs: VecDeque<Value> },
}

/// What a replayed request got.
enum Got {
    Response { status: u16, content_type: String, body: Vec<u8> },
    Head { status: u16 },
    Line(Value),
    Err(String),
}

/// Replay `f`'s client side against `d` (started with [`daemon`]) and check
/// the daemon's answers: statuses equal, bodies, messages and lines of the
/// recorded shape ([`fixture::shape`]), each recorded message and line
/// among what came (in any order; `state`, `delta`, `block` and `notice`
/// only if one came), and every word of the recorded output in the pane's
/// output before the client's next step. Placeholders in what the client
/// sends become this daemon's values.
pub fn replay(d: &Daemon, f: &Fixture) -> Result<(), String> {
    let streams = f.events.iter().filter(|e| matches!(&e.what, What::Response(r) if r.stream)).map(|e| e.c).collect();
    let mut r =
        Replay { d, norm: normalizer(d), streams, conns: HashMap::new(), text: HashMap::new(), want: HashMap::new() };
    for (i, e) in f.events.iter().enumerate() {
        let step = || format!("{} step {} ({})", f.header.fixture, i + 1, serde_json::to_string(e).unwrap_or_default());
        if e.from == Side::Client {
            r.settle().map_err(|why| format!("{}: {why}", step()))?;
        }
        r.step(e).map_err(|why| format!("{}: {why}", step()))?;
    }
    r.settle().map_err(|why| format!("{}: at the end: {why}", f.header.fixture))
}

struct Replay<'d> {
    d: &'d Daemon,
    norm: Normalize,
    /// The connections whose answers stream.
    streams: HashSet<u32>,
    conns: HashMap<u32, Conn>,
    /// Each pane's output so far, from any WebSocket, as plain normalized
    /// text.
    text: HashMap<PaneId, String>,
    /// Words of recorded output not yet seen, per pane.
    want: HashMap<PaneId, Vec<String>>,
}

impl Replay<'_> {
    fn step(&mut self, e: &Event) -> Result<(), String> {
        match (&e.from, &e.what) {
            (Side::Client, What::Request { method, path, body }) => {
                let path = self.norm.back(path);
                let body = body.clone().map(|mut b| {
                    self.norm.value_back(&mut b);
                    b
                });
                let res =
                    send(self.d.port, &self.d.bearer(), method, &path, body.as_ref()).map_err(|e| e.to_string())?;
                let sock = res.r.get_ref().try_clone().map_err(|e| e.to_string())?;
                let (tx, rx) = mpsc::channel();
                let stream = self.streams.contains(&e.c);
                std::thread::spawn(move || read_answer(res, stream, tx));
                self.conns.insert(e.c, Conn::Http { rx, sock, lines: VecDeque::new() });
            }
            (Side::Daemon, What::Response(want)) => {
                let Some(Conn::Http { rx, .. }) = self.conns.get_mut(&e.c) else {
                    return Err("no such request".into());
                };
                match rx.recv_timeout(STEP) {
                    Ok(Got::Head { status }) if status == want.status && want.stream => {}
                    Ok(Got::Response { status, content_type, body }) if status == want.status && !want.stream => {
                        if let Some(w) = &want.body {
                            let mut got: Value = serde_json::from_slice(&body).map_err(|_| {
                                format!("not JSON ({content_type}): {}", String::from_utf8_lossy(&body))
                            })?;
                            self.norm.learn(&got);
                            self.norm.value(&mut got);
                            fixture::shape(w, &got)?;
                        }
                        if let Some(w) = &want.text {
                            let got = self.norm.string(&String::from_utf8_lossy(&body));
                            let missing: Vec<String> = fixture::words(&fixture::clean(w.as_bytes()))
                                .into_iter()
                                .filter(|w| !got.contains(w.as_str()))
                                .collect();
                            if !missing.is_empty() {
                                return Err(format!("the answer lacks {missing:?}: {got:?}"));
                            }
                        }
                    }
                    Ok(Got::Head { status } | Got::Response { status, .. }) => {
                        return Err(format!("status {status}, not {}", want.status));
                    }
                    Ok(Got::Err(why)) => return Err(why),
                    Ok(Got::Line(_)) => return Err("lines before the head".into()),
                    Err(_) => return Err("no answer".into()),
                }
            }
            (Side::Daemon, What::Line(want)) => {
                let Some(Conn::Http { rx, lines, .. }) = self.conns.get_mut(&e.c) else {
                    return Err("no such stream".into());
                };
                let deadline = Instant::now() + STEP;
                loop {
                    if let Some(i) = lines.iter().position(|l| fixture::shape(want, l).is_ok()) {
                        lines.remove(i);
                        break;
                    }
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(Got::Line(mut l)) => {
                            self.norm.learn(&l);
                            self.norm.value(&mut l);
                            lines.push_back(l);
                        }
                        Ok(Got::Err(why)) => return Err(why),
                        Ok(_) => {}
                        Err(_) => return Err(format!("no such line among {lines:?}")),
                    }
                }
            }
            (Side::Client, What::Open(path)) => {
                let path = self.norm.back(path);
                let stream = TcpStream::connect(("127.0.0.1", self.d.port)).map_err(|e| e.to_string())?;
                let (ws, _) =
                    tungstenite::client(self.d.ws(&path), stream).map_err(|e| format!("the WebSocket: {e}"))?;
                ws.get_ref().set_read_timeout(Some(Duration::from_millis(20))).map_err(|e| e.to_string())?;
                self.conns.insert(e.c, Conn::Ws { ws: Box::new(ws), msgs: VecDeque::new() });
            }
            (Side::Client, What::Msg(m)) => {
                let mut m = m.clone();
                self.norm.value_back(&mut m);
                self.ws(e.c)?.send(Message::Text(m.to_string().into())).map_err(|e| e.to_string())?;
            }
            (Side::Client, What::Frame(f)) => {
                let mut f = f.frame().ok_or("a frame of no known kind")?;
                f.data = self.norm.back(&String::from_utf8_lossy(&f.data)).into_bytes();
                self.ws(e.c)?.send(Message::Binary(f.encode().into())).map_err(|e| e.to_string())?;
            }
            (Side::Daemon, What::Msg(want)) => self.msg(e.c, want)?,
            (Side::Daemon, What::Frame(f)) => {
                if matches!(f.kind(), Some(FrameKind::Output | FrameKind::Snapshot)) {
                    let words = fixture::words(&fixture::clean(&f.bytes()));
                    self.want.entry(f.pane).or_default().extend(words);
                }
            }
            (Side::Client, What::Close(_)) => match self.conns.remove(&e.c) {
                Some(Conn::Http { sock, .. }) => {
                    let _ = sock.shutdown(Shutdown::Both);
                }
                Some(Conn::Ws { mut ws, .. }) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                }
                None => {}
            },
            // The daemon ending a stream or a WebSocket: not something a
            // client counts on.
            (Side::Daemon, What::Close(_)) => {}
            (from, what) => return Err(format!("{what:?} can't come from the {from:?}")),
        }
        Ok(())
    }

    fn ws(&mut self, c: u32) -> Result<&mut WebSocket<TcpStream>, String> {
        match self.conns.get_mut(&c) {
            Some(Conn::Ws { ws, .. }) => Ok(ws),
            _ => Err("no such WebSocket".into()),
        }
    }

    /// Read what has come on every WebSocket, waiting up to `timeout`.
    fn read(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut got = false;
            for conn in self.conns.values_mut() {
                let Conn::Ws { ws, msgs } = conn else { continue };
                loop {
                    match ws.read() {
                        Ok(Message::Text(t)) => {
                            let mut v: Value = serde_json::from_str(&t).unwrap_or(Value::String(t.to_string()));
                            self.norm.learn(&v);
                            self.norm.value(&mut v);
                            msgs.push_back(v);
                            got = true;
                        }
                        Ok(Message::Binary(b)) => {
                            if let Ok(f) = Frame::decode(&b)
                                && matches!(f.kind, FrameKind::Output | FrameKind::Snapshot)
                            {
                                let t = self.norm.string(&fixture::clean(&f.data));
                                self.text.entry(f.pane).or_default().push_str(&t);
                            }
                            got = true;
                        }
                        Ok(Message::Close(_)) => return Err("the daemon closed the WebSocket".into()),
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e))
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                        {
                            break;
                        }
                        Err(e) => return Err(format!("the WebSocket: {e}")),
                    }
                }
            }
            if got || Instant::now() >= deadline {
                return Ok(());
            }
        }
    }

    /// A message the daemon sent on `c`.
    fn msg(&mut self, c: u32, want: &Value) -> Result<(), String> {
        let soft = SOFT.contains(&want["type"].as_str().unwrap_or_default());
        let deadline = Instant::now() + if soft { Duration::from_millis(500) } else { STEP };
        loop {
            let Some(Conn::Ws { msgs, .. }) = self.conns.get_mut(&c) else { return Err("no such WebSocket".into()) };
            if let Some(i) = msgs.iter().position(|m| fixture::shape(want, m).is_ok()) {
                msgs.remove(i);
                return Ok(());
            }
            let same = msgs.iter().find(|m| m["type"] == want["type"]);
            if Instant::now() >= deadline {
                return match same {
                    Some(m) => Err(fixture::shape(want, m).unwrap_err()),
                    None if soft => Ok(()),
                    None => {
                        Err(format!("no such message among {:?}", msgs.iter().map(|m| &m["type"]).collect::<Vec<_>>()))
                    }
                };
            }
            self.read(Duration::from_millis(50))?;
        }
    }

    /// Before the client's next step: every recorded word of output has
    /// shown up in its pane.
    fn settle(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + STEP;
        loop {
            self.read(Duration::ZERO)?;
            let text = &self.text;
            for (pane, words) in self.want.iter_mut() {
                let have = text.get(pane).map_or("", String::as_str);
                words.retain(|w| !have.contains(w.as_str()));
            }
            self.want.retain(|_, w| !w.is_empty());
            if self.want.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let (pane, words) = self.want.iter().next().unwrap();
                return Err(format!("%{pane}'s output lacks {words:?}: {:?}", self.text.get(pane)));
            }
            self.read(Duration::from_millis(50))?;
        }
    }
}

/// Read an answer on its own thread: whole, or line by line if it streams.
fn read_answer(mut res: Res, stream: bool, tx: mpsc::Sender<Got>) {
    if !stream || res.status != 200 {
        let got = match res.body() {
            Ok(body) => Got::Response { status: res.status, content_type: res.content_type.clone(), body },
            Err(e) => Got::Err(e.to_string()),
        };
        let _ = tx.send(got);
        return;
    }
    let _ = tx.send(Got::Head { status: res.status });
    while let Some(line) = res.line() {
        let v = serde_json::from_str(&line).unwrap_or(Value::String(line));
        if tx.send(Got::Line(v)).is_err() {
            return;
        }
    }
}
