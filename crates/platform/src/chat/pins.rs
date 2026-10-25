//! A channel's topic and description, and pinned messages (#119).
//!
//! - A channel's members write its topic and description (a team can leave
//!   that to its owners, `topic_owners`); the header shows the topic where
//!   one is set, else the computed line. Changing either posts a system
//!   line in the room.
//! - Whoever may post in a room pins or unpins any message in it (an agent
//!   with `threads.post`, for its owner). Pinning posts a system line too.
//!   A deleted message drops its pin.
//!
//! A system line is a message the room numbers like any other
//! (`chat_system` says what happened), so it arrives, pages and resumes
//! as one. Unlike others it is never counted unread, searched, pushed,
//! edited, reacted to or pinned, and it says who did it as its author.
//! The stream hears of a topic through [`ChatLive::Room`] (the room list
//! has it) and of a pin through [`ChatLive::Pinned`].

use super::*;
use crate::db::chat_pins::{System, Topic};
use arugula_control_wire::chat::{ChatPin, ChatPins, ChatSystem, ChatTopicSet, MAX_DESCRIPTION, MAX_TOPIC};

/// Pins a room holds, so its list stays something to read.
pub const MAX_PINS: usize = 50;

/// Topic changes and pins, per account per minute (system lines aren't
/// free: they fill the room).
const CHAT_PINS: (&str, usize) = ("chat-pin", 60);

/// What a page of rooms shows of their topics and pins, read once for the
/// lot ([`rooms`](super::rooms)).
#[derive(Default)]
pub(super) struct Extras {
    topics: HashMap<String, Topic>,
    pins: HashMap<String, u32>,
    /// Teams whose owners alone set topics.
    owners_only: HashSet<String>,
}

impl Extras {
    pub(super) fn of(db: &Db, rooms: &[&Room]) -> anyhow::Result<Self> {
        let ids: Vec<&str> = rooms.iter().map(|r| r.id.as_str()).collect();
        let mut teams: Vec<&str> =
            rooms.iter().filter(|r| r.kind == "channel").filter_map(|r| r.scope.strip_prefix("team:")).collect();
        teams.sort_unstable();
        teams.dedup();
        let (topics, pins, owners_only) = db.chat_room_extras(&ids, &teams)?;
        Ok(Self { topics, pins, owners_only })
    }

    /// `(topic, description, pins, whether the reader sets the topic)`.
    pub(super) fn of_room(
        &self,
        facts: &Facts,
        room: &Room,
        account: &str,
        writable: bool,
    ) -> (String, String, u32, bool) {
        let t = self.topics.get(&room.id);
        let team = room.scope.strip_prefix("team:").filter(|_| room.kind == "channel");
        let sets = writable
            && team.is_some_and(|t| !self.owners_only.contains(t) || facts.role(t, account) == Some(TeamRole::Owner));
        (
            t.map(|t| t.topic.clone()).unwrap_or_default(),
            t.map(|t| t.description.clone()).unwrap_or_default(),
            self.pins.get(&room.id).copied().unwrap_or(0),
            sets,
        )
    }
}

/// Which of some messages are system lines, and which are pinned: one
/// query each per room, over the numbers it spans.
#[derive(Default)]
pub(super) struct Marks {
    system: HashMap<(String, i64), System>,
    pinned: HashSet<(String, i64)>,
}

impl Marks {
    pub(super) fn of(db: &Db, ms: &[&Message]) -> anyhow::Result<Self> {
        let mut span: HashMap<&str, (i64, i64)> = HashMap::new();
        for m in ms {
            let e = span.entry(m.room.as_str()).or_insert((m.seq, m.seq));
            *e = (e.0.min(m.seq), e.1.max(m.seq));
        }
        let mut out = Self::default();
        for (room, (from, to)) in span {
            for (seq, s) in db.chat_system(room, from, to)? {
                out.system.insert((room.to_owned(), seq), s);
            }
            for seq in db.chat_pinned_among(room, from, to)? {
                out.pinned.insert((room.to_owned(), seq));
            }
        }
        Ok(out)
    }

    pub(super) fn pinned(&self, m: &Message) -> bool {
        self.pinned.contains(&(m.room.clone(), m.seq))
    }

    pub(super) fn system(&self, m: &Message) -> Option<ChatSystem> {
        self.system.get(&(m.room.clone(), m.seq)).map(|s| ChatSystem { kind: s.kind.clone(), target: s.target })
    }
}

/// Whether message `seq` is a system line: not for editing, deleting,
/// reacting to or pinning.
pub(super) fn is_system(app: &Platform, room: &Room, seq: i64) -> anyhow::Result<bool> {
    Ok(!app.db.chat_system(&room.id, seq, seq)?.is_empty())
}

