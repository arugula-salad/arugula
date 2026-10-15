//! The account stream (#942, F1): one `GET /api/events/stream?blocks=true`
//! per login, every event of every conversation the account owns, fanned
//! out to the daemon's own subscribers ([`Hub`]).
//!
//! **One daemon runs it**, the home one: `arugulad
//! --fountain-account-stream` (`ARUGULA_FOUNTAIN_ACCOUNT_STREAM=1`), off by
//! default, so two machines signed in to one account don't both raise a
//! card for each permission request. The `fountain` flag must be on too. It
//! uses [`Account::resolve`]'s login and starts again on every sign-in or
//! sign-out.
//!
//! **The cursor.** Fountain numbers events across the account, and replays
//! what came after `Last-Event-ID` (finished conversations too). The last id
//! handed to the subscribers is kept in `fountain/stream.json` (0600), with
//! the account it belongs to, after each event; a reconnect, and a daemon
//! started again (after a crash too), sends it, so nothing is missed. An
//! event at or below the cursor (Fountain's replay overlapping what was
//! seen) is dropped, so nothing is handed on twice. The write isn't synced
//! to disk: a power cut can hand on the last few events again, never skip
//! one. With no cursor yet, Fountain sends only what happens from then on.
//!
//! **Reconnecting.** Fountain sends a heartbeat every 15 s and closes a
//! stream after 60 s idle; a close is normal and reconnects at once. Silence
//! past [`idle`] counts as a dead connection. Errors back off (1 s doubling to
//! a minute, with jitter; `Retry-After` when Fountain gives one). A refused
//! key (401) or another 4xx waits for a new sign-in (or five minutes).
//!
//! **Subscribers** get [`Event`]s: `output` and `stage` log events (with
//! `blocks`, so a `permission_request` carries its `request_id`) and
//! `conversations` signals (the list changed: re-list). By conversation
//! ([`Hub::subscribe`]) or all of them ([`Hub::subscribe_all`]). A subscriber
//! that falls behind loses the oldest (a broadcast channel's `Lagged`) and
//! should read the conversation's history to catch up.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::{
    account::{Account, Resolved},
    client::{self, Error},
};

/// One event off the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Fountain's id (0 for a signal, which has none).
    pub id: u64,
    /// The SSE event: `output`, `stage` or `conversations`.
    pub kind: String,
    /// The conversation it's from (none for `conversations`).
    pub conversation_id: Option<String>,
    /// The payload: `{conversation_id, kind, stream, data, stage, state,
    /// turn_id, ts, blocks, …}`, or `{reason}` for a signal.
    pub data: Value,
}

/// How many events a subscriber may fall behind by.
const CAPACITY: usize = 1024;

/// The fan-out: every event to the all-events subscribers, and each to its
/// conversation's.
pub struct Hub {
    all: broadcast::Sender<Arc<Event>>,
    convs: Mutex<HashMap<String, broadcast::Sender<Arc<Event>>>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self { all: broadcast::channel(CAPACITY).0, convs: Mutex::default() }
    }
}

impl Hub {
    /// Every event, and every `conversations` signal.
    pub fn subscribe_all(&self) -> broadcast::Receiver<Arc<Event>> {
        self.all.subscribe()
    }

    /// One conversation's events.
    #[allow(dead_code)] // The conversation link (F3).
    pub fn subscribe(&self, conversation_id: &str) -> broadcast::Receiver<Arc<Event>> {
        let mut convs = self.convs.lock().unwrap();
        convs.entry(conversation_id.to_owned()).or_insert_with(|| broadcast::channel(CAPACITY).0).subscribe()
    }

    pub fn publish(&self, ev: Event) {
        let ev = Arc::new(ev);
        if let Some(c) = &ev.conversation_id {
            let mut convs = self.convs.lock().unwrap();
            if let Some(tx) = convs.get(c)
                && tx.send(ev.clone()).is_err()
            {
                // Nobody listens to it any more.
                convs.remove(c);
            }
        }
        let _ = self.all.send(ev);
    }
}

