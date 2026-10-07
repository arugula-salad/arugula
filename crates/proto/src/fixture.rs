//! Client fixtures (#200): golden sessions recorded from a real daemon and
//! checked in as `crates/proto/fixtures/<name>.jsonl`, usable from both
//! sides. A client replays one against `arugula-fixture-server` (the
//! `fixture-server` feature, [`server`]), which plays the daemon's side and
//! fails on anything the client sends that the fixture doesn't expect. The
//! daemon's own tests replay the client's side against a real daemon and
//! check its answers (`arugula_testkit::fixture::replay`).
//!
//! A file is a [`Header`] line, then one [`Event`] per line: an HTTP
//! request or its answer, a line of a streamed answer, or a WebSocket
//! frame in one direction, with its time from the session's start and the
//! connection it went over. What changes from run to run (times, the state
//! dir, ports, tokens, session names) is replaced by placeholders when it's
//! recorded ([`Normalize`]); ids the daemon hands out in order (panes,
//! tabs, sessions, clients) are kept, since a fresh daemon gives the same
//! ones.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Frame, FrameKind, PaneId};

#[cfg(feature = "fixture-server")]
pub mod server;

/// The first line of a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Its name: the file is `<fixture>.jsonl`.
    pub fixture: String,
    /// The daemon version that recorded it.
    pub version: String,
    /// What the session does, for whoever reads the diff.
    pub about: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Client,
    Daemon,
}

/// One thing that crossed the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Milliseconds since the session started.
    pub t: u64,
    /// The connection it went over: one per HTTP request (clients make one
    /// request per connection), one per WebSocket.
    pub c: u32,
    pub from: Side,
    #[serde(flatten)]
    pub what: What,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum What {
    /// From the client: an HTTP request. `path` has its query.
    Request {
        method: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<Value>,
    },
    /// From the daemon: the answer's head, and its body unless it streams.
    Response(Response),
    /// From the daemon: one line of a streamed answer (`/api/events`), as
    /// JSON (a line that isn't JSON is a string).
    Line(Value),
    /// From the client: a WebSocket opened at this path.
    Open(String),
    /// A text frame: a control message ([`crate::ClientMsg`],
    /// [`crate::ServerMsg`]).
    Msg(Value),
    /// A binary frame ([`Frame`]).
    Frame(FrameRec),
    /// That side ended the connection (or the stream). Always `true`.
    Close(bool),
}

impl What {
    /// Sort the keys of the JSON it carries, at every depth.
    fn sort_keys(&mut self) {
        match self {
            What::Request { body: Some(v), .. } | What::Response(Response { body: Some(v), .. }) => {
                v.sort_all_objects()
            }
            What::Line(v) | What::Msg(v) => v.sort_all_objects(),
            _ => {}
        }
    }
}

/// An HTTP answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    /// Its `Content-Type`.
    #[serde(rename = "type")]
    pub content_type: String,
    /// A JSON body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Any other body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Chunked and open: [`What::Line`]s follow, then [`What::Close`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
}

/// A [`Frame`] as a fixture writes it: the payload as text when it's
/// UTF-8, otherwise as hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRec {
    /// `output`, `snapshot`, `input` or `snapshot_zstd`.
    pub kind: String,
    pub pane: PaneId,
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
}

fn kind_name(k: FrameKind) -> &'static str {
    match k {
        FrameKind::Output => "output",
        FrameKind::Snapshot => "snapshot",
        FrameKind::Input => "input",
        FrameKind::SnapshotZstd => "snapshot_zstd",
    }
}

impl FrameRec {
    pub fn new(f: &Frame) -> Self {
        let (data, hex) = match std::str::from_utf8(&f.data) {
            Ok(s) => (Some(s.to_owned()), None),
            Err(_) => (None, Some(f.data.iter().map(|b| format!("{b:02x}")).collect())),
        };
        Self { kind: kind_name(f.kind).into(), pane: f.pane, offset: f.offset, data, hex }
    }

    pub fn bytes(&self) -> Vec<u8> {
        match (&self.data, &self.hex) {
            (Some(d), _) => d.as_bytes().to_vec(),
            (None, Some(h)) => {
                (0..h.len() / 2).filter_map(|i| u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
            }
            (None, None) => vec![],
        }
    }

    pub fn kind(&self) -> Option<FrameKind> {
        [FrameKind::Output, FrameKind::Snapshot, FrameKind::Input, FrameKind::SnapshotZstd]
            .into_iter()
            .find(|k| kind_name(*k) == self.kind)
    }

    pub fn frame(&self) -> Option<Frame> {
        Some(Frame { kind: self.kind()?, pane: self.pane, offset: self.offset, data: self.bytes() })
    }
}

/// A recorded session.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    pub header: Header,
    pub events: Vec<Event>,
}

