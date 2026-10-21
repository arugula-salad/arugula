//! Chat held by control, in plain text (M122, from S41): a team's channels
//! and DMs, and the threads about a pane or a session, which name their
//! machine. Control numbers messages, keeps read marks, resolves mentions,
//! pushes, searches, and tells open pages over one Server-Sent Events
//! stream each. No machine needs to be up for any of it.
//!
//! It's the platform's (M138, "Control is two processes: a hub and a
//! platform"): it moved here from control, with its tables
//! ([`crate::db::chat`]), and reaches the hub only through
//! [`HubPort`](crate::HubPort), through the platform's [`Cache`] of its
//! answers: who is in which team, which machine is whose and who may reach
//! it (asked for a page of rooms at once, never room by room), names, Labs
//! flags, and push. A roster, lock, machine, share or flag change reaches
//! who sees what here through the hub's change feed, within
//! [`cache::DELAY`] (a feed tick plus a cache tick).
//!
//! **Who sees a room** ([`sight`]):
//! - a channel: everyone in its team's latest roster (only owners while the
//!   team is locked);
//! - a DM: its two people, while each is in the team. One who left doesn't
//!   see it; the other still reads it but can't write (decision 10);
//! - the thread on a pane or session: whom its machine last said may read
//!   it ([`daemon_readers`], decision 2), each from their "from now" floor,
//!   and only while control would route them to the machine at all
//!   (`HubPort::may_reach`). Before the machine has said, its owner and, for
//!   a team's machine, the team's owners. The machine's owner always reads.
//!
//! - an archive (an old conversation a machine brought in, M124): its
//!   members, each from when they joined it, read-only.
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
//!
//! [`Cache`]: crate::cache::Cache

use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::chat::{
    self as wire, ChatEdit, ChatFound, ChatHit, ChatKind, ChatLive, ChatMessage, ChatMessages, ChatMessagesQuery,
    ChatMute, ChatNewChannel, ChatNewDm, ChatOpen, ChatOpenTarget, ChatPane, ChatPerson, ChatPost, ChatPosted,
    ChatPushWhy, ChatPushed, ChatRead, ChatReaderList, ChatReaders, ChatReadersStored, ChatRoom, ChatRoomChange,
    ChatRoomDetail, ChatRooms, ChatSearchQuery,
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
    Platform,
    auth::{Machine, Person},
    cache,
    db::{
        Taken,
        chat::{Message, NewMessage, OldMessage, Page, Room, Search},
    },
    hub::{Account, Daemon, Flags, PushOptions, Team},
};

type J<T> = Result<Json<T>, ApiError>;

/// The Labs flag chat is behind.
pub const FLAG: &str = "chat";
/// The reason a refusal gives when chat isn't on for the account.
pub const CHAT_OFF: &str = "chat_off";
/// ...and when a viewer posts in a session's or pane's thread.
pub const WATCHING: &str = "watching";
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

/// Chat's rate limits (M122), counted by the platform alone. Posts are
/// counted per minute: one account's, and everyone's together, a brake on
/// the database.
const CHAT_POSTS: (&str, usize) = ("chat-post", 60);
const ALL_CHAT_POSTS: (&str, usize) = ("chat-post-all", 6_000);
/// Channels, DMs and pane threads made, per account, per hour.
const CHAT_ROOMS: (&str, usize) = ("chat-room", 120);
/// Searches, per account, per hour.
const CHAT_SEARCHES: (&str, usize) = ("chat-search", 600);
/// Reader lists a daemon sends, per hour.
const CHAT_READERS: (&str, usize) = ("chat-readers", 600);
/// Imports of a machine's old threads (M124), per hour: a big one comes in parts.
const CHAT_IMPORTS: (&str, usize) = ("chat-imports", 120);

// ---------------------------------------------------------------- who

fn account_owner(account: &str) -> String {
    format!("account:{account}")
}

fn team_owner(team: &str) -> String {
    format!("team:{team}")
}

fn flag_of(flags: &HashMap<String, Flags>, owner: &str) -> Option<bool> {
    flags.get(owner).and_then(|f| f.get(FLAG)).copied()
}

/// Whether chat is on for an account: its own flag if it set one, else on
/// when any team it's in turned it on.
pub async fn chat_on(app: &Platform, account: &str) -> anyhow::Result<bool> {
    let teams = app.cache.teams_of(&*app.hub, account).await?;
    let mut owners = vec![account_owner(account)];
    owners.extend(teams.iter().map(|t| team_owner(t)));
    let flags = app.cache.flags(&*app.hub, &owners).await?;
    if let Some(on) = flag_of(&flags, &account_owner(account)) {
        return Ok(on);
    }
    Ok(teams.iter().any(|t| flag_of(&flags, &team_owner(t)) == Some(true)))
}