// ------------------------------------------------------------------ SSE

/// One message off the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub id: Option<String>,
    pub event: String,
    pub data: String,
}

/// Splits bytes into SSE messages: a blank line ends one (`\n` or `\r\n`),
/// comments (`: heartbeat`) are skipped, `data:` lines are joined.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
}

impl Parser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Message> {
        self.buf.extend_from_slice(bytes);
        let mut out = vec![];
        while let Some((end, len)) = boundary(&self.buf) {
            let chunk: Vec<u8> = self.buf.drain(..end + len).take(end).collect();
            if let Some(m) = parse_chunk(&String::from_utf8_lossy(&chunk)) {
                out.push(m);
            }
        }
        out
    }
}

fn boundary(b: &[u8]) -> Option<(usize, usize)> {
    let lf = b.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = b.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(c)) => Some(if a.0 <= c.0 { a } else { c }),
        (a, c) => a.or(c),
    }
}

fn parse_chunk(chunk: &str) -> Option<Message> {
    let mut m = Message { event: "message".into(), ..Message::default() };
    let mut data: Vec<&str> = vec![];
    for line in chunk.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "id" => m.id = Some(value.to_owned()),
            "event" => m.event = value.to_owned(),
            "data" => data.push(value),
            _ => {}
        }
    }
    if data.is_empty() && m.id.is_none() {
        return None;
    }
    m.data = data.join("\n");
    Some(m)
}

// --------------------------------------------------------------- cursor

/// The kept cursor: the last id handed on, and whose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Cursor {
    account: String,
    last_event_id: u64,
}

/// Which account a cursor belongs to: the instance and the account's id
/// (else a hash of the key: a new key starts afresh).
fn account_of(login: &Resolved) -> String {
    use sha2::Digest;
    let who = match &login.user_id {
        Some(id) => format!("user:{id}"),
        None => format!("key:{}", &hex::encode(sha2::Sha256::digest(login.key.as_bytes()))[..16]),
    };
    format!("{} {who}", login.base_url)
}

fn cursor_file(dir: &Path) -> PathBuf {
    dir.join("stream.json")
}

fn load_cursor(dir: &Path, account: &str) -> u64 {
    std::fs::read(cursor_file(dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<Cursor>(&b).ok())
        .filter(|c| c.account == account)
        .map_or(0, |c| c.last_event_id)
}

/// Write the cursor whole (a rename), without waiting for the disk: one
/// write per event.
fn save_cursor(dir: &Path, c: &Cursor) -> std::io::Result<()> {
    crate::store::private_dir(dir)?;
    let file = cursor_file(dir);
    let tmp = file.with_extension("tmp");
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        crate::perm::open_mode(o.write(true).create(true).truncate(true), 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(&serde_json::to_vec(c).unwrap_or_default())?;
    }
    std::fs::rename(&tmp, &file)
}

// ---------------------------------------------------------------- the run

/// Timings, from the environment for tests.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Silence (not even a heartbeat) that counts as a dead connection.
    pub idle: Duration,
    /// The first wait after an error; doubles to `max`.
    pub retry: Duration,
    pub max: Duration,
    /// After a refusal no retry fixes (401, another 4xx).
    pub refused: Duration,
}

/// Fountain's heartbeat is every 15 s: three missed is a dead connection.
pub fn idle() -> Duration {
    ms_var("ARUGULA_FOUNTAIN_STREAM_IDLE_MS").unwrap_or(Duration::from_secs(45))
}

fn ms_var(k: &str) -> Option<Duration> {
    std::env::var(k).ok()?.trim().parse().ok().map(Duration::from_millis)
}

