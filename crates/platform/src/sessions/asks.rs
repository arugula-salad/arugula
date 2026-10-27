//! Asks (Arugula #214): what a thread's session still needs from its asker,
//! addressed to them.
//!
//! - **What's an ask:** which repo, when the project's workspaces have
//!   several and the `@arugula` named none ([`REPO`],
//!   `crate::chat::sessions`), and what a session's agent asks while it's
//!   `needs_input` ([`opened`]: a question or a permission).
//! - **A message, also in the channel.** Each is the platform's message in
//!   the thread (no author, `via` `ask:repo` or `ask:<ses>`), sent to the
//!   channel too, so the channel shows it where its asker reads. Pages draw
//!   it as a form: which repo, one button each (`GET`/`POST`
//!   `/api/chat/rooms/{room}/asks/{seq}`); the agent's, the session's open
//!   question as its card asks it, its choices as buttons. Its text is the
//!   plain fallback.
//! - **A mention of the asker** while it's open: it counts as their unread
//!   mention in the channel's list and the thread, as any mention does, and
//!   it's pushed to them once ([`crate::chat::push_ask`]).
//! - **Answered, it mentions nobody** ([`close`]): the session started, or
//!   its agent was answered or went on, or the session ended. So an answered
//!   ask stops counting, read or not.
//! - **And says the answer** ([`answered`], Arugula #215): answered from
//!   its form or the card, its message's text gets a line under the
//!   question, `<name> answered: <choice>`, so the thread shows what the
//!   agent was told. Without it, the agent's next words read as if it
//!   had answered itself.

use anyhow::Result;
use arugula_control_wire::{chat::ChatLive, sessions::ask_via};
use arugula_e2e::now_ms;
use tracing::warn;

use crate::{
    Platform,
    db::{
        chat::{NewMessage, Room},
        sessions::{Event, Kind, Session, SessionDetail, State},
    },
};

/// Which repo, before a session is made.
pub const REPO: &str = "repo";

/// The longest question an ask's text quotes, in characters.
const QUESTION_MAX: usize = 500;

/// The `via` of an ask about `of` (`repo`, or a session's id).
pub fn via(of: &str) -> String {
    ask_via(of)
}

/// Where the platform's message in `room`'s `thread` goes: its parent
/// (`None`: the room's own messages, as on an issue's thread), and
/// whether it's also sent to the channel.
pub fn place(room: &Room, thread: u64) -> (Option<i64>, bool) {
    let threaded = matches!(room.kind.as_str(), "channel" | "dm" | "pane" | "session");
    let parent = i64::try_from(thread).ok().filter(|p| *p > 0 && threaded);
    (parent, parent.is_some())
}

/// Post an ask about `of` for `asker` in `room`'s `thread` (module docs):
/// its message's number.
pub async fn post(app: &Platform, room: &Room, thread: u64, of: &str, asker: &str, text: &str) -> Result<i64> {
    let (parent, also) = place(room, thread);
    let via = via(of);
    let mentions = [asker.to_owned()];
    let seq = app.db.add_chat_message(
        &room.id,
        &NewMessage {
            author: "",
            at: now_ms(),
            text,
            quote: None,
            ref_machine: None,
            ref_pane: None,
            mentions: &mentions,
            agent: None,
            via: Some(&via),
            parent,
            also,
        },
    )?;
    if let Some(parent) = parent {
        app.chat.publish(&app.db, ChatLive::Reply { room: room.id.clone(), parent, seq }, None);
    }
    if also || parent.is_none() {
        app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq }, None);
    }
    if let Err(e) = crate::chat::push_ask(app, room, (seq, parent, also), text, asker).await {
        warn!(room = room.id, seq, error = %format!("{e:#}"), "an ask's push");
    }
    Ok(seq)
}

/// The open asks about `of` in `room`'s `thread` are answered: they
/// mention nobody now, and pages read them again.
pub fn close(app: &Platform, room: &Room, thread: u64, of: &str) -> Result<()> {
    let (parent, _) = place(room, thread);
    for (seq, parent) in app.db.close_chat_asks(&room.id, parent, &via(of))? {
        app.chat.publish(&app.db, ChatLive::Changed { room: room.id.clone(), seq }, None);
        if let Some(parent) = parent {
            app.chat.publish(&app.db, ChatLive::Reply { room: room.id.clone(), parent, seq }, None);
        }
    }
    Ok(())
}

/// Session `s`'s asks are answered.
pub fn close_session(app: &Platform, s: &Session) -> Result<()> {
    let Some(room) = app.db.chat_room(&s.room)? else { return Ok(()) };
    close(app, &room, s.parent_seq, &s.id)
}

