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
//! - the thread on an object (an issue, a mirrored PR or issue, M196): its
//!   project's members, and for a private repo's item only those with their
//!   own access to it ([`objects`]).
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
//! **Agents (M197, platform step 6).** A machine's agents read and post in
//! team channels, DMs and object threads through grants its owner gave
//! ([`crate::grants`]), never more than the owner sees ([`agent_room`]).
//! What one posts is the owner's, with the machine beside it
//! ([`post_as`]), shown as "<owner>'s agent on <machine>"; a room any live
//! agent grant reaches says whose agents, to everyone in it
//! ([`room_agents`]). Nothing here wakes an agent. MCP's thread tools
//! reach them through [`agent_threads`] (M198), and `@<machine>` mentions
//! a machine's agents there ([`agent_mentions`]).
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
    self as wire, ChatEdit, ChatFound, ChatHit, ChatKind, ChatLevel, ChatLive, ChatMessage, ChatMessages,
    ChatMessagesQuery, ChatMute, ChatNewChannel, ChatNewDm, ChatOpen, ChatOpenTarget, ChatPane, ChatPerson, ChatPost,
    ChatPosted, ChatPushWhy, ChatPushed, ChatReaction, ChatReactor, ChatRead, ChatReaderList, ChatReaders,
    ChatReadersStored, ChatRoom, ChatRoomAgent, ChatRoomChange, ChatRoomDetail, ChatRooms, ChatSearchQuery,
    NotifyLevel,
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
        Db, Taken,
        chat::{Message, NewChatEvent, NewMessage, OldMessage, Page, Room, Search, UnreadAsk, Word},
        chat_files::ChatFileRow,
        chat_reactions::Reaction,
        objects::Holder,
        projects::{ISSUES_WRITE, THREADS_POST, THREADS_READ},
    },
    grants::{AGENT_UNGRANTED, AgentPrincipal, AgentScope, agent_may},
    hub::{Account, Daemon, Flags, PushOptions, Team},
};

pub(crate) mod agent_notice;
pub(crate) mod agent_threads;
pub(crate) mod alerts;
pub(crate) mod files;
mod link_fetch;
pub(crate) mod objects;
pub(crate) mod people;
pub(crate) mod pins;
pub mod previews;
pub(crate) mod reactions;
pub(crate) mod saved;
pub(crate) mod sidebar;
pub(crate) mod typing;

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
/// Live streams: open at once, per account and in all (per machine); the
/// most events one is sent again when it resumes, and its channel's
/// backlog; how often each re-reads what it may see anyway.
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
    /// A page's rooms ([`Facts::page`]): their DMs' members and their
    /// machines' readers, each read once for the page rather than once or
    /// more per room (a round trip each on Postgres). Empty for one room's
    /// facts, whose callers may write in between: those read the database.
    page: PageReads,
    /// Object threads' projects (whose each is), and what their targets
    /// name (M196).
    projects: HashMap<String, Holder>,
    objects: HashMap<String, objects::Object>,
}

#[derive(Default)]
struct PageReads {
    /// The machines whose readers are in `readers`.
    readers_on: HashSet<String>,
    readers: HashMap<(String, String, String), Vec<arugula_control_wire::chat::ChatReader>>,
    /// The rooms whose members are in `members`.
    members_of: HashSet<String>,
    members: HashMap<String, Vec<String>>,
}

impl Facts {
    async fn of(app: &Platform, reader: Option<&str>, rooms: &[&Room]) -> anyhow::Result<Facts> {
        Self::read(app, reader, rooms, false).await
    }

    /// [`Facts::of`] for a page of rooms nothing writes to before it's
    /// answered (a chat list, a live stream's refresh, search): the
    /// database's part read once for all of them too.
    async fn page(app: &Platform, reader: Option<&str>, rooms: &[&Room]) -> anyhow::Result<Facts> {
        Self::read(app, reader, rooms, true).await
    }

    async fn read(app: &Platform, reader: Option<&str>, rooms: &[&Room], whole: bool) -> anyhow::Result<Facts> {
        let hub = &*app.hub;
        let mut machines: Vec<String> = rooms.iter().filter_map(|r| r.machine.clone()).collect();
        machines.sort();
        machines.dedup();
        let daemons = app.cache.daemons(hub, &machines).await?;
        let mut teams: Vec<String> =
            rooms.iter().filter_map(|r| r.scope.strip_prefix("team:").map(str::to_owned)).collect();
        teams.extend(daemons.values().filter_map(|d| d.team.clone()));
        // Object threads: their projects, those projects' teams, and what
        // their targets name, each read once for all of them.
        let on_objects: Vec<&Room> = rooms.iter().copied().filter(|r| r.kind == "object").collect();
        let mut held: Vec<String> =
            on_objects.iter().filter_map(|r| objects::project_of(r)).map(str::to_owned).collect();
        held.sort();
        held.dedup();
        let projects = app.db.project_holders(&held)?;
        teams.extend(projects.values().filter_map(|h| h.team.clone()));
        let targets: Vec<&str> = on_objects.iter().filter_map(|r| r.target.as_deref()).collect();
        let objects = objects::find(app, &targets)?;
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
        let mut page = PageReads::default();
        let mut people = Vec::new();
        if whole {
            // Readers matter on others' machines (an owner sees their own
            // threads), or on any for nobody in particular (an audience).
            let mut on: Vec<String> = rooms
                .iter()
                .filter(|r| matches!(r.kind.as_str(), "pane" | "session"))
                .filter_map(|r| r.machine.clone())
                .filter(|m| reader.is_none_or(|me| daemons.get(m).is_some_and(|d| d.account != me)))
                .collect();
            on.sort();
            on.dedup();
            page.readers = app.db.chat_readers_on(&on)?;
            page.readers_on = on.into_iter().collect();
            let dms: Vec<&str> = rooms.iter().filter(|r| r.kind == "dm").map(|r| r.id.as_str()).collect();
            page.members = app.db.chat_members_of(&dms)?;
            page.members_of = dms.iter().map(|d| (*d).to_owned()).collect();
            people.extend(page.members.values().flatten().cloned());
        } else {
            for r in rooms.iter().filter(|r| r.kind == "dm") {
                people.extend(app.db.chat_members(&r.id)?);
            }
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
            page,
            projects,
            objects,
        })
    }

    /// A room's members: from the page, or the database.
    fn members(&self, app: &Platform, room: &str) -> anyhow::Result<Vec<String>> {
        if self.page.members_of.contains(room) {
            return Ok(self.page.members.get(room).cloned().unwrap_or_default());
        }
        app.db.chat_members(room)
    }

    /// Who reads a pane's or session's thread, as its machine last said:
    /// from the page, or the database.
    fn readers(
        &self,
        app: &Platform,
        machine: &str,
        kind: &str,
        target: &str,
    ) -> anyhow::Result<Option<Vec<arugula_control_wire::chat::ChatReader>>> {
        if self.page.readers_on.contains(machine) {
            return Ok(self.page.readers.get(&(machine.to_owned(), kind.to_owned(), target.to_owned())).cloned());
        }
        app.db.chat_readers(machine, kind, target)
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
    Ok(match facts.readers(app, machine, kind, target)? {
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
            let member = facts.members(app, &room.id)?.iter().any(|a| a == account);
            (member && facts.role(team, account).is_some()).then_some(0)
        }
        ("pane" | "session", _) => match (&room.machine, &room.target) {
            (Some(m), Some(t)) => target_sight(app, facts, account, m, &room.kind, t).await?,
            _ => None,
        },
        ("archive", _) => app.db.chat_member_from(&room.id, account)?,
        ("object", _) => objects::sight(app, facts, account, room).await?,
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
    Ok(facts.readers(app, m, &room.kind, t)?.is_some_and(|l| l.iter().any(|r| r.account == account && r.watching)))
}

/// Whether `account`, who sees `room`, may post in it now.
fn writable(app: &Platform, facts: &Facts, room: &Room, account: &str) -> anyhow::Result<bool> {
    if room.archived.is_some() {
        return Ok(false);
    }
    Ok(match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel", _) => true,
        ("pane" | "session", _) => !watching(app, facts, account, room)?,
        // Every member who sees it, a team's viewers too.
        ("object", _) => true,
        // Both still in the team: one who left ends it for the other.
        ("dm", Some(team)) => facts.members(app, &room.id)?.iter().all(|a| facts.role(team, a).is_some()),
        _ => false,
    })
}