impl Default for Timing {
    fn default() -> Self {
        let retry = ms_var("ARUGULA_FOUNTAIN_STREAM_RETRY_MS").unwrap_or(Duration::from_secs(1));
        Self { idle: idle(), retry, max: (retry * 60).max(retry), refused: Duration::from_secs(300) }
    }
}

/// How one connection ended.
enum Ended {
    /// Fountain closed it (idle), or it went silent: reconnect now.
    Closed,
    Failed(Error),
}

/// The status, as the stream changes it.
fn set(account: &Account, f: impl FnOnce(&mut arugula_proto::api::FountainStream)) {
    f(&mut account.stream.lock().unwrap());
}

/// Run the account stream for as long as the daemon does.
pub async fn run(account: Arc<Account>, timing: Timing) {
    set(&account, |s| s.enabled = true);
    let mut changes = account.changes();
    loop {
        changes.borrow_and_update();
        if !arugula_proto::flags::on(account.state_dir()).has(arugula_proto::flags::FOUNTAIN) {
            set(&account, |s| {
                s.connected = false;
                s.error = Some("the fountain flag is off here".into());
            });
            wait(&mut changes, Duration::from_secs(30)).await;
            continue;
        }
        let login = match account.resolve().await {
            Ok(l) => l,
            Err(e) => {
                set(&account, |s| {
                    s.connected = false;
                    s.error = Some(e);
                });
                wait(&mut changes, Duration::from_secs(60)).await;
                continue;
            }
        };
        follow(&account, &login, &mut changes, timing).await;
    }
}

/// Wait for a sign-in or sign-out, or `d`.
async fn wait(changes: &mut tokio::sync::watch::Receiver<u64>, d: Duration) {
    let _ = tokio::time::timeout(d, changes.changed()).await;
}

/// Follow one login's stream until the login changes (or its key is
/// refused for long enough to read it again).
async fn follow(
    account: &Arc<Account>,
    login: &Resolved,
    changes: &mut tokio::sync::watch::Receiver<u64>,
    timing: Timing,
) {
    let who = account_of(login);
    let mut cursor = Cursor { last_event_id: load_cursor(account.dir(), &who), account: who };
    set(account, |s| s.cursor = cursor.last_event_id);
    info!(base_url = %login.base_url, source = login.source.name(), cursor = cursor.last_event_id, "Fountain account stream");
    let http = client::http(None);
    let mut failures = 0u32;
    loop {
        let ended = tokio::select! {
            _ = changes.changed() => return,
            e = connect(account, &http, login, &mut cursor, timing.idle) => e,
        };
        set(account, |s| s.connected = false);
        let pause = match ended {
            Ended::Closed => {
                failures = 0;
                Duration::from_millis(100)
            }
            Ended::Failed(e) => {
                failures += 1;
                set(account, |s| s.error = Some(e.to_string()));
                if e.transient() {
                    let backoff = timing.retry.saturating_mul(1 << (failures - 1).min(16)).min(timing.max);
                    let pause = e.retry_after().unwrap_or(backoff);
                    warn!(error = %e, retry_ms = pause.as_millis() as u64, "Fountain account stream");
                    jitter(pause)
                } else {
                    warn!(error = %e, "Fountain account stream: refused; waiting for a new sign-in");
                    wait(changes, timing.refused).await;
                    return;
                }
            }
        };
        tokio::select! {
            _ = changes.changed() => return,
            _ = tokio::time::sleep(pause) => {}
        }
    }
}

/// Up to a quarter more, so daemons that lost Fountain together don't come
/// back together.
fn jitter(d: Duration) -> Duration {
    let r: [u8; 2] = crate::push::random();
    let frac = u16::from_le_bytes(r) as u64 % 1000;
    d + d.mul_f64(frac as f64 / 4000.0)
}

