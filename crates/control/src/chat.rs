//! S41 prototype (throwaway, not for main): chat held by control in plain
//! text (#1092). A team's channels and DMs, and the threads about a pane
//! or a session, which name their machine. Control numbers messages,
//! keeps read marks, resolves mentions against the roster, pushes, and
//! searches. Live delivery is one Server-Sent Events stream per page.
//!
//! Who sees a room:
//! - a channel: everyone in the team's latest roster;
//! - a DM: its two people, while both are in the team;
//! - a pane or session thread: whoever may reach the machine through
//!   control (`teams::may_reach`), which is what the daemon's thread
//!   rights mostly amount to today.
//!
//! A message may name a pane (machine and pane id) and quote its text. The
//! quote is what the poster chose to share; the pane itself stays on its
//! machine. A reader who may not reach the machine sees the quote and a
//! pane chip that doesn't open.

use std::{collections::HashSet, convert::Infallible, sync::Arc, sync::LazyLock, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::{
    ApiError, App,
    auth::Session,
    db::chat::{Message, NewMessage, Room},
    err, teams,
};

type R = Result<Json<Value>, ApiError>;

/// A message is at most this much text, and a quote this much (as the
/// daemon's threads have it today).
const MAX_TEXT: usize = 8 * 1024;
const MAX_QUOTE: usize = 16 * 1024;

/// What the live stream carries: a room's new message. Each subscriber
/// checks whether it may see the room before sending it on.
#[derive(Clone, Debug)]
pub struct Posted {
    pub room: String,
    pub seq: u64,
}

static LIVE: LazyLock<broadcast::Sender<Posted>> = LazyLock::new(|| broadcast::channel(1024).0);

fn now() -> u64 {
    arugula_e2e::now_ms()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", hex::encode(arugula_e2e::random::<8>()))
}

/// The scopes an account sees channels and DMs in: its teams.
fn scopes(app: &App, account: &str) -> anyhow::Result<Vec<String>> {
    let mut out = vec![format!("account:{account}")];
    for body in app.db.teams_of(account)? {
        out.push(teams::parse(&body)?.team);
    }
    Ok(out)
}

/// Machines whose pane threads an account sees: its own and those it may
/// reach.
fn machines(app: &App, account: &str) -> anyhow::Result<Vec<String>> {
    let mut ids: Vec<String> = app.db.daemons(account)?.into_iter().map(|d| d.id).collect();
    for id in teams::reachable(app, account)? {
        if teams::may_reach(app, account, &id)? {
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn in_team(app: &App, team: &str, account: &str) -> bool {
    teams::latest(app, team).ok().is_some_and(|r| r.member(account).is_some())
}

/// May `account` see `room`?
pub fn may_see(app: &App, account: &str, room: &Room) -> anyhow::Result<bool> {
    Ok(match room.kind.as_str() {
        "pane" | "session" => room.machine.as_deref().is_some_and(|m| teams::may_reach(app, account, m).unwrap_or(false)),
        "dm" => {
            let members = app.db.chat_dm_members(&room.id)?;
            members.iter().any(|a| a == account) && members.iter().all(|a| in_team(app, &room.scope, a))
        }
        _ => {
            if let Some(a) = room.scope.strip_prefix("account:") {
                a == account
            } else {
                in_team(app, &room.scope, account)
            }
        }
    })
}

/// Everyone who sees a room: whom to tell and whom a mention can reach.
fn audience(app: &App, room: &Room) -> anyhow::Result<Vec<String>> {
    let mut out: Vec<String> = match room.kind.as_str() {
        "dm" => app.db.chat_dm_members(&room.id)?,
        "pane" | "session" => {
            let m = room.machine.as_deref().unwrap_or_default();
            let mut v: Vec<String> = app.db.daemon_account(m)?.into_iter().collect();
            if let Some(team) = app.db.daemon_team(m)?
                && let Ok(r) = teams::latest(app, &team)
            {
                v.extend(r.members.into_iter().map(|m| m.account));
            }
            v.extend(app.db.daemon_accounts(m).unwrap_or_default());
            v
        }
        _ => match room.scope.strip_prefix("account:") {
            Some(a) => vec![a.to_owned()],
            None => teams::latest(app, &room.scope)
                .map(|r| r.members.into_iter().map(|m| m.account).collect())
                .unwrap_or_default(),
        },
    };
    out.sort();
    out.dedup();
    let mut seen = Vec::new();
    for a in out {
        if may_see(app, &a, room)? {
            seen.push(a);
        }
    }
    Ok(seen)
}

fn room_for(app: &App, s: &Session, id: &str) -> Result<Room, ApiError> {
    let room = app.db.chat_room(id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such room"))?;
    if !may_see(app, &s.account, &room)? {
        return Err(err(StatusCode::NOT_FOUND, "no such room"));
    }
    Ok(room)
}

fn name_of(app: &App, account: &str) -> String {
    app.db.account(account).ok().flatten().map(|a| if a.name.is_empty() { a.login } else { a.name }).unwrap_or_default()
}

/// A message as `reader` sees it: names, and whether its pane opens for them.
fn shown(app: &App, reader: &str, m: &Message) -> Value {
    let pane = m.ref_machine.as_ref().map(|machine| {
        let name = app.db.daemon_row(machine).ok().flatten().map(|(_, d)| d.name).unwrap_or_default();
        json!({
            "machine": machine, "machine_name": name, "pane": m.ref_pane,
            "opens": teams::may_reach(app, reader, machine).unwrap_or(false),
        })
    });
    json!({
        "seq": m.seq, "at": m.at, "author": m.author, "name": name_of(app, &m.author),
        "text": m.text, "quote": m.quote, "pane": pane, "mentions": m.mentions,
        "imported": m.imported,
    })
}

fn listed(app: &App, account: &str, r: &Room) -> anyhow::Result<Value> {
    let read = app.db.chat_read(&r.id, account)?;
    let (unread, mentions) = app.db.chat_unread(&r.id, account, read)?;
    let with = if r.kind == "dm" {
        app.db.chat_dm_members(&r.id)?.into_iter().find(|a| a != account).map(|a| json!({"account": a, "name": name_of(app, &a)}))
    } else {
        None
    };
    Ok(json!({
        "id": r.id, "kind": r.kind, "scope": r.scope, "name": r.name, "machine": r.machine, "target": r.target,
        "last": r.last_seq, "read": read, "unread": unread, "mentions": mentions, "with": with,
    }))
}

// ---- the API

/// `GET /api/chat`: every room I see, with unread and mention counts.
pub async fn rooms(State(app): State<Arc<App>>, s: Session) -> R {
    let rooms = app.db.chat_rooms(&scopes(&app, &s.account)?, &machines(&app, &s.account)?)?;
    let mut out = Vec::new();
    for r in rooms {
        if may_see(&app, &s.account, &r)? {
            out.push(listed(&app, &s.account, &r)?);
        }
    }
    Ok(Json(json!({ "rooms": out })))
}

#[derive(Deserialize)]
pub struct NewChannel {
    name: String,
}

/// `POST /api/chat/teams/{team}/channels`: a channel, by any member who
/// may edit (viewers read and post, but don't make channels).
pub async fn create_channel(
    State(app): State<Arc<App>>,
    s: Session,
    Path(team): Path<String>,
    Json(b): Json<NewChannel>,
) -> R {
    let r = teams::latest(&app, &team)?;
    match teams::role_in(&r, &s.account) {
        Some(arugula_e2e::team::TeamRole::Viewer) | None => {
            return Err(err(StatusCode::FORBIDDEN, "only the team's owners and editors make channels"));
        }
        _ => {}
    }
    let name = b.name.trim().trim_start_matches('#').to_lowercase();
    if name.is_empty() || name.len() > 80 || name.chars().any(|c| c.is_whitespace()) {
        return Err(err(StatusCode::BAD_REQUEST, "a channel's name is one word"));
    }
    let room = Room {
        id: new_id("ch_"),
        scope: team,
        kind: "channel".into(),
        name,
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now(),
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &[]).map_err(|_| err(StatusCode::CONFLICT, "there's a channel by that name"))?;
    Ok(Json(json!({ "room": room.id })))
}

#[derive(Deserialize)]
pub struct NewDm {
    with: String,
}

/// `POST /api/chat/teams/{team}/dms`: the DM between me and a teammate
/// (made the first time).
pub async fn open_dm(State(app): State<Arc<App>>, s: Session, Path(team): Path<String>, Json(b): Json<NewDm>) -> R {
    if b.with == s.account || !in_team(&app, &team, &s.account) || !in_team(&app, &team, &b.with) {
        return Err(err(StatusCode::NOT_FOUND, "nobody by that account in this team"));
    }
    if let Some(r) = app.db.chat_dm(&team, &s.account, &b.with)? {
        return Ok(Json(json!({ "room": r.id })));
    }
    let room = Room {
        id: new_id("dm_"),
        scope: team,
        kind: "dm".into(),
        name: String::new(),
        machine: None,
        target: None,
        created_by: s.account.clone(),
        created: now(),
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &[&s.account, &b.with])?;
    Ok(Json(json!({ "room": room.id })))
}

#[derive(Deserialize)]
pub struct NewTargetRoom {
    machine: String,
    /// 'pane' or 'session'.
    kind: String,
    target: String,
}

/// `POST /api/chat/targets`: the thread about a pane or a session (made
/// the first time), for someone who may reach its machine.
pub async fn open_target(State(app): State<Arc<App>>, s: Session, Json(b): Json<NewTargetRoom>) -> R {
    if !matches!(b.kind.as_str(), "pane" | "session") || !teams::may_reach(&app, &s.account, &b.machine)? {
        return Err(err(StatusCode::NOT_FOUND, "no such machine"));
    }
    if let Some(r) = app.db.chat_room_for(&b.kind, &b.machine, &b.target)? {
        return Ok(Json(json!({ "room": r.id })));
    }
    let scope = match app.db.daemon_team(&b.machine)? {
        Some(t) => t,
        None => format!("account:{}", app.db.daemon_account(&b.machine)?.unwrap_or_default()),
    };
    let room = Room {
        id: new_id("th_"),
        scope,
        kind: b.kind.clone(),
        name: format!("{} {}", b.kind, b.target),
        machine: Some(b.machine),
        target: Some(b.target),
        created_by: s.account.clone(),
        created: now(),
        last_seq: 0,
    };
    app.db.add_chat_room(&room, &[])?;
    Ok(Json(json!({ "room": room.id })))
}

#[derive(Deserialize)]
pub struct After {
    #[serde(default)]
    after: u64,
}

/// `GET /api/chat/rooms/{id}/messages?after=N`.
pub async fn messages(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>, Query(q): Query<After>) -> R {
    let room = room_for(&app, &s, &id)?;
    let ms: Vec<Value> = app.db.chat_messages(&room.id, q.after, 200)?.iter().map(|m| shown(&app, &s.account, m)).collect();
    Ok(Json(json!({ "room": listed(&app, &s.account, &room)?, "messages": ms })))
}

#[derive(Deserialize)]
pub struct PaneRef {
    machine: String,
    pane: String,
}

#[derive(Deserialize)]
pub struct Post {
    text: String,
    #[serde(default)]
    pane: Option<PaneRef>,
    #[serde(default)]
    quote: Option<String>,
}

/// The accounts `@words` in `text` name, among those who see the room:
/// by their roster name, their login or their name.
fn mentions(app: &App, text: &str, audience: &[String]) -> Vec<String> {
    let words: HashSet<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ':' | ';' | '!' | '?' | '(' | ')'))
        .filter_map(|w| w.strip_prefix('@'))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut out = Vec::new();
    for a in audience {
        let Some(acc) = app.db.account(a).ok().flatten() else { continue };
        let names = [acc.login.to_lowercase(), acc.name.to_lowercase(), teams::member_name(&acc).to_lowercase()];
        if names.iter().any(|n| !n.is_empty() && words.contains(n)) {
            out.push(a.clone());
        }
    }
    out
}

/// `POST /api/chat/rooms/{id}/messages`: post. Control numbers it, tells
/// whoever's listening, and pushes to those it mentions (and, in a DM, to
/// the other person).
pub async fn post(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>, Json(b): Json<Post>) -> R {
    let room = room_for(&app, &s, &id)?;
    let text = b.text.trim();
    if text.is_empty() && b.quote.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "say something"));
    }
    if text.len() > MAX_TEXT || b.quote.as_ref().is_some_and(|q| q.len() > MAX_QUOTE) {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a message is at most 8 KB, a quote 16 KB"));
    }
    // You name only panes you can see.
    if let Some(p) = &b.pane
        && !teams::may_reach(&app, &s.account, &p.machine)?
    {
        return Err(err(StatusCode::FORBIDDEN, "you can't see that pane"));
    }
    let who = audience(&app, &room)?;
    let named: Vec<String> = mentions(&app, text, &who).into_iter().filter(|a| a != &s.account).collect();
    let at = now();
    let seq = app.db.add_chat_message(
        &room.id,
        &NewMessage {
            author: &s.account,
            at,
            text,
            ref_machine: b.pane.as_ref().map(|p| p.machine.as_str()),
            ref_pane: b.pane.as_ref().map(|p| p.pane.as_str()),
            quote: b.quote.as_deref(),
            mentions: &named,
            imported: None,
        },
    )?;
    let _ = LIVE.send(Posted { room: room.id.clone(), seq });
    // Pushes: a mention, or anything in a DM.
    let me = name_of(&app, &s.account);
    let mut push: Vec<(String, &str)> = named.iter().map(|a| (a.clone(), "mention")).collect();
    if room.kind == "dm" {
        for a in &who {
            if a != &s.account && !named.contains(a) {
                push.push((a.clone(), "dm"));
            }
        }
    }
    let place = match room.kind.as_str() {
        "channel" => format!("#{}", room.name),
        "dm" => "a direct message".into(),
        _ => room.name.clone(),
    };
    for (account, why) in &push {
        let subs = app.db.push_subs(account).map(|s| s.len()).unwrap_or(0);
        app.db.note_chat_push(account, &room.id, seq, why, subs, at)?;
        let title = if *why == "mention" { format!("{me} mentioned you in {place}") } else { format!("{me} wrote to you") };
        let body: String = text.chars().take(200).collect();
        crate::push::notify(&app, vec![account.clone()], &format!("chat-{}", room.id), title, body);
    }
    Ok(Json(json!({ "seq": seq, "mentions": named, "pushed": push.iter().map(|(a, w)| json!({"account": a, "why": w})).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct Read {
    seq: u64,
}

/// `POST /api/chat/rooms/{id}/read`.
pub async fn read(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>, Json(b): Json<Read>) -> R {
    let room = room_for(&app, &s, &id)?;
    app.db.set_chat_read(&room.id, &s.account, b.seq.min(room.last_seq))?;
    Ok(Json(listed(&app, &s.account, &room)?))
}

#[derive(Deserialize)]
pub struct Search {
    q: String,
}

/// `GET /api/chat/search?q=`: full text (SQLite FTS5) over the rooms I see.
pub async fn search(State(app): State<Arc<App>>, s: Session, Query(q): Query<Search>) -> R {
    let mut ids = Vec::new();
    for r in app.db.chat_rooms(&scopes(&app, &s.account)?, &machines(&app, &s.account)?)? {
        if may_see(&app, &s.account, &r)? {
            ids.push(r.id);
        }
    }
    // Words as a phrase each, so FTS5's syntax isn't the person's to type.
    let query = q.q.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', ""))).collect::<Vec<_>>().join(" ");
    if query.is_empty() {
        return Ok(Json(json!({ "found": [] })));
    }
    let found: Vec<Value> = app
        .db
        .chat_search(&ids, &query, 50)?
        .iter()
        .map(|m| json!({ "room": m.room, "message": shown(&app, &s.account, m) }))
        .collect();
    Ok(Json(json!({ "found": found })))
}

/// `GET /api/chat/live`: Server-Sent Events, one per new message in a
/// room I see (`{room, seq}`; the page fetches it). A keep-alive comment
/// every 25 s holds it open through proxies.
pub async fn live(State(app): State<Arc<App>>, s: Session) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = LIVE.subscribe();
    let account = s.account;
    let stream = futures_util::stream::unfold((rx, app, account), |(mut rx, app, account)| async move {
        loop {
            match rx.recv().await {
                Ok(p) => {
                    let ok = app.db.chat_room(&p.room).ok().flatten().is_some_and(|r| may_see(&app, &account, &r).unwrap_or(false));
                    if ok {
                        let ev = Event::default().event("message").data(json!({"room": p.room, "seq": p.seq}).to_string());
                        return Some((Ok(ev), (rx, app, account)));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let ev = Event::default().event("resync").data("{}");
                    return Some((Ok(ev), (rx, app, account)));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25)))
}

// ---- migration (the prototype's): a machine's owner uploads its old
// pane and session threads. In the product the daemon does this itself,
// signed, on upgrade.

#[derive(Deserialize)]
pub struct OldMessage {
    id: u64,
    at: u64,
    /// The daemon's principal: `owner`, `account:<id>`, `tailnet:<login>`.
    who: String,
    #[serde(default)]
    name: String,
    text: String,
    #[serde(default)]
    quote: Option<Value>,
}

#[derive(Deserialize)]
pub struct OldThread {
    /// 'pane' or 'session'.
    kind: String,
    target: String,
    messages: Vec<OldMessage>,
}

#[derive(Deserialize)]
pub struct Import {
    machine: String,
    threads: Vec<OldThread>,
}

/// `POST /api/chat/import`: a machine's owner brings its threads in. Each
/// becomes that pane's (or session's) room, with the old numbers kept; an
/// author control can't place is kept by name, as the account that
/// imported it, marked imported.
pub async fn import(State(app): State<Arc<App>>, s: Session, Json(b): Json<Import>) -> R {
    if app.db.daemon_account(&b.machine)?.as_deref() != Some(s.account.as_str()) {
        return Err(err(StatusCode::FORBIDDEN, "only a machine's owner brings its threads in"));
    }
    let mut out = Vec::new();
    for t in b.threads {
        if !matches!(t.kind.as_str(), "pane" | "session") {
            continue;
        }
        if app.db.chat_room_for(&t.kind, &b.machine, &t.target)?.is_some() {
            out.push(json!({"target": t.target, "skipped": "already here"}));
            continue;
        }
        let scope = match app.db.daemon_team(&b.machine)? {
            Some(team) => team,
            None => format!("account:{}", s.account),
        };
        let room = Room {
            id: new_id("th_"),
            scope,
            kind: t.kind.clone(),
            name: format!("{} {}", t.kind, t.target),
            machine: Some(b.machine.clone()),
            target: Some(t.target.clone()),
            created_by: s.account.clone(),
            created: t.messages.first().map_or(now(), |m| m.at),
            last_seq: 0,
        };
        app.db.add_chat_room(&room, &[])?;
        let mut n = 0;
        for m in &t.messages {
            let (author, imported) = match m.who.as_str() {
                "owner" => (s.account.clone(), None),
                w => match w.strip_prefix("account:") {
                    Some(a) if app.db.account(a)?.is_some() => (a.to_owned(), None),
                    _ => (s.account.clone(), Some(format!("{} ({})", m.name, m.who))),
                },
            };
            let quote = m.quote.as_ref().and_then(|q| q.get("text")).and_then(Value::as_str);
            let pane = m.quote.as_ref().and_then(|q| q.get("pane")).map(|p| p.to_string());
            let seq = app.db.add_chat_message(
                &room.id,
                &NewMessage {
                    author: &author,
                    at: m.at,
                    text: &m.text,
                    ref_machine: pane.as_ref().map(|_| b.machine.as_str()),
                    ref_pane: pane.as_deref(),
                    quote,
                    mentions: &[],
                    imported: Some(imported.as_deref().unwrap_or("imported")),
                },
            )?;
            if seq == m.id {
                n += 1;
            }
        }
        out.push(json!({"target": t.target, "room": room.id, "messages": t.messages.len(), "same_numbers": n}));
    }
    Ok(Json(json!({ "imported": out })))
}