/// The rooms an account sees, with each one's floor, and what the hub said
/// about them (one call each for the lot).
async fn rooms_seen(app: &Platform, account: &str) -> anyhow::Result<(Vec<(Room, u64)>, Facts)> {
    let hub = &*app.hub;
    let (teams, machines) = tokio::try_join!(app.cache.teams_of(hub, account), app.cache.machines_for(hub, account))?;
    let scopes: Vec<String> = teams.iter().map(|t| team_scope(t)).collect();
    let mut rooms = app.db.chat_rooms_seen(&scopes, &machines, account)?;
    let mut ids = HashSet::new();
    rooms.retain(|r| ids.insert(r.id.clone()));
    let facts = Facts::page(app, Some(account), &rooms.iter().collect::<Vec<_>>()).await?;
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
        ("object", _) => objects::audience(&facts, room),
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
pub(crate) async fn name_of(app: &Platform, account: &str) -> anyhow::Result<String> {
    Ok(app.cache.accounts(&*app.hub, &[account.to_owned()]).await?.remove(account).map(|a| a.name).unwrap_or_default())
}

fn kind_of(kind: &str) -> ChatKind {
    match kind {
        "channel" => ChatKind::Channel,
        "dm" => ChatKind::Dm,
        "pane" => ChatKind::Pane,
        "session" => ChatKind::Session,
        "object" => ChatKind::Object,
        _ => ChatKind::Archive,
    }
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", hex::encode(arugula_e2e::random::<8>()))
}

/// A room as `account` sees it.
fn listed(app: &Platform, facts: &Facts, account: &str, room: &Room, from: u64) -> anyhow::Result<ChatRoom> {
    let mark = app.db.chat_mark(&room.id, account)?;
    let counts = app.db.chat_unread(&room.id, account, mark.0, from)?;
    let mut r = listed_as(app, facts, account, room, mark, counts, &pins::Extras::of(&app.db, &[room])?)?;
    with_levels(app, account, std::slice::from_mut(&mut r), None)?;
    Ok(r)
}

/// What each room is set to for `account` (#113): their own choice, else
/// their default for its kind; and whether a DM's other person has
/// notifications paused (#115). `own` is the reader's choices when the
/// caller read them with the marks ([`rooms`]); else one statement reads
/// them. One more reads the reader's defaults and the DM people's pauses
/// together, whatever the page's size (`GET /api/chat`'s budget,
/// `hub_calls_tests`).
fn with_levels(
    app: &Platform,
    account: &str,
    rooms: &mut [ChatRoom],
    own: Option<HashMap<String, NotifyLevel>>,
) -> anyhow::Result<()> {
    let own = match own {
        Some(o) => o,
        None => {
            let ids: Vec<&str> = rooms.iter().map(|r| r.id.as_str()).collect();
            app.db.chat_levels_for(account, &ids)?
        }
    };
    let mut who: Vec<String> = rooms.iter().filter_map(|r| r.with.as_ref().map(|w| w.account.clone())).collect();
    who.push(account.to_owned());
    let mut prefs = app.db.notify_prefs_of(&who)?;
    let mine = prefs.remove(account).unwrap_or_default();
    for r in rooms.iter_mut() {
        let kind = match r.kind {
            ChatKind::Dm => "dm",
            ChatKind::Channel => "channel",
            _ => "thread",
        };
        let chosen = own.get(&r.id).copied();
        r.level = chosen.unwrap_or_else(|| crate::notify::default_level(&mine, kind));
        r.level_own = chosen.is_some();
        // The old flag reads the same as `nothing`.
        r.muted = r.level == NotifyLevel::Nothing;
    }
    let now = now_ms();
    for w in rooms.iter_mut().filter_map(|r| r.with.as_mut()) {
        w.dnd = prefs.get(&w.account).is_some_and(|p| crate::notify::paused(p, now));
    }
    Ok(())
}

/// Which of these accounts have notifications paused now (#115).
fn paused_among(app: &Platform, accounts: &[String]) -> anyhow::Result<HashSet<String>> {
    let now = now_ms();
    Ok(app
        .db
        .notify_prefs_of(accounts)?
        .into_iter()
        .filter(|(_, p)| crate::notify::paused(p, now))
        .map(|(a, _)| a)
        .collect())
}

/// [`listed`], with the reader's mark (and whether they muted it) and the
/// room's (unread, mentions) already read: a page reads those once for all
/// its rooms ([`rooms`]).
fn listed_as(
    app: &Platform,
    facts: &Facts,
    account: &str,
    room: &Room,
    (read, muted): (i64, bool),
    (unread, mentions): (u64, u64),
    extras: &pins::Extras,
) -> anyhow::Result<ChatRoom> {
    let writable = writable(app, facts, room, account)?;
    let (topic, description, pins, topic_writable) = extras.of_room(facts, room, account, writable);
    let with = if room.kind == "dm" {
        facts.members(app, &room.id)?.into_iter().find(|a| a != account).map(|a| ChatPerson {
            name: facts.people.get(&a).map(|p| p.name.clone()).unwrap_or_default(),
            account: a,
            dnd: false,
        })
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
        project: objects::project_of(room).map(str::to_owned),
        with,
        last: room.last_seq,
        read,
        unread,
        mentions,
        muted,
        // The reader's own and default levels are filled in by
        // [`with_levels`], for a page of rooms at once.
        level: if muted { NotifyLevel::Nothing } else { NotifyLevel::Mentions },
        level_own: muted,
        archived: room.archived.is_some(),
        writable,
        topic,
        description,
        pins,
        topic_writable,
    })
}

/// Mark the people who have notifications paused.
fn with_dnd(app: &Platform, people: &mut [ChatPerson]) -> anyhow::Result<()> {
    let ids: Vec<String> = people.iter().map(|p| p.account.clone()).collect();
    let paused = paused_among(app, &ids)?;
    for p in people {
        p.dnd = paused.contains(&p.account);
    }
    Ok(())
}

/// Names and pane chips, asked of the hub once per answer.
struct Shower {
    names: HashMap<String, String>,
    machines: HashMap<String, (String, bool)>,
    /// Agents' machines' names (M197): "" for one that's gone.
    agents: HashMap<String, String>,
    /// The reactions on these messages (#104), first reaction first.
    reactions: HashMap<(String, i64), Vec<Reaction>>,
    /// The files on these messages (#107), in the order they were chosen.
    files: HashMap<(String, i64), Vec<ChatFileRow>>,
    /// Which are system lines or pinned (#119).
    marks: pins::Marks,
    /// Who is reading: a reaction is "mine" when it is this account's and
    /// this agent's ("" for the person), so an agent sees its own apart
    /// from its owner's.
    reader: (String, String),
}