impl Fixture {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty());
        let (_, first) = lines.next().ok_or("an empty fixture")?;
        let header: Header = serde_json::from_str(first).map_err(|e| format!("line 1: {e}"))?;
        let events = lines
            .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
            .collect::<Result<_, _>>()?;
        Ok(Self { header, events })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The file's text. The keys of recorded JSON (bodies, messages,
    /// lines) are sorted, so the text is the same whether or not
    /// serde_json keeps insertion order (its `preserve_order` feature, on
    /// when anything in the build turns it on).
    pub fn to_jsonl(&self) -> String {
        let mut out = serde_json::to_string(&self.header).expect("a header serializes");
        out.push('\n');
        for e in &self.events {
            let mut e = e.clone();
            e.what.sort_keys();
            out.push_str(&serde_json::to_string(&e).expect("an event serializes"));
            out.push('\n');
        }
        out
    }

    /// Write it to `dir/<name>.jsonl`.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.jsonl", self.header.fixture));
        std::fs::write(&path, self.to_jsonl())?;
        Ok(path)
    }
}

/// Where the fixtures are checked in: `crates/proto/fixtures`.
pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Every `*.jsonl` in `dir`, by name.
pub fn load_dir(dir: &Path) -> Result<Vec<Fixture>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    paths.sort();
    paths.iter().map(|p| Fixture::load(p)).collect()
}

/// The newest `n` release sets under `fixtures/releases/<version>/`, newest
/// first, by version number: what a released client sent and saw.
pub fn releases(n: usize) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir().join("releases")) else { return vec![] };
    let mut v: Vec<(Vec<u64>, String, PathBuf)> = rd
        .filter_map(|e| {
            let p = e.ok()?.path();
            let name = p.file_name()?.to_str()?.to_owned();
            let key = name.split(['.', '-']).map(|x| x.parse().unwrap_or(0)).collect();
            p.is_dir().then_some((key, name, p))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(n).map(|(_, name, p)| (name, p)).collect()
}

/// Integers this big are times (milliseconds or nanoseconds since 1970,
/// a pane's epoch): written as 0.
const TIME: u64 = 1_000_000_000_000;

/// Replaces what changes between runs with placeholders: strings the
/// recorder names (the state dir, `$HOME`, the port, the token), session
/// names as it learns them, the values of fields named like a token, and
/// times. Placeholders look like `<state>`: a string that is one is a
/// wildcard when matching ([`is_placeholder`]). Numbers stay numbers (times
/// become 0), so a client can still parse what the fixture server sends.
#[derive(Debug, Clone, Default)]
pub struct Normalize {
    /// Real value and placeholder, longest real value first.
    subs: Vec<(String, String)>,
    sessions: u32,
}

impl Normalize {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write `placeholder` wherever `real` appears in a string.
    pub fn sub(&mut self, real: impl Into<String>, placeholder: &str) {
        let real = real.into();
        if real.is_empty() || self.subs.iter().any(|(r, _)| *r == real) {
            return;
        }
        self.subs.push((real, placeholder.to_owned()));
        self.subs.sort_by_key(|s| std::cmp::Reverse(s.0.len()));
    }

    /// Session names (made up, two random words) seen in `v`, from
    /// `session_name` fields and `sessions[].name`: each gets
    /// `<session:N>`, in the order seen.
    pub fn learn(&mut self, v: &Value) {
        match v {
            Value::Object(m) => {
                if let Some(Value::String(s)) = m.get("session_name") {
                    self.session(s);
                }
                if let Some(Value::Array(a)) = m.get("sessions") {
                    for s in a {
                        if let Some(Value::String(n)) = s.get("name") {
                            self.session(n);
                        }
                    }
                }
                m.values().for_each(|x| self.learn(x));
            }
            Value::Array(a) => a.iter().for_each(|x| self.learn(x)),
            _ => {}
        }
    }

    fn session(&mut self, name: &str) {
        if !name.is_empty() && !is_placeholder(name) && !self.subs.iter().any(|(r, _)| r == name) {
            self.sessions += 1;
            let p = format!("<session:{}>", self.sessions);
            self.sub(name, &p);
        }
    }

    pub fn string(&self, s: &str) -> String {
        let mut s = s.to_owned();
        for (real, p) in &self.subs {
            if s.contains(real.as_str()) {
                s = s.replace(real.as_str(), p);
            }
        }
        s
    }

    /// Normalize `v` in place.
    pub fn value(&self, v: &mut Value) {
        match v {
            Value::String(s) => *s = self.string(s),
            Value::Number(n) => {
                if n.as_u64().is_some_and(|n| n >= TIME) {
                    *v = Value::from(0);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| self.value(x)),
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if k.contains("token") && x.is_string() {
                        *x = Value::from("<token>");
                    } else {
                        self.value(x);
                    }
                }
            }
            Value::Null | Value::Bool(_) => {}
        }
    }

    /// The other way, for replaying a client's side: placeholders back to
    /// this run's values. Placeholders it has no value for stay.
    pub fn back(&self, s: &str) -> String {
        let mut s = s.to_owned();
        for (real, p) in &self.subs {
            if s.contains(p.as_str()) {
                s = s.replace(p.as_str(), real);
            }
        }
        s
    }

    pub fn value_back(&self, v: &mut Value) {
        match v {
            Value::String(s) => *s = self.back(s),
            Value::Array(a) => a.iter_mut().for_each(|x| self.value_back(x)),
            Value::Object(m) => m.values_mut().for_each(|x| self.value_back(x)),
            _ => {}
        }
    }

    /// A frame, its text normalized. A payload that changes length moves
    /// the offsets of the pane's later frames by as much (`shift`, per
    /// pane, kept across one connection's frames), so a client counting
    /// bytes still finds them contiguous.
    pub fn frame(&self, f: &Frame, shift: &mut HashMap<PaneId, i64>) -> FrameRec {
        let mut r = FrameRec::new(f);
        let s = shift.entry(f.pane).or_default();
        r.offset = (f.offset as i64 + *s).max(0) as u64;
        if let Some(d) = &r.data {
            let n = self.string(d);
            if f.kind == FrameKind::Output {
                *s += n.len() as i64 - d.len() as i64;
            }
            r.data = Some(n);
        }
        r
    }
}

