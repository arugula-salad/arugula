//! Permission requests from any of the account's conversations (#942, F2):
//! a card and a phone push for each, answered from Arugula or anywhere else.
//!
//! **Where they come from.** The account stream ([`super::stream`]) carries
//! every conversation's events. An agent blocked on
//! `session/request_permission` shows there twice: an `output` event whose
//! `blocks` (`?blocks=true`) hold a `permission_request` (`request_id`, the
//! tool call's `name` and `summary`, the `options`), then a `request` stage
//! event, `started`, with the same `request_id`, the `tool`, the `options`
//! and `timeout_ms`. The stage event opens a request here; the block only
//! says more about it. Fountain resolves it on every outcome (an answer from
//! any client, a deny, the five-minute timeout, the turn ending) with the
//! `request` stage `done`, which closes it here: so an answer given in the
//! CLI, an editor or another app clears the card. The turn ending
//! (`turn` `done`, `failed` or `interrupted`) closes its conversation's too,
//! and one past its timeout is dropped whatever the stream says.
//!
//! **Only the daemon that runs the account stream** has any: the others'
//! [`Requests`] stay empty, so two machines signed in to one account never
//! push the same request twice. A replay after a restart opens a request
//! only if it can still be answered (its `ts` plus `timeout_ms` is ahead).
//!
//! **Where the card goes.** On an account view block ([`super::FountainBlock`]):
//! the open one with the lowest id holds them ([`Requests::holder`]), and if
//! none is open when a request comes, one is opened in a session of its own
//! (`fountain`), so a card always has a pane. It shows the newest request as
//! the card (the one just pushed) and lists every open one. A request whose
//! conversation an agent block here already shows (`agent: fountain`
//! following it) is listed but raises no card: that block asks itself.
//!
//! **Answering** ([`Requests::answer`]) is `POST
//! /api/conversations/:id/requests/:request_id` with one of the options the
//! agent offered, never one it didn't. The first answer wins: losing the race
//! (409, or 404/410 when the conversation went) closes the card and says the
//! request was already settled.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_proto::{BlockType, PaneId, api::OpenRequest};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};
use tracing::{info, warn};

use super::{account::Account, client::Error, stream::Event};
use crate::{server::App, store::now_ms};

/// Fountain's own timeout, when the stage event doesn't say.
const TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// How long a request outlives its timeout here before it's dropped (the
/// `done` that Fountain sends at the timeout should come first).
const GRACE_MS: u64 = 30_000;

/// How many resolved ids, and blocks' details, are remembered.
const REMEMBERED: usize = 256;

/// The session a card's block opens in when none is open.
pub const SESSION: &str = "fountain";

/// One open permission request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    /// Fountain's `request_id`: the card's id.
    pub id: String,
    pub conversation_id: String,
    /// The tool, as the stage event names it.
    pub tool: Option<String>,
    /// The tool call's name and what it does (the `permission_request`
    /// block's).
    pub name: Option<String>,
    pub summary: Option<String>,
    /// What the agent offered (`{optionId, name, kind}`), in its order.
    pub options: Vec<Value>,
    pub turn_id: Option<String>,
    pub at_ms: u64,
    /// When Fountain gives up on it and denies it.
    pub expires_ms: u64,
    /// The order it came in (two in one second).
    #[serde(skip)]
    pub seq: u64,
}

impl Request {
    /// What it asks to do, in a few words.
    pub fn title(&self) -> String {
        let s = |o: &Option<String>| o.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        s(&self.summary).or_else(|| s(&self.name)).or_else(|| s(&self.tool)).unwrap_or_else(|| "a tool".into())
    }

    /// The option an answer sends: `allow` (once, or `always`) or `deny`.
    /// Only one the agent offered.
    pub fn option(&self, allow: bool, always: bool) -> Result<String, String> {
        let kind = |o: &Value| o["kind"].as_str().unwrap_or_default().to_owned();
        let id = |o: &Value| o["optionId"].as_str().or(o["option_id"].as_str()).map(str::to_owned);
        let (exact, prefix) = match (allow, always) {
            (true, true) => ("allow_always", "allow"),
            (true, false) => ("allow_once", "allow"),
            (false, _) => ("reject_once", "reject"),
        };
        let pick = self
            .options
            .iter()
            .find(|o| kind(o) == exact)
            .or_else(|| self.options.iter().find(|o| kind(o).starts_with(prefix)))
            .and_then(id);
        pick.ok_or_else(|| {
            let what = if allow { "allowing" } else { "denying" };
            format!("the agent offered no option for {what} it: answer it where it was asked")
        })
    }
}

/// What a handled event changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Opened(String),
    Closed(Vec<String>),
}