/// One connection: read until it ends, handing each new event on and
/// keeping the cursor.
async fn connect(
    account: &Account,
    http: &reqwest::Client,
    login: &Resolved,
    cursor: &mut Cursor,
    idle: Duration,
) -> Ended {
    let url = format!("{}/api/events/stream?blocks=true", login.base_url);
    let mut req = http.get(&url).bearer_auth(&login.key).header(reqwest::header::ACCEPT, "text/event-stream");
    if cursor.last_event_id > 0 {
        req = req.header("last-event-id", cursor.last_event_id.to_string());
    }
    let res = match tokio::time::timeout(idle, req.send()).await {
        Err(_) => return Ended::Failed(Error::Unreachable(format!("{}: no answer", login.base_url))),
        Ok(Err(e)) => return Ended::Failed(Error::Unreachable(format!("{}: {e}", login.base_url))),
        Ok(Ok(r)) => r,
    };
    let status = res.status();
    if !status.is_success() {
        let after = client::retry_after(res.headers());
        let body = res.bytes().await.unwrap_or_default();
        return Ended::Failed(client::error_for(status.as_u16(), &body, after));
    }
    set(account, |s| {
        s.connected = true;
        s.error = None;
    });
    let mut res = res;
    let mut parser = Parser::default();
    loop {
        let chunk = match tokio::time::timeout(idle, res.chunk()).await {
            Err(_) => {
                info!("Fountain account stream: silent; reconnecting");
                return Ended::Closed;
            }
            Ok(Err(e)) => return Ended::Failed(Error::Unreachable(format!("the stream broke: {e}"))),
            Ok(Ok(None)) => return Ended::Closed,
            Ok(Ok(Some(c))) => c,
        };
        for m in parser.push(&chunk) {
            handle(account, cursor, m);
        }
    }
}