/// A placeholder: `<...>` with no spaces, as [`Normalize`] writes them.
pub fn is_placeholder(s: &str) -> bool {
    s.len() > 2 && s.starts_with('<') && s.ends_with('>') && !s[1..s.len() - 1].contains(['<', '>', ' ', '\t', '\n'])
}

/// Does `got` read as `want`, each placeholder in `want` standing for any
/// text (`<home>/x` matches `/Users/a/x`)?
pub fn text_matches(want: &str, got: &str) -> bool {
    // The literal pieces between placeholders.
    let mut parts = vec![];
    let (mut lit, mut rest) = (String::new(), want);
    while let Some(i) = rest.find('<') {
        let ph = rest[i..].find('>').map(|j| &rest[i..i + j + 1]).filter(|p| is_placeholder(p));
        match ph {
            Some(p) => {
                lit.push_str(&rest[..i]);
                parts.push(std::mem::take(&mut lit));
                rest = &rest[i + p.len()..];
            }
            None => {
                lit.push_str(&rest[..i + 1]);
                rest = &rest[i + 1..];
            }
        }
    }
    lit.push_str(rest);
    parts.push(lit);
    if parts.len() == 1 {
        return want == got;
    }
    let (first, last) = (&parts[0], &parts[parts.len() - 1]);
    if !got.starts_with(first.as_str()) || got.len() < first.len() + last.len() {
        return false;
    }
    let mut g = &got[first.len()..];
    for p in &parts[1..parts.len() - 1] {
        match g.find(p.as_str()) {
            Some(i) => g = &g[i + p.len()..],
            None => return false,
        }
    }
    g.ends_with(last.as_str())
}

fn at(path: &str, k: impl std::fmt::Display) -> String {
    if path.is_empty() { k.to_string() } else { format!("{path}.{k}") }
}

/// For the fixture server: does what a client sent (`got`) say what the
/// fixture's client said (`want`)? Every field of `want` must be in `got`
/// with the same value (a `null` matches a missing field, and a client may
/// send fields the fixture doesn't have); arrays match element by element;
/// a placeholder matches anything.
pub fn subset(want: &Value, got: &Value) -> Result<(), String> {
    subset_at("", want, got)
}

