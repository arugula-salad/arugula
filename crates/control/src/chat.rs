//! Chat held by control, in plain text (M122, from S41): a team's channels
//! and DMs, and the threads about a pane or a session, which name their
//! machine. Control numbers messages, keeps read marks, resolves mentions,
//! pushes, searches, and tells open pages over one Server-Sent Events
//! stream each. No machine needs to be up for any of it.
//!
//! **Who sees a room** ([`sight`]):
//! - a channel: everyone in its team's latest roster (only owners while the
//!   team is locked);
//! - a DM: its two people, while each is in the team. One who left doesn't
//!   see it; the other still reads it but can't write (decision 10);
//! - the thread on a pane or session: whom its machine last said may read
//!   it ([`daemon_readers`], decision 2), each from their "from now" floor,
//!   and only while control would route them to the machine at all
//!   (`teams::may_reach`). Before the machine has said, its owner and, for a
//!   team's machine, the team's owners. The machine's owner always reads.
//!
//! Anyone else gets 404, as if the room weren't there.
//!
//! **What a message holds:** text, and optionally a pane it's about
//! (machine and pane id) with the lines the poster chose to quote from it
//! (decision 1). The pane stays on its machine; a reader who may not reach
//! the machine sees the quote and a chip that doesn't open.
//!
//! **Gating:** control's `chat` Labs flag ([`chat_on`]): the account's own
//! if it set one, else on when any of its teams turned it on.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_control_wire::chat::{
    self as wire, ChatEdit, ChatFound, ChatHit, ChatKind, ChatLive, ChatMessage, ChatMessages, ChatMessagesQuery,
    ChatMute, ChatNewChannel, ChatNewDm, ChatOpen, ChatOpenTarget, ChatPane, ChatPerson, ChatPost, ChatPosted,
    ChatPushWhy, ChatPushed, ChatRead, ChatReaders, ChatReadersStored, ChatRoom, ChatRoomChange, ChatRoomDetail,
    ChatRooms, ChatSearchQuery,
};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use serde_json::json;
use tokio::sync::broadcast;

use crate::{
    ApiError, App,
    auth::{DaemonAuth, Session},
    db::chat::{Message, NewMessage, Page, Room, Search},
    err, labs_flags, refusal, teams,
};

type J<T> = Result<Json<T>, ApiError>;

/// The Labs flag chat is behind.
pub const FLAG: &str = "chat";
/// The reason a refusal gives when chat isn't on for the account.
pub const CHAT_OFF: &str = "chat_off";
/// At most this many channels per team, rooms per machine, and bytes of
/// text and quotes per team (or machine).
const MAX_CHANNELS: u64 = 200;
const MAX_THREADS: u64 = 10_000;
const MAX_SCOPE_BYTES: u64 = 1 << 30;
/// A page of messages, and the most one asks for.
const PAGE: u32 = 50;
const MAX_PAGE: u32 = 200;
const SEARCH_HITS: u32 = 50;
/// Live streams: open at once, per account and in all; the events kept
/// for resuming; how often each re-reads what it may see anyway.
const STREAMS_PER_ACCOUNT: usize = 20;
const STREAMS: usize = 5_000;
const RING: usize = 1_024;
const REFRESH: Duration = Duration::from_secs(60);
/// At most one push per room per person in this long.
const PUSH_GAP_MS: u64 = 60_000;

// ---------------------------------------------------------------- who