impl Shower {
    async fn of(app: &Platform, reader: &str, ms: &[&Message]) -> anyhow::Result<Self> {
        let hub = &*app.hub;
        let reactions = reactions_of(&app.db, ms)?;
        let files = files::files_of(&app.db, ms)?;
        let marks = pins::Marks::of(&app.db, ms)?;
        let mut authors: Vec<String> = ms.iter().map(|m| m.author.clone()).filter(|a| !a.is_empty()).collect();
        authors.extend(reactions.values().flatten().map(|r| r.account.clone()));
        authors.sort();
        authors.dedup();
        let mut machines: Vec<String> =
            ms.iter().filter(|m| m.ref_pane.is_some()).filter_map(|m| m.ref_machine.clone()).collect();
        machines.sort();
        machines.dedup();
        // The agents' machines, for their names only.
        let mut named = machines.clone();
        named.extend(ms.iter().filter_map(|m| m.agent.clone()));
        named.extend(reactions.values().flatten().filter(|r| !r.agent.is_empty()).map(|r| r.agent.clone()));
        named.sort();
        named.dedup();
        let (accounts, daemons, reach) = tokio::try_join!(
            app.cache.accounts(hub, &authors),
            crate::grants::agent_names(app, &named),
            app.cache.may_reach(hub, reader, &machines),
        )?;
        let agents = ms
            .iter()
            .filter_map(|m| m.agent.clone())
            .chain(reactions.values().flatten().filter(|r| !r.agent.is_empty()).map(|r| r.agent.clone()))
            .map(|a| {
                let name = daemons.get(&a).map(|d| d.name.clone()).unwrap_or_default();
                (a, name)
            })
            .collect();
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
        Ok(Self { names, machines, agents, reactions, files, marks, reader: (reader.to_owned(), String::new()) })
    }

    /// Read as the agent on `machine` (for its owner `reader`).
    pub(super) fn for_agent(mut self, machine: &str) -> Self {
        self.reader.1 = machine.to_owned();
        self
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
            agent: m.agent.as_ref().map(|a| wire::ChatAgent {
                machine: a.clone(),
                machine_name: self.agents.get(a).cloned().unwrap_or_default(),
            }),
            via: m.via.clone(),
            reactions: self.reacted(m),
            // An agent reads them at its machine's address.
            files: self
                .files
                .get(&(m.room.clone(), m.seq))
                .into_iter()
                .flatten()
                .map(|f| files::shown(f, !self.reader.1.is_empty()))
                .collect(),
            pinned: self.marks.pinned(m),
            system: self.marks.system(m),
        }
    }

    /// A message's reactions, grouped by emoji in the order each was
    /// first made.
    fn reacted(&self, m: &Message) -> Vec<ChatReaction> {
        let mut out: Vec<ChatReaction> = Vec::new();
        for r in self.reactions.get(&(m.room.clone(), m.seq)).into_iter().flatten() {
            let at = match out.iter().position(|x| x.emoji == r.emoji) {
                Some(i) => i,
                None => {
                    out.push(ChatReaction { emoji: r.emoji.clone(), count: 0, mine: false, who: vec![] });
                    out.len() - 1
                }
            };
            let c = &mut out[at];
            c.count += 1;
            c.mine |= (r.account.as_str(), r.agent.as_str()) == (self.reader.0.as_str(), self.reader.1.as_str());
            if c.who.len() < wire::MAX_REACTORS_SHOWN {
                c.who.push(ChatReactor {
                    name: self.names.get(&r.account).cloned().unwrap_or_default(),
                    account: r.account.clone(),
                    agent: (!r.agent.is_empty()).then(|| wire::ChatAgent {
                        machine: r.agent.clone(),
                        machine_name: self.agents.get(&r.agent).cloned().unwrap_or_default(),
                    }),
                });
            }
        }
        out
    }
}

/// The reactions on `ms`, by (room, seq): one query a room, over the
/// numbers it spans.
fn reactions_of(db: &Db, ms: &[&Message]) -> anyhow::Result<HashMap<(String, i64), Vec<Reaction>>> {
    let mut span: HashMap<&str, (i64, i64)> = HashMap::new();
    for m in ms.iter().filter(|m| m.deleted.is_none() && m.imported.is_none()) {
        let e = span.entry(m.room.as_str()).or_insert((m.seq, m.seq));
        *e = (e.0.min(m.seq), e.1.max(m.seq));
    }
    let mut out: HashMap<(String, i64), Vec<Reaction>> = HashMap::new();
    for (room, (from, to)) in span {
        for r in db.chat_reactions(room, from, to)? {
            out.entry((r.room.clone(), r.seq)).or_default().push(r);
        }
    }
    Ok(out)
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
pub(crate) async fn mentions(
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

/// The agents a text mentions (M198): an `@word` that named nobody, but
/// names a machine whose live `threads.read` grant reaches `room` (one
/// that can read the mention; [`room_agents`]), by its name: `@geek`, or
/// `@geek's agent` (the word stops at the `'`). Each is stored beside the
/// accounts as `machine:<id>`, and its word leaves `unreached`. Nobody is
/// pushed for one and nothing wakes an agent: its agents read it over MCP.
/// An agent doesn't mention its own machine.
async fn agent_mentions(
    app: &Platform,
    facts: &Facts,
    room: &Room,
    unreached: &mut Vec<String>,
    by: Option<&AgentPrincipal>,
) -> anyhow::Result<Vec<String>> {
    if unreached.is_empty() {
        return Ok(vec![]);
    }
    let mut agents = room_agents(app, facts, room).await?;
    agents.retain(|a| a.powers.iter().any(|p| p == THREADS_READ));
    let named = |a: &ChatRoomAgent, w: &str| !a.machine_name.is_empty() && a.machine_name.to_lowercase() == w;
    let mut out: Vec<String> = Vec::new();
    for a in &agents {
        let mention = format!("{}{}", wire::AGENT_MENTION, a.machine);
        if unreached.iter().any(|w| named(a, w)) && by.is_none_or(|b| b.machine != a.machine) && !out.contains(&mention)
        {
            out.push(mention);
        }
    }
    unreached.retain(|w| !agents.iter().any(|a| named(a, w)));
    Ok(out)
}

// ---------------------------------------------------------------- live

/// One event for open pages, numbered; `to` limits it to one account (its
/// read marks).
pub struct Out {
    n: u64,
    ev: ChatLive,
    to: Option<String>,
}

/// What's open on this machine, in memory: the live streams, and when each
/// person was last pushed for each room.
///
/// **The events aren't kept here (M144).** Each is a row of `chat_events`,
/// numbered across every machine the platform runs on ([`Hub::publish`]).
/// A machine sends its streams the rows after the last one it sent
/// ([`Hub::catch_up`]): when it publishes one itself, when another machine
/// wakes it (`pg_notify`, heard by [`start`]), and every
/// [`CATCH_UP`] besides. So a stream hears what was posted through any
/// machine, and a page that dropped resumes on whichever machine it
/// reaches next (`Last-Event-ID` is `c-<seq>`), from the database, as long
/// as the rows it missed are still kept
/// ([`CHAT_EVENTS_KEPT`](crate::db::chat::CHAT_EVENTS_KEPT)); otherwise it
/// resyncs.
pub struct Hub {
    /// The newest event sent to this machine's streams.
    last: AtomicU64,
    /// Held while events are read in, so each goes out once, in order.
    reading: Mutex<()>,
    tx: broadcast::Sender<Arc<Out>>,
    streams: Mutex<HashMap<String, StreamInfo>>,
    /// Moves when who sees what may have changed (a roster, a reader list,
    /// a flag, read from the hub's change feed): each stream re-reads its
    /// rooms.
    generation: AtomicU64,
    pushed: Mutex<HashMap<(String, String), u64>>,
    /// When each person last pinged typing in each room ([`typing`]).
    typed: Mutex<HashMap<(String, String), u64>>,
}

/// Event ids are `c-<seq>`, `seq` being the event's row in `chat_events`
/// (M144), the same on every machine and across restarts. An id from
/// before (`<epoch>-<n>`, numbered by one process) resyncs.
const EPOCH: &str = "c";

/// How often a machine reads events in even when nothing woke it: what a
/// lost notification can cost.
pub const CATCH_UP: Duration = Duration::from_secs(5);

/// Events read in at a time.
const EVENTS_PAGE: u32 = 500;

struct StreamInfo {
    account: String,
    open: Option<String>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            last: AtomicU64::new(0),
            reading: Mutex::new(()),
            tx: broadcast::channel(RING).0,
            streams: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            pushed: Mutex::new(HashMap::new()),
            typed: Mutex::new(HashMap::new()),
        }
    }
}