/// Session `s`'s open ask was answered by `who` (their name), with
/// `said` (Arugula #215, [`said`]): its message says so under the question,
/// so whoever reads the thread (its page, an agent's `read_thread`) sees
/// the answer the agent got, not only that there was one. It mentions
/// nobody now.
pub fn answered(app: &Platform, s: &Session, who: &str, said: &str) -> Result<()> {
    let Some(room) = app.db.chat_room(&s.room)? else { return Ok(()) };
    let (parent, _) = place(&room, s.parent_seq);
    let who = if who.trim().is_empty() { "someone" } else { who.trim() };
    for (seq, parent) in app.db.close_chat_asks(&room.id, parent, &via(&s.id))? {
        if let Some(m) = app.db.chat_message(&room.id, seq)? {
            let text = format!("{}\n{who} {said}", m.text.lines().next().unwrap_or_default());
            app.db.edit_chat_message(&room.id, seq, &text, &[], now_ms())?;
        }
        app.chat.publish(&app.db, ChatLive::Changed { room: room.id.clone(), seq }, None);
        if let Some(parent) = parent {
            app.chat.publish(&app.db, ChatLive::Reply { room: room.id.clone(), parent, seq }, None);
        }
    }
    Ok(())
}

/// What an answer of `kind` said, in words, after its answerer's name:
/// `answered: Blue`, `allowed it`, `denied it: not that file`. A
/// question's answer is its fields' words, in order.
pub fn said(kind: &str, answer: Option<&serde_json::Value>, reason: Option<&str>) -> String {
    match kind {
        "approve" => "allowed it".into(),
        "deny" => match reason.map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => format!("denied it: {}", clip(r)),
            None => "denied it".into(),
        },
        _ => {
            let mut words = Vec::new();
            if let Some(serde_json::Value::Object(o)) = answer {
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort_by_key(|k| field_order(k));
                for k in keys {
                    match &o[k.as_str()] {
                        serde_json::Value::String(v) if !v.trim().is_empty() => words.push(v.trim().to_owned()),
                        serde_json::Value::Array(vs) => {
                            words.extend(vs.iter().filter_map(|v| v.as_str()).map(str::to_owned))
                        }
                        serde_json::Value::Number(n) => words.push(n.to_string()),
                        serde_json::Value::Bool(b) => words.push(b.to_string()),
                        _ => {}
                    }
                }
            }
            match words.is_empty() {
                true => "answered, choosing nothing".into(),
                false => format!("answered: {}", clip(&words.join("; "))),
            }
        }
    }
}

/// A form's fields in the order asked: `question_<n>`, then its
/// `question_<n>_custom`, by `n`; any other after, by name.
fn field_order(k: &str) -> (u64, u8, String) {
    let n = k.strip_prefix("question_").map(|r| r.trim_end_matches("_custom"));
    match n.and_then(|n| n.parse().ok()) {
        Some(n) => (n, u8::from(k.ends_with("_custom")), String::new()),
        None => (u64::MAX, 0, k.to_owned()),
    }
}

fn clip(s: &str) -> String {
    match s.chars().count() > QUESTION_MAX {
        true => s.chars().take(QUESTION_MAX - 1).chain(['…']).collect(),
        false => s.to_owned(),
    }
}

/// Session `s` moved: once it isn't waiting on a person, what it asked is
/// answered.
pub fn settle(app: &Platform, s: &Session) {
    if s.seen_as() == State::NeedsInput {
        return;
    }
    if let Err(e) = close_session(app, s) {
        warn!(session = s.id, error = %format!("{e:#}"), "closing a session's asks");
    }
}

/// Session `s`'s box reported `events`: when its agent asks something now
/// (`working` to `needs_input`), that's an ask for its asker.
pub async fn opened(app: &Platform, s: &Session, events: &[Event]) {
    let asked = events.iter().rev().find(|e| {
        e.kind == Kind::State && e.from == Some(State::Working) && e.to == State::NeedsInput && e.daemon.is_some()
    });
    let Some(e) = asked.filter(|_| s.seen_as() == State::NeedsInput && !s.asker.is_empty()) else { return };
    if let Err(e) = ask_for(app, s, e).await {
        warn!(session = s.id, error = %format!("{e:#}"), "a session's ask");
    }
}

async fn ask_for(app: &Platform, s: &Session, e: &Event) -> Result<()> {
    let Some(room) = app.db.chat_room(&s.room)? else { return Ok(()) };
    let d: SessionDetail = serde_json::from_str(&e.detail).unwrap_or_default();
    let q = d.question.as_deref().map(str::trim).filter(|q| !q.is_empty()).unwrap_or("it waits for an answer");
    let q: String = match q.chars().count() > QUESTION_MAX {
        true => q.chars().take(QUESTION_MAX - 1).chain(['…']).collect(),
        false => q.to_owned(),
    };
    let who = crate::chat::name_of(app, &s.asker).await?;
    let who = if who.is_empty() { "its asker".to_owned() } else { who };
    let text = match d.ask.as_deref() {
        Some("permission") => format!("@arugula needs {who} to allow it: {q}"),
        _ => format!("@arugula asks {who}: {q}"),
    };
    // One open ask per session: an older one is answered by now.
    close(app, &room, s.parent_seq, &s.id)?;
    post(app, &room, s.parent_seq, &s.id, &s.asker, &text).await?;
    Ok(())
}