/// Whether chat is on for an account: its own flag if it set one, else on
/// when any team it's in turned it on.
pub fn chat_on(app: &App, account: &str) -> anyhow::Result<bool> {
    if let Some(on) = app.db.labs_flags(&labs_flags::account_owner(account))?.get(FLAG) {
        return Ok(*on);
    }
    for body in app.db.teams_of(account)? {
        let team = teams::parse(&body)?.team;
        if app.db.labs_flags(&labs_flags::team_owner(&team))?.get(FLAG) == Some(&true) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn gate(app: &App, account: &str) -> Result<(), ApiError> {
    if chat_on(app, account)? {
        Ok(())
    } else {
        Err(refusal(StatusCode::FORBIDDEN, CHAT_OFF, "chat isn't on for you: it's a Labs feature (the chat flag)"))
    }
}

fn team_scope(team: &str) -> String {
    format!("team:{team}")
}

fn machine_scope(machine: &str) -> String {
    format!("machine:{machine}")
}

/// An account's role in a team, if it's in it now: a locked team's members
/// other than owners are as good as out.
fn team_role(app: &App, team: &str, account: &str) -> anyhow::Result<Option<TeamRole>> {
    let Some(body) = app.db.latest_roster(team)? else { return Ok(None) };
    let role = teams::role_in(&teams::parse(&body)?, account);
    let locked = app.db.team(team)?.is_some_and(|t| t.locked);
    Ok(role.filter(|r| !locked || *r == TeamRole::Owner))
}

/// Whether `account` reads the thread on `machine`'s pane or session, and
/// from when (ms): see the top of this file.
fn target_sight(app: &App, account: &str, machine: &str, kind: &str, target: &str) -> anyhow::Result<Option<u64>> {
    let Some(owner) = app.db.daemon_account(machine)? else { return Ok(None) };
    if owner == account {
        return Ok(Some(0));
    }
    if !teams::may_reach(app, account, machine)? {
        return Ok(None);
    }
    Ok(match app.db.chat_readers(machine, kind, target)? {
        Some(list) => list.iter().find(|r| r.account == account).map(|r| r.from.unwrap_or(0)),
        // Before the machine has said: its team's owners.
        None => match app.db.daemon_team(machine)? {
            Some(team) if team_role(app, &team, account)? == Some(TeamRole::Owner) => Some(0),
            _ => None,
        },
    })
}

/// Whether `account` sees `room`, and from when (ms; 0 is everything).
pub fn sight(app: &App, account: &str, room: &Room) -> anyhow::Result<Option<u64>> {
    let team = room.scope.strip_prefix("team:");
    Ok(match (room.kind.as_str(), team) {
        ("channel", Some(team)) => team_role(app, team, account)?.map(|_| 0),
        ("dm", Some(team)) => {
            let member = app.db.chat_members(&room.id)?.iter().any(|a| a == account);
            (member && team_role(app, team, account)?.is_some()).then_some(0)
        }
        ("pane" | "session", _) => match (&room.machine, &room.target) {
            (Some(m), Some(t)) => target_sight(app, account, m, &room.kind, t)?,
            _ => None,
        },
        ("archive", _) => app.db.chat_members(&room.id)?.iter().any(|a| a == account).then_some(0),
        _ => None,
    })
}

/// Whether someone who sees `room` may post in it now.
fn writable(app: &App, room: &Room) -> anyhow::Result<bool> {
    if room.archived.is_some() {
        return Ok(false);
    }
    Ok(match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel" | "pane" | "session", _) => true,
        // Both still in the team: one who left ends it for the other.
        ("dm", Some(team)) => {
            let mut all = true;
            for a in app.db.chat_members(&room.id)? {
                all &= team_role(app, team, &a)?.is_some();
            }
            all
        }
        _ => false,
    })
}

/// The rooms an account sees, with each one's floor.
fn rooms_seen(app: &App, account: &str) -> anyhow::Result<Vec<(Room, u64)>> {
    let mut scopes = Vec::new();
    for body in app.db.teams_of(account)? {
        scopes.push(team_scope(&teams::parse(&body)?.team));
    }
    let mut machines: Vec<String> = app.db.daemons(account)?.into_iter().map(|d| d.id).collect();
    machines.extend(teams::reachable(app, account)?);
    let mut rooms = app.db.chat_rooms_in(&scopes, &machines)?;
    rooms.extend(app.db.chat_rooms_member(account)?);
    let mut ids = HashSet::new();
    let mut out = Vec::new();
    for r in rooms {
        if ids.insert(r.id.clone())
            && let Some(from) = sight(app, account, &r)?
        {
            out.push((r, from));
        }
    }
    out.sort_by(|a, b| (a.0.created, &a.0.id).cmp(&(b.0.created, &b.0.id)));
    Ok(out)
}

/// Everyone who sees a room (and has chat on): whom a mention can reach.
fn audience(app: &App, room: &Room) -> anyhow::Result<Vec<String>> {
    let mut maybe: Vec<String> = match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel", Some(team)) => match app.db.latest_roster(team)? {
            Some(body) => teams::parse(&body)?.members.into_iter().map(|m| m.account).collect(),
            None => vec![],
        },
        ("pane" | "session", _) => {
            let m = room.machine.as_deref().unwrap_or_default();
            let mut v: Vec<String> = app.db.daemon_account(m)?.into_iter().collect();
            if let Some(team) = app.db.daemon_team(m)?
                && let Some(body) = app.db.latest_roster(&team)?
            {
                v.extend(teams::parse(&body)?.members.into_iter().map(|x| x.account));
            }
            if let Some(list) = app.db.chat_readers(m, &room.kind, room.target.as_deref().unwrap_or_default())? {
                v.extend(list.into_iter().map(|r| r.account));
            }
            v
        }
        _ => app.db.chat_members(&room.id)?,
    };
    maybe.sort();
    maybe.dedup();
    let mut out = Vec::new();
    for a in maybe {
        if sight(app, &a, room)?.is_some() && chat_on(app, &a)? {
            out.push(a);
        }
    }
    Ok(out)
}

/// The room `id`, if `account` sees it (404 otherwise, as if it weren't
/// there), and its floor for them.
fn room_for(app: &App, account: &str, id: &str) -> Result<(Room, u64), ApiError> {
    let gone = || err(StatusCode::NOT_FOUND, "no such room");
    let room = app.db.chat_room(id)?.ok_or_else(gone)?;
    let from = sight(app, account, &room)?.ok_or_else(gone)?;
    Ok((room, from))
}

fn name_of(app: &App, account: &str) -> String {
    app.db.account(account).ok().flatten().map(|a| if a.name.is_empty() { a.login } else { a.name }).unwrap_or_default()
}

fn kind_of(kind: &str) -> ChatKind {
    match kind {
        "channel" => ChatKind::Channel,
        "dm" => ChatKind::Dm,
        "pane" => ChatKind::Pane,
        "session" => ChatKind::Session,
        _ => ChatKind::Archive,
    }
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", hex::encode(arugula_e2e::random::<8>()))
}

/// A room as `account` sees it.
fn listed(app: &App, account: &str, room: &Room, from: u64) -> anyhow::Result<ChatRoom> {
    let (read, muted) = app.db.chat_mark(&room.id, account)?;
    let (unread, mentions) = app.db.chat_unread(&room.id, account, read, from)?;
    let with = if room.kind == "dm" {
        app.db
            .chat_members(&room.id)?
            .into_iter()
            .find(|a| a != account)
            .map(|a| ChatPerson { name: name_of(app, &a), account: a })
    } else {
        None
    };
    let machine_name = match &room.machine {
        Some(m) => app.db.daemon_row(m)?.map(|(_, d)| d.name),
        None => None,
    };
    Ok(ChatRoom {
        id: room.id.clone(),
        kind: kind_of(&room.kind),
        team: room.scope.strip_prefix("team:").map(str::to_owned),
        name: room.name.clone(),
        machine: room.machine.clone(),
        machine_name,
        target: room.target.clone(),
        with,
        last: room.last_seq,
        read,
        unread,
        mentions,
        muted,
        archived: room.archived.is_some(),
        writable: writable(app, room)?,
    })
}

/// Names and pane chips, looked up once per answer.
struct Shower<'a> {
    app: &'a App,
    reader: &'a str,
    names: HashMap<String, String>,
    machines: HashMap<String, (String, bool)>,
}

