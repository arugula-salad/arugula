//! Agents in conversations over MCP (M198, platform step 6): the routes a
//! machine signs to list, read, search and post in the rooms its owner's
//! grants reach (M197): team channels, the owner's DMs and object threads.
//! MCP's `list` kind `threads`, `read_thread`, `search_threads` and
//! `post_thread` call them; the daemon keeps no thread of its own
//! (aru-091).
//!
//! Every call asks again: a room through [`agent_room`] (or
//! [`agent_object_room`] for an object's id), which reads the grant and
//! checks that the owner still sees the room; a list or a search through
//! the machine's live grants ([`grants_of`]), over the rooms its owner sees
//! now ([`rooms_seen`]). So a revoked grant, or an owner who left the team,
//! stops the next call. What an agent posts is the owner's, labelled with
//! the machine ([`post_as`]).
//!
//! Mentions of an agent (`@geek`, [`super::agent_mentions`]) are kept in a
//! message's `mentions` as `machine:<id>`; a read can ask for only those.
//! Nothing here wakes an agent.

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::ApiError;
use arugula_control_wire::chat::{
    self as wire, AgentMessages, AgentMessagesQuery, AgentReach, AgentRoom, AgentRooms, ChatFound, ChatMessage,
    ChatPost, ChatPosted, ChatReact, ChatReactQuery, ChatSearchQuery,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};

use super::{
    Facts, J, MAX_PAGE, PAGE, Shower, agent_object_room, agent_room, agent_scope, files, gate, in_team, listed_as,
    objects, post_as, reactions, rooms_seen, search_in,
};
use crate::{
    Platform,
    auth::Machine,
    db::{
        chat::{Message, Page, Room},
        projects::{THREADS_POST, THREADS_READ},
    },
    grants::{AgentPrincipal, AgentScope, owner_reads},
};

/// Lists and reads a machine's agents make, per hour.
const AGENT_READS: (&str, usize) = ("chat-agent-read", 3_600);

/// The powers a machine's live agent grants give it (M197), by scope
/// (`("project", id)` or `("account", id)`).
type Powers = HashMap<(String, String), Vec<String>>;

fn grants_of(app: &Platform, d: &Machine) -> anyhow::Result<Powers> {
    let mut out: Powers = HashMap::new();
    for g in app.db.agent_grants_of(&d.daemon, &d.account)? {
        out.entry((g.scope_kind, g.scope_id)).or_default().push(g.power);
    }
    Ok(out)
}

fn key(scope: &AgentScope) -> (String, String) {
    match scope {
        AgentScope::Project(p) => ("project".into(), p.clone()),
        AgentScope::Account(a) => ("account".into(), a.clone()),
    }
}

/// The thread powers over a room's scope.
fn thread_powers(powers: &Powers, scope: Option<&AgentScope>) -> Vec<String> {
    scope
        .and_then(|s| powers.get(&key(s)))
        .map(|p| p.iter().filter(|p| *p == THREADS_READ || *p == THREADS_POST).cloned().collect())
        .unwrap_or_default()
}

/// How a message names this machine's agents.
fn mention_of(d: &Machine) -> String {
    format!("{}{}", wire::AGENT_MENTION, d.daemon)
}

/// A room as the machine's agents see it: as its owner lists it, with the
/// agents' powers, and posting only with `threads.post`.
fn shown(
    app: &Platform,
    facts: &Facts,
    owner: &str,
    room: &Room,
    powers: Vec<String>,
    last_mention: Option<i64>,
) -> anyhow::Result<AgentRoom> {
    let r = listed_as(app, facts, owner, room, (0, false), (0, 0))?;
    let writable = r.writable && powers.iter().any(|p| p == THREADS_POST);
    Ok(AgentRoom {
        id: r.id,
        kind: r.kind,
        team: r.team,
        name: r.name,
        target: r.target,
        project: r.project,
        with: r.with,
        last: r.last,
        last_mention,
        powers,
        writable,
    })
}