impl Hub {
    /// Start after the newest event the database has: a machine that
    /// starts sends its streams only what comes after.
    pub fn start_at(&self, db: &Db) {
        match db.chat_event_bounds() {
            Ok((_, newest)) => self.last.store(newest, Ordering::SeqCst),
            Err(e) => tracing::warn!(error = %e, "chat: reading the newest live event"),
        }
    }

    /// Keep `ev` (for `to` only, if it's someone's) for every machine's
    /// streams, then send this machine's what's new.
    fn publish(&self, db: &Db, ev: ChatLive, to: Option<String>) {
        let body = data(&ev);
        let e = NewChatEvent { kind: "live", body: &body, to: to.as_deref(), at: now_ms() };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a live event");
        }
        self.catch_up(db);
    }

    /// Send this machine's streams every event after the last one sent,
    /// in order: what any machine published since.
    pub fn catch_up(&self, db: &Db) {
        let _one = self.reading.lock().unwrap();
        loop {
            let page = match db.chat_events_after(self.last.load(Ordering::SeqCst), EVENTS_PAGE) {
                Ok(p) => p,
                Err(e) => return tracing::warn!(error = %e, "chat: reading live events"),
            };
            for e in &page {
                match e.kind.as_str() {
                    "live" => match serde_json::from_str::<ChatLive>(&e.body) {
                        Ok(ev) => {
                            if matches!(ev, ChatLive::Room { .. }) {
                                self.changed();
                            }
                            let _ = self.tx.send(Arc::new(Out { n: e.seq, ev, to: e.to.clone() }));
                        }
                        Err(err) => {
                            tracing::warn!(seq = e.seq, error = %err, "chat: a live event this build can't read")
                        }
                    },
                    // A ping of #111: sent while it's fresh, not replayed
                    // after (a machine that was away, a stream that resumes).
                    "typing" => {
                        if now_ms().saturating_sub(e.at) <= wire::TYPING_MS
                            && let Ok(ev) = serde_json::from_str::<ChatLive>(&e.body)
                        {
                            // `n: 0`: not numbered, so no stream keeps its place by it.
                            let _ = self.tx.send(Arc::new(Out { n: 0, ev, to: None }));
                        }
                    }
                    _ => self.changed(),
                }
                self.last.store(e.seq, Ordering::SeqCst);
            }
            if page.len() < EVENTS_PAGE as usize {
                return;
            }
        }
    }

    /// Who sees what may have changed, here: streams re-read their rooms.
    /// The hub's change feed moves it on every machine, each following it.
    pub fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Who sees what may have changed, through this machine (a reader list,
    /// an import): every machine's streams re-read their rooms.
    pub fn changed_everywhere(&self, db: &Db) {
        self.changed();
        let e = NewChatEvent { kind: "changed", body: "", to: None, at: now_ms() };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a live event");
        }
        self.catch_up(db);
    }

    /// A room appeared, changed or went.
    pub fn room(&self, db: &Db, room: &str) {
        self.changed();
        self.publish(db, ChatLive::Room { room: room.to_owned() }, None);
    }

    /// The object `target`'s thread is on changed (M203): pages showing
    /// it read it again. Nothing when nobody has made its thread.
    pub fn object_changed(&self, db: &Db, scope: &str, target: &str) {
        match db.chat_room_on(scope, target) {
            Ok(Some(room)) => {
                self.publish(db, ChatLive::Object { room: room.id, target: target.to_owned() }, None);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "chat: finding an object's thread"),
        }
    }

    fn id(&self, n: u64) -> String {
        format!("{EPOCH}-{n}")
    }

    /// The events a page that last had `id` missed, from the database, to
    /// send again; `None` when they can't all be: an id from before M144
    /// or from another database, or more missed than are kept or than
    /// [`RING`].
    fn missed(&self, db: &Db, id: &str) -> Option<(u64, Vec<Arc<Out>>)> {
        let n: u64 = id.strip_prefix(EPOCH)?.strip_prefix('-')?.parse().ok()?;
        self.catch_up(db);
        if n > self.last.load(Ordering::SeqCst) {
            return None;
        }
        // Pruned past it: what it missed may be gone.
        let (oldest, _) = db.chat_event_bounds().ok()?;
        if oldest > n + 1 {
            return None;
        }
        let rows = db.chat_events_after(n, RING as u32 + 1).ok()?;
        if rows.len() > RING {
            return None;
        }
        let outs = rows
            .into_iter()
            .filter(|r| r.kind == "live")
            .filter_map(|r| Some(Arc::new(Out { n: r.seq, ev: serde_json::from_str(&r.body).ok()?, to: r.to })))
            .collect();
        Some((n, outs))
    }

    fn has_open(&self, account: &str, room: &str) -> bool {
        self.streams.lock().unwrap().values().any(|s| s.account == account && s.open.as_deref() == Some(room))
    }

    /// Whether `account` may be pushed for `room` now (and note it). Per
    /// machine (M144): with two, a room's pushes are at most one a
    /// [`PUSH_GAP_MS`] from each.
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
        let last = {
            let mut streams = self.app.chat.streams.lock().unwrap();
            let account = streams.remove(&self.id).map(|s| s.account);
            account.filter(|a| !streams.values().any(|s| s.account == *a))
        };
        // The person's last open page here closed: they're gone now, not
        // when the beat goes stale (a page on another machine's streams
        // says so again within [`people::STREAM_BEAT`]).
        if let Some(a) = last
            && let Err(e) = self.app.db.chat_beat(&a, now_ms(), false, true)
        {
            tracing::warn!(error = %e, "chat: noting a page closed");
        }
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
    /// Keeps the person seen while the stream is open, even if the page's
    /// own timers are throttled in a background tab.
    beat: tokio::time::Interval,
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
        // Typing isn't in the log (`n` 0): no id, never replayed, and heard
        // only by another person's page that has the room open.
        if let ChatLive::Typing { room, account, .. } = &out.ev {
            // `open` is the page's word: the room is also one this stream sees.
            let hears = *account != self.account
                && self.rooms.contains(room)
                && self.app().chat.stream_has_open(&self.reg.id, room);
            return hears.then(|| Event::default().data(data(&out.ev)));
        }
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
            ChatLive::Message { room, .. }
            | ChatLive::Changed { room, .. }
            | ChatLive::Reacted { room, .. }
            | ChatLive::Pinned { room }
            | ChatLive::Read { room, .. }
            | ChatLive::Object { room, .. } => self.rooms.contains(room),
            // Sidebar and Saved are told to their account only (`out.to`).
            ChatLive::Hello { .. }
            | ChatLive::Sidebar {}
            | ChatLive::Saved {}
            | ChatLive::Resync {}
            | ChatLive::Typing { .. } => true,
        };
        ok.then(|| Event::default().id(self.app().chat.id(out.n)).data(data(&out.ev)))
    }

    async fn resync(&mut self) -> Event {
        self.seen = self.app().chat.last.load(Ordering::SeqCst);
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
    // A page that opened is a person who's here: seen now, active at the
    // page's first beat (it says whether it's in use).
    if let Err(e) = app.db.chat_beat(&s.account, now_ms(), false, false) {
        tracing::warn!(error = %e, "chat: noting a page opened");
    }
    let last = headers.get("last-event-id").and_then(|v| v.to_str().ok()).map(str::to_owned);
    // Subscribe first, then read what was missed from the database: what's
    // published in between is in both, and `seen` drops the second copy. A
    // new stream starts at what was sent before it subscribed.
    let start = app.chat.last.load(Ordering::SeqCst);
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
        beat: tokio::time::interval_at(tokio::time::Instant::now() + people::STREAM_BEAT, people::STREAM_BEAT),
    };
    live.refresh().await;
    live.pending.push_back(Event::default().data(data(&ChatLive::Hello { stream: id })));
    match last.map(|id| app.chat.missed(&app.db, &id)) {
        Some(Some((n, outs))) => {
            live.seen = n;
            for o in outs {
                if let Some(ev) = live.deliver(&o).await {
                    live.pending.push_back(ev);
                }
            }
        }
        Some(None) => {
            let ev = live.resync().await;
            live.pending.push_back(ev);
        }
        None => live.seen = start,
    }
    let stream = futures_util::stream::unfold(live, |mut live| async move {
        loop {
            if let Some(ev) = live.pending.pop_front() {
                return Some((Ok(ev), live));
            }
            let got = tokio::select! {
                got = live.rx.recv() => got,
                _ = live.beat.tick() => {
                    if let Err(e) = live.app().db.chat_beat(&live.account, now_ms(), false, false) {
                        tracing::warn!(error = %e, "chat: noting a page is still open");
                    }
                    continue;
                }
            };
            match got {
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
    // The page's marks and unread counts: one statement each, for every
    // room (a round trip each on Postgres, where one per room added up).
    let ids: Vec<&str> = seen.iter().map(|(r, _)| r.id.as_str()).collect();
    let marks = app.db.chat_marks(&ids, &s.account)?;
    let mark = |id: &str| marks.get(id).map(|m| (m.0, m.1)).unwrap_or((0, false));
    let asks: Vec<UnreadAsk<'_>> =
        seen.iter().map(|(r, from)| UnreadAsk { room: &r.id, read: mark(&r.id).0, from: *from }).collect();
    let counts = app.db.chat_unread_in(&s.account, &asks)?;
    let extras = pins::Extras::of(&app.db, &seen.iter().map(|(r, _)| r).collect::<Vec<_>>())?;
    let mut rooms = Vec::new();
    for (r, _) in &seen {
        let n = counts.get(&r.id).copied().unwrap_or((0, 0));
        rooms.push(listed_as(&app, &facts, &s.account, r, mark(&r.id), n, &extras)?);
    }
    let own = marks.iter().filter_map(|(id, m)| m.2.map(|l| (id.clone(), l))).collect();
    with_levels(&app, &s.account, &mut rooms, Some(own))?;
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
    // The team's default project (made the first time).
    let project = crate::projects::team_project(&app, &team).await?;
    app.db.add_chat_room(&room, &[], project.as_deref()).map_err(|e| {
        if e.is::<Taken>() { err(StatusCode::CONFLICT, "there's a channel by that name") } else { e.into() }
    })?;
    app.chat.room(&app.db, &room.id);
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
    app.chat.room(&app.db, &room.id);
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
        wire::ChatTargetKind::Object => return open_object(&app, &s.account, &b.target).await,
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
        Ok(()) => app.chat.room(&app.db, &room.id),
        // Someone made it just now.
        Err(e) if e.is::<Taken>() => {
            let r = app.db.chat_room_for(kind, &b.machine, &b.target)?.ok_or_else(nothing)?;
            return Ok(Json(listed(&app, &facts, &s.account, &r, from)?));
        }
        Err(e) => return Err(e.into()),
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// The thread on an object (M196, made the first time), for a member of
/// its project who may read it ([`objects`]).
async fn open_object(app: &Platform, account: &str, target: &str) -> J<ChatRoom> {
    let nothing = || err(StatusCode::NOT_FOUND, "no such object");
    let obj = objects::resolve(app, target)?;
    let scope = objects::project_scope(&obj.project);
    let room = Room {
        id: new_id("ob_"),
        scope: scope.clone(),
        kind: "object".into(),
        name: obj.name.clone(),
        machine: None,
        target: Some(target.to_owned()),
        created_by: account.to_owned(),
        created: now_ms(),
        archived: None,
        last_seq: 0,
    };
    let facts = Facts::of(app, Some(account), &[&room]).await?;
    objects::check_open(app, &facts, account, target, &obj).await?;
    // There already in this project. A repo relinked to another project
    // gets a room of its own there; the old one stays the old project's,
    // which no longer sees it (`objects::sight`).
    let there = |r: Room| async move {
        let facts = Facts::of(app, Some(account), &[&r]).await?;
        let from = sight(app, &facts, account, &r).await?.ok_or_else(nothing)?;
        Ok::<_, ApiError>(Json(listed(app, &facts, account, &r, from)?))
    };
    if let Some(r) = app.db.chat_room_on(&scope, target)? {
        return there(r).await;
    }
    // No `MAX_THREADS` here: the objects bound their rooms, one each, and a
    // big linked repo would reach any cap on its own.
    app.limits.check_account(CHAT_ROOMS, account)?;
    match app.db.add_chat_room(&room, &[], Some(&obj.project)) {
        Ok(()) => app.chat.room(&app.db, &room.id),
        // Someone made it just now.
        Err(e) if e.is::<Taken>() => return there(app.db.chat_room_on(&scope, target)?.ok_or_else(nothing)?).await,
        Err(e) => return Err(e.into()),
    }
    Ok(Json(listed(app, &facts, account, &room, 0)?))
}

/// `GET /api/chat/rooms/{id}`: the room, and who sees it.
pub async fn room(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<ChatRoomDetail> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    let audience = audience(&app, &room).await?;
    let names = app.cache.accounts(&*app.hub, &audience).await?;
    let people = audience
        .into_iter()
        .map(|a| ChatPerson { name: names.get(&a).map(|x| x.name.clone()).unwrap_or_default(), account: a, dnd: false })
        .collect();
    let mut people: Vec<ChatPerson> = people;
    with_dnd(&app, &mut people)?;
    let agents = room_agents(&app, &facts, &room).await?;
    Ok(Json(ChatRoomDetail { room: listed(&app, &facts, &s.account, &room, from)?, people, agents }))
}

// ---------------------------------------------------------------- agents

/// Where an agent's grant has to reach for `room` (M197): a channel's
/// team's default project, an object thread's project (M196: an issue's, a
/// mirrored PR's), or `owner`'s own account for a DM. `None`: agents don't
/// come here (pane and session threads, archives).
fn agent_scope(app: &Platform, room: &Room, owner: &str) -> anyhow::Result<Option<AgentScope>> {
    if room.kind == "object" {
        return Ok(objects::project_of(room).map(|p| AgentScope::Project(p.to_owned())));
    }
    Ok(match (room.kind.as_str(), room.scope.strip_prefix("team:")) {
        ("channel", Some(team)) => app.db.team_default_project(team)?.map(AgentScope::Project),
        ("dm", Some(_)) => Some(AgentScope::Account(owner.to_owned())),
        _ => None,
    })
}

/// The room `id` for an agent on `machine` using `power` there
/// (`threads.read` or `threads.post`): it has to be a room its owner sees
/// (404 otherwise, as for the owner), of a kind agents come to, with a live
/// grant over its scope ([`agent_may`], whose refusal says how to grant
/// one). Chat has to be on for the owner ([`gate`]). The room, the owner's
/// floor in it, and who the agent is. The MCP thread routes (M198) start
/// here, and post with [`post_as`].
pub async fn agent_room(
    app: &Platform,
    machine: &Machine,
    power: &str,
    id: &str,
) -> Result<(Room, u64, AgentPrincipal), ApiError> {
    gate(app, &machine.account).await?;
    let (room, from, _) = room_for(app, &machine.account, id).await?;
    let Some(scope) = agent_scope(app, &room, &machine.account)? else {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            AGENT_UNGRANTED,
            "agents read and post in team channels, DMs and object threads, not here",
        ));
    };
    let agent = agent_may(app, machine, power, &scope).await?;
    Ok((room, from, agent))
}

/// [`agent_room`] for the thread on an object (M196: a native issue, a
/// mirrored PR or issue), by the object's id: made now if nobody has
/// opened it yet, as when its owner opens it. The grant over the object's
/// project is asked first, so an agent without one makes nothing; then the
/// owner has to be able to open it ([`objects::check_open`]: a member of its
/// project, and refused on a private repo's item as everyone is until
/// M165). Chat has to be on for the owner ([`gate`]), before anything is
/// made. How an agent comments on an issue (M198, M200).
pub async fn agent_object_room(
    app: &Platform,
    machine: &Machine,
    power: &str,
    target: &str,
) -> Result<(Room, u64, AgentPrincipal), ApiError> {
    gate(app, &machine.account).await?;
    let obj = objects::resolve(app, target)?;
    let scope = AgentScope::Project(obj.project.clone());
    // Outside its owner's projects it isn't there, as for the owner.
    if crate::grants::owner_reads(app, &machine.account, &scope, false).await?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "no such object"));
    }
    agent_may(app, machine, power, &scope).await?;
    let room = open_object(app, &machine.account, target).await?.0;
    agent_room(app, machine, power, &room.id).await
}