impl<'a> Shower<'a> {
    fn new(app: &'a App, reader: &'a str) -> Self {
        Self { app, reader, names: HashMap::new(), machines: HashMap::new() }
    }

    fn name(&mut self, account: &str) -> String {
        if account.is_empty() {
            return String::new();
        }
        if let Some(n) = self.names.get(account) {
            return n.clone();
        }
        let n = name_of(self.app, account);
        self.names.insert(account.to_owned(), n.clone());
        n
    }

    fn show(&mut self, m: &Message) -> ChatMessage {
        let pane = match (&m.ref_machine, &m.ref_pane) {
            (Some(machine), Some(pane)) => {
                let (machine_name, opens) = match self.machines.get(machine) {
                    Some(x) => x.clone(),
                    None => {
                        let name =
                            self.app.db.daemon_row(machine).ok().flatten().map(|(_, d)| d.name).unwrap_or_default();
                        let opens = teams::may_reach(self.app, self.reader, machine).unwrap_or(false);
                        self.machines.insert(machine.clone(), (name.clone(), opens));
                        (name, opens)
                    }
                };
                Some(ChatPane { machine: machine.clone(), machine_name, pane: pane.clone(), opens })
            }
            _ => None,
        };
        ChatMessage {
            seq: m.seq,
            at: m.at,
            author: m.author.clone(),
            name: self.name(&m.author),
            text: m.text.clone(),
            quote: m.quote.clone(),
            pane,
            mentions: m.mentions.clone(),
            edited: m.edited,
            deleted: m.deleted,
            imported: m.imported.clone(),
        }
    }
}

// ---------------------------------------------------------------- mentions

/// The `@words` in a text, lowercased: an `@` at the start or after
/// anything but a letter or digit (not an email address), then letters,
/// digits, `_`, `-` and `.` (not a trailing one).
fn at_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        if c == '@' && !prev.is_some_and(char::is_alphanumeric) {
            let rest = &text[i + 1..];
            let end = rest
                .char_indices()
                .find(|(_, ch)| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')))
                .map_or(rest.len(), |(j, _)| j);
            let w = rest[..end].trim_end_matches(['.', '-']).to_lowercase();
            if !w.is_empty() && !out.contains(&w) {
                out.push(w);
            }
        }
        prev = Some(c);
    }
    out
}

/// Whom a text mentions among `audience` (by login, name, or the one word
/// a roster calls them), leaving out the author, and the `@words` that
/// named nobody there.
fn mentions(app: &App, text: &str, audience: &[String], author: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let words = at_words(text);
    if words.is_empty() {
        return Ok((vec![], vec![]));
    }
    let mut hit: HashSet<&str> = HashSet::new();
    let mut named = Vec::new();
    for a in audience {
        let Some(acc) = app.db.account(a)? else { continue };
        let names = [acc.login.to_lowercase(), acc.name.to_lowercase(), teams::member_name(&acc).to_lowercase()];
        let mut found = false;
        for w in &words {
            if names.iter().any(|n| !n.is_empty() && n == w) {
                hit.insert(w);
                found = true;
            }
        }
        if found && a != author {
            named.push(a.clone());
        }
    }
    let unreached = words.iter().filter(|w| !hit.contains(w.as_str())).cloned().collect();
    Ok((named, unreached))
}

// ---------------------------------------------------------------- live

/// One event for open pages, numbered; `to` limits it to one account (its
/// read marks).
pub struct Out {
    n: u64,
    ev: ChatLive,
    to: Option<String>,
}

/// What's open, in memory: the live streams, the last [`RING`] events for
/// resuming, and when each person was last pushed for each room.
pub struct Hub {
    /// Event ids are `<epoch>-<n>`: a restart starts a new epoch, and a
    /// page resuming an old one is told to resync.
    epoch: String,
    next: AtomicU64,
    tx: broadcast::Sender<Arc<Out>>,
    ring: Mutex<VecDeque<Arc<Out>>>,
    streams: Mutex<HashMap<String, StreamInfo>>,
    /// Moves when who sees what may have changed (a roster, a reader list,
    /// a flag): each stream re-reads its rooms.
    generation: AtomicU64,
    pushed: Mutex<HashMap<(String, String), u64>>,
}

struct StreamInfo {
    account: String,
    open: Option<String>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            epoch: format!("{:x}", now_ms()),
            next: AtomicU64::new(0),
            tx: broadcast::channel(RING).0,
            ring: Mutex::new(VecDeque::new()),
            streams: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            pushed: Mutex::new(HashMap::new()),
        }
    }
}

impl Hub {
    fn publish(&self, ev: ChatLive, to: Option<String>) {
        // Numbered and sent under the ring's lock, so the ring, the numbers
        // and the channel agree on the order.
        let mut ring = self.ring.lock().unwrap();
        let n = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let out = Arc::new(Out { n, ev, to });
        ring.push_back(out.clone());
        while ring.len() > RING {
            ring.pop_front();
        }
        let _ = self.tx.send(out);
    }

    /// Who sees what may have changed: streams re-read their rooms.
    pub fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// A room appeared, changed or went.
    pub fn room(&self, room: &str) {
        self.changed();
        self.publish(ChatLive::Room { room: room.to_owned() }, None);
    }

    fn id(&self, n: u64) -> String {
        format!("{}-{n}", self.epoch)
    }

    fn has_open(&self, account: &str, room: &str) -> bool {
        self.streams.lock().unwrap().values().any(|s| s.account == account && s.open.as_deref() == Some(room))
    }

    /// Whether `account` may be pushed for `room` now (and note it).
    fn may_push(&self, account: &str, room: &str, now: u64) -> bool {
        let mut p = self.pushed.lock().unwrap();
        if p.len() > 50_000 {
            p.retain(|_, at| now.saturating_sub(*at) < PUSH_GAP_MS);
        }
        let key = (account.to_owned(), room.to_owned());
        if p.get(&key).is_some_and(|at| now.saturating_sub(*at) < PUSH_GAP_MS) {
            return false;
        }
        p.insert(key, now);
        true
    }
}