async fn gate(app: &Platform, account: &str) -> Result<(), ApiError> {
    if chat_on(app, account).await? {
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

/// What the hub says about the rooms one answer looks at, asked for once
/// per answer: one call each for a page of rooms, never one per room. Their
/// teams, their machines, the accounts of DMs' people, and (for the
/// reader) which of the machines whose threads are there control routes
/// them to.
struct Facts {
    reader: Option<String>,
    teams: HashMap<String, Team>,
    daemons: HashMap<String, Daemon>,
    people: HashMap<String, Account>,
    /// The machines asked about for the reader, and those they may reach.
    asked: HashSet<String>,
    reach: HashSet<String>,
}

impl Facts {
    async fn of(app: &Platform, reader: Option<&str>, rooms: &[&Room]) -> anyhow::Result<Facts> {
        let hub = &*app.hub;
        let mut machines: Vec<String> = rooms.iter().filter_map(|r| r.machine.clone()).collect();
        machines.sort();
        machines.dedup();
        let daemons = app.cache.daemons(hub, &machines).await?;
        let mut teams: Vec<String> =
            rooms.iter().filter_map(|r| r.scope.strip_prefix("team:").map(str::to_owned)).collect();
        teams.extend(daemons.values().filter_map(|d| d.team.clone()));
        teams.sort();
        teams.dedup();
        // The threads on others' machines: whether the reader may reach them.
        let mut asked: Vec<String> = match reader {
            Some(me) => rooms
                .iter()
                .filter(|r| matches!(r.kind.as_str(), "pane" | "session"))
                .filter_map(|r| r.machine.clone())
                .filter(|m| daemons.get(m).is_some_and(|d| d.account != me))
                .collect(),
            None => vec![],
        };
        asked.sort();
        asked.dedup();
        let mut people = Vec::new();
        for r in rooms.iter().filter(|r| r.kind == "dm") {
            people.extend(app.db.chat_members(&r.id)?);
        }
        people.sort();
        people.dedup();
        let (teams, reach, people) = tokio::try_join!(
            app.cache.teams(hub, &teams),
            app.cache.may_reach(hub, reader.unwrap_or_default(), &asked),
            app.cache.accounts(hub, &people),
        )?;
        Ok(Facts {
            reader: reader.map(str::to_owned),
            teams,
            daemons,
            people,
            asked: asked.into_iter().collect(),
            reach,
        })
    }

    /// An account's role in a team, if it's in it now: a locked team's
    /// members other than owners are as good as out.
    fn role(&self, team: &str, account: &str) -> Option<TeamRole> {
        self.teams.get(team).and_then(|t| t.role(account))
    }

    fn owner(&self, machine: &str) -> Option<&str> {
        self.daemons.get(machine).map(|d| d.account.as_str())
    }

    /// Whether control routes `account` to `machine`: known for the reader,
    /// asked of the hub (or the cache) for anyone else.
    async fn reaches(&self, app: &Platform, account: &str, machine: &str) -> anyhow::Result<bool> {
        if self.reader.as_deref() == Some(account) && self.asked.contains(machine) {
            return Ok(self.reach.contains(machine));
        }
        Ok(app.cache.may_reach(&*app.hub, account, &[machine.to_owned()]).await?.contains(machine))
    }
}

/// Whether `account` reads the thread on `machine`'s pane or session, and
/// from when (ms): see the top of this file.
async fn target_sight(
    app: &Platform,
    facts: &Facts,
    account: &str,
    machine: &str,
    kind: &str,
    target: &str,
) -> anyhow::Result<Option<u64>> {
    let Some(d) = facts.daemons.get(machine) else { return Ok(None) };
    if d.account == account {
        return Ok(Some(0));
    }
    if !facts.reaches(app, account, machine).await? {
        return Ok(None);
    }
    Ok(match app.db.chat_readers(machine, kind, target)? {
        Some(list) => list.iter().find(|r| r.account == account).map(|r| r.from.unwrap_or(0)),
        // Before the machine has said: its team's owners.
        None => match &d.team {
            Some(team) if facts.role(team, account) == Some(TeamRole::Owner) => Some(0),
            _ => None,
        },
    })
}

/// Whether `account` sees `room`, and from when (ms; 0 is everything).
async fn sight(app: &Platform, facts: &Facts, account: &str, room: &Room) -> anyhow::Result<Option<u64>> {
    let team = room.scope.strip_prefix("team:");
    Ok(match (room.kind.as_str(), team) {
        ("channel", Some(team)) => facts.role(team, account).map(|_| 0),
        ("dm", Some(team)) => {
            let member = app.db.chat_members(&room.id)?.iter().any(|a| a == account);
            (member && facts.role(team, account).is_some()).then_some(0)
        }
        ("pane" | "session", _) => match (&room.machine, &room.target) {
            (Some(m), Some(t)) => target_sight(app, facts, account, m, &room.kind, t).await?,
            _ => None,
        },
        ("archive", _) => app.db.chat_member_from(&room.id, account)?,
        _ => None,
    })
}

/// Whether `account` only watches the thread on `machine`'s pane or
/// session: its machine said they're a viewer there (Jake, 2026-10-19).
/// They read it and don't post.
fn watching(app: &Platform, facts: &Facts, account: &str, room: &Room) -> anyhow::Result<bool> {
    let (Some(m), Some(t)) = (&room.machine, &room.target) else { return Ok(false) };
    if facts.owner(m) == Some(account) {
        return Ok(false);
    }
    Ok(app.db.chat_readers(m, &room.kind, t)?.is_some_and(|l| l.iter().any(|r| r.account == account && r.watching)))
}

/// Whether `account`, who sees `room`, may post in it now.
fn writable(app: &Platform, facts: &Facts, room: &Room, account: &str) -> anyhow::Result<bool> {
    if room.archived.is_some() {
        return Ok(false);
    }
    Ok(match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel", _) => true,
        ("pane" | "session", _) => !watching(app, facts, account, room)?,
        // Both still in the team: one who left ends it for the other.
        ("dm", Some(team)) => app.db.chat_members(&room.id)?.iter().all(|a| facts.role(team, a).is_some()),
        _ => false,
    })
}

/// The rooms an account sees, with each one's floor, and what the hub said
/// about them (one call each for the lot).
async fn rooms_seen(app: &Platform, account: &str) -> anyhow::Result<(Vec<(Room, u64)>, Facts)> {
    let hub = &*app.hub;
    let (teams, machines) = tokio::try_join!(app.cache.teams_of(hub, account), app.cache.machines_for(hub, account))?;
    let scopes: Vec<String> = teams.iter().map(|t| team_scope(t)).collect();
    let mut rooms = app.db.chat_rooms_in(&scopes, &machines)?;
    rooms.extend(app.db.chat_rooms_member(account)?);
    let mut ids = HashSet::new();
    rooms.retain(|r| ids.insert(r.id.clone()));
    let facts = Facts::of(app, Some(account), &rooms.iter().collect::<Vec<_>>()).await?;
    let mut out = Vec::new();
    for r in rooms {
        if let Some(from) = sight(app, &facts, account, &r).await? {
            out.push((r, from));
        }
    }
    out.sort_by(|a, b| (a.0.created, &a.0.id).cmp(&(b.0.created, &b.0.id)));
    Ok((out, facts))
}

/// Everyone who sees a room (and has chat on): whom a mention can reach.
async fn audience(app: &Platform, room: &Room) -> anyhow::Result<Vec<String>> {
    let facts = Facts::of(app, None, &[room]).await?;
    let members = |team: Option<&str>| -> Vec<String> {
        team.and_then(|t| facts.teams.get(t))
            .map(|t| t.members.iter().map(|m| m.account.clone()).collect())
            .unwrap_or_default()
    };
    let mut maybe: Vec<String> = match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel", Some(team)) => members(Some(team)),
        ("pane" | "session", _) => {
            let m = room.machine.as_deref().unwrap_or_default();
            let mut v: Vec<String> = facts.owner(m).map(str::to_owned).into_iter().collect();
            v.extend(members(facts.daemons.get(m).and_then(|d| d.team.as_deref())));
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
        if sight(app, &facts, &a, room).await?.is_some() && chat_on(app, &a).await? {
            out.push(a);
        }
    }
    Ok(out)
}

/// The room `id`, if `account` sees it (404 otherwise, as if it weren't
/// there), its floor for them, and what the hub said about it.
async fn room_for(app: &Platform, account: &str, id: &str) -> Result<(Room, u64, Facts), ApiError> {
    let gone = || err(StatusCode::NOT_FOUND, "no such room");
    let room = app.db.chat_room(id)?.ok_or_else(gone)?;
    let facts = Facts::of(app, Some(account), &[&room]).await?;
    let from = sight(app, &facts, account, &room).await?.ok_or_else(gone)?;
    Ok((room, from, facts))
}

/// An account's name: the one it chose, else its login, else "".
async fn name_of(app: &Platform, account: &str) -> anyhow::Result<String> {
    Ok(app.cache.accounts(&*app.hub, &[account.to_owned()]).await?.remove(account).map(|a| a.name).unwrap_or_default())
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
fn listed(app: &Platform, facts: &Facts, account: &str, room: &Room, from: u64) -> anyhow::Result<ChatRoom> {
    let (read, muted) = app.db.chat_mark(&room.id, account)?;
    let (unread, mentions) = app.db.chat_unread(&room.id, account, read, from)?;
    let with = if room.kind == "dm" {
        app.db
            .chat_members(&room.id)?
            .into_iter()
            .find(|a| a != account)
            .map(|a| ChatPerson { name: facts.people.get(&a).map(|p| p.name.clone()).unwrap_or_default(), account: a })
    } else {
        None
    };
    let machine_name = room.machine.as_ref().and_then(|m| facts.daemons.get(m)).map(|d| d.name.clone());
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
        writable: writable(app, facts, room, account)?,
    })
}

