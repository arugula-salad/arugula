//! The daemon's side of a fixture, served to a client on a TCP port
//! (`arugula-fixture-server`, or [`Server`] in a test): HTTP requests and
//! WebSockets at the paths the fixture has, answered as recorded, with no
//! daemon or PTY behind them.
//!
//! The fixture is played in order. What the daemon sent goes out as soon as
//! everything the client sent before it in the recording has arrived; what
//! the client sends is matched against the client's next steps, which may
//! come in any order among themselves (two requests on two connections, a
//! `view` and an `attach` sent together). A request or message matches when
//! it has what the recorded one had ([`super::subset`]: a client may add
//! fields, and placeholders match anything); typed bytes match however the
//! client splits them. Something that matches a later step waits for its
//! turn. A few messages that change nothing are let through anywhere:
//! `ping` (answered with a `pong`), `ack`, `focus`, and a repeat of a
//! message already matched on that connection; and the recording's own
//! pings needn't come (the recorder pings to wait). Anything else is
//! unexpected: the server answers it with a 404 (HTTP) or an `error` and a
//! close (WebSocket), and it goes in the [`Report`].
//!
//! Once only the client's closes are left, the server closes what's still
//! open (an events stream a client would follow forever, a WebSocket),
//! and those closes count as done.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tungstenite::Message;

use super::{Event, Fixture, FrameRec, Response, Side, What, subset, text_matches};
use crate::{Frame, FrameKind, PaneId};

/// How a fixture played out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// What the client sent that the fixture didn't expect.
    pub unexpected: Vec<String>,
    /// The steps that never happened, in order.
    pub left: Vec<String>,
    /// How many steps did.
    pub done: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.unexpected.is_empty()
    }
}

/// What a connection writes, from the fixture.
enum Out {
    Response(Response),
    Line(Value),
    Msg(Value),
    Frame(FrameRec),
    Close,
}

/// What came in on a connection.
#[derive(Debug)]
enum In {
    Request { method: String, path: String, body: Option<Value> },
    Open(String),
    Msg(Value),
    Input(Frame),
    Close,
}

struct Live {
    tx: mpsc::Sender<Out>,
    /// The fixture's connection it plays, once its first step matched.
    c: Option<u32>,
    open: bool,
    /// Messages matched on it, so a repeat passes.
    matched: Vec<Value>,
    /// Typed bytes not matched yet, per pane.
    typed: HashMap<PaneId, Vec<u8>>,
    /// Requests and messages that match a later step, in arrival order.
    waiting: Vec<In>,
}

struct Player {
    events: Vec<Event>,
    done: Vec<bool>,
    live: HashMap<usize, Live>,
    unexpected: Vec<String>,
    closed_all: bool,
    /// Recorded pings the client didn't send (connection, id): their pongs
    /// aren't sent.
    unasked: Vec<(u32, Value)>,
}

/// Messages that change nothing, let through wherever they come.
const CHATTER: [&str; 3] = ["ping", "ack", "focus"];

impl Player {
    fn bound(&self, c: u32) -> Option<usize> {
        self.live.iter().find(|(_, l)| l.c == Some(c)).map(|(id, _)| *id)
    }

    /// The client's next steps: from the first one not done, up to the
    /// daemon's next.
    fn window(&self) -> Vec<usize> {
        (0..self.events.len()).filter(|i| !self.done[*i]).take_while(|i| self.events[*i].from == Side::Client).collect()
    }

    /// The client's steps after the window, not done.
    fn later(&self) -> Vec<usize> {
        let w = self.window();
        let from = w.last().map_or(0, |i| i + 1);
        (from..self.events.len()).filter(|i| !self.done[*i] && self.events[*i].from == Side::Client).collect()
    }