/// Takes a stream out of the hub's list when it ends.
struct Registered {
    app: Arc<App>,
    id: String,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.app.chat.streams.lock().unwrap().remove(&self.id);
    }
}

/// One page's stream.
struct Live {
    reg: Registered,
    account: String,
    rx: broadcast::Receiver<Arc<Out>>,
    pending: VecDeque<Event>,
    /// The last event number handled.
    seen: u64,
    rooms: HashSet<String>,
    generation: u64,
    read_at: Instant,
}

fn data(ev: &ChatLive) -> String {
    serde_json::to_string(ev).unwrap_or_default()
}

impl Live {
    fn app(&self) -> &App {
        &self.reg.app
    }

    fn refresh(&mut self) {
        let app = self.reg.app.clone();
        let generation = app.chat.generation.load(Ordering::SeqCst);
        let rooms = match chat_on(&app, &self.account) {
            Ok(true) => rooms_seen(&app, &self.account).map(|v| v.into_iter().map(|(r, _)| r.id).collect()),
            Ok(false) => Ok(HashSet::new()),
            Err(e) => Err(e),
        };
        match rooms {
            Ok(r) => self.rooms = r,
            Err(e) => tracing::warn!(error = %e, "chat: re-reading a stream's rooms"),
        }
        self.generation = generation;
        self.read_at = Instant::now();
    }

    /// The event to send for `out`, if this stream gets it.
    fn deliver(&mut self, out: &Out) -> Option<Event> {
        if out.n <= self.seen {
            return None;
        }
        self.seen = out.n;
        if out.to.as_ref().is_some_and(|to| *to != self.account) {
            return None;
        }
        if self.app().chat.generation.load(Ordering::SeqCst) != self.generation || self.read_at.elapsed() > REFRESH {
            self.refresh();
        }
        let ok = match &out.ev {
            ChatLive::Room { room } => {
                let was = self.rooms.contains(room);
                self.refresh();
                was || self.rooms.contains(room)
            }
            ChatLive::Message { room, .. } | ChatLive::Changed { room, .. } | ChatLive::Read { room, .. } => {
                self.rooms.contains(room)
            }
            ChatLive::Hello { .. } | ChatLive::Resync {} => true,
        };
        ok.then(|| Event::default().id(self.app().chat.id(out.n)).data(data(&out.ev)))
    }

    fn resync(&mut self) -> Event {
        self.seen = self.app().chat.next.load(Ordering::SeqCst);
        self.refresh();
        Event::default().id(self.app().chat.id(self.seen)).data(data(&ChatLive::Resync {}))
    }
}

/// `GET /api/chat/live`: Server-Sent Events, one JSON [`ChatLive`] each,
/// for the rooms the person sees. `Last-Event-ID` resumes after a drop; a
/// keep-alive comment every 25 s holds it open through proxies.
pub async fn live(
    State(app): State<Arc<App>>,
    s: Session,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    gate(&app, &s.account)?;
    let id = hex::encode(arugula_e2e::random::<12>());
    {
        let mut streams = app.chat.streams.lock().unwrap();
        if streams.len() >= STREAMS {
            return Err(err(StatusCode::SERVICE_UNAVAILABLE, "too many open pages here right now; try again soon"));
        }
        if streams.values().filter(|x| x.account == s.account).count() >= STREAMS_PER_ACCOUNT {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many open chat pages; close some"));
        }
        streams.insert(id.clone(), StreamInfo { account: s.account.clone(), open: None });
    }
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once('-'))
        .and_then(|(e, n)| Some((e.to_owned(), n.parse::<u64>().ok()?)));
    // Subscribe first, then look at the ring: whatever is published in
    // between is in both, and `seen` drops the second copy. A new stream
    // starts at what was published before it subscribed.
    let start = app.chat.next.load(Ordering::SeqCst);
    let rx = app.chat.tx.subscribe();
    let mut live = Live {
        reg: Registered { app: app.clone(), id: id.clone() },
        account: s.account,
        rx,
        pending: VecDeque::new(),
        seen: 0,
        rooms: HashSet::new(),
        generation: 0,
        read_at: Instant::now(),
    };
    live.refresh();
    live.pending.push_back(Event::default().data(data(&ChatLive::Hello { stream: id })));
    let replay: Option<Vec<Arc<Out>>> = {
        let ring = app.chat.ring.lock().unwrap();
        let next = app.chat.next.load(Ordering::SeqCst);
        match &last {
            Some((epoch, n)) if *epoch == app.chat.epoch && *n <= next => {
                let whole = match ring.front() {
                    Some(f) => f.n <= n + 1,
                    None => *n == next,
                };
                whole.then(|| ring.iter().filter(|o| o.n > *n).cloned().collect())
            }
            _ => None,
        }
    };
    match (last, replay) {
        (Some((_, n)), Some(outs)) => {
            live.seen = n;
            for o in outs {
                if let Some(ev) = live.deliver(&o) {
                    live.pending.push_back(ev);
                }
            }
        }
        (Some(_), None) => {
            let ev = live.resync();
            live.pending.push_back(ev);
        }
        (None, _) => live.seen = start,
    }
    let stream = futures_util::stream::unfold(live, |mut live| async move {
        loop {
            if let Some(ev) = live.pending.pop_front() {
                return Some((Ok(ev), live));
            }
            match live.rx.recv().await {
                Ok(out) => {
                    if let Some(ev) = live.deliver(&out) {
                        return Some((Ok(ev), live));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let ev = live.resync();
                    return Some((Ok(ev), live));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25))))
}

/// `POST /api/chat/live/open`: the room a stream's page has open.
pub async fn open(State(app): State<Arc<App>>, s: Session, Json(b): Json<ChatOpen>) -> J<wire::ChatOpen> {
    let mut streams = app.chat.streams.lock().unwrap();
    let st = streams
        .get_mut(&b.stream)
        .filter(|x| x.account == s.account)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such stream"))?;
    st.open = b.room.clone();
    Ok(Json(b))
}

// ---------------------------------------------------------------- rooms

/// `GET /api/chat`: every room I see, with unread and mention counts.
pub async fn rooms(State(app): State<Arc<App>>, s: Session) -> J<ChatRooms> {
    gate(&app, &s.account)?;
    let mut rooms = Vec::new();
    for (r, from) in rooms_seen(&app, &s.account)? {
        rooms.push(listed(&app, &s.account, &r, from)?);
    }
    Ok(Json(ChatRooms { rooms }))
}

/// A channel's name: one lowercase word of letters, digits, `-` and `_`.
fn channel_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim().trim_start_matches('#').to_lowercase();
    let ok =
        (1..=80).contains(&name.chars().count()) && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(name)
    } else {
        Err(err(StatusCode::BAD_REQUEST, "a channel's name is one word: letters, digits, - and _"))
    }
}