/// Names and pane chips, asked of the hub once per answer.
struct Shower {
    names: HashMap<String, String>,
    machines: HashMap<String, (String, bool)>,
}

impl Shower {
    async fn of(app: &Platform, reader: &str, ms: &[&Message]) -> anyhow::Result<Self> {
        let hub = &*app.hub;
        let mut authors: Vec<String> = ms.iter().map(|m| m.author.clone()).filter(|a| !a.is_empty()).collect();
        authors.sort();
        authors.dedup();
        let mut machines: Vec<String> =
            ms.iter().filter(|m| m.ref_pane.is_some()).filter_map(|m| m.ref_machine.clone()).collect();
        machines.sort();
        machines.dedup();
        let (accounts, daemons, reach) = tokio::try_join!(
            app.cache.accounts(hub, &authors),
            app.cache.daemons(hub, &machines),
            app.cache.may_reach(hub, reader, &machines),
        )?;
        let names = authors
            .into_iter()
            .map(|a| (a.clone(), accounts.get(&a).map(|x| x.name.clone()).unwrap_or_default()))
            .collect();
        let machines = machines
            .into_iter()
            .map(|m| {
                let name = daemons.get(&m).map(|d| d.name.clone()).unwrap_or_default();
                let opens = reach.contains(&m);
                (m, (name, opens))
            })
            .collect();
        Ok(Self { names, machines })
    }