fn subset_at(path: &str, want: &Value, got: &Value) -> Result<(), String> {
    match (want, got) {
        (Value::String(w), _) if is_placeholder(w) => Ok(()),
        (Value::String(w), Value::String(g)) if text_matches(w, g) => Ok(()),
        (Value::Object(w), Value::Object(g)) => {
            for (k, wv) in w {
                match g.get(k) {
                    Some(gv) => subset_at(&at(path, k), wv, gv)?,
                    None if wv.is_null() => {}
                    None => return Err(format!("{}: missing", at(path, k))),
                }
            }
            Ok(())
        }
        (Value::Array(w), Value::Array(g)) => {
            if w.len() != g.len() {
                return Err(format!("{path}: {} items, not {}", g.len(), w.len()));
            }
            w.iter().zip(g).enumerate().try_for_each(|(i, (w, g))| subset_at(&at(path, i), w, g))
        }
        (Value::Number(w), Value::Number(g)) if w.as_f64() == g.as_f64() => Ok(()),
        (w, g) if w == g => Ok(()),
        (w, g) => Err(format!("{}: {g}, not {w}", if path.is_empty() { "value" } else { path })),
    }
}

/// The fields whose values say what a message is: equal in [`shape`].
const KINDS: [&str; 3] = ["type", "kind", "op"];

/// For replaying against a daemon: does its answer (`got`) still give a
/// client that saw `want` what it relied on? Every field of `want` must be
/// in `got`, of the same JSON type, recursively; `type`, `kind` and `op`
/// must be equal (they say what a message is). Values otherwise may
/// differ, `null` on either side matches anything, extra fields are fine,
/// and each item of an array in `want` must have an item in `got` of its
/// shape.
pub fn shape(want: &Value, got: &Value) -> Result<(), String> {
    shape_at("", want, got)
}