/// A topic, one line: newlines and tabs become spaces.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `PUT /api/chat/rooms/{id}/topic`: a channel's topic and description.
pub async fn set_topic(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(b): Json<ChatTopicSet>,
) -> J<ChatRoom> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    if room.kind != "channel" {
        return Err(err(StatusCode::BAD_REQUEST, "only a channel has a topic"));
    }
    if !writable(&app, &facts, &room, &s.account)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let extras = Extras::of(&app.db, &[&room])?;
    if !extras.of_room(&facts, &room, &s.account, true).3 {
        return Err(err(StatusCode::FORBIDDEN, "this team leaves the topic to its owners"));
    }
    let old = app.db.chat_topic(&room.id)?.unwrap_or_default();
    let topic = b.topic.as_deref().map_or_else(|| old.topic.clone(), one_line);
    let description = b.description.as_deref().map_or_else(|| old.description.clone(), |d| d.trim().to_owned());
    if topic.chars().count() > MAX_TOPIC || description.chars().count() > MAX_DESCRIPTION {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a topic is at most 250 characters, a description 1000"));
    }
    let mut says: Vec<(&str, &str)> = vec![];
    if topic != old.topic {
        says.push(("topic", &topic));
    }
    // The line says that it changed, never what: it can be long.
    if description != old.description {
        says.push(("description", ""));
    }
    if !says.is_empty() {
        app.limits.check_account_per_minute(CHAT_PINS, &s.account)?;
        let new = Topic { topic: topic.clone(), description, author: s.account.clone(), at: now_ms() };
        for seq in app.db.set_chat_topic(&room.id, &new, None, &says)? {
            app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq }, None);
        }
        app.chat.room(&app.db, &room.id);
    }
    Ok(Json(listed(&app, &facts, &s.account, &room, from)?))
}

/// Pin (`on`) or unpin message `seq` of `room`, which the caller has
/// already found they may read, for `account` (or for its `agent`). Once
/// is as good as twice; the stream hears only of a change. The message, as
/// the person (or the agent) sees it.
pub(super) async fn set_pin(
    app: &Platform,
    account: &str,
    agent: Option<&AgentPrincipal>,
    (room, from, facts): (&Room, u64, &Facts),
    seq: i64,
    on: bool,
) -> Result<ChatMessage, ApiError> {
    if watching(app, facts, account, room)? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            WATCHING,
            "you're watching this session: you read its threads, and its owner can let you post",
        ));
    }
    if !writable(app, facts, room, account)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    let m = message_in(app, room, seq)?;
    if m.at < from {
        return Err(err(StatusCode::NOT_FOUND, "no such message"));
    }
    if on && (m.deleted.is_some() || m.imported.is_some() || is_system(app, room, seq)?) {
        return Err(err(StatusCode::CONFLICT, "that message can't be pinned"));
    }
    if on && app.db.chat_pins(&room.id)?.len() >= MAX_PINS && app.db.chat_pinned_among(&room.id, seq, seq)?.is_empty() {
        return Err(err(StatusCode::CONFLICT, "a room holds at most 50 pins: unpin one first"));
    }
    app.limits.check_account_per_minute(CHAT_PINS, account)?;
    let by = (account, agent.map(|a| a.machine.as_str()));
    if let Some(said) = app.db.set_chat_pin(&room.id, seq, on, by, now_ms())? {
        app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq: said }, None);
        app.chat.publish(&app.db, ChatLive::Pinned { room: room.id.clone() }, None);
    }
    let m = message_in(app, room, seq)?;
    let mut shower = Shower::of(app, account, &[&m]).await?;
    if let Some(a) = agent {
        shower = shower.for_agent(&a.machine);
    }
    Ok(shower.show(&m))
}

/// `PUT /api/chat/rooms/{id}/messages/{seq}/pin`: pin it.
pub async fn pin(State(app): State<Arc<Platform>>, s: Person, Path((id, seq)): Path<(String, i64)>) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    Ok(Json(set_pin(&app, &s.account, None, (&room, from, &facts), seq, true).await?))
}

/// `DELETE /api/chat/rooms/{id}/messages/{seq}/pin`: unpin it.
pub async fn unpin(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    Ok(Json(set_pin(&app, &s.account, None, (&room, from, &facts), seq, false).await?))
}

/// A room's pins as `account` reads them (or `agent`, for its owner), the
/// newest pin first.
pub(super) async fn pinned(
    app: &Platform,
    account: &str,
    agent: Option<&str>,
    room: &Room,
    from: u64,
) -> anyhow::Result<Vec<ChatPin>> {
    let pins = app.db.chat_pins(&room.id)?;
    let ms: Vec<Message> =
        pins.iter().filter_map(|p| app.db.chat_message(&room.id, p.seq).transpose()).collect::<Result<_, _>>()?;
    let ms: Vec<&Message> = ms.iter().filter(|m| m.at >= from && m.deleted.is_none()).collect();
    let mut shower = Shower::of(app, account, &ms).await?;
    if let Some(a) = agent {
        shower = shower.for_agent(a);
    }
    let mut pinners: Vec<String> = pins.iter().map(|p| p.author.clone()).filter(|a| !a.is_empty()).collect();
    pinners.sort();
    pinners.dedup();
    let names = app.cache.accounts(&*app.hub, &pinners).await?;
    Ok(pins
        .iter()
        .filter_map(|p| {
            let m = ms.iter().find(|m| m.seq == p.seq)?;
            Some(ChatPin {
                message: shower.show(m),
                by: p.author.clone(),
                by_name: names.get(&p.author).map(|a| a.name.clone()).unwrap_or_default(),
                at: p.at,
            })
        })
        .collect())
}

/// `GET /api/chat/rooms/{id}/pins`.
pub async fn list(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<ChatPins> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    Ok(Json(ChatPins { pins: pinned(&app, &s.account, None, &room, from).await? }))
}