    fn show(&self, m: &Message) -> ChatMessage {
        let pane = match (&m.ref_machine, &m.ref_pane) {
            (Some(machine), Some(pane)) => {
                let (machine_name, opens) = self.machines.get(machine).cloned().unwrap_or_default();
                Some(ChatPane { machine: machine.clone(), machine_name, pane: pane.clone(), opens })
            }
            _ => None,
        };
        ChatMessage {
            seq: m.seq,
            at: m.at,
            author: m.author.clone(),
            name: self.names.get(&m.author).cloned().unwrap_or_default(),
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

/// The one word a roster calls a person (control's `teams::member_name`):
/// the name they chose, its words joined by `-`, else `account-` and the
/// start of their id.
fn member_name(id: &str, chosen: &str) -> String {
    let words: Vec<&str> =
        chosen.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect();
    let name: String = words.join("-").chars().take(120).collect();
    if name.is_empty() { format!("account-{}", &id[..6.min(id.len())]) } else { name }
}

/// What an `@word` may name an account by: its login, its name, and the one
/// word a roster calls it. With no name of its own an account goes by its
/// login (`Account::name`), and the roster's word for it is its id's.
fn names_of(a: &Account) -> Vec<String> {
    let mut v = vec![a.login.to_lowercase(), a.name.to_lowercase(), member_name(&a.id, &a.name).to_lowercase()];
    if a.name == a.login {
        v.push(member_name(&a.id, "").to_lowercase());
    }
    v
}

/// Whom a text mentions among `audience` (by login, name, or the one word
/// a roster calls them), leaving out the author, and the `@words` that
/// named nobody there.
async fn mentions(
    app: &Platform,
    text: &str,
    audience: &[String],
    author: &str,
) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let words = at_words(text);
    if words.is_empty() {
        return Ok((vec![], vec![]));
    }
    let accounts = app.cache.accounts(&*app.hub, audience).await?;
    let mut hit: HashSet<&str> = HashSet::new();
    let mut named = Vec::new();
    for a in audience {
        let Some(acc) = accounts.get(a) else { continue };
        let names = names_of(acc);
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
    /// a flag, read from the hub's change feed): each stream re-reads its
    /// rooms.
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
    app: Arc<Platform>,
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
    fn app(&self) -> &Platform {
        &self.reg.app
    }

    async fn refresh(&mut self) {
        let app = self.reg.app.clone();
        let generation = app.chat.generation.load(Ordering::SeqCst);
        let rooms = match chat_on(&app, &self.account).await {
            Ok(true) => rooms_seen(&app, &self.account).await.map(|(v, _)| v.into_iter().map(|(r, _)| r.id).collect()),
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
    async fn deliver(&mut self, out: &Out) -> Option<Event> {
        if out.n <= self.seen {
            return None;
        }
        self.seen = out.n;
        if out.to.as_ref().is_some_and(|to| *to != self.account) {
            return None;
        }
        if self.app().chat.generation.load(Ordering::SeqCst) != self.generation || self.read_at.elapsed() > REFRESH {
            self.refresh().await;
        }
        let ok = match &out.ev {
            ChatLive::Room { room } => {
                let was = self.rooms.contains(room);
                self.refresh().await;
                was || self.rooms.contains(room)
            }
            ChatLive::Message { room, .. } | ChatLive::Changed { room, .. } | ChatLive::Read { room, .. } => {
                self.rooms.contains(room)
            }
            ChatLive::Hello { .. } | ChatLive::Resync {} => true,
        };
        ok.then(|| Event::default().id(self.app().chat.id(out.n)).data(data(&out.ev)))
    }

    async fn resync(&mut self) -> Event {
        self.seen = self.app().chat.next.load(Ordering::SeqCst);
        self.refresh().await;
        Event::default().id(self.app().chat.id(self.seen)).data(data(&ChatLive::Resync {}))
    }
}

/// `GET /api/chat/live`: Server-Sent Events, one JSON [`ChatLive`] each,
/// for the rooms the person sees. `Last-Event-ID` resumes after a drop; a
/// keep-alive comment every 25 s holds it open through proxies.
pub async fn live(
    State(app): State<Arc<Platform>>,
    s: Person,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    gate(&app, &s.account).await?;
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
    live.refresh().await;
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
                if let Some(ev) = live.deliver(&o).await {
                    live.pending.push_back(ev);
                }
            }
        }
        (Some(_), None) => {
            let ev = live.resync().await;
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
                    if let Some(ev) = live.deliver(&out).await {
                        return Some((Ok(ev), live));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let ev = live.resync().await;
                    return Some((Ok(ev), live));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25))))
}

/// `POST /api/chat/live/open`: the room a stream's page has open.
pub async fn open(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<ChatOpen>) -> J<wire::ChatOpen> {
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
pub async fn rooms(State(app): State<Arc<Platform>>, s: Person) -> J<ChatRooms> {
    gate(&app, &s.account).await?;
    let (seen, facts) = rooms_seen(&app, &s.account).await?;
    let mut rooms = Vec::new();
    for (r, from) in seen {
        rooms.push(listed(&app, &facts, &s.account, &r, from)?);
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
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
    Json(b): Json<ChatNewChannel>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let scope = team_scope(&team);
    let room = Room {
        id: new_id("ch_"),
        scope: scope.clone(),
        kind: "channel".into(),
        name: String::new(),
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    let facts = Facts::of(&app, Some(&s.account), &[&room]).await?;
    match facts.role(&team, &s.account) {
        None => return Err(err(StatusCode::NOT_FOUND, "no such team")),
        Some(TeamRole::Viewer) => {
            return Err(err(StatusCode::FORBIDDEN, "a team's owners and editors make its channels"));
        }
        _ => {}
    }
    let name = channel_name(&b.name)?;
    if app.db.chat_channels(&scope)? >= MAX_CHANNELS {
        return Err(err(StatusCode::CONFLICT, "a team has at most 200 channels; archive or reuse one"));
    }
    app.limits.check_account(CHAT_ROOMS, &s.account)?;
    let room = Room { name, ..room };
    // The team's default project, which the hub keeps.
    let project = app.hub.team_project(&team).await?;
    app.db.add_chat_room(&room, &[], project.as_deref()).map_err(|e| {
        if e.is::<Taken>() { err(StatusCode::CONFLICT, "there's a channel by that name") } else { e.into() }
    })?;
    app.chat.room(&room.id);
    app.analytics.capture_team(&s.account, &team, "chat channel made", json!({}));
    Ok(Json(listed(&app, &facts, &s.account, &room, 0)?))
}

/// `POST /api/chat/teams/{team}/dms`: the DM with a teammate (made the
/// first time).
pub async fn new_dm(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
    Json(b): Json<ChatNewDm>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let scope = team_scope(&team);
    let room = Room {
        id: new_id("dm_"),
        scope: scope.clone(),
        kind: "dm".into(),
        name: String::new(),
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    let facts = Facts::of(&app, Some(&s.account), &[&room]).await?;
    if b.with == s.account || facts.role(&team, &s.account).is_none() || facts.role(&team, &b.with).is_none() {
        return Err(err(StatusCode::NOT_FOUND, "nobody by that account in this team"));
    }
    if let Some(r) = app.db.chat_dm(&scope, &s.account, &b.with)? {
        let facts = Facts::of(&app, Some(&s.account), &[&r]).await?;
        return Ok(Json(listed(&app, &facts, &s.account, &r, 0)?));
    }
    app.limits.check_account(CHAT_ROOMS, &s.account)?;
    app.db.add_chat_room(&room, &[&s.account, &b.with], None)?;
    app.chat.room(&room.id);
    let facts = Facts::of(&app, Some(&s.account), &[&room]).await?;
    Ok(Json(listed(&app, &facts, &s.account, &room, 0)?))
}

/// A pane's or session's id as a daemon names it: short, no spaces.
fn valid_target(t: &str) -> bool {
    (1..=64).contains(&t.len()) && t.chars().all(|c| c.is_ascii_graphic())
}

/// `POST /api/chat/targets`: the thread on a pane or session (made the
/// first time), for someone who reads it.
pub async fn open_target(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<ChatOpenTarget>) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let kind = match b.kind {
        wire::ChatTargetKind::Pane => "pane",
        wire::ChatTargetKind::Session => "session",
    };
    let nothing = || err(StatusCode::NOT_FOUND, "no such pane or session");
    if !valid_target(&b.target) {
        return Err(nothing());
    }
    let scope = machine_scope(&b.machine);
    let room = Room {
        id: new_id("th_"),
        scope: scope.clone(),
        kind: kind.into(),
        name: format!("{kind} {}", b.target),
        machine: Some(b.machine.clone()),
        target: Some(b.target.clone()),
        created_by: s.account.clone(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    let facts = Facts::of(&app, Some(&s.account), &[&room]).await?;
    let from = target_sight(&app, &facts, &s.account, &b.machine, kind, &b.target).await?.ok_or_else(nothing)?;
    if let Some(r) = app.db.chat_room_for(kind, &b.machine, &b.target)? {
        return Ok(Json(listed(&app, &facts, &s.account, &r, from)?));
    }
    if app.db.chat_rooms_count(&scope)? >= MAX_THREADS {
        return Err(err(StatusCode::CONFLICT, "this machine has too many threads"));
    }
    app.limits.check_account(CHAT_ROOMS, &s.account)?;
    match app.db.add_chat_room(&room, &[], None) {
        Ok(()) => app.chat.room(&room.id),
        // Someone made it just now.
        Err(e) if e.is::<Taken>() => {
            let r = app.db.chat_room_for(kind, &b.machine, &b.target)?.ok_or_else(nothing)?;
            return Ok(Json(listed(&app, &facts, &s.account, &r, from)?));
        }
        Err(e) => return Err(e.into()),
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// `GET /api/chat/rooms/{id}`: the room, and who sees it.
pub async fn room(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<ChatRoomDetail> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let audience = audience(&app, &room).await?;
    let names = app.cache.accounts(&*app.hub, &audience).await?;
    let people = audience
        .into_iter()
        .map(|a| ChatPerson { name: names.get(&a).map(|x| x.name.clone()).unwrap_or_default(), account: a })
        .collect();
    Ok(Json(ChatRoomDetail { room: listed(&app, &facts, &s.account, &room, from)?, people }))
}

/// `PATCH /api/chat/rooms/{id}`: a channel renamed or archived, by its
/// team's owners.
pub async fn change_room(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatRoomChange>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let team = room.scope.strip_prefix("team:").filter(|_| room.kind == "channel");
    let Some(team) = team else { return Err(err(StatusCode::BAD_REQUEST, "only a channel is renamed or archived")) };
    if facts.role(team, &s.account) != Some(TeamRole::Owner) {
        return Err(err(StatusCode::FORBIDDEN, "a team's owners rename and archive its channels"));
    }
    if let Some(name) = &b.name {
        let name = channel_name(name)?;
        app.db.rename_chat_room(&room.id, &name).map_err(|e| {
            if e.is::<Taken>() { err(StatusCode::CONFLICT, "there's a channel by that name") } else { e.into() }
        })?;
    }
    if let Some(archived) = b.archived {
        app.db.archive_chat_room(&room.id, archived.then(now_ms))?;
    }
    app.chat.room(&room.id);
    let room = app.db.chat_room(&room.id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such room"))?;
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// `POST /api/chat/rooms/{id}/mute`.
pub async fn mute(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatMute>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    app.db.set_chat_muted(&room.id, &s.account, b.muted)?;
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

// ---------------------------------------------------------------- messages

/// `GET /api/chat/rooms/{id}/messages?after=N|before=N&limit=`: a page,
/// oldest first, as the reader sees it.
pub async fn messages(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Query(q): Query<ChatMessagesQuery>,
) -> J<ChatMessages> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let page = match (q.after, q.before) {
        (Some(a), _) => Page::After(a),
        (None, Some(b)) => Page::Before(b),
        (None, None) => Page::Newest,
    };
    let (ms, more) = app.db.chat_messages(&room.id, page, q.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE))?;
    let ms: Vec<&Message> = ms.iter().filter(|m| m.at >= from).collect();
    let shower = Shower::of(&app, &s.account, &ms).await?;
    let messages = ms.into_iter().map(|m| shower.show(m)).collect();
    Ok(Json(ChatMessages { room: listed(&app, &facts, &s.account, &room, from)?, messages, more }))
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
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatPost>,
) -> J<ChatPosted> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    if watching(&app, &facts, &s.account, &room)? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            WATCHING,
            "you're watching this session: you read its threads, and its owner can let you post",
        ));
    }
    if !writable(&app, &facts, &room, &s.account)? {
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
        && (!valid_target(&p.pane)
            || !app
                .cache
                .may_reach(&*app.hub, &s.account, std::slice::from_ref(&p.machine))
                .await?
                .contains(&p.machine))
    {
        return Err(err(StatusCode::FORBIDDEN, "you can't see that pane"));
    }
    if app.db.chat_bytes(&room.scope)? + (text.len() + quote.map_or(0, str::len)) as u64 > MAX_SCOPE_BYTES {
        return Err(err(StatusCode::INSUFFICIENT_STORAGE, "this team's chat is full (1 GB): delete some first"));
    }
    app.limits.check_account_per_minute(CHAT_POSTS, &s.account)?;
    app.limits.check_all_per_minute(ALL_CHAT_POSTS)?;
    let people = audience(&app, &room).await?;
    let (named, unreached) = mentions(&app, text, &people, &s.account).await?;
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
        &facts,
        &room,
        &s.account,
        seq,
        text.is_empty().then_some(quote).flatten().unwrap_or(text),
        &named,
        &people,
    )
    .await?;
    Ok(Json(ChatPosted { seq, mentions: named, unreached, pushed }))
}

/// How chat's pushes go: to every subscription the account has, with the
/// hub's TTL, as control's own notices always have (M122).
const PUSHES: PushOptions = PushOptions { ttl: None, any_subscription: true };

/// Push a new message to whom it's for (S41 q4): mentions, and the other
/// person in a DM. Not the author, nobody with the room open or muted, and
/// nobody twice a minute for one room. The hub sends them
/// ([`HubPort::push_with`](crate::HubPort::push_with)), in the
/// background: a failure is only logged.
#[allow(clippy::too_many_arguments)]
async fn push(
    app: &Arc<Platform>,
    facts: &Facts,
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
    let me = name_of(app, author).await?;
    let place = match room.kind.as_str() {
        "channel" => format!("#{}", room.name),
        _ => match room.machine.as_deref().and_then(|m| facts.daemons.get(m)) {
            Some(d) => format!("{} on {}", room.name, d.name),
            None => room.name.clone(),
        },
    };
    let body: String = text.chars().take(wire::PUSH_CHARS).collect();
    let now = now_ms();
    let mut out = Vec::new();
    let mut sends = Vec::new();
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
        let msg = json!({
            "chat": { "room": room.id, "seq": seq },
            "title": title, "body": body, "tag": format!("chat-{}", room.id), "control": true,
        });
        sends.push((account.clone(), msg));
        out.push(ChatPushed { account, why });
    }
    let app = app.clone();
    tokio::spawn(async move {
        for (account, msg) in sends {
            if let Err(e) = app.hub.push_with(&account, &msg, PUSHES).await {
                tracing::warn!(error = %e, %account, "chat's push didn't go");
            }
        }
    });
    Ok(out)
}

/// The message `seq` in a room the person sees.
fn message_in(app: &Platform, room: &Room, seq: i64) -> Result<Message, ApiError> {
    app.db.chat_message(&room.id, seq)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such message"))
}

/// One message as `reader` sees it.
async fn shown(app: &Platform, reader: &str, m: &Message) -> anyhow::Result<ChatMessage> {
    Ok(Shower::of(app, reader, &[m]).await?.show(m))
}

/// `PATCH /api/chat/rooms/{id}/messages/{seq}`: its author's new text.
/// Whom it mentions is worked out again; nobody is pushed for an edit.
pub async fn edit(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<ChatEdit>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    let m = message_in(&app, &room, seq)?;
    if m.author != s.account || m.deleted.is_some() {
        return Err(err(StatusCode::FORBIDDEN, "only its author edits a message"));
    }
    if watching(&app, &facts, &s.account, &room)? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            WATCHING,
            "you're watching this session: you read its threads, and its owner can let you post",
        ));
    }
    if !writable(&app, &facts, &room, &s.account)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let text = b.text.trim();
    if text.is_empty() && m.quote.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "say something, or delete it"));
    }
    sizes(text, None)?;
    app.limits.check_account_per_minute(CHAT_POSTS, &s.account)?;
    let (named, _) = mentions(&app, text, &audience(&app, &room).await?, &s.account).await?;
    app.db.edit_chat_message(&room.id, seq, text, &named, now_ms())?;
    app.chat.publish(ChatLive::Changed { room: room.id.clone(), seq }, None);
    Ok(Json(shown(&app, &s.account, &message_in(&app, &room, seq)?).await?))
}

