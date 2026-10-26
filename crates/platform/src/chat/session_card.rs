//! An agent session's status card in its thread (M257, thread = session,
//! Arugula #129): the one card kind a chat timeline shows (aru-115).
//!
//! - **One message per session** ([`post_card`]), posted when the session
//!   is made: no author, `via` `session:<ses>`
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

use arugula_control_wire::{chat::ChatLive, sessions::card_via};
use arugula_e2e::now_ms;

use super::Hub;
use crate::{
    Platform,
    db::{Db, chat::NewMessage, sessions::Session},
};

/// Whether a room of `kind` takes replies in a thread (as the page's
/// `threads()` says): a team's people talk in these; an object's comments
/// are one list.
fn threaded(kind: &str) -> bool {
    matches!(kind, "channel" | "dm" | "pane" | "session")
}

/// Post session `s`'s card in its thread, once, when the session is made
/// (M256 calls it): its message's number. The message is the platform's,
/// with no author, and pages draw the card over it.
pub fn post_card(app: &Platform, s: &Session) -> anyhow::Result<i64> {
    let room = app.db.chat_room(&s.room)?.ok_or_else(|| anyhow::anyhow!("no room {} for session {}", s.room, s.id))?;
    let parent = i64::try_from(s.parent_seq).ok().filter(|p| *p > 0 && threaded(&room.kind));
    let short: String = s.id.trim_start_matches("ses_").chars().take(8).collect();
    let text = format!("Agent session {short} on {}", s.branch);
    let via = card_via(&s.id);
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
            also: parent.is_some(),
        },
    )?;
    if let Some(parent) = parent {
        app.chat.publish(&app.db, ChatLive::Reply { room: room.id.clone(), parent, seq }, None);
    }
    app.chat.publish(&app.db, ChatLive::Message { room: room.id, seq }, None);
    Ok(seq)
}

impl Hub {
    /// Session `s` changed: pages showing its card read it again, on every
    /// machine (through `chat_events`).
    pub fn session(&self, db: &Db, s: &Session) {
        self.publish(db, ChatLive::Session { room: s.room.clone(), ses: s.id.clone() }, None);
    }
}