/// The room `id` names for an agent using `power` there: a room's id, or
/// an object's global id for its thread (made the first time).
async fn room_of(app: &Platform, d: &Machine, power: &str, id: &str) -> Result<(Room, u64, AgentPrincipal), ApiError> {
    if objects::is_object(id) {
        agent_object_room(app, d, power, id).await
    } else {
        agent_room(app, d, power, id).await
    }
}

/// The rooms its owner sees now that the machine's grants reach, with its
/// thread powers in each. A team's default project is asked once a team.
async fn reached(app: &Platform, d: &Machine, powers: &Powers) -> anyhow::Result<(Vec<(Room, Vec<String>)>, Facts)> {
    let (seen, facts) = rooms_seen(app, &d.account).await?;
    let mut defaults: HashMap<String, Option<String>> = HashMap::new();
    let mut out = Vec::new();
    for (r, _) in seen {
        let scope = match (r.kind.as_str(), r.scope.strip_prefix("team:")) {
            ("channel", Some(team)) => {
                if !defaults.contains_key(team) {
                    defaults.insert(team.to_owned(), app.db.team_default_project(team)?);
                }
                defaults[team].clone().map(AgentScope::Project)
            }
            _ => agent_scope(app, &r, &d.account)?,
        };
        let p = thread_powers(powers, scope.as_ref());
        if !p.is_empty() {
            out.push((r, p));
        }
    }
    Ok((out, facts))
}

/// What the machine's grants reach among what its owner reads now, by name.
async fn reach(app: &Platform, d: &Machine, powers: &Powers) -> anyhow::Result<Vec<AgentReach>> {
    let mut out = Vec::new();
    for ((kind, id), p) in powers {
        let scope = if kind == "account" { AgentScope::Account(id.clone()) } else { AgentScope::Project(id.clone()) };
        if let Some(name) = owner_reads(app, &d.account, &scope, false).await? {
            out.push(AgentReach { kind: kind.clone(), id: id.clone(), name, powers: p.clone() });
        }
    }
    out.sort_by(|a, b| (&a.kind, &a.name, &a.id).cmp(&(&b.kind, &b.name, &b.id)));
    Ok(out)
}

/// `GET /api/daemon/chat/rooms`: the rooms this machine's agents reach,
/// and what its grants reach.
pub async fn rooms(State(app): State<Arc<Platform>>, d: Machine) -> J<AgentRooms> {
    gate(&app, &d.account).await?;
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    let powers = grants_of(&app, &d)?;
    if powers.is_empty() {
        return Ok(Json(AgentRooms { rooms: vec![], reach: vec![] }));
    }
    let reach = reach(&app, &d, &powers).await?;
    let (rooms, facts) = reached(&app, &d, &powers).await?;
    let ids: Vec<&str> = rooms.iter().map(|(r, _)| r.id.as_str()).collect();
    let last = app.db.chat_last_mentions(&ids, &mention_of(&d))?;
    let rooms = rooms
        .iter()
        .map(|(r, p)| shown(&app, &facts, &d.account, r, p.clone(), last.get(&r.id).copied()))
        .collect::<anyhow::Result<_>>()?;
    Ok(Json(AgentRooms { rooms, reach }))
}

/// `GET /api/daemon/chat/rooms/{id}/messages?after=N|before=N&limit=&mentions=`:
/// a page, oldest first, as the owner reads it (`threads.read`); with
/// `mentions`, only those that mention this machine's agents.
pub async fn messages(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Query(q): Query<AgentMessagesQuery>,
) -> J<AgentMessages> {
    gate(&app, &d.account).await?;
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    let (room, from, _) = room_of(&app, &d, THREADS_READ, &id).await?;
    let page = match (q.after, q.before) {
        (Some(a), _) => Page::After(a),
        (None, Some(b)) => Page::Before(b),
        (None, None) => Page::Newest,
    };
    let limit = q.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
    let me = mention_of(&d);
    let (ms, more) = if q.mentions {
        app.db.chat_mentioning(&room.id, &me, page, limit)?
    } else {
        app.db.chat_messages(&room.id, page, limit)?
    };
    let ms: Vec<&Message> = ms.iter().filter(|m| m.at >= from).collect();
    let shower = Shower::of(&app, &d.account, &ms).await?.as_agent(&d.daemon);
    let messages = ms.into_iter().map(|m| shower.show(m)).collect();
    let facts = Facts::of(&app, Some(&d.account), &[&room]).await?;
    let scope = agent_scope(&app, &room, &d.account)?;
    let powers = thread_powers(&grants_of(&app, &d)?, scope.as_ref());
    let last = app.db.chat_last_mentions(&[&room.id], &me)?.remove(&room.id);
    let room = shown(&app, &facts, &d.account, &room, powers, last)?;
    Ok(Json(AgentMessages { room, messages, more }))
}