    /// Send what the daemon sent next, up to the client's next step; then
    /// try what was waiting.
    fn pump(&mut self) {
        loop {
            let mut moved = false;
            while let Some(i) = (0..self.events.len()).find(|i| !self.done[*i]) {
                let e = &self.events[i];
                if e.from == Side::Client {
                    // A recorded ping (or ack, or focus) is the recorder's
                    // own business: a client needn't send it, and the
                    // pong it got isn't sent.
                    let What::Msg(m) = &e.what else { break };
                    let kind = m["type"].as_str().unwrap_or_default();
                    if !CHATTER.contains(&kind) {
                        break;
                    }
                    if kind == "ping" {
                        self.unasked.push((e.c, m["id"].clone()));
                    }
                    self.done[i] = true;
                    moved = true;
                    continue;
                }
                self.done[i] = true;
                moved = true;
                let out = match &e.what {
                    What::Response(r) => Out::Response(r.clone()),
                    What::Line(v) => Out::Line(v.clone()),
                    What::Msg(v) => {
                        let unasked = (e.c, v["id"].clone());
                        if v["type"] == "pong" && self.unasked.contains(&unasked) {
                            self.unasked.retain(|u| *u != unasked);
                            continue;
                        }
                        Out::Msg(v.clone())
                    }
                    What::Frame(f) => Out::Frame(f.clone()),
                    What::Close(_) => Out::Close,
                    What::Request { .. } | What::Open(_) => continue,
                };
                if let Some(l) = self.bound(e.c).and_then(|id| self.live.get(&id))
                    && l.open
                {
                    let _ = l.tx.send(out);
                }
            }
            // What waited, in order, and typed bytes.
            let ids: Vec<usize> = self.live.keys().copied().collect();
            for id in ids {
                let waiting = std::mem::take(&mut self.live.get_mut(&id).unwrap().waiting);
                for w in waiting {
                    moved |= self.take(id, w, true);
                }
                let panes: Vec<PaneId> = self.live[&id].typed.keys().copied().collect();
                for p in panes {
                    moved |= self.typed(id, p);
                }
            }
            if !moved {
                break;
            }
        }
        if !self.closed_all && self.complete() {
            self.closed_all = true;
            // Closing a connection stands for the client closing it: an
            // events stream it would follow forever has no other end.
            let mut closed = vec![];
            for l in self.live.values().filter(|l| l.open) {
                let _ = l.tx.send(Out::Close);
                closed.extend(l.c);
            }
            for (e, d) in self.events.iter().zip(&mut self.done) {
                if closed.contains(&e.c) && e.from == Side::Client && matches!(e.what, What::Close(_)) {
                    *d = true;
                }
            }
        }
    }

    /// Only the client's closes are left.
    fn complete(&self) -> bool {
        self.events
            .iter()
            .zip(&self.done)
            .all(|(e, d)| *d || (e.from == Side::Client && matches!(e.what, What::Close(_))))
    }

    fn unexpected(&mut self, id: usize, what: String) {
        self.unexpected.push(what.clone());
        if let Some(l) = self.live.get_mut(&id) {
            if l.c.is_some() {
                let _ = l
                    .tx
                    .send(Out::Msg(json!({"type": "error", "id": null, "message": format!("fixture server: {what}")})));
            }
            let _ = l.tx.send(Out::Close);
            l.open = false;
        }
    }

    /// Whether step `i` is what live connection `id` sent.
    fn matches(&self, id: usize, i: usize, input: &In) -> bool {
        let e = &self.events[i];
        let c = self.live[&id].c;
        let mine = match c {
            Some(c) => e.c == c,
            // A new connection's first step: a connection nobody plays yet.
            None => self.bound(e.c).is_none(),
        };
        if !mine {
            return false;
        }
        match (&e.what, input) {
            (What::Request { method, path, body }, In::Request { method: m, path: p, body: b }) => {
                c.is_none()
                    && method.eq_ignore_ascii_case(m)
                    && text_matches(path, p)
                    && match (body, b) {
                        (None, _) => true,
                        (Some(w), Some(g)) => subset(w, g).is_ok(),
                        (Some(w), None) => w.is_null(),
                    }
            }
            (What::Open(path), In::Open(p)) => c.is_none() && text_matches(path, p),
            (What::Msg(w), In::Msg(g)) => c.is_some() && subset(w, g).is_ok(),
            (What::Close(_), In::Close) => c.is_some(),
            _ => false,
        }
    }