/// `DELETE /api/chat/rooms/{id}/messages/{seq}`: by its author, a team's
/// owner (in the team's rooms) or the machine's owner (in its threads).
/// Its number stays, with nothing in it.
pub async fn delete(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    let m = message_in(&app, &room, seq)?;
    let may = m.author == s.account
        || match room.scope.split_once(':') {
            Some(("team", team)) => facts.role(team, &s.account) == Some(TeamRole::Owner),
            Some(("machine", machine)) => facts.owner(machine) == Some(s.account.as_str()),
            _ => false,
        };
    if !may {
        return Err(err(StatusCode::FORBIDDEN, "only its author, or an owner, deletes a message"));
    }
    if m.deleted.is_none() {
        app.db.delete_chat_message(&room.id, seq, now_ms())?;
        app.chat.publish(ChatLive::Changed { room: room.id.clone(), seq }, None);
    }
    Ok(Json(shown(&app, &s.account, &message_in(&app, &room, seq)?).await?))
}

/// `POST /api/chat/rooms/{id}/read`: read up to `seq` (only forward).
pub async fn read(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatRead>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let seq = b.seq.min(room.last_seq);
    if app.db.set_chat_read(&room.id, &s.account, seq)? {
        app.chat.publish(ChatLive::Read { room: room.id.clone(), seq }, Some(s.account.clone()));
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
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
pub async fn search(State(app): State<Arc<Platform>>, s: Person, Query(q): Query<ChatSearchQuery>) -> J<ChatFound> {
    gate(&app, &s.account).await?;
    let asked = parse_search(&q.q);
    if asked.fts.is_none() && asked.channel.is_none() && asked.from.is_none() && !asked.has_quote {
        return Ok(Json(ChatFound { found: vec![] }));
    }
    app.limits.check_account(CHAT_SEARCHES, &s.account)?;
    let author = match &asked.from {
        // The login's account, else the one account by that name.
        Some(who) => match app.hub.accounts_called(who).await?.as_slice() {
            [one] => Some(one.id.clone()),
            _ => return Ok(Json(ChatFound { found: vec![] })),
        },
        None => None,
    };
    let (seen, facts) = rooms_seen(&app, &s.account).await?;
    let mut floors = HashMap::new();
    for (r, from) in seen {
        let in_team = match &q.team {
            Some(t) => {
                r.scope == team_scope(t)
                    || r.machine
                        .as_deref()
                        .is_some_and(|m| facts.daemons.get(m).and_then(|d| d.team.as_deref()) == Some(t.as_str()))
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
    let hits: Vec<&Message> = hits.iter().filter(|m| floors.get(&m.room).is_some_and(|from| m.at >= *from)).collect();
    let shower = Shower::of(&app, &s.account, &hits).await?;
    let found = hits.into_iter().map(|m| ChatHit { room: m.room.clone(), message: shower.show(m) }).collect();
    Ok(Json(ChatFound { found }))
}

// ---------------------------------------------------------------- daemons

/// Whether chat is on for a machine: for its owner, or for its team.
async fn machine_chat_on(app: &Platform, machine: &Daemon) -> anyhow::Result<bool> {
    if chat_on(app, &machine.account).await? {
        return Ok(true);
    }
    Ok(match &machine.team {
        Some(t) => {
            let owner = team_owner(t);
            flag_of(&app.cache.flags(&*app.hub, std::slice::from_ref(&owner)).await?, &owner) == Some(true)
        }
        None => false,
    })
}

/// The calling machine and its owner, if chat is on for it.
async fn chat_machine(app: &Platform, d: Machine) -> Result<(String, String), ApiError> {
    let machine = d.daemon;
    let row = app
        .cache
        .daemons(&*app.hub, std::slice::from_ref(&machine))
        .await?
        .remove(&machine)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?;
    if !machine_chat_on(app, &row).await? {
        return Err(refusal(StatusCode::FORBIDDEN, CHAT_OFF, "chat isn't on for this machine's account or team"));
    }
    Ok((machine, row.account))
}

fn check_reader_lists(lists: &[ChatReaderList]) -> Result<(), ApiError> {
    if lists.len() > wire::MAX_READER_TARGETS
        || lists.iter().any(|t| t.readers.len() > wire::MAX_READERS || !valid_target(&t.target))
    {
        return Err(err(StatusCode::BAD_REQUEST, "at most 500 threads of 500 readers each, by their ids"));
    }
    Ok(())
}

fn target_kind(k: wire::ChatTargetKind) -> &'static str {
    match k {
        wire::ChatTargetKind::Pane => "pane",
        wire::ChatTargetKind::Session => "session",
    }
}

/// Keep a machine's reader lists; the rooms they change.
fn store_readers(app: &Platform, machine: &str, lists: &[ChatReaderList]) -> anyhow::Result<Vec<String>> {
    let now = now_ms();
    let mut rooms = Vec::new();
    for t in lists {
        let kind = target_kind(t.kind);
        let mut readers: Vec<wire::ChatReader> = Vec::new();
        for r in &t.readers {
            if !r.account.is_empty() && r.account.len() <= 64 && !readers.iter().any(|x| x.account == r.account) {
                readers.push(r.clone());
            }
        }
        app.db.set_chat_readers(machine, kind, &t.target, &readers, now)?;
        if let Some(r) = app.db.chat_room_for(kind, machine, &t.target)? {
            rooms.push(r.id);
        }
    }
    Ok(rooms)
}

/// `POST /api/daemon/chat/readers`: a machine says who may read the threads
/// on its panes and sessions (decision 2). Each list it names replaces the
/// last; control still routes nobody it wouldn't anyway (`target_sight`).
pub async fn daemon_readers(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Json(b): Json<ChatReaders>,
) -> J<ChatReadersStored> {
    let (machine, _) = chat_machine(&app, d).await?;
    check_reader_lists(&b.targets)?;
    app.limits.check_daemon(CHAT_READERS, &machine)?;
    let rooms = store_readers(&app, &machine, &b.targets)?;
    app.chat.changed();
    for r in rooms {
        app.chat.room(&r);
    }
    Ok(Json(ChatReadersStored { stored: b.targets.len() }))
}

/// `s`, at most `max` bytes, cut on a character.
fn cut(s: &str, max: usize) -> &str {
    let mut at = s.len().min(max);
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    &s[..at]
}

/// Whether `a` may be an author or a mention in `room` brought in from
/// `owner`'s machine: the owner, or someone who sees the room.
async fn sees(
    app: &Platform,
    facts: &Facts,
    owner: &str,
    room: &Room,
    a: &str,
    cache: &mut HashMap<String, bool>,
) -> anyhow::Result<bool> {
    if let Some(s) = cache.get(a) {
        return Ok(*s);
    }
    let s = a == owner || sight(app, facts, a, room).await?.is_some();
    cache.insert(a.to_owned(), s);
    Ok(s)
}

/// Bring a thread's or conversation's old messages into its room.
async fn import(
    app: &Platform,
    machine: &str,
    owner: &str,
    room: &Room,
    msgs: &[wire::ChatImportMessage],
    out: &mut wire::ChatImported,
) -> Result<(), ApiError> {
    let bytes: u64 = msgs.iter().map(|m| (m.text.len() + m.quote.as_ref().map_or(0, String::len)) as u64).sum();
    if app.db.chat_bytes(&room.scope)? + bytes > MAX_SCOPE_BYTES {
        return Err(err(StatusCode::INSUFFICIENT_STORAGE, "this machine's chat is full (1 GB)"));
    }
    let facts = Facts::of(app, None, &[room]).await?;
    let mut cache = HashMap::new();
    let mut kept = Vec::with_capacity(msgs.len());
    for m in msgs {
        let author = match &m.account {
            Some(a) if sees(app, &facts, owner, room, a, &mut cache).await? => a.clone(),
            _ => owner.to_owned(),
        };
        let mut mentions = Vec::new();
        for a in &m.mentions {
            if a != &author && !mentions.contains(a) && sees(app, &facts, owner, room, a, &mut cache).await? {
                mentions.push(a.clone());
            }
        }
        kept.push((author, mentions));
    }
    let old: Vec<OldMessage> = msgs
        .iter()
        .zip(&kept)
        .map(|(m, (author, mentions))| OldMessage {
            id: m.id,
            name: cut(&m.name, 200),
            m: NewMessage {
                author,
                at: m.at,
                text: cut(&m.text, wire::MAX_TEXT),
                quote: m.quote.as_deref().map(|q| cut(q, wire::MAX_QUOTE)),
                ref_machine: m.pane.as_ref().filter(|p| valid_target(p)).map(|_| machine),
                ref_pane: m.pane.as_deref().filter(|p| valid_target(p)),
                mentions,
            },
        })
        .collect();
    let (added, already) = app.db.import_chat_messages(&room.id, &old)?;
    out.messages += added;
    out.already += already;
    if added > 0 {
        out.rooms += 1;
    }
    Ok(())
}

/// `POST /api/daemon/chat/import` (M124, S41 q6): a machine brings in the
/// threads it kept itself, once, signed as itself.
/// - Each pane's or session's thread goes into its room on this machine,
///   made if it isn't there. Its messages keep their numbers, unless the
///   room has messages of control's already: then they go before them, 0
///   and below.
/// - Each conversation becomes a read-only archive in its owner's account,
///   read by its members (with an account) from when they joined it.
/// - An author is kept by account when they had one and see the room;
///   anyone else (a tailnet login, an agent, someone who no longer sees it)
///   shows under the machine's owner, by the name they had there.
/// - Every message is taken once, by its old number, so the call may be
///   sent again, or in parts. Imported messages count as read.
pub async fn daemon_import(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Json(b): Json<wire::ChatImport>,
) -> J<wire::ChatImported> {
    let (machine, owner) = chat_machine(&app, d).await?;
    check_reader_lists(&b.readers)?;
    if b.threads.len() + b.archives.len() > wire::MAX_IMPORT_ROOMS
        || b.threads.iter().any(|t| !valid_target(&t.target))
        || b.archives.iter().any(|a| !valid_target(&a.id) || a.members.len() > wire::MAX_READERS)
    {
        return Err(err(StatusCode::BAD_REQUEST, "at most 500 threads and conversations, by their ids"));
    }
    app.limits.check_daemon(CHAT_IMPORTS, &machine)?;
    let mut touched = store_readers(&app, &machine, &b.readers)?;
    let mut out = wire::ChatImported::default();
    let now = now_ms();

    for t in &b.threads {
        let kind = target_kind(t.kind);
        let room = match app.db.chat_room_for(kind, &machine, &t.target)? {
            Some(r) => r,
            None => {
                let scope = machine_scope(&machine);
                if app.db.chat_rooms_count(&scope)? >= MAX_THREADS {
                    return Err(err(StatusCode::CONFLICT, "this machine has too many threads"));
                }
                let room = Room {
                    id: new_id("th_"),
                    scope,
                    kind: kind.into(),
                    name: format!("{kind} {}", t.target),
                    machine: Some(machine.clone()),
                    target: Some(t.target.clone()),
                    created_by: owner.clone(),
                    created: now,
                    archived: None,
                    last_seq: 0,
                };
                match app.db.add_chat_room(&room, &[], None) {
                    Ok(()) => room,
                    Err(e) if e.is::<Taken>() => app
                        .db
                        .chat_room_for(kind, &machine, &t.target)?
                        .ok_or_else(|| err(StatusCode::CONFLICT, "try again"))?,
                    Err(e) => return Err(e.into()),
                }
            }
        };
        import(&app, &machine, &owner, &room, &t.messages, &mut out).await?;
        touched.push(room.id);
    }

    for a in &b.archives {
        let room = match app.db.chat_room_for("archive", &machine, &a.id)? {
            Some(r) => r,
            None => {
                let room = Room {
                    id: new_id("ar_"),
                    scope: format!("account:{owner}"),
                    kind: "archive".into(),
                    name: cut(a.title.trim(), 200).to_owned(),
                    machine: Some(machine.clone()),
                    target: Some(a.id.clone()),
                    created_by: owner.clone(),
                    created: if a.created > 0 { a.created } else { now },
                    archived: Some(now),
                    last_seq: 0,
                };
                match app.db.add_chat_room(&room, &[&owner], None) {
                    Ok(()) => room,
                    Err(e) if e.is::<Taken>() => app
                        .db
                        .chat_room_for("archive", &machine, &a.id)?
                        .ok_or_else(|| err(StatusCode::CONFLICT, "try again"))?,
                    Err(e) => return Err(e.into()),
                }
            }
        };
        // Its members, each from when they joined; the owner reads it all.
        let ids: Vec<String> = a.members.iter().map(|m| m.account.clone()).collect();
        let known = app.cache.accounts(&*app.hub, &ids).await?;
        for m in &a.members {
            if m.account == owner || !known.contains_key(&m.account) {
                continue;
            }
            app.db.set_chat_member_from(&room.id, &m.account, m.from.unwrap_or(0))?;
        }
        app.db.set_chat_member_from(&room.id, &owner, 0)?;
        import(&app, &machine, &owner, &room, &a.messages, &mut out).await?;
        touched.push(room.id);
    }

    app.chat.changed();
    touched.sort();
    touched.dedup();
    for r in touched {
        app.chat.room(&r);
    }
    if out.messages > 0 {
        app.analytics.capture(&owner, "chat imported", json!({ "rooms": out.rooms, "messages": out.messages }));
    }
    Ok(Json(out))
}

// ---------------------------------------------------------------- accounts

/// What deleting an account takes from chat, in the platform's
/// transaction (`PlatformPort::account_deleted`): its DMs (both sides),
/// the threads on its machines, the rooms of the teams it disbanded, and
/// its name on everything else. The rooms that go, for open pages.
pub(crate) fn erase_account(tx: &rusqlite::Transaction, deleted: &crate::Deleted) -> anyhow::Result<Vec<String>> {
    let dms: Vec<String> = {
        let mut q = tx.prepare("SELECT room FROM chat_members WHERE account = ?1")?;
        q.query_map([&deleted.account], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    crate::db::chat::erase_account(tx, &deleted.account, &deleted.machines, &deleted.disbanded)?;
    Ok(dms)
}

/// After an account went: open pages hear of its rooms, and nothing kept of
/// the hub's answers outlives it.
pub(crate) fn account_gone(app: &Platform, rooms: &[String]) {
    app.cache.drop_all();
    for r in rooms {
        app.chat.room(r);
    }
    app.chat.changed();
}

/// Chat's background work: following the hub's change feed
/// ([`cache::start`]), which moves who sees what.
pub fn start(app: &Arc<Platform>) {
    cache::start(app);
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

    #[test]
    fn an_account_is_named_by_its_login_name_or_roster_word() {
        let a = |id: &str, login: &str, name: &str| Account { id: id.into(), login: login.into(), name: name.into() };
        assert_eq!(names_of(&a("a1jake", "jhgaylor", "Jake Gaylor")), ["jhgaylor", "jake gaylor", "jake-gaylor"]);
        // With no name of its own: its login, and the roster's word for it.
        assert_eq!(names_of(&a("a2alex", "alex", "alex")), ["alex", "alex", "alex", "account-a2alex"]);
        assert_eq!(names_of(&a("a3pass", "", "")), ["", "", "account-a3pass", "account-a3pass"]);
    }
}