fn shape_at(path: &str, want: &Value, got: &Value) -> Result<(), String> {
    let name = |path: &str| if path.is_empty() { "value".to_owned() } else { path.to_owned() };
    match (want, got) {
        (Value::Null, _) | (_, Value::Null) => Ok(()),
        (Value::String(w), _) if is_placeholder(w) => Ok(()),
        (Value::Object(w), Value::Object(g)) => {
            for (k, wv) in w {
                let p = at(path, k);
                match g.get(k) {
                    None if wv.is_null() => {}
                    None => return Err(format!("{p}: missing")),
                    Some(gv) if KINDS.contains(&k.as_str()) && wv.is_string() => {
                        if gv != wv {
                            return Err(format!("{p}: {gv}, not {wv}"));
                        }
                    }
                    Some(gv) => shape_at(&p, wv, gv)?,
                }
            }
            Ok(())
        }
        (Value::Array(w), Value::Array(g)) => {
            for (i, wv) in w.iter().enumerate() {
                let p = at(path, i);
                if !g.iter().any(|gv| shape_at(&p, wv, gv).is_ok()) {
                    let why = g.first().map(|gv| shape_at(&p, wv, gv).unwrap_err()).unwrap_or(format!("{p}: missing"));
                    return Err(why);
                }
            }
            Ok(())
        }
        (Value::String(_), Value::String(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::Bool(_), Value::Bool(_)) => Ok(()),
        (w, g) => Err(format!("{}: {g}, not like {w}", name(path))),
    }
}

/// Terminal output as plain text: escape sequences and control characters
/// other than newlines taken out.
pub fn clean(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => match it.next() {
                // CSI: parameters, then a final byte in @..~.
                Some('[') => {
                    for c in it.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC, DCS, APC, PM, SOS: up to BEL or ST.
                Some(']' | 'P' | '_' | '^' | 'X') => {
                    while let Some(c) = it.next() {
                        if c == '\x07' || (c == '\x1b' && it.peek() == Some(&'\\')) {
                            if c == '\x1b' {
                                it.next();
                            }
                            break;
                        }
                    }
                }
                // Charset designations take one more.
                Some('(' | ')' | '*' | '+' | '#' | '%') => {
                    it.next();
                }
                _ => {}
            },
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The words of `text` (letters, digits, `-` and `_`) three or more long:
/// what a replay looks for in a pane's output, whatever the chunking,
/// prompt redraws and escape sequences around them.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_round_trip() {
        let text = r#"{"fixture":"x","version":"0.1.0","about":"a test"}
{"t":0,"c":1,"from":"client","request":{"method":"GET","path":"/api/panes"}}
{"t":1,"c":1,"from":"daemon","response":{"status":200,"type":"application/json","body":[]}}
{"t":2,"c":2,"from":"client","open":"/ws"}
{"t":3,"c":2,"from":"daemon","msg":{"type":"pong","id":1}}
{"t":4,"c":2,"from":"daemon","frame":{"kind":"output","pane":1,"offset":0,"data":"hi"}}
{"t":5,"c":2,"from":"client","close":true}
"#;
        // Written back with the keys of what it carries sorted, whatever
        // order serde_json keeps them in.
        let sorted = text.replace(r#"{"type":"pong","id":1}"#, r#"{"id":1,"type":"pong"}"#);
        let f = Fixture::parse(text).unwrap();
        assert_eq!(f.events.len(), 6);
        assert!(matches!(&f.events[3].what, What::Msg(m) if m["type"] == "pong"));
        assert_eq!(
            f.events[4].what,
            What::Frame(FrameRec::new(&Frame { kind: FrameKind::Output, pane: 1, offset: 0, data: b"hi".to_vec() }))
        );
        assert_eq!(f.to_jsonl(), sorted);
        assert_eq!(Fixture::parse(&sorted).unwrap().to_jsonl(), sorted);
    }

    #[test]
    fn frames_keep_bytes() {
        let f = Frame { kind: FrameKind::Output, pane: 2, offset: 9, data: vec![0x1b, b'[', 0xff, 0] };
        let r = FrameRec::new(&f);
        assert!(r.data.is_none());
        assert_eq!(r.frame().unwrap(), f);
    }

    #[test]
    fn normalizing() {
        let mut n = Normalize::new();
        n.sub("/tmp/ilg-x-1", "<state>");
        n.sub("/tmp", "<tmp>");
        let mut v = json!({"cwd": "/tmp/ilg-x-1/home", "other": "/tmp/y", "at_ms": 1791242902807u64, "token": "abc", "n": 3,
                           "sessions": [{"name": "misty maple"}]});
        n.learn(&v);
        n.value(&mut v);
        assert_eq!(
            v,
            json!({"cwd": "<state>/home", "other": "<tmp>/y", "at_ms": 0, "token": "<token>", "n": 3,
                   "sessions": [{"name": "<session:1>"}]})
        );
        assert_eq!(n.back("<state>/home"), "/tmp/ilg-x-1/home");
        // Output that gets longer moves the pane's later offsets.
        let mut shift = HashMap::new();
        let a = n
            .frame(&Frame { kind: FrameKind::Output, pane: 1, offset: 0, data: b"/tmp/ilg-x-1 ".to_vec() }, &mut shift);
        let b = n.frame(&Frame { kind: FrameKind::Output, pane: 1, offset: 13, data: b"$ ".to_vec() }, &mut shift);
        assert_eq!((a.data.as_deref(), a.offset), (Some("<state> "), 0));
        assert_eq!(b.offset, 8);
    }

    #[test]
    fn matching() {
        let want = json!({"type": "attach", "panes": [{"pane": 1, "offset": null}], "cwd": "<home>"});
        let tui = json!({"type": "attach", "panes": [{"pane": 1, "offset": null, "history": 10000}], "zstd": true, "cwd": "/x"});
        assert_eq!(subset(&want, &tui), Ok(()));
        assert!(subset(&want, &json!({"type": "attach", "panes": [{"pane": 2}]})).is_err());
        assert!(text_matches("/api/run?cwd=<home>/x&a=<n>", "/api/run?cwd=/Users/a/x&a=4"));
        assert!(!text_matches("/api/run?cwd=<home>/x", "/api/run?cwd=/Users/a/y"));
        assert!(text_matches("a < b", "a < b") && !text_matches("<x>y", "y2"));

        let want = json!({"type": "state", "state": {"panes": [{"id": 1, "cwd": "<home>", "last": {"exit": 0}}]}});
        let later = json!({"type": "state", "state": {"panes": [{"id": 4, "cwd": "/y", "last": null, "new": true}]}});
        assert_eq!(shape(&want, &later), Ok(()));
        assert!(shape(&want, &json!({"type": "delta"})).is_err());
        assert!(shape(&want, &json!({"type": "state", "state": {"panes": [{"id": "1"}]}})).is_err());
        assert!(shape(&want, &json!({"type": "state", "state": {"panes": [{"cwd": "/y"}]}})).is_err());
    }

    #[test]
    fn text_of_output() {
        let out = clean(b"\x1b]133;A\x07$ echo hi\r\n\x1b[1mhi\x1b[0m\r\n");
        assert_eq!(out, "$ echo hi\nhi\n");
        assert_eq!(words("$ echo fixture-42\n"), ["echo", "fixture-42"]);
    }
}