    /// Handle what came in on `id`. `again`: it waited, and is tried
    /// again. Whether a step was done.
    fn take(&mut self, id: usize, input: In, again: bool) -> bool {
        if !self.live.get(&id).is_some_and(|l| l.open) {
            return false;
        }
        if let In::Input(f) = input {
            self.live.get_mut(&id).unwrap().typed.entry(f.pane).or_default().extend(f.data);
            let moved = self.typed(id, f.pane);
            if moved && !again {
                self.pump();
            }
            return moved;
        }
        if let Some(i) = self.window().into_iter().find(|i| self.matches(id, *i, &input)) {
            self.done[i] = true;
            let c = self.events[i].c;
            let l = self.live.get_mut(&id).unwrap();
            l.c = Some(c);
            if let In::Msg(v) = &input {
                l.matched.push(v.clone());
            }
            if matches!(input, In::Close) {
                l.open = false;
            }
            if !again {
                self.pump();
            }
            return true;
        }
        let l = &self.live[&id];
        match &input {
            In::Close => {
                // The client left; what it would have done stays left.
                self.live.get_mut(&id).unwrap().open = false;
                return false;
            }
            In::Msg(v) if l.matched.contains(v) => return false,
            In::Msg(v) if CHATTER.contains(&v["type"].as_str().unwrap_or_default()) => {
                if v["type"] == "ping" && !self.later().iter().any(|i| self.matches(id, *i, &input)) {
                    let _ = l.tx.send(Out::Msg(json!({"type": "pong", "id": v["id"]})));
                }
                if !self.later().iter().any(|i| self.matches(id, *i, &input)) {
                    return false;
                }
            }
            _ => {}
        }
        if self.later().iter().any(|i| self.matches(id, *i, &input)) {
            self.live.get_mut(&id).unwrap().waiting.push(input);
            return false;
        }
        let what = match &input {
            In::Request { method, path, body } => {
                format!("{method} {path}{}", body.as_ref().map(|b| format!(" {b}")).unwrap_or_default())
            }
            In::Open(p) => format!("a WebSocket at {p}"),
            In::Msg(v) => format!("{v}"),
            In::Input(_) | In::Close => unreachable!("handled above"),
        };
        self.unexpected(id, what);
        false
    }

    /// Match typed bytes on `pane` against the input frames the fixture
    /// expects there, however the client split them. Whether a step was
    /// done (the caller pumps).
    fn typed(&mut self, id: usize, pane: PaneId) -> bool {
        let Some(c) = self.live[&id].c else { return false };
        let is_input = |e: &Event| {
            e.c == c
                && e.from == Side::Client
                && matches!(&e.what, What::Frame(f) if f.kind == "input" && f.pane == pane)
        };
        let mut moved = false;
        loop {
            let buf = &self.live[&id].typed[&pane];
            if buf.is_empty() {
                return moved;
            }
            let window = self.window();
            let next = window.iter().copied().find(|i| is_input(&self.events[*i]));
            let want = match next {
                Some(i) => {
                    let What::Frame(f) = &self.events[i].what else { unreachable!() };
                    f.bytes()
                }
                None => {
                    // Not yet: fine if it's how a later step starts.
                    let later = self.later().into_iter().find(|i| is_input(&self.events[*i]));
                    let fits = later.is_some_and(|i| {
                        let What::Frame(f) = &self.events[i].what else { unreachable!() };
                        let w = f.bytes();
                        w.starts_with(buf) || buf.starts_with(&w)
                    });
                    if !fits {
                        let what = format!("typed {:?} in %{pane}", String::from_utf8_lossy(buf));
                        self.unexpected(id, what);
                    }
                    return moved;
                }
            };
            if buf.starts_with(&want) {
                self.live.get_mut(&id).unwrap().typed.get_mut(&pane).unwrap().drain(..want.len());
                self.done[next.unwrap()] = true;
                moved = true;
            } else if want.starts_with(buf) {
                return moved;
            } else {
                let what = format!(
                    "typed {:?} in %{pane}, not {:?}",
                    String::from_utf8_lossy(buf),
                    String::from_utf8_lossy(&want)
                );
                self.unexpected(id, what);
                return moved;
            }
        }
    }

    fn report(&self) -> Report {
        let left = self
            .events
            .iter()
            .zip(&self.done)
            .filter(|(_, d)| !**d)
            .map(|(e, _)| serde_json::to_string(e).unwrap_or_default())
            .collect();
        Report { unexpected: self.unexpected.clone(), left, done: self.done.iter().filter(|d| **d).count() }
    }
}

type Shared = Arc<(Mutex<Player>, Condvar)>;

