//! An agent session's status card in its thread (M257, thread = session,
//! Arugula #129): the one card kind a chat timeline shows (aru-115).
//!
//! - **One message per session** ([`post_card`]), posted when the session
//!   is made (a Try 3's sibling's when it's picked, M264; a review's only
//!   in the thread): no author, `via` `session:<ses>`
//!   ([`arugula_control_wire::sessions::card_via`]), a reply in the
//!   session's thread that's also sent to the room, so the card is in the
//!   timeline and in the thread. A room whose messages take no threads (an
//!   issue's comments) gets it as a message of its own. Its text says what
//!   it is in words, for search and for a client that draws no card; it
//!   isn't anyone's, so nobody is pushed for it.
//! - **Updated in place.** The message never changes: pages draw the card
//!   from the session as it is (`crate::session_live`, `GET
//!   /api/sessions/{ses}`), and read it again when they hear
//!   [`ChatLive::Session`] ([`Hub::session`]), which goes through
//!   `chat_events` like every live event, so a page on either of the
//!   platform's machines hears it.
//! - **Its PR's thread** (M259, [`note_pr`]): once its asker sent the
//!   session's PR and the mirror has it, the PR's thread (its `fi_` item's
//!   object thread, made now if nobody opened it) gets one system line,
//!   `session_pr`, by the asker, whose text is the session's id: the page
//!   links it back to the session's thread.

use arugula_control_wire::{chat::ChatLive, sessions::card_via};
use arugula_e2e::now_ms;

use crate::{
    Platform,
    db::{
        Taken,
        chat::{NewMessage, Room},
        pr_drafts::PrDraft,
        sessions::{Role, Session},
    },
};

/// Whether a room of `kind` takes replies in a thread (as the page's
/// `threads()` says): a team's people talk in these; an object's comments
/// are one list.
pub(crate) fn threaded(kind: &str) -> bool {
    matches!(kind, "channel" | "dm" | "pane" | "session")
}

/// Post session `s`'s card in its thread, once, when the session is made
/// (M256 calls it): its message's number. The message is the platform's,
/// with no author, and pages draw the card over it.
pub fn post_card(app: &Platform, s: &Session) -> anyhow::Result<i64> {
    post_card_by(app, s, None)
}

/// [`post_card`] for a session its asker's agent started (aru-121):
/// `started_by` ("<asker>'s agent on <machine>") is in its text too.
pub fn post_card_by(app: &Platform, s: &Session, started_by: Option<&str>) -> anyhow::Result<i64> {
    let room = app.db.chat_room(&s.room)?.ok_or_else(|| anyhow::anyhow!("no room {} for session {}", s.room, s.id))?;
    let parent = i64::try_from(s.parent_seq).ok().filter(|p| *p > 0 && threaded(&room.kind));
    let short: String = s.id.trim_start_matches("ses_").chars().take(8).collect();
    let mut text = format!("Agent session {short} on {}", s.branch);
    if let Some(by) = started_by {
        text += &format!(", started by {by}");
    }
    let via = card_via(&s.id);
    // A review's card (M264) is the thread's business: not in the timeline.
    let also = parent.is_some() && s.role != Role::Review;
    let seq = app.db.add_chat_message(
        &room.id,
        &NewMessage {
            author: "",
            at: now_ms(),
            text: &text,
            quote: None,
            ref_machine: None,
            ref_pane: None,
            mentions: &[],
            agent: None,
            via: Some(&via),
            parent,
            also,
            agent_session: None,
        },
    )?;
    if let Some(parent) = parent {
        app.chat.publish(&app.db, ChatLive::Reply { room: room.id.clone(), parent, seq }, None);
    }
    if also || parent.is_none() {
        app.chat.publish(&app.db, ChatLive::Message { room: room.id, seq }, None);
    }
    Ok(seq)
}

/// Say in the thread of PR draft `d`'s PR that it came from its session,
/// once (module docs): the line's room and number, or `None` when the
/// mirror hasn't the PR yet, it's in another project now, or it was said.
pub fn note_pr(app: &Platform, d: &PrDraft) -> anyhow::Result<Option<(String, i64)>> {
    let Some(item) = d.item.as_deref().filter(|_| d.state == "sent" && d.noted_at.is_none()) else { return Ok(None) };
    let Ok(obj) = super::objects::resolve(app, item) else { return Ok(None) };
    if obj.project != d.project {
        return Ok(None);
    }
    let scope = super::objects::project_scope(&obj.project);
    let room = match app.db.chat_room_on(&scope, item)? {
        Some(r) => r,
        None => {
            let room = Room {
                id: super::new_id("ob_"),
                scope: scope.clone(),
                kind: "object".into(),
                name: obj.name.clone(),
                machine: None,
                target: Some(item.to_owned()),
                created_by: d.author.clone(),
                created: now_ms(),
                archived: None,
                last_seq: 0,
            };
            match app.db.add_chat_room(&room, &[], Some(&obj.project)) {
                Ok(()) => {
                    app.chat.room(&app.db, &room.id);
                    room
                }
                // Someone opened it just now.
                Err(e) if e.is::<Taken>() => {
                    app.db.chat_room_on(&scope, item)?.ok_or_else(|| anyhow::anyhow!("no thread on {item}"))?
                }
                Err(e) => return Err(e),
            }
        }
    };
    let Some(seq) = app.db.note_pr(d, &room.id, now_ms())? else { return Ok(None) };
    app.chat.publish(&app.db, ChatLive::Message { room: room.id.clone(), seq }, None);
    Ok(Some((room.id, seq)))
}