/// `POST /api/chat/teams/{team}/channels`: owners and editors make them;
/// viewers read and post.
pub async fn new_channel(
    State(app): State<Arc<App>>,
    s: Session,
    Path(team): Path<String>,
    Json(b): Json<ChatNewChannel>,
) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    match team_role(&app, &team, &s.account)? {
        None => return Err(err(StatusCode::NOT_FOUND, "no such team")),
        Some(TeamRole::Viewer) => {
            return Err(err(StatusCode::FORBIDDEN, "a team's owners and editors make its channels"));
        }
        _ => {}
    }
    let name = channel_name(&b.name)?;
    let scope = team_scope(&team);
    if app.db.chat_channels(&scope)? >= MAX_CHANNELS {
        return Err(err(StatusCode::CONFLICT, "a team has at most 200 channels; archive or reuse one"));
    }
    app.limits.check_account(crate::limit::CHAT_ROOMS, &s.account)?;
    let room = Room {
        id: new_id("ch_"),
        scope,
        kind: "channel".into(),
        name,
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &[]).map_err(|e| {
        if e.is::<crate::db::Taken>() { err(StatusCode::CONFLICT, "there's a channel by that name") } else { e.into() }
    })?;
    app.chat.room(&room.id);
    app.analytics.capture_team(&s.account, &team, "chat channel made", json!({}));
    Ok(Json(listed(&app, &s.account, &room, 0)?))
}

/// `POST /api/chat/teams/{team}/dms`: the DM with a teammate (made the
/// first time).
pub async fn new_dm(
    State(app): State<Arc<App>>,
    s: Session,
    Path(team): Path<String>,
    Json(b): Json<ChatNewDm>,
) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    if b.with == s.account
        || team_role(&app, &team, &s.account)?.is_none()
        || team_role(&app, &team, &b.with)?.is_none()
    {
        return Err(err(StatusCode::NOT_FOUND, "nobody by that account in this team"));
    }
    let scope = team_scope(&team);
    if let Some(r) = app.db.chat_dm(&scope, &s.account, &b.with)? {
        return Ok(Json(listed(&app, &s.account, &r, 0)?));
    }
    app.limits.check_account(crate::limit::CHAT_ROOMS, &s.account)?;
    let room = Room {
        id: new_id("dm_"),
        scope,
        kind: "dm".into(),
        name: String::new(),
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &[&s.account, &b.with])?;
    app.chat.room(&room.id);
    Ok(Json(listed(&app, &s.account, &room, 0)?))
}

/// A pane's or session's id as a daemon names it: short, no spaces.
fn valid_target(t: &str) -> bool {
    (1..=64).contains(&t.len()) && t.chars().all(|c| c.is_ascii_graphic())
}

/// `POST /api/chat/targets`: the thread on a pane or session (made the
/// first time), for someone who reads it.
pub async fn open_target(State(app): State<Arc<App>>, s: Session, Json(b): Json<ChatOpenTarget>) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    let kind = match b.kind {
        wire::ChatTargetKind::Pane => "pane",
        wire::ChatTargetKind::Session => "session",
    };
    let nothing = || err(StatusCode::NOT_FOUND, "no such pane or session");
    if !valid_target(&b.target) {
        return Err(nothing());
    }
    let from = target_sight(&app, &s.account, &b.machine, kind, &b.target)?.ok_or_else(nothing)?;
    if let Some(r) = app.db.chat_room_for(kind, &b.machine, &b.target)? {
        return Ok(Json(listed(&app, &s.account, &r, from)?));
    }
    let scope = machine_scope(&b.machine);
    if app.db.chat_rooms_count(&scope)? >= MAX_THREADS {
        return Err(err(StatusCode::CONFLICT, "this machine has too many threads"));
    }
    app.limits.check_account(crate::limit::CHAT_ROOMS, &s.account)?;
    let room = Room {
        id: new_id("th_"),
        scope,
        kind: kind.into(),
        name: format!("{kind} {}", b.target),
        machine: Some(b.machine.clone()),
        target: Some(b.target.clone()),
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    match app.db.add_chat_room(&room, &[]) {
        Ok(()) => app.chat.room(&room.id),
        // Someone made it just now.
        Err(e) if e.is::<crate::db::Taken>() => {
            let r = app.db.chat_room_for(kind, &b.machine, &b.target)?.ok_or_else(nothing)?;
            return Ok(Json(listed(&app, &s.account, &r, from)?));
        }
        Err(e) => return Err(e.into()),
    }
    Ok(Json(listed(&app, &s.account, &room, from)?))
}

/// `GET /api/chat/rooms/{id}`: the room, and who sees it.
pub async fn room(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>) -> J<ChatRoomDetail> {
    gate(&app, &s.account)?;
    let (room, from) = room_for(&app, &s.account, &id)?;
    let people =
        audience(&app, &room)?.into_iter().map(|a| ChatPerson { name: name_of(&app, &a), account: a }).collect();
    Ok(Json(ChatRoomDetail { room: listed(&app, &s.account, &room, from)?, people }))
}