/// A fixture served on a port.
pub struct Server {
    addr: SocketAddr,
    player: Shared,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Serve `fixture` on `listen` (`127.0.0.1:0` for any port).
    pub fn start(fixture: &Fixture, listen: &str) -> std::io::Result<Self> {
        let l = TcpListener::bind(listen)?;
        let addr = l.local_addr()?;
        let player = Player {
            done: vec![false; fixture.events.len()],
            events: fixture.events.clone(),
            live: HashMap::new(),
            unexpected: vec![],
            closed_all: false,
            unasked: vec![],
        };
        let player: Shared = Arc::new((Mutex::new(player), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (player.clone(), stop.clone());
        thread::Builder::new().name("fixture-accept".into()).spawn(move || {
            for (n, stream) in l.incoming().enumerate() {
                if s.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let p = p.clone();
                let _ = thread::Builder::new().name(format!("fixture-conn-{n}")).spawn(move || serve(stream, n, p));
            }
        })?;
        Ok(Self { addr, player, stop })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn report(&self) -> Report {
        self.player.0.lock().unwrap().report()
    }

    /// Wait until the fixture has played out and every connection is
    /// closed, or for `timeout`; then how it went.
    pub fn wait(&self, timeout: Duration) -> Report {
        let deadline = Instant::now() + timeout;
        let (m, cv) = &*self.player;
        let mut p = m.lock().unwrap();
        loop {
            let finished = p.complete() && p.live.values().all(|l| !l.open);
            let now = Instant::now();
            if finished || !p.unexpected.is_empty() || now >= deadline {
                return p.report();
            }
            p = cv.wait_timeout(p, (deadline - now).min(Duration::from_millis(100))).unwrap().0;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect(self.addr);
        let p = self.player.0.lock().unwrap();
        for l in p.live.values().filter(|l| l.open) {
            let _ = l.tx.send(Out::Close);
        }
    }
}

/// Hand `input` to the player, and wake whoever waits on it.
fn take(player: &Shared, id: usize, input: In) {
    let (m, cv) = &**player;
    m.lock().unwrap().take(id, input, false);
    cv.notify_all();
}

fn serve(stream: TcpStream, id: usize, player: Shared) {
    let (tx, rx) = mpsc::channel();
    {
        let live = Live { tx, c: None, open: true, matched: vec![], typed: HashMap::new(), waiting: vec![] };
        player.0.lock().unwrap().live.insert(id, live);
    }
    // A client that goes away mid-answer is no failure of the fixture.
    match head(&stream) {
        Some((_, path, true)) => drop(websocket(stream, id, &player, rx, path)),
        Some(_) => drop(http(stream, id, &player, rx)),
        None => {}
    }
    let (m, cv) = &*player;
    if let Some(l) = m.lock().unwrap().live.get_mut(&id) {
        l.open = false;
    }
    cv.notify_all();
}

/// The request line and whether it's a WebSocket upgrade, without reading
/// it off the stream.
fn head(stream: &TcpStream) -> Option<(String, String, bool)> {
    let mut buf = vec![0; 16384];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let n = stream.peek(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(end) = text.find("\r\n\r\n") {
            let head = &text[..end];
            let mut words = head.lines().next()?.split_whitespace();
            let (method, path) = (words.next()?.to_owned(), words.next()?.to_owned());
            let upgrade = head.lines().any(|l| {
                let l = l.to_ascii_lowercase();
                l.starts_with("upgrade:") && l.contains("websocket")
            });
            return Some((method, path, upgrade));
        }
        if n == buf.len() || Instant::now() > deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn http(stream: TcpStream, id: usize, player: &Shared, rx: mpsc::Receiver<Out>) -> std::io::Result<()> {
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let (method, path) = (words.next().unwrap_or_default().to_owned(), words.next().unwrap_or_default().to_owned());
    let mut len = 0;
    loop {
        line.clear();
        r.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let body = (!body.is_empty()).then(|| {
        serde_json::from_slice(&body).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into()))
    });
    take(player, id, In::Request { method, path, body });
    let mut w = stream;
    let mut streaming = false;
    // Whatever the fixture says, as it says it; a 404 for what it doesn't.
    while let Ok(out) = rx.recv() {
        match out {
            Out::Response(res) => {
                let ok = |s: u16| if s < 400 { "OK" } else { "Error" };
                if res.stream {
                    streaming = true;
                    write!(
                        w,
                        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                        res.status,
                        ok(res.status),
                        res.content_type
                    )?;
                    w.flush()?;
                    continue;
                }
                let body = match (&res.body, &res.text) {
                    (Some(b), _) => b.to_string(),
                    (None, Some(t)) => t.clone(),
                    (None, None) => String::new(),
                };
                write!(
                    w,
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    res.status,
                    ok(res.status),
                    res.content_type,
                    body.len()
                )?;
                return w.flush();
            }
            Out::Line(v) => {
                let text = match v {
                    Value::String(s) => format!("{s}\n"),
                    v => format!("{v}\n"),
                };
                write!(w, "{:x}\r\n{text}\r\n", text.len())?;
                w.flush()?;
            }
            Out::Close if streaming => {
                w.write_all(b"0\r\n\r\n")?;
                return w.flush();
            }
            Out::Close => {
                // Not in the fixture: unexpected (already reported).
                let body = r#"{"error":"not in the fixture"}"#;
                write!(
                    w,
                    "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
                return w.flush();
            }
            Out::Msg(_) | Out::Frame(_) => {}
        }
    }
    Ok(())
}

fn websocket(
    stream: TcpStream,
    id: usize,
    player: &Shared,
    rx: mpsc::Receiver<Out>,
    path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut ws = tungstenite::accept(stream)?;
    take(player, id, In::Open(path));
    ws.get_mut().set_read_timeout(Some(Duration::from_millis(10)))?;
    loop {
        while let Ok(out) = rx.try_recv() {
            match out {
                Out::Msg(v) => ws.send(Message::Text(v.to_string().into()))?,
                Out::Frame(f) => {
                    if let Some(f) = f.frame() {
                        ws.send(Message::Binary(f.encode().into()))?;
                    }
                }
                Out::Close => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
                Out::Response(_) | Out::Line(_) => {}
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let v = serde_json::from_str(&t).unwrap_or(Value::String(t.to_string()));
                take(player, id, In::Msg(v));
            }
            Ok(Message::Binary(b)) => match Frame::decode(&b) {
                Ok(f) if f.kind == FrameKind::Input => take(player, id, In::Input(f)),
                Ok(f) => take(player, id, In::Msg(json!({"frame": super::FrameRec::new(&f)}))),
                Err(e) => take(player, id, In::Msg(Value::String(format!("a bad frame: {e}")))),
            },
            Ok(Message::Close(_)) => {
                take(player, id, In::Close);
                return Ok(());
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => {
                take(player, id, In::Close);
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(lines: &[Value]) -> Fixture {
        let mut text = r#"{"fixture":"t","version":"0","about":""}"#.to_owned() + "\n";
        for l in lines {
            text += &format!("{l}\n");
        }
        Fixture::parse(&text).unwrap()
    }

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn answers_what_it_recorded_and_refuses_the_rest() {
        let f = fixture(&[
            json!({"t": 0, "c": 1, "from": "client", "request": {"method": "GET", "path": "/api/panes"}}),
            json!({"t": 1, "c": 1, "from": "daemon", "response": {"status": 200, "type": "application/json", "body": [{"id": 1}]}}),
        ]);
        let s = Server::start(&f, "127.0.0.1:0").unwrap();
        assert!(get(s.addr(), "/api/nope").starts_with("HTTP/1.1 404"));
        let res = get(s.addr(), "/api/panes");
        assert!(res.starts_with("HTTP/1.1 200") && res.ends_with(r#"[{"id":1}]"#), "{res}");
        let r = s.wait(Duration::from_secs(5));
        assert_eq!(r.unexpected, ["GET /api/nope"]);
        assert!(r.left.is_empty());
    }

    #[test]
    fn typed_bytes_match_however_they_are_split() {
        let f = fixture(&[
            json!({"t": 0, "c": 1, "from": "client", "open": "/ws"}),
            json!({"t": 1, "c": 1, "from": "daemon", "msg": {"type": "hello"}}),
            json!({"t": 2, "c": 1, "from": "client", "frame": {"kind": "input", "pane": 1, "offset": 0, "data": "echo hi\r"}}),
            json!({"t": 3, "c": 1, "from": "daemon", "frame": {"kind": "output", "pane": 1, "offset": 0, "data": "hi\r\n"}}),
            json!({"t": 4, "c": 1, "from": "client", "close": true}),
        ]);
        let s = Server::start(&f, "127.0.0.1:0").unwrap();
        let (mut ws, _) = tungstenite::connect(format!("ws://{}/ws", s.addr())).unwrap();
        assert!(matches!(ws.read().unwrap(), Message::Text(t) if t.contains("hello")));
        // A ping between the parts passes. (After the last part, only the
        // client's close is left and the server closes.)
        for part in ["ec", "ho hi", "\r"] {
            let f = Frame { kind: FrameKind::Input, pane: 1, offset: 0, data: part.as_bytes().to_vec() };
            ws.send(Message::Binary(f.encode().into())).unwrap();
            if part == "ec" {
                ws.send(Message::Text(r#"{"type":"ping","id":9}"#.into())).unwrap();
            }
        }
        let mut got = vec![];
        while got.len() < 2 {
            match ws.read().unwrap() {
                Message::Binary(b) => got.push(String::from_utf8_lossy(&Frame::decode(&b).unwrap().data).into_owned()),
                Message::Text(t) => got.push(t.to_string()),
                _ => {}
            }
        }
        assert!(got.contains(&"hi\r\n".to_owned()) && got.iter().any(|m| m.contains("pong")), "{got:?}");
        let _ = ws.close(None);
        let r = s.wait(Duration::from_secs(5));
        assert!(r.ok() && r.left.is_empty(), "{r:?}");
    }
}