/// The thread on an object in `project` for a hud box posting `author`'s
/// review (M237, [`crate::boxes::post`]): its room's id, made now if nobody
/// has opened it yet. An object outside the box's project is no such
/// object, as for anyone it isn't there for; then `author` has to be able
/// to open it themselves ([`objects::check_open`]), with chat on for them
/// ([`gate`]).
pub(crate) async fn box_object_room(
    app: &Platform,
    author: &str,
    project: &str,
    target: &str,
) -> Result<String, ApiError> {
    gate(app, author).await?;
    let obj = objects::resolve(app, target)?;
    if obj.project != project {
        return Err(err(StatusCode::NOT_FOUND, "no such object"));
    }
    Ok(open_object(app, author, target).await?.0.id)
}

/// The agents a live grant lets read or post in `room` (M197), as everyone
/// in it is told: thread grants over its scope (its project; for a DM, each
/// of its people's accounts), and on a native issue's thread `issues.write`
/// too (an agent that may edit or close the issue), whose owner sees the
/// room now and whose machine is still theirs. By owner, then machine.
async fn room_agents(app: &Platform, facts: &Facts, room: &Room) -> anyhow::Result<Vec<ChatRoomAgent>> {
    let mut scopes: Vec<(&str, String)> = vec![];
    if room.kind == "dm" {
        scopes.extend(facts.members(app, &room.id)?.into_iter().map(|a| ("account", a)));
    } else if let Some(AgentScope::Project(p)) = agent_scope(app, room, "")? {
        scopes.push(("project", p));
    }
    let scopes: Vec<(&str, &str)> = scopes.iter().map(|(k, id)| (*k, id.as_str())).collect();
    let on_issue = room.kind == "object" && room.target.as_deref().is_some_and(|t| t.starts_with("iss_"));
    let mut by: std::collections::BTreeMap<(String, String), Vec<String>> = Default::default();
    for g in app.db.agent_grants_over(&scopes)? {
        if g.power == THREADS_READ || g.power == THREADS_POST || (on_issue && g.power == ISSUES_WRITE) {
            by.entry((g.granted_by, g.subject_id)).or_default().push(g.power);
        }
    }
    if by.is_empty() {
        return Ok(vec![]);
    }
    let mut machines: Vec<String> = by.keys().map(|(_, m)| m.clone()).collect();
    machines.sort();
    machines.dedup();
    let mut owners: Vec<String> = by.keys().map(|(o, _)| o.clone()).collect();
    owners.dedup();
    let (daemons, names) =
        tokio::try_join!(crate::grants::agent_names(app, &machines), app.cache.accounts(&*app.hub, &owners))?;
    let mut out = vec![];
    for ((owner, machine), mut powers) in by {
        let Some(d) = daemons.get(&machine).filter(|d| d.account == owner) else { continue };
        if sight(app, facts, &owner, room).await?.is_none() {
            continue;
        }
        powers.sort();
        out.push(ChatRoomAgent {
            owner_name: names.get(&owner).map(|a| a.name.clone()).unwrap_or_default(),
            owner,
            machine,
            machine_name: d.name.clone(),
            powers,
        });
    }
    Ok(out)
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
    app.chat.room(&app.db, &room.id);
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
    // The old switch, kept for callers that have it: muting is the
    // `nothing` level, and unmuting goes back to the default.
    app.db.set_chat_level(&room.id, &s.account, b.muted.then_some(NotifyLevel::Nothing))?;
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// `POST /api/chat/rooms/{id}/level`: what I'm notified of in this room,
/// or (none) my default for rooms like it.
pub async fn level(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatLevel>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    app.db.set_chat_level(&room.id, &s.account, b.level)?;
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
    Ok(Json(post_as(&app, &s.account, None, &id, b).await?))
}

/// Post in room `id` as `author`, or as an agent on one of the author's
/// machines (M197: the caller has asked [`agent_room`] for
/// `threads.post`), which the message keeps beside its author.
pub async fn post_as(
    app: &Arc<Platform>,
    author: &str,
    agent: Option<&AgentPrincipal>,
    id: &str,
    b: ChatPost,
) -> Result<ChatPosted, ApiError> {
    post_full(app, author, agent, None, id, b).await
}

/// [`post_as`], and `via`: the product that posted it for `author` (M237,
/// a hud box's review: [`crate::boxes::post`]), which the message keeps
/// beside its author as the agent's machine is kept.
pub(crate) async fn post_full(
    app: &Platform,
    author: &str,
    agent: Option<&AgentPrincipal>,
    via: Option<&str>,
    id: &str,
    b: ChatPost,
) -> Result<ChatPosted, ApiError> {
    let s = Person { account: author.to_owned() };
    gate(app, &s.account).await?;
    let (room, _, facts) = room_for(app, &s.account, id).await?;
    if watching(app, &facts, &s.account, &room)? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            WATCHING,
            "you're watching this session: you read its threads, and its owner can let you post",
        ));
    }
    if !writable(app, &facts, &room, &s.account)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let text = b.text.trim();
    let quote = b.quote.as_deref().filter(|q| !q.trim().is_empty());
    let attach = files::to_attach(app, &room, &s.account, &b.files)?;
    let files_label = files::label(&attach);
    if text.is_empty() && quote.is_none() && attach.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "say something"));
    }
    sizes(text, quote)?;
    // You name only panes you may reach: as the cache has it, and asked
    // again before refusing.
    if let Some(p) = &b.pane {
        let machine = std::slice::from_ref(&p.machine);
        let reach = |r: HashSet<String>| r.contains(&p.machine);
        if !valid_target(&p.pane)
            || (!reach(app.cache.may_reach(&*app.hub, &s.account, machine).await?)
                && !reach(app.cache.may_reach_now(&*app.hub, &s.account, machine).await?))
        {
            return Err(err(StatusCode::FORBIDDEN, "you can't see that pane"));
        }
    }
    if app.db.chat_bytes(&room.scope)? + (text.len() + quote.map_or(0, str::len)) as u64 > MAX_SCOPE_BYTES {
        return Err(err(StatusCode::INSUFFICIENT_STORAGE, "this team's chat is full (1 GB): delete some first"));
    }
    app.limits.check_account_per_minute(CHAT_POSTS, &s.account)?;
    app.limits.check_all_per_minute(ALL_CHAT_POSTS)?;
    let people = audience(app, &room).await?;
    let (mut named, mut unreached) = mentions(app, text, &people, &s.account).await?;
    // `@channel`, `@here` and keywords are mentions too (#114): stored
    // beside the named, so counts and Activity see them.
    let extra = alerts::extra(app, &facts, &room, text, &people, &s.account, &named).await?;
    unreached.retain(|w| !matches!(w.as_str(), alerts::CHANNEL | alerts::HERE));
    for a in &extra.broadcast {
        if !named.contains(a) {
            named.push(a.clone());
        }
    }
    named.extend(extra.keyword.iter().cloned());
    let mut stored = named.clone();
    stored.extend(agent_mentions(app, &facts, &room, &mut unreached, agent).await?);
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
            mentions: &stored,
            agent: agent.map(|a| a.machine.as_str()),
            via,
        },
    )?;
    if !attach.is_empty() {
        let ids: Vec<String> = attach.into_iter().map(|f| f.id).collect();
        app.db.attach_chat_files(&room.id, seq, &s.account, &ids)?;
    }
    app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq }, None);
    let pushed = push(
        app,
        &facts,
        &room,
        (&s.account, agent, via),
        seq,
        if text.is_empty() && quote.is_none() {
            &files_label
        } else {
            text.is_empty().then_some(quote).flatten().unwrap_or(text)
        },
        &named,
        &extra.keyword,
        &people,
    )
    .await?;
    Ok(ChatPosted { seq, mentions: stored, unreached, pushed })
}