/// `PATCH /api/chat/rooms/{id}`: a channel renamed or archived, by its
/// team's owners.
pub async fn change_room(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Json(b): Json<ChatRoomChange>,
) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    let (room, from) = room_for(&app, &s.account, &id)?;
    let team = room.scope.strip_prefix("team:").filter(|_| room.kind == "channel");
    let Some(team) = team else { return Err(err(StatusCode::BAD_REQUEST, "only a channel is renamed or archived")) };
    if team_role(&app, team, &s.account)? != Some(TeamRole::Owner) {
        return Err(err(StatusCode::FORBIDDEN, "a team's owners rename and archive its channels"));
    }
    if let Some(name) = &b.name {
        let name = channel_name(name)?;
        app.db.rename_chat_room(&room.id, &name).map_err(|e| {
            if e.is::<crate::db::Taken>() {
                err(StatusCode::CONFLICT, "there's a channel by that name")
            } else {
                e.into()
            }
        })?;
    }
    if let Some(archived) = b.archived {
        app.db.archive_chat_room(&room.id, archived.then(now_ms))?;
    }
    app.chat.room(&room.id);
    let room = app.db.chat_room(&room.id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such room"))?;
    Ok(Json(listed(&app, &s.account, &room, from)?))
}

/// `POST /api/chat/rooms/{id}/mute`.
pub async fn mute(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Json(b): Json<ChatMute>,
) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    let (room, from) = room_for(&app, &s.account, &id)?;
    app.db.set_chat_muted(&room.id, &s.account, b.muted)?;
    Ok(Json(listed(&app, &s.account, &room, from)?))
}

// ---------------------------------------------------------------- messages

/// `GET /api/chat/rooms/{id}/messages?after=N|before=N&limit=`: a page,
/// oldest first, as the reader sees it.
pub async fn messages(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Query(q): Query<ChatMessagesQuery>,
) -> J<ChatMessages> {
    gate(&app, &s.account)?;
    let (room, from) = room_for(&app, &s.account, &id)?;
    let page = match (q.after, q.before) {
        (Some(a), _) => Page::After(a),
        (None, Some(b)) => Page::Before(b),
        (None, None) => Page::Newest,
    };
    let (ms, more) = app.db.chat_messages(&room.id, page, q.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE))?;
    let mut shower = Shower::new(&app, &s.account);
    let messages = ms.iter().filter(|m| m.at >= from).map(|m| shower.show(m)).collect();
    Ok(Json(ChatMessages { room: listed(&app, &s.account, &room, from)?, messages, more }))
}

fn sizes(text: &str, quote: Option<&str>) -> Result<(), ApiError> {
    if text.len() > wire::MAX_TEXT || quote.is_some_and(|q| q.len() > wire::MAX_QUOTE) {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a message is at most 8 KB, and a quote 16 KB"));
    }
    Ok(())
}

/// `POST /api/chat/rooms/{id}/messages`: post. Control numbers it, tells
/// open pages, and pushes whom it mentions (and, in a DM, the other
/// person).
pub async fn post(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Json(b): Json<ChatPost>,
) -> J<ChatPosted> {
    gate(&app, &s.account)?;
    let (room, _) = room_for(&app, &s.account, &id)?;
    if !writable(&app, &room)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let text = b.text.trim();
    let quote = b.quote.as_deref().filter(|q| !q.trim().is_empty());
    if text.is_empty() && quote.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "say something"));
    }
    sizes(text, quote)?;
    // You name only panes you may reach.
    if let Some(p) = &b.pane
        && (!valid_target(&p.pane) || !teams::may_reach(&app, &s.account, &p.machine)?)
    {
        return Err(err(StatusCode::FORBIDDEN, "you can't see that pane"));
    }
    if app.db.chat_bytes(&room.scope)? + (text.len() + quote.map_or(0, str::len)) as u64 > MAX_SCOPE_BYTES {
        return Err(err(StatusCode::INSUFFICIENT_STORAGE, "this team's chat is full (1 GB): delete some first"));
    }
    app.limits.check_account_per_minute(crate::limit::CHAT_POSTS, &s.account)?;
    app.limits.check_all_per_minute(crate::limit::ALL_CHAT_POSTS)?;
    let people = audience(&app, &room)?;
    let (named, unreached) = mentions(&app, text, &people, &s.account)?;
    let at = now_ms();
    let seq = app.db.add_chat_message(
        &room.id,
        &NewMessage {
            author: &s.account,
            at,
            text,
            quote,
            ref_machine: b.pane.as_ref().map(|p| p.machine.as_str()),
            ref_pane: b.pane.as_ref().map(|p| p.pane.as_str()),
            mentions: &named,
        },
    )?;
    app.chat.publish(ChatLive::Message { room: room.id.clone(), seq }, None);
    let pushed = push(
        &app,
        &room,
        &s.account,
        seq,
        text.is_empty().then_some(quote).flatten().unwrap_or(text),
        &named,
        &people,
    )?;
    Ok(Json(ChatPosted { seq, mentions: named, unreached, pushed }))
}

