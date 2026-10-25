//! Emoji reactions on messages (#104). Whoever sees a room and may post in
//! it reacts: a person on their own, an agent for its owner with
//! `threads.post` (shown as "<owner>'s agent on <machine>"). Nobody is
//! pushed for one, and a message's reactions come with the message
//! ([`Shower`]); the live stream says only that a message's changed
//! ([`ChatLive::Reacted`]), and each page reads it again, as it would after
//! an edit.

use super::*;
use crate::db::chat_reactions::Added;
use arugula_control_wire::chat::{ChatReact, ChatReactQuery};

/// Reactions added or taken back, per account per minute.
const CHAT_REACTS: (&str, usize) = ("chat-react", 240);

/// What reacts: some emoji, not text. One grapheme cluster of a few
/// characters at most, with a non-ASCII character in it (so `1️⃣` is
/// one, and `1` is not).
fn valid(emoji: &str) -> bool {
    emoji.len() <= 64
        && emoji.chars().count() <= 16
        && !emoji.is_ascii()
        && emoji.chars().all(|c| !c.is_whitespace() && !c.is_control() && !c.is_alphabetic())
}

/// Add (`on`) or take back `account`'s reaction (its `agent`'s, when one is
/// reacting for it) to message `seq` of `room`, which the caller has
/// already found they may read. Once is as good as twice; the stream hears
/// of it only when something changed. The message, as the person (or the
/// agent) sees it.
pub(super) async fn set(
    app: &Platform,
    account: &str,
    agent: Option<&AgentPrincipal>,
    (room, from, facts): (&Room, u64, &Facts),
    seq: i64,
    emoji: &str,
    on: bool,
) -> Result<ChatMessage, ApiError> {
    if !valid(emoji) {
        return Err(err(StatusCode::BAD_REQUEST, "react with an emoji"));
    }
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
    if on && super::pins::is_system(app, room, seq)? {
        return Err(err(StatusCode::CONFLICT, "react to what was said, not to a note of what happened"));
    }
    if on && m.deleted.is_some() {
        return Err(err(StatusCode::CONFLICT, "that message was deleted"));
    }
    if on && m.imported.is_some() {
        return Err(err(StatusCode::CONFLICT, "an archive's messages are read-only"));
    }
    app.limits.check_account_per_minute(CHAT_REACTS, account)?;
    let by = agent.map_or("", |a| a.machine.as_str());
    let changed = if on {
        let r = Reaction {
            room: room.id.clone(),
            seq,
            emoji: emoji.to_owned(),
            account: account.to_owned(),
            agent: by.to_owned(),
            at: now_ms(),
        };
        match app.db.add_chat_reaction(&r)? {
            Added::Yes => true,
            Added::Already => false,
            Added::Full => {
                return Err(err(StatusCode::CONFLICT, "a message holds at most 20 different emoji"));
            }
        }
    } else {
        app.db.remove_chat_reaction(&room.id, seq, emoji, account, by)?
    };
    if changed {
        app.chat.publish(&app.db, ChatLive::Reacted { room: room.id.clone(), seq }, None);
    }
    let m = message_in(app, room, seq)?;
    let mut shower = Shower::of(app, account, &[&m]).await?;
    if let Some(a) = agent {
        shower = shower.for_agent(&a.machine);
    }
    Ok(shower.show(&m))
}

/// `POST /api/chat/rooms/{id}/messages/{seq}/reactions`: your reaction.
pub async fn add(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<ChatReact>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    Ok(Json(set(&app, &s.account, None, (&room, from, &facts), seq, &b.emoji, true).await?))
}

/// `DELETE /api/chat/rooms/{id}/messages/{seq}/reactions?emoji=`: take yours back.
pub async fn remove(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    Query(q): Query<ChatReactQuery>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, facts) = room_for(&app, &s.account, &id).await?;
    Ok(Json(set(&app, &s.account, None, (&room, from, &facts), seq, &q.emoji, false).await?))
}