/// Hand one message on, unless it's one already handed on.
fn handle(account: &Account, cursor: &mut Cursor, m: Message) {
    let id = m.id.as_deref().and_then(|i| i.trim().parse::<u64>().ok()).unwrap_or(0);
    if id > 0 && id <= cursor.last_event_id {
        return;
    }
    let data: Value =
        if m.data.is_empty() { Value::Null } else { serde_json::from_str(&m.data).unwrap_or(Value::String(m.data)) };
    let conversation_id = data["conversation_id"].as_str().map(str::to_owned);
    account.hub().publish(Event { id, kind: m.event, conversation_id, data });
    if id > 0 {
        cursor.last_event_id = id;
        if let Err(e) = save_cursor(account.dir(), cursor) {
            warn!(error = %e, "Fountain account stream: can't keep the cursor");
        }
        set(account, |s| {
            s.cursor = id;
            s.delivered += 1;
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use axum::{
        Router,
        body::Body,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::json;

    use super::*;
    use crate::labs::fountain::account::Source;

    #[test]
    fn sse_as_fountain_frames_it() {
        let mut p = Parser::default();
        assert!(p.push(b": connected\n\n").is_empty());
        let a = p.push(b"id: 7\nevent: output\ndata: {\"conversation_id\":\"c1\"}\n\nid: 8\nev");
        assert_eq!(
            a,
            [Message { id: Some("7".into()), event: "output".into(), data: "{\"conversation_id\":\"c1\"}".into() }]
        );
        let b = p.push(b"ent: stage\r\ndata: a\r\ndata: b\r\n\r\n: heartbeat\n\nevent: conversations\ndata: {\"reason\":\"changed\"}\n\n");
        assert_eq!(b[0], Message { id: Some("8".into()), event: "stage".into(), data: "a\nb".into() });
        assert_eq!(b[1].event, "conversations");
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn the_hub_fans_out_by_conversation() {
        let hub = Hub::default();
        let mut all = hub.subscribe_all();
        let mut c1 = hub.subscribe("c1");
        let ev = |id, c: Option<&str>| Event {
            id,
            kind: "output".into(),
            conversation_id: c.map(str::to_owned),
            data: Value::Null,
        };
        hub.publish(ev(1, Some("c1")));
        hub.publish(ev(2, Some("c2")));
        hub.publish(ev(0, None));
        assert_eq!(c1.try_recv().unwrap().id, 1);
        assert!(c1.try_recv().is_err(), "only c1's");
        let ids: Vec<u64> = (0..3).map(|_| all.try_recv().unwrap().id).collect();
        assert_eq!(ids, [1, 2, 0]);
        drop(c1);
        hub.publish(ev(3, Some("c1")));
        assert!(hub.convs.lock().unwrap().is_empty(), "a channel nobody reads is dropped");
    }

    /// A stub account stream: events 1..=`total`, replaying after
    /// `Last-Event-ID` (and, to be awkward, the event at it again). Each
    /// connection sends at most `per` events and closes, as an idle
    /// Fountain does. The cursors it was sent are recorded.
    #[derive(Clone)]
    struct Stub {
        total: Arc<Mutex<u64>>,
        per: u64,
        cursors: Arc<Mutex<Vec<Option<u64>>>>,
        refuse: Arc<AtomicBool>,
        hits: Arc<AtomicU32>,
    }

    async fn stream(State(s): State<Stub>, h: HeaderMap, q: axum::extract::RawQuery) -> Response {
        assert_eq!(q.0.as_deref(), Some("blocks=true"));
        assert_eq!(h["authorization"], "Bearer k");
        s.hits.fetch_add(1, Ordering::SeqCst);
        if s.refuse.load(Ordering::SeqCst) {
            return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "error": "bad key", "reason": "invalid_api_key" })))
                .into_response();
        }
        let after = h.get("last-event-id").map(|v| v.to_str().unwrap().parse::<u64>().unwrap());
        s.cursors.lock().unwrap().push(after);
        let total = *s.total.lock().unwrap();
        let from = after.map_or(1, |a| a.max(1));
        let mut body = String::from(": connected\n\n");
        for id in (from..=total).take(s.per as usize) {
            body.push_str(&format!(
                "id: {id}\nevent: output\ndata: {}\n\n",
                json!({ "conversation_id": format!("c{}", id % 2), "kind": "output", "data": format!("e{id}") })
            ));
            if id % 3 == 0 {
                body.push_str("event: conversations\ndata: {\"reason\":\"changed\"}\n\n: heartbeat\n\n");
            }
        }
        Response::builder().header("content-type", "text/event-stream").body(Body::from(body)).unwrap()
    }

    async fn serve(s: Stub) -> String {
        let app = Router::new().route("/api/events/stream", get(stream)).with_state(s);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        at
    }

    fn fast() -> Timing {
        Timing {
            idle: Duration::from_secs(5),
            retry: Duration::from_millis(20),
            max: Duration::from_millis(100),
            refused: Duration::from_millis(100),
        }
    }

    /// Collect `n` events' ids from `rx`.
    async fn take(rx: &mut broadcast::Receiver<Arc<Event>>, n: usize) -> Vec<u64> {
        let mut ids = vec![];
        while ids.len() < n {
            let ev = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("events").unwrap();
            if ev.id > 0 {
                ids.push(ev.id);
            }
        }
        ids
    }

    /// F1's "done when": a daemon stopped mid-stream (here: its task
    /// aborted, as a crash would) and started again resumes from the kept
    /// cursor, with no gap and nothing twice, across Fountain's idle closes
    /// (each reconnect is sent the event at the cursor again: dropped).
    #[tokio::test]
    async fn a_restart_resumes_from_the_cursor_with_no_gap_or_duplicate() {
        let stub = Stub {
            total: Arc::new(Mutex::new(10)),
            per: 4,
            cursors: Arc::default(),
            refuse: Arc::default(),
            hits: Arc::default(),
        };
        let base = serve(stub.clone()).await;
        let dir = std::env::temp_dir().join(format!("ilg-fountain-stream-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        arugula_proto::flags::set(&dir, arugula_proto::flags::FOUNTAIN, true).unwrap();
        let login =
            Resolved { base_url: base.clone(), key: "k".into(), source: Source::Env, user_id: Some("u1".into()) };

        // The first daemon: 1..=10 over three connections (4, 4, 2).
        let a = Arc::new(Account::open(&dir, "n", None, Duration::from_millis(10)));
        let mut all = a.hub().subscribe_all();
        let mut c1 = a.hub().subscribe("c1");
        let (mut changes, t) = (a.changes(), fast());
        let a2 = a.clone();
        let l2 = login.clone();
        let task = tokio::spawn(async move { follow(&a2, &l2, &mut changes, t).await });
        let first = take(&mut all, 10).await;
        assert_eq!(first, (1..=10).collect::<Vec<_>>());
        let odd: Vec<u64> = (0..5).map(|_| c1.try_recv().unwrap().id).collect();
        assert_eq!(odd, [1, 3, 5, 7, 9], "c1's only");
        task.abort();
        let _ = task.await;
        assert_eq!(load_cursor(&dir.join("fountain"), &account_of(&login)), 10);
        assert_eq!(a.stream.lock().unwrap().delivered, 10);

        // While it's down, more happens.
        *stub.total.lock().unwrap() = 17;
        let b = Arc::new(Account::open(&dir, "n", None, Duration::from_millis(10)));
        let mut all = b.hub().subscribe_all();
        let (mut changes, b2) = (b.changes(), b.clone());
        let task = tokio::spawn(async move { follow(&b2, &login, &mut changes, t).await });
        let second = take(&mut all, 7).await;
        assert_eq!(second, (11..=17).collect::<Vec<_>>(), "resumed: nothing missed, nothing twice");
        tokio::time::sleep(Duration::from_millis(300)).await;
        while let Ok(ev) = all.try_recv() {
            assert_eq!(ev.id, 0, "only signals after 17");
        }
        assert_eq!(b.stream.lock().unwrap().cursor, 17);
        assert_eq!(b.stream.lock().unwrap().delivered, 7);
        let cursors = stub.cursors.lock().unwrap().clone();
        assert_eq!(&cursors[..4], [None, Some(4), Some(7), Some(10)], "{cursors:?}");
        assert!(cursors[4..].iter().all(|c| c.is_some_and(|c| c >= 10)), "{cursors:?}");
        task.abort();

        // Another account doesn't resume this one's cursor.
        let other = Resolved { base_url: base, key: "k".into(), source: Source::Env, user_id: Some("u2".into()) };
        assert_eq!(load_cursor(&dir.join("fountain"), &account_of(&other)), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("fountain/stream.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_refused_key_waits_for_a_new_sign_in() {
        let stub = Stub {
            total: Arc::new(Mutex::new(1)),
            per: 1,
            cursors: Arc::default(),
            refuse: Arc::new(AtomicBool::new(true)),
            hits: Arc::default(),
        };
        let base = serve(stub.clone()).await;
        let dir = std::env::temp_dir().join(format!("ilg-fountain-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Arc::new(Account::open(&dir, "n", None, Duration::from_millis(10)));
        let login = Resolved { base_url: base, key: "k".into(), source: Source::Env, user_id: None };
        let mut changes = a.changes();
        let t = Timing { refused: Duration::from_secs(30), ..fast() };
        let a2 = a.clone();
        let task = tokio::spawn(async move { follow(&a2, &login, &mut changes, t).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        let s = a.stream.lock().unwrap().clone();
        assert!(!s.connected && s.error.as_deref().is_some_and(|e| e.contains("refused the key")), "{s:?}");
        assert_eq!(stub.hits.load(Ordering::SeqCst), 1, "asked once, not again and again");
        // A sign-in (or sign-out) ends the wait.
        a.changed.send_modify(|n| *n += 1);
        tokio::time::timeout(Duration::from_secs(5), task).await.expect("follow returns").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