/// `POST /api/daemon/chat/rooms/{id}/messages`: an agent's post
/// (`threads.post`), as its owner's, labelled with the machine.
pub async fn post(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(b): Json<ChatPost>,
) -> J<ChatPosted> {
    let (room, _, agent) = room_of(&app, &d, THREADS_POST, &id).await?;
    Ok(Json(post_as(&app, &d.account, Some(&agent), &room.id, b).await?))
}

/// `POST /api/daemon/chat/rooms/{id}/files?name=`: an agent's file
/// (`threads.post`), kept for its next post in the room, as a person's is.
pub async fn upload(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Query(q): Query<wire::ChatUploadQuery>,
    body: axum::body::Bytes,
) -> J<wire::ChatFile> {
    let (room, _, _) = room_of(&app, &d, THREADS_POST, &id).await?;
    let facts = Facts::of(&app, Some(&d.account), &[&room]).await?;
    let mut f = files::store(&app, &d.account, (&room, &facts), &q.name, &body).await?;
    f.url = files::url(&room.id, &f.id, true);
    Ok(Json(f))
}

/// `GET /api/daemon/chat/rooms/{id}/files/{file}`: a file in a room the
/// agent reads (`threads.read`); the bytes, as a person gets them.
pub async fn download(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, file)): Path<(String, String)>,
    Query(q): Query<files::Download>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    gate(&app, &d.account).await?;
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    let (room, from, _) = room_of(&app, &d, THREADS_READ, &id).await?;
    files::fetch(&app, (&room, from), &d.account, &file, &headers, q.download.is_some()).await
}

/// `POST /api/daemon/chat/rooms/{id}/messages/{seq}/reactions`: an agent's
/// reaction (`threads.post`), its owner's, labelled with the machine.
pub async fn react(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<ChatReact>,
) -> J<ChatMessage> {
    reacting(&app, &d, &id, seq, &b.emoji, true).await
}

/// `DELETE` the same with `?emoji=`: takes back the agent's own.
pub async fn unreact(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, seq)): Path<(String, i64)>,
    Query(q): Query<ChatReactQuery>,
) -> J<ChatMessage> {
    reacting(&app, &d, &id, seq, &q.emoji, false).await
}

async fn reacting(app: &Platform, d: &Machine, id: &str, seq: i64, emoji: &str, on: bool) -> J<ChatMessage> {
    let (room, from, agent) = room_of(app, d, THREADS_POST, id).await?;
    let facts = Facts::of(app, Some(&d.account), &[&room]).await?;
    Ok(Json(reactions::set(app, &d.account, Some(&agent), (&room, from, &facts), seq, emoji, on).await?))
}

/// `GET /api/daemon/chat/search?q=&team=`: over the rooms this machine's
/// agents may read (`threads.read`), as a person's search.
pub async fn search(State(app): State<Arc<Platform>>, d: Machine, Query(q): Query<ChatSearchQuery>) -> J<ChatFound> {
    gate(&app, &d.account).await?;
    let powers = grants_of(&app, &d)?;
    if !powers.values().flatten().any(|p| p == THREADS_READ) {
        return Ok(Json(ChatFound { found: vec![] }));
    }
    let (rooms, _) = reached(&app, &d, &powers).await?;
    let readable: std::collections::HashSet<String> =
        rooms.into_iter().filter(|(_, p)| p.iter().any(|p| p == THREADS_READ)).map(|(r, _)| r.id).collect();
    let team = q.team.as_deref();
    let found =
        search_in(&app, &d.account, &q.q, |r, facts| Ok(readable.contains(&r.id) && in_team(facts, r, team))).await?;
    Ok(Json(found))
}
