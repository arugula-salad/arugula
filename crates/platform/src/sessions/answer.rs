//! A session's answers (thread = session, Arugula #129): each turn's last
//! message, posted in the session's thread, so what the agent said is in
//! the conversation and not only on the session's page.
//!
//! - **Every turn's** that carries one (`SessionDetail::reply`: the
//!   agent's last message of the turn), pushed or not, in any role but a
//!   review's: a review's reply is its review, which [`super::review`]
//!   posts. A box from before every turn sent it sends none, and its turns
//!   post nothing. An empty one posts nothing.
//! - **Once.** The box's report that kept the turn posts it; the same
//!   report sent again (its answer was lost) is `Reported::Already`, and
//!   posts nothing. The turn's number in the session is the key.
//! - **In the thread only**: a reply in the session's thread, not also
//!   sent to the channel; in an object's thread (an issue's), a message in
//!   its list.
//! - **Whose:** its asker's, `via` `session:<ses>`, as a review's post is
//!   its asker's (aru-115: a session's turns are its asker's). With an
//!   author it isn't drawn as the session's card, which has none.
//! - **Cut** at [`ANSWER_MAX`] characters, on a character's edge, ending
//!   with a line linking the session's page, where the rest is.
//! - **Plain text that wakes nobody.** It's posted straight to the room
//!   (`Db::add_chat_message`) with no mentions, never through chat's post
//!   (`chat::post_full`), as the platform's own lines are: an `@` the
//!   agent wrote names nobody and pushes nobody, and it never starts or
//!   prompts a session (`chat::sessions::posted` isn't called for it).

use anyhow::Result;
use arugula_control_wire::sessions::card_via;
use tracing::info;

use crate::{
    Platform,
    chat::session_card::threaded,
    db::sessions::{Event, Kind, Role, Session, SessionDetail},
};

/// The most characters of an answer the thread gets.
pub const ANSWER_MAX: usize = 2000;

/// What a cut answer ends with, before the session's link.
pub const THE_REST: &str = "…the rest is in the session:";

/// The thread's text for `reply`: none when it's empty; past
/// [`ANSWER_MAX`] characters, cut, with a line linking `link`.
pub fn text(reply: &str, link: &str) -> Option<String> {
    let reply = reply.trim();
    if reply.is_empty() {
        return None;
    }
    Some(match reply.char_indices().nth(ANSWER_MAX) {
        None => reply.to_owned(),
        Some((at, _)) => format!("{}\n\n{THE_REST} {link}", reply[..at].trim_end()),
    })
}

/// After session `s`'s box reported `events` (`session_live::report`):
/// each turn's answer, posted in its thread (module docs).
pub fn reported(app: &Platform, s: &Session, events: &[Event]) -> Result<()> {
    if s.role == Role::Review {
        return Ok(());
    }
    for e in events.iter().filter(|e| e.kind == Kind::Turn) {
        let d: SessionDetail = serde_json::from_str(&e.detail).unwrap_or_default();
        let link = super::pr::session_link(app, s);
        let Some(text) = d.reply.as_deref().and_then(|r| text(r, &link)) else { continue };
        let Some(room) = app.db.chat_room(&s.room)? else { continue };
        let thread = if threaded(&room.kind) { s.parent_seq } else { 0 };
        let seq = super::review::post(app, &room, thread, &s.asker, &card_via(&s.id), &text)?;
        info!(session = s.id, n = ?e.daemon_n, seq, "a session's turn: its answer posted in its thread");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_answer_is_cut_with_the_sessions_link() {
        let link = "https://app.example/p/prj_1/sessions/ses_1";
        assert_eq!(text("  \n ", link), None);
        assert_eq!(text(" The README is current.\n", link).as_deref(), Some("The README is current."));
        let exact = "é".repeat(ANSWER_MAX);
        assert_eq!(text(&exact, link).as_deref(), Some(exact.as_str()), "at the limit: whole");
        let long = format!("{}{}", "é".repeat(ANSWER_MAX - 1), "xyz");
        let out = text(&long, link).unwrap();
        let (kept, rest) = out.split_once("\n\n").unwrap();
        assert_eq!(kept.chars().count(), ANSWER_MAX, "cut on a character's edge");
        assert!(kept.ends_with("éx"), "{kept}");
        assert_eq!(rest, format!("{THE_REST} {link}"));
    }
}