/// Push a new message to whom it's for (S41 q4): mentions, and the other
/// person in a DM. Not the author, nobody with the room open or muted, and
/// nobody twice a minute for one room.
fn push(
    app: &Arc<App>,
    room: &Room,
    author: &str,
    seq: i64,
    text: &str,
    named: &[String],
    people: &[String],
) -> anyhow::Result<Vec<ChatPushed>> {
    let mut to: Vec<(String, ChatPushWhy)> = named.iter().map(|a| (a.clone(), ChatPushWhy::Mention)).collect();
    if room.kind == "dm" {
        for a in people {
            if a != author && !named.contains(a) {
                to.push((a.clone(), ChatPushWhy::Dm));
            }
        }
    }
    if to.is_empty() {
        return Ok(vec![]);
    }
    let me = name_of(app, author);
    let place = match room.kind.as_str() {
        "channel" => format!("#{}", room.name),
        _ => {
            let machine =
                room.machine.as_deref().and_then(|m| app.db.daemon_row(m).ok().flatten()).map(|(_, d)| d.name);
            match machine {
                Some(m) => format!("{} on {m}", room.name),
                None => room.name.clone(),
            }
        }
    };
    let body: String = text.chars().take(wire::PUSH_CHARS).collect();
    let now = now_ms();
    let mut out = Vec::new();
    for (account, why) in to {
        if account == author
            || app.db.chat_mark(&room.id, &account)?.1
            || app.chat.has_open(&account, &room.id)
            || !app.chat.may_push(&account, &room.id, now)
        {
            continue;
        }
        let title = match why {
            ChatPushWhy::Mention if room.kind == "dm" => format!("{me} mentioned you"),
            ChatPushWhy::Mention => format!("{me} mentioned you in {place}"),
            ChatPushWhy::Dm => format!("{me} wrote to you"),
        };
        let mut more = serde_json::Map::new();
        more.insert("chat".into(), json!({ "room": room.id, "seq": seq }));
        crate::push::notify_with(app, vec![account.clone()], &format!("chat-{}", room.id), title, body.clone(), more);
        out.push(ChatPushed { account, why });
    }
    Ok(out)
}

/// The message `seq` in a room the person sees.
fn message_in(app: &App, room: &Room, seq: i64) -> Result<Message, ApiError> {
    app.db.chat_message(&room.id, seq)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such message"))
}

/// `PATCH /api/chat/rooms/{id}/messages/{seq}`: its author's new text.
/// Whom it mentions is worked out again; nobody is pushed for an edit.
pub async fn edit(
    State(app): State<Arc<App>>,
    s: Session,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<ChatEdit>,
) -> J<ChatMessage> {
    gate(&app, &s.account)?;
    let (room, _) = room_for(&app, &s.account, &id)?;
    let m = message_in(&app, &room, seq)?;
    if m.author != s.account || m.deleted.is_some() {
        return Err(err(StatusCode::FORBIDDEN, "only its author edits a message"));
    }
    if !writable(&app, &room)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let text = b.text.trim();
    if text.is_empty() && m.quote.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "say something, or delete it"));
    }
    sizes(text, None)?;
    app.limits.check_account_per_minute(crate::limit::CHAT_POSTS, &s.account)?;
    let (named, _) = mentions(&app, text, &audience(&app, &room)?, &s.account)?;
    app.db.edit_chat_message(&room.id, seq, text, &named, now_ms())?;
    app.chat.publish(ChatLive::Changed { room: room.id.clone(), seq }, None);
    Ok(Json(Shower::new(&app, &s.account).show(&message_in(&app, &room, seq)?)))
}

/// `DELETE /api/chat/rooms/{id}/messages/{seq}`: by its author, a team's
/// owner (in the team's rooms) or the machine's owner (in its threads).
/// Its number stays, with nothing in it.
pub async fn delete(State(app): State<Arc<App>>, s: Session, Path((id, seq)): Path<(String, i64)>) -> J<ChatMessage> {
    gate(&app, &s.account)?;
    let (room, _) = room_for(&app, &s.account, &id)?;
    let m = message_in(&app, &room, seq)?;
    let may = m.author == s.account
        || match room.scope.split_once(':') {
            Some(("team", team)) => team_role(&app, team, &s.account)? == Some(TeamRole::Owner),
            Some(("machine", machine)) => app.db.daemon_account(machine)?.as_deref() == Some(s.account.as_str()),
            _ => false,
        };
    if !may {
        return Err(err(StatusCode::FORBIDDEN, "only its author, or an owner, deletes a message"));
    }
    if m.deleted.is_none() {
        app.db.delete_chat_message(&room.id, seq, now_ms())?;
        app.chat.publish(ChatLive::Changed { room: room.id.clone(), seq }, None);
    }
    Ok(Json(Shower::new(&app, &s.account).show(&message_in(&app, &room, seq)?)))
}

/// `POST /api/chat/rooms/{id}/read`: read up to `seq` (only forward).
pub async fn read(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Json(b): Json<ChatRead>,
) -> J<ChatRoom> {
    gate(&app, &s.account)?;
    let (room, from) = room_for(&app, &s.account, &id)?;
    let seq = b.seq.min(room.last_seq);
    if app.db.set_chat_read(&room.id, &s.account, seq)? {
        app.chat.publish(ChatLive::Read { room: room.id.clone(), seq }, Some(s.account.clone()));
    }
    Ok(Json(listed(&app, &s.account, &room, from)?))
}

// ---------------------------------------------------------------- search

/// What a search box's words ask for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Asked {
    /// FTS5, each word a phrase (`"mean"`), or a prefix (`"mea"*`), so its
    /// syntax isn't the person's to type.
    fts: Option<String>,
    channel: Option<String>,
    from: Option<String>,
    has_quote: bool,
}

fn parse_search(q: &str) -> Asked {
    let mut a = Asked::default();
    let mut words = Vec::new();
    for tok in q.split_whitespace() {
        if let Some(c) = tok.strip_prefix("in:") {
            a.channel = Some(c.trim_start_matches('#').to_lowercase());
        } else if let Some(f) = tok.strip_prefix("from:") {
            a.from = Some(f.trim_start_matches('@').to_lowercase());
        } else if tok == "has:quote" {
            a.has_quote = true;
        } else {
            let prefix = tok.ends_with('*');
            let w: String = tok.trim_end_matches('*').chars().filter(|c| *c != '"').collect();
            if !w.is_empty() {
                words.push(if prefix { format!("\"{w}\"*") } else { format!("\"{w}\"") });
            }
        }
    }
    if !words.is_empty() {
        a.fts = Some(words.join(" "));
    }
    a
}