/// The open requests, and who shows them.
pub struct Requests {
    open: Mutex<BTreeMap<String, Request>>,
    /// What the `permission_request` blocks said, by request id.
    details: Mutex<VecDeque<(String, Value)>>,
    /// Requests already resolved (a replay may bring one again).
    resolved: Mutex<VecDeque<String>>,
    /// The account view blocks open now.
    holders: Mutex<BTreeSet<PaneId>>,
    seq: std::sync::atomic::AtomicU64,
    changed: watch::Sender<u64>,
}

impl Default for Requests {
    fn default() -> Self {
        Self {
            open: Mutex::default(),
            details: Mutex::default(),
            resolved: Mutex::default(),
            holders: Mutex::default(),
            seq: Default::default(),
            changed: watch::channel(0).0,
        }
    }
}

static REQUESTS: LazyLock<Arc<Requests>> = LazyLock::new(Arc::default);

/// This daemon's.
pub fn requests() -> Arc<Requests> {
    REQUESTS.clone()
}

fn remember<T>(q: &mut VecDeque<T>, t: T) {
    q.push_back(t);
    while q.len() > REMEMBERED {
        q.pop_front();
    }
}

/// An ISO 8601 time in UTC (`2026-10-15T12:34:56.123456Z`, or without the
/// zone, as Elixir writes a naive one) as ms since the epoch.
pub fn iso_ms(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('Z');
    let s = s.split(['+']).next()?;
    let (date, time) = s.split_once(['T', ' '])?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms: i64 = frac.chars().chain("000".chars()).take(3).collect::<String>().parse().unwrap_or(0);
    let total = ((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000) + ms;
    u64::try_from(total).ok()
}

/// A stage event's own data: a JSON string (as Fountain logs it) or an
/// object.
fn meta(data: &Value) -> Value {
    match &data["data"] {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        v => v.clone(),
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned).filter(|s| !s.is_empty())
}

impl Requests {
    /// Told whenever the open requests, or who holds them, change.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    /// The open requests, newest first.
    pub fn open(&self) -> Vec<Request> {
        let mut v: Vec<Request> = self.open.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| b.seq.cmp(&a.seq)));
        v
    }

    pub fn get(&self, id: &str) -> Option<Request> {
        self.open.lock().unwrap().get(id).cloned()
    }

    /// The block that shows the cards: the lowest open one.
    pub fn holder(&self) -> Option<PaneId> {
        self.holders.lock().unwrap().first().copied()
    }

    pub fn hold(&self, pane: PaneId) {
        if self.holders.lock().unwrap().insert(pane) {
            self.bump();
        }
    }

    pub fn release(&self, pane: PaneId) {
        if self.holders.lock().unwrap().remove(&pane) {
            self.bump();
        }
    }

    /// One event off the account stream.
    pub fn handle(&self, ev: &Event, now: u64) -> Option<Change> {
        let conv = ev.conversation_id.as_deref()?;
        match (ev.kind.as_str(), ev.data["stage"].as_str(), ev.data["state"].as_str()) {
            ("output", ..) => {
                for b in ev.data["blocks"].as_array().into_iter().flatten() {
                    if b["kind"] != "permission_request" {
                        continue;
                    }
                    let Some(id) = text(&b["request_id"]) else { continue };
                    let enriched = match self.open.lock().unwrap().get_mut(&id) {
                        Some(r) => {
                            r.name = text(&b["name"]).or(r.name.take());
                            r.summary = text(&b["summary"]).or(r.summary.take());
                            true
                        }
                        None => false,
                    };
                    if enriched {
                        self.bump();
                    } else {
                        remember(&mut self.details.lock().unwrap(), (id, b.clone()));
                    }
                }
                None
            }
            ("stage", Some("request"), Some("started")) => {
                let m = meta(&ev.data);
                let id = text(&m["request_id"])?;
                if self.resolved.lock().unwrap().contains(&id) || self.open.lock().unwrap().contains_key(&id) {
                    return None;
                }
                let timeout = m["timeout_ms"].as_u64().filter(|t| *t > 0).unwrap_or(TIMEOUT_MS);
                let at = ev.data["ts"].as_str().and_then(iso_ms).unwrap_or(now);
                let expires_ms = at.saturating_add(timeout);
                if expires_ms <= now {
                    // Replayed after it could be answered.
                    return None;
                }
                let block = self.details.lock().unwrap().iter().rev().find(|(i, _)| *i == id).map(|d| d.1.clone());
                let block = block.unwrap_or(Value::Null);
                let mut options: Vec<Value> = m["options"].as_array().cloned().unwrap_or_default();
                if options.is_empty() {
                    options = block["options"].as_array().cloned().unwrap_or_default();
                }
                let r = Request {
                    id: id.clone(),
                    conversation_id: conv.to_owned(),
                    tool: text(&m["tool"]),
                    name: text(&block["name"]),
                    summary: text(&block["summary"]),
                    options,
                    turn_id: text(&ev.data["turn_id"]),
                    at_ms: at,
                    expires_ms,
                    seq: self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                };
                info!(conversation = conv, request = %id, tool = ?r.tool, "Fountain asks for permission");
                self.open.lock().unwrap().insert(id.clone(), r);
                self.bump();
                Some(Change::Opened(id))
            }
            ("stage", Some("request"), Some("done")) => {
                let id = text(&meta(&ev.data)["request_id"])?;
                self.resolve(&[id])
            }
            ("stage", Some("turn"), Some("done" | "failed" | "interrupted" | "cancelled")) => {
                // A request never outlives the turn that raised it.
                let turn = text(&ev.data["turn_id"]);
                let ids: Vec<String> = self
                    .open
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|r| {
                        r.conversation_id == conv && (turn.is_none() || r.turn_id.is_none() || r.turn_id == turn)
                    })
                    .map(|r| r.id.clone())
                    .collect();
                self.resolve(&ids)
            }
            _ => None,
        }
    }

    /// These are settled.
    fn resolve(&self, ids: &[String]) -> Option<Change> {
        let mut gone = vec![];
        {
            let mut open = self.open.lock().unwrap();
            let mut resolved = self.resolved.lock().unwrap();
            for id in ids {
                if !resolved.contains(id) {
                    remember(&mut resolved, id.clone());
                }
                if open.remove(id).is_some() {
                    gone.push(id.clone());
                }
            }
        }
        if gone.is_empty() {
            return None;
        }
        info!(requests = ?gone, "Fountain's permission requests settled");
        self.bump();
        Some(Change::Closed(gone))
    }

    /// Drop what's past its timeout (and the grace after it).
    pub fn prune(&self, now: u64) {
        let late: Vec<String> = self
            .open
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.expires_ms.saturating_add(GRACE_MS) <= now)
            .map(|r| r.id.clone())
            .collect();
        if !late.is_empty() {
            warn!(requests = ?late, "Fountain never said these were settled; dropping them past their timeout");
            self.resolve(&late);
        }
    }

    /// Answer one with the option `allow` (`always`) or deny picks, with the
    /// account's login. Losing the race closes it all the same.
    pub async fn answer(&self, account: &Account, id: &str, allow: bool, always: bool) -> Result<Value, String> {
        let r =
            self.get(id).ok_or_else(|| format!("no open Fountain request {id:?} (it was answered, or timed out)"))?;
        let option = r.option(allow, always)?;
        let login = account.resolve().await?;
        match login.client().answer(&r.conversation_id, &r.id, &option).await {
            Ok(()) => {
                info!(conversation = r.conversation_id, request = id, option, "answered a Fountain request");
                self.resolve(std::slice::from_ref(&r.id));
                Ok(json!({ "answered": r.id, "option": option, "conversation_id": r.conversation_id }))
            }
            Err(e @ (Error::Conflict { .. } | Error::NotFound { .. } | Error::Gone { .. })) => {
                info!(request = id, error = %e, "a Fountain request was settled before this answer");
                self.resolve(std::slice::from_ref(&r.id));
                Err(format!(
                    "{} was already settled on Fountain (answered elsewhere, timed out or its turn ended)",
                    r.title()
                ))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Follow the account stream for requests, for as long as the daemon runs:
/// the daemon that runs the stream starts this beside it.
pub async fn run(app: Arc<App>, account: Arc<Account>) {
    let reqs = requests();
    let mut rx = account.hub().subscribe_all();
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(ev) => {
                    if let Some(Change::Opened(_)) = reqs.handle(&ev, now_ms()) {
                        ensure_holder(&app, &reqs).await;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(missed = n, "Fountain attention fell behind the account stream");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = tick.tick() => reqs.prune(now_ms()),
        }
    }
}

/// A card needs a pane: an account view block, opened if none is.
async fn ensure_holder(app: &App, reqs: &Requests) {
    if reqs.holder().is_some() {
        return;
    }
    let req = OpenRequest {
        kind: BlockType::Fountain,
        config: json!({}),
        session: Some(SESSION.into()),
        local: true,
        ..Default::default()
    };
    match app.mux.api(|r| crate::mux::Api::Open(req, None, r)).await {
        Some(Ok(b)) => info!(block = b, "opened the Fountain account view for a permission request"),
        Some(Err(e)) => warn!(error = %e, "couldn't open the Fountain account view for a permission request"),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, conv: &str, data: Value) -> Event {
        Event { id: 1, kind: kind.into(), conversation_id: Some(conv.into()), data }
    }

    fn started(conv: &str, id: &str, ts: &str) -> Event {
        let meta = json!({ "request_id": id, "tool": "Bash", "timeout_ms": 300_000,
            "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                        { "optionId": "always", "name": "Always", "kind": "allow_always" },
                        { "optionId": "reject", "name": "Reject", "kind": "reject_once" }] });
        ev(
            "stage",
            conv,
            json!({ "conversation_id": conv, "kind": "stage", "stage": "request", "state": "started",
                    "data": meta.to_string(), "ts": ts, "turn_id": "t1" }),
        )
    }

    fn done(conv: &str, id: &str) -> Event {
        let meta = json!({ "request_id": id, "outcome": "answered", "option_id": "allow" });
        ev("stage", conv, json!({ "stage": "request", "state": "done", "data": meta.to_string() }))
    }

    #[test]
    fn times() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("2026-10-15T12:00:00.250000Z"), Some(1_792_065_600_250));
        assert_eq!(iso_ms("2026-10-15T12:00:00"), Some(1_792_065_600_000));
        assert_eq!(iso_ms("2000-02-29T00:00:01.5Z"), Some(951_782_401_500));
        assert_eq!(iso_ms("nope"), None);
    }

    #[test]
    fn a_request_opens_says_more_and_closes() {
        let r = Requests::default();
        let now = iso_ms("2026-10-15T12:00:10Z").unwrap();
        let block = json!({ "kind": "permission_request", "request_id": "0.ab", "name": "Bash", "summary": "rm -rf build",
                            "options": [] });
        // The block comes first, then the stage event.
        assert_eq!(r.handle(&ev("output", "c1", json!({ "blocks": [block] })), now), None);
        assert_eq!(r.handle(&started("c1", "0.ab", "2026-10-15T12:00:05Z"), now), Some(Change::Opened("0.ab".into())));
        let open = r.open();
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].title(), open[0].conversation_id.as_str()), ("rm -rf build".to_owned(), "c1"));
        assert_eq!(open[0].expires_ms, iso_ms("2026-10-15T12:05:05Z").unwrap());
        assert_eq!(open[0].option(true, false).unwrap(), "allow");
        assert_eq!(open[0].option(true, true).unwrap(), "always");
        assert_eq!(open[0].option(false, false).unwrap(), "reject");
        // The same again (a replay) is nothing new.
        assert_eq!(r.handle(&started("c1", "0.ab", "2026-10-15T12:00:05Z"), now), None);
        // Settled anywhere: closed, and a replay doesn't open it again.
        assert_eq!(r.handle(&done("c1", "0.ab"), now), Some(Change::Closed(vec!["0.ab".into()])));
        assert!(r.open().is_empty());
        assert_eq!(r.handle(&started("c1", "0.ab", "2026-10-15T12:00:05Z"), now), None);
    }

    #[test]
    fn stale_and_ended_requests_go() {
        let r = Requests::default();
        let now = iso_ms("2026-10-15T12:10:00Z").unwrap();
        // Replayed after its timeout: never opened.
        assert_eq!(r.handle(&started("c1", "old", "2026-10-15T12:00:00Z"), now), None);
        // Two in c2, one in c3; c2's turn ends.
        r.handle(&started("c2", "a", "2026-10-15T12:09:00Z"), now);
        r.handle(&started("c2", "b", "2026-10-15T12:09:30Z"), now);
        r.handle(&started("c3", "c", "2026-10-15T12:09:40Z"), now);
        assert_eq!(r.open().iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"], "newest first");
        let end = ev("stage", "c2", json!({ "stage": "turn", "state": "interrupted", "turn_id": "t1" }));
        let Some(Change::Closed(mut gone)) = r.handle(&end, now) else { panic!() };
        gone.sort();
        assert_eq!(gone, ["a", "b"]);
        // Past its timeout and the grace: dropped.
        r.prune(iso_ms("2026-10-15T12:15:00Z").unwrap());
        assert_eq!(r.open().len(), 1);
        r.prune(iso_ms("2026-10-15T12:15:11Z").unwrap());
        assert!(r.open().is_empty());
    }

    #[test]
    fn holders() {
        let r = Requests::default();
        let mut rx = r.subscribe();
        r.hold(7);
        r.hold(3);
        assert_eq!(r.holder(), Some(3));
        r.release(3);
        assert_eq!(r.holder(), Some(7));
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();
        r.release(9);
        assert!(!rx.has_changed().unwrap());
    }
}