/// How chat's pushes go: to every subscription the account has, with the
/// hub's TTL, as control's own notices always have (M122).
const PUSHES: PushOptions = PushOptions { ttl: None, any_subscription: true };

/// Push a new message to whom it's for (S41 q4): mentions, the other
/// person in a DM, and (#113) whoever set the room, or their default for
/// rooms like it, to every message; each by their level for the room
/// ([`crate::notify`]), and nobody while they've paused notifications
/// (#115). Not the author, nobody with the room open or muted, and
/// nobody twice a minute for one room. The hub sends them
/// ([`HubPort::push_with`](crate::HubPort::push_with)), in the
/// background: a failure is only logged.
///
/// A mention in the thread on a native issue or a mirrored item is an
/// inbox entry instead (M202), and the inbox pushes it, with its limits:
/// one path, so one push ([`crate::inbox::thread_mentioned`]).
#[allow(clippy::too_many_arguments)]
async fn push(
    app: &Platform,
    facts: &Facts,
    room: &Room,
    (author, agent, via): (&str, Option<&AgentPrincipal>, Option<&str>),
    seq: i64,
    text: &str,
    named: &[String],
    keyed: &[String],
    people: &[String],
) -> anyhow::Result<Vec<ChatPushed>> {
    let inboxed = room.kind == "object" && room.target.as_deref().and_then(crate::inbox::thread_reason).is_some();
    let mut to: Vec<(String, ChatPushWhy)> = named
        .iter()
        .filter(|_| !inboxed)
        .map(|a| (a.clone(), if keyed.contains(a) { ChatPushWhy::Keyword } else { ChatPushWhy::Mention }))
        .collect();
    if room.kind == "dm" {
        for a in people {
            if a != author && !named.contains(a) {
                to.push((a.clone(), ChatPushWhy::Dm));
            }
        }
    }
    // Everyone else in a channel who asked for every message there (#113):
    // by their level for the room, else their default.
    if !inboxed && room.kind != "dm" {
        for a in people {
            if a != author && !named.contains(a) {
                to.push((a.clone(), ChatPushWhy::Message));
            }
        }
    }
    // Their levels and do not disturb (#113, #115). One statement each for
    // everyone, whatever the channel's size.
    if !to.is_empty() {
        let accounts: Vec<String> = to.iter().map(|(a, _)| a.clone()).collect();
        let own = app.db.chat_levels_in(&room.id, &accounts)?;
        let prefs = app.db.notify_prefs_of(&accounts)?;
        let (now, fresh) = (now_ms(), crate::db::notify::Prefs::default());
        to.retain(|(a, why)| {
            // The sender's "notify anyway" isn't on a post yet: only the
            // decision knows it.
            crate::notify::chat_push(prefs.get(a).unwrap_or(&fresh), &room.kind, own.get(a).copied(), *why, now, false)
        });
    }
    if to.is_empty() && !(inboxed && !named.is_empty()) {
        return Ok(vec![]);
    }
    let me = match agent {
        Some(a) => format!("{}'s agent on {}", name_of(app, author).await?, a.machine_name),
        None => name_of(app, author).await?,
    };
    // Posted for them by a product (M237): it says so, here as on the page.
    let me = match via {
        Some(v) => format!("{me} via {v}"),
        None => me,
    };
    if inboxed {
        let mut quiet = Vec::new();
        for a in named {
            if app.db.chat_mark(&room.id, a)?.1 || app.chat.has_open(a, &room.id) {
                quiet.push(a.clone());
            }
        }
        let named: Vec<String> = named.iter().filter(|a| *a != author).cloned().collect();
        let pushed = crate::inbox::thread_mentioned(app, room, &me, (seq, text), &named, &quiet)?;
        return Ok(pushed.into_iter().map(|account| ChatPushed { account, why: ChatPushWhy::Mention }).collect());
    }
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
            ChatPushWhy::Keyword => format!("{me} wrote a word you watch in {place}"),
            ChatPushWhy::Dm => format!("{me} wrote to you"),
            ChatPushWhy::Message => format!("{me} in {place}"),
        };
        let msg = json!({
            "chat": { "room": room.id, "seq": seq },
            "title": title, "body": body, "tag": format!("chat-{}", room.id), "control": true,
        });
        sends.push((account.clone(), msg));
        out.push(ChatPushed { account, why });
    }
    let hub = app.hub.clone();
    tokio::spawn(async move {
        for (account, msg) in sends {
            if let Err(e) = hub.push_with(&account, &msg, PUSHES).await {
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
    if pins::is_system(&app, &room, seq)? {
        return Err(err(StatusCode::FORBIDDEN, "a note of what happened in the room isn't edited"));
    }
    if m.author != s.account || m.deleted.is_some() {
        return Err(err(StatusCode::FORBIDDEN, "only its author edits a message"));
    }
    if m.agent.is_some() {
        return Err(err(StatusCode::FORBIDDEN, "an agent's message isn't edited by hand: delete it instead"));
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
    let people = audience(&app, &room).await?;
    let (mut named, mut unreached) = mentions(&app, text, &people, &s.account).await?;
    let extra = alerts::extra(&app, &facts, &room, text, &people, &s.account, &named).await?;
    unreached.retain(|w| !matches!(w.as_str(), alerts::CHANNEL | alerts::HERE));
    named.extend(extra.broadcast.iter().filter(|a| !named.contains(a)).cloned().collect::<Vec<_>>());
    named.extend(extra.keyword);
    named.extend(agent_mentions(&app, &facts, &room, &mut unreached, None).await?);
    app.db.edit_chat_message(&room.id, seq, text, &named, now_ms())?;
    app.chat.publish(&app.db, ChatLive::Changed { room: room.id.clone(), seq }, None);
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
    if pins::is_system(&app, &room, seq)? {
        return Err(err(StatusCode::FORBIDDEN, "a note of what happened in the room isn't deleted"));
    }
    let may = m.author == s.account
        || match room.scope.split_once(':') {
            Some(("team", team)) => facts.role(team, &s.account) == Some(TeamRole::Owner),
            Some(("machine", machine)) => facts.owner(machine) == Some(s.account.as_str()),
            Some(("project", _)) => objects::owns(&facts, &room, &s.account),
            _ => false,
        };
    if !may {
        return Err(err(StatusCode::FORBIDDEN, "only its author, or an owner, deletes a message"));
    }
    if m.deleted.is_none() {
        let was_pinned = !app.db.chat_pinned_among(&room.id, seq, seq)?.is_empty();
        app.db.delete_chat_message(&room.id, seq, now_ms())?;
        app.chat.publish(&app.db, ChatLive::Changed { room: room.id.clone(), seq }, None);
        if was_pinned {
            app.chat.publish(&app.db, ChatLive::Pinned { room: room.id.clone() }, None);
        }
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
        app.chat.publish(&app.db, ChatLive::Read { room: room.id.clone(), seq }, Some(s.account.clone()));
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

// ---------------------------------------------------------------- search

/// What a search box's words ask for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Asked {
    /// The words a message has (`mea*`: a word starting with `mea`), as
    /// text: no search syntax is the person's to type
    /// (`db::chat::fts5_query`, `db::chat::tsquery`).
    words: Vec<Word>,
    channel: Option<String>,
    from: Option<String>,
    has_quote: bool,
}

fn parse_search(q: &str) -> Asked {
    let mut a = Asked::default();
    for tok in q.split_whitespace() {
        if let Some(c) = tok.strip_prefix("in:") {
            a.channel = Some(c.trim_start_matches('#').to_lowercase());
        } else if let Some(f) = tok.strip_prefix("from:") {
            a.from = Some(f.trim_start_matches('@').to_lowercase());
        } else if tok == "has:quote" {
            a.has_quote = true;
        } else {
            let prefix = tok.ends_with('*');
            let text: String = tok.trim_end_matches('*').chars().filter(|c| *c != '"').collect();
            if !text.is_empty() {
                a.words.push(Word { text, prefix });
            }
        }
    }
    a
}

/// `GET /api/chat/search?q=&team=`: over the rooms I see (or one team's),
/// newest first.
pub async fn search(State(app): State<Arc<Platform>>, s: Person, Query(q): Query<ChatSearchQuery>) -> J<ChatFound> {
    gate(&app, &s.account).await?;
    let team = q.team.as_deref();
    Ok(Json(search_in(&app, &s.account, &q.q, |r, facts| Ok(in_team(facts, r, team))).await?))
}

/// Whether `r` is one team's: its channel or DM, a thread on its machine,
/// or an object in its project. `None`: any.
fn in_team(facts: &Facts, r: &Room, team: Option<&str>) -> bool {
    match team {
        Some(t) => {
            r.scope == team_scope(t)
                || objects::team_of(facts, r) == Some(t)
                || r.machine.as_deref().is_some_and(|m| facts.daemons.get(m).and_then(|d| d.team.as_deref()) == Some(t))
        }
        None => true,
    }
}

/// A search (`q` as a search box has it) over the rooms `account` sees
/// that `keep` keeps: for a person ([`search`]) or their machine's agents
/// ([`agent_threads::search`]), newest first.
async fn search_in(
    app: &Platform,
    account: &str,
    q: &str,
    mut keep: impl FnMut(&Room, &Facts) -> anyhow::Result<bool>,
) -> Result<ChatFound, ApiError> {
    let asked = parse_search(q);
    if asked.words.is_empty() && asked.channel.is_none() && asked.from.is_none() && !asked.has_quote {
        return Ok(ChatFound { found: vec![] });
    }
    app.limits.check_account(CHAT_SEARCHES, account)?;
    let author = match &asked.from {
        // The login's account, else the one account by that name.
        Some(who) => match app.cache.accounts_called(&*app.hub, who).await?.as_slice() {
            [one] => Some(one.id.clone()),
            _ => return Ok(ChatFound { found: vec![] }),
        },
        None => None,
    };
    let (seen, facts) = rooms_seen(app, account).await?;
    let mut floors = HashMap::new();
    for (r, from) in seen {
        let in_channel = asked.channel.as_ref().is_none_or(|c| r.kind == "channel" && r.name == *c);
        if in_channel && keep(&r, &facts)? {
            floors.insert(r.id, from);
        }
    }
    let rooms: Vec<String> = floors.keys().cloned().collect();
    let hits = app.db.chat_search(
        &rooms,
        &Search { words: &asked.words, author: author.as_deref(), has_quote: asked.has_quote },
        SEARCH_HITS,
    )?;
    let hits: Vec<&Message> = hits.iter().filter(|m| floors.get(&m.room).is_some_and(|from| m.at >= *from)).collect();
    let shower = Shower::of(app, account, &hits).await?;
    let found = hits.into_iter().map(|m| ChatHit { room: m.room.clone(), message: shower.show(m) }).collect();
    Ok(ChatFound { found })
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
    let ids = std::slice::from_ref(&machine);
    // As the cache has it; a machine it hasn't heard of yet (one that just
    // joined) is asked about again before it's refused.
    let row = match app.cache.daemons(&*app.hub, ids).await?.remove(&machine) {
        Some(row) => row,
        None => app
            .cache
            .daemons_now(&*app.hub, ids)
            .await?
            .remove(&machine)
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?,
    };
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
    // An object's thread is its project's members', never a machine's say.
    if lists.iter().any(|t| t.kind == wire::ChatTargetKind::Object) {
        return Err(err(StatusCode::BAD_REQUEST, "a machine names its panes' and sessions' readers only"));
    }
    Ok(())
}

fn target_kind(k: wire::ChatTargetKind) -> &'static str {
    match k {
        wire::ChatTargetKind::Pane => "pane",
        wire::ChatTargetKind::Session => "session",
        // Refused before it gets here (`check_reader_lists`, `daemon_import`).
        wire::ChatTargetKind::Object => "object",
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
    app.chat.changed_everywhere(&app.db);
    for r in rooms {
        app.chat.room(&app.db, &r);
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
                agent: None,
                via: None,
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
        || b.threads.iter().any(|t| !valid_target(&t.target) || t.kind == wire::ChatTargetKind::Object)
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

    app.chat.changed_everywhere(&app.db);
    touched.sort();
    touched.dedup();
    for r in touched {
        app.chat.room(&app.db, &r);
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
pub(crate) fn erase_account(tx: &impl crate::db::Sql, deleted: &crate::Deleted) -> anyhow::Result<Vec<String>> {
    let dms: Vec<String> = tx.query_map(
        "SELECT room FROM chat_members WHERE account = ?1",
        crate::db::sql::params![deleted.account],
        |r| r.get(0),
    )?;
    crate::db::chat::erase_account(tx, &deleted.account, &deleted.machines, &deleted.disbanded)?;
    Ok(dms)
}

/// After an account went: open pages hear of its rooms, and nothing kept of
/// the hub's answers outlives it.
pub(crate) fn account_gone(app: &Platform, rooms: &[String]) {
    app.cache.drop_all();
    for r in rooms {
        app.chat.room(&app.db, r);
    }
    app.chat.changed_everywhere(&app.db);
}

/// Chat's background work: following the hub's change feed
/// ([`cache::start`]), which moves who sees what; sending this machine's
/// streams the events other machines published (M144: woken by their
/// `pg_notify`, else every [`CATCH_UP`]); and pruning the oldest events
/// kept, every minute unless paused (any machine: it's the same delete).
pub fn start(app: &Arc<Platform>) {
    cache::start(app);
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        let mut pruned = Instant::now();
        loop {
            let Some(p) = app.upgrade() else { return };
            p.chat.catch_up(&p.db);
            if let Err(e) = p.db.prune_typing(now_ms()) {
                tracing::warn!(error = %e, "chat: pruning typing");
            }
            // Held with the rest of the background work for a window.
            if !p.is_paused() {
                saved::remind(&p).await;
            }
            if pruned.elapsed() >= Duration::from_secs(60) && !p.is_paused() {
                if let Err(e) = p.db.prune_chat_events() {
                    tracing::warn!(error = %e, "chat: pruning live events");
                }
                if let Err(e) = p.db.sweep_chat_files(now_ms()) {
                    tracing::warn!(error = %e, "chat: sweeping files nobody posted");
                }
                pruned = Instant::now();
            }
            let heard = p.listening.as_ref().map(|l| l.heard.clone());
            drop(p);
            match heard {
                Some(h) => drop(tokio::time::timeout(CATCH_UP, h.notified()).await),
                None => tokio::time::sleep(CATCH_UP).await,
            }
        }
    });
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
        let word = |text: &str, prefix| Word { text: text.into(), prefix };
        let asked = parse_search("mean mea* \"OR\" in:#General from:@Alex has:quote");
        assert_eq!(
            asked,
            Asked {
                words: vec![word("mean", false), word("mea", true), word("OR", false)],
                channel: Some("general".into()),
                from: Some("alex".into()),
                has_quote: true,
            }
        );
        assert_eq!(parse_search("\"\" *"), Asked::default());
        // Search syntax is only ever text: inside a phrase for FTS5, and
        // letters and digits for Postgres.
        let words = parse_search("a NEAR(b) -c main.rs* it's").words;
        assert_eq!(crate::db::chat::fts5_query(&words), "\"a\" \"NEAR(b)\" \"-c\" \"main.rs\"* \"it's\"");
        assert_eq!(
            crate::db::chat::tsquery(&words).unwrap(),
            "'a' & 'NEAR' <-> 'b' & 'c' & 'main' <-> 'rs':* & 'it' <-> 's'"
        );
        assert_eq!(crate::db::chat::tsquery(&parse_search("--").words), None);
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