/// `GET /api/chat/search?q=&team=`: over the rooms I see (or one team's),
/// newest first.
pub async fn search(State(app): State<Arc<App>>, s: Session, Query(q): Query<ChatSearchQuery>) -> J<ChatFound> {
    gate(&app, &s.account)?;
    let asked = parse_search(&q.q);
    if asked.fts.is_none() && asked.channel.is_none() && asked.from.is_none() && !asked.has_quote {
        return Ok(Json(ChatFound { found: vec![] }));
    }
    app.limits.check_account(crate::limit::CHAT_SEARCHES, &s.account)?;
    let author = match &asked.from {
        Some(who) => match app.db.account_by_login(who)? {
            Some(a) => Some(a.id),
            None => match app.db.accounts_named(who)?.as_slice() {
                [one] => Some(one.clone()),
                _ => return Ok(Json(ChatFound { found: vec![] })),
            },
        },
        None => None,
    };
    let mut floors = HashMap::new();
    for (r, from) in rooms_seen(&app, &s.account)? {
        let in_team = match &q.team {
            Some(t) => {
                r.scope == team_scope(t)
                    || r.machine.as_deref().is_some_and(|m| app.db.daemon_team(m).ok().flatten().as_deref() == Some(t))
            }
            None => true,
        };
        let in_channel = asked.channel.as_ref().is_none_or(|c| r.kind == "channel" && r.name == *c);
        if in_team && in_channel {
            floors.insert(r.id, from);
        }
    }
    let rooms: Vec<String> = floors.keys().cloned().collect();
    let hits = app.db.chat_search(
        &rooms,
        &Search { fts: asked.fts.as_deref(), author: author.as_deref(), has_quote: asked.has_quote },
        SEARCH_HITS,
    )?;
    let mut shower = Shower::new(&app, &s.account);
    let found = hits
        .iter()
        .filter(|m| floors.get(&m.room).is_some_and(|from| m.at >= *from))
        .map(|m| ChatHit { room: m.room.clone(), message: shower.show(m) })
        .collect();
    Ok(Json(ChatFound { found }))
}

// ---------------------------------------------------------------- daemons

/// Whether chat is on for a machine: for its owner, or for its team.
fn machine_chat_on(app: &App, machine: &str, owner: &str) -> anyhow::Result<bool> {
    if chat_on(app, owner)? {
        return Ok(true);
    }
    Ok(match app.db.daemon_team(machine)? {
        Some(t) => app.db.labs_flags(&labs_flags::team_owner(&t))?.get(FLAG) == Some(&true),
        None => false,
    })
}

/// `POST /api/daemon/chat/readers`: a machine says who may read the threads
/// on its panes and sessions (decision 2). Each list it names replaces the
/// last; control still routes nobody it wouldn't anyway (`target_sight`).
pub async fn daemon_readers(
    State(app): State<Arc<App>>,
    d: DaemonAuth,
    Json(b): Json<ChatReaders>,
) -> J<ChatReadersStored> {
    let machine = d.cert.device;
    let owner = app.db.daemon_account(&machine)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?;
    if !machine_chat_on(&app, &machine, &owner)? {
        return Err(refusal(StatusCode::FORBIDDEN, CHAT_OFF, "chat isn't on for this machine's account or team"));
    }
    if b.targets.len() > wire::MAX_READER_TARGETS
        || b.targets.iter().any(|t| t.readers.len() > wire::MAX_READERS || !valid_target(&t.target))
    {
        return Err(err(StatusCode::BAD_REQUEST, "at most 500 threads of 500 readers each, by their ids"));
    }
    app.limits.check_daemon(crate::limit::CHAT_READERS, &machine)?;
    let now = now_ms();
    let mut rooms = Vec::new();
    for t in &b.targets {
        let kind = match t.kind {
            wire::ChatTargetKind::Pane => "pane",
            wire::ChatTargetKind::Session => "session",
        };
        let mut readers: Vec<wire::ChatReader> = Vec::new();
        for r in &t.readers {
            if !r.account.is_empty() && r.account.len() <= 64 && !readers.iter().any(|x| x.account == r.account) {
                readers.push(r.clone());
            }
        }
        app.db.set_chat_readers(&machine, kind, &t.target, &readers, now)?;
        if let Some(r) = app.db.chat_room_for(kind, &machine, &t.target)? {
            rooms.push(r.id);
        }
    }
    app.chat.changed();
    for r in rooms {
        app.chat.room(&r);
    }
    Ok(Json(ChatReadersStored { stored: b.targets.len() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_words_are_names_not_emails() {
        assert_eq!(at_words("@alex the mean test fails, cc @Sam-Stranger."), ["alex", "sam-stranger"]);
        assert_eq!(at_words("mail jake@example.com"), Vec::<String>::new());
        assert_eq!(at_words("(@jhgaylor) and @jhgaylor again, @"), ["jhgaylor"]);
        assert_eq!(at_words("@ünïcode @a.b."), ["ünïcode", "a.b"]);
    }

    #[test]
    fn search_words_are_phrases_and_filters_come_apart() {
        assert_eq!(
            parse_search("mean mea* \"OR\" in:#General from:@Alex has:quote"),
            Asked {
                fts: Some("\"mean\" \"mea\"* \"OR\"".into()),
                channel: Some("general".into()),
                from: Some("alex".into()),
                has_quote: true,
            }
        );
        assert_eq!(parse_search("\"\" *"), Asked::default());
        // FTS5's own syntax is only ever text inside a phrase.
        assert_eq!(parse_search("a NEAR(b) -c").fts.as_deref(), Some("\"a\" \"NEAR(b)\" \"-c\""));
    }

    #[test]
    fn channel_names_are_one_word() {
        assert_eq!(channel_name(" #General ").unwrap(), "general");
        assert_eq!(channel_name("ops_on-call2").unwrap(), "ops_on-call2");
        for bad in ["", "#", "two words", "a/b", &"x".repeat(81)] {
            assert!(channel_name(bad).is_err(), "{bad}");
        }
    }
}
