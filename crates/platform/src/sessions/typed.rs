//! A turn typed in a session's block itself (Arugula #215): someone opened
//! the block from the session's card, or prompted it from the MCP cloud
//! tool, and typed there. The thread got that turn's answer but never saw
//! what it answered. The box's report of the turn's start (`in_block`)
//! says who typed it (`by`) and how it starts (`typed`), and the thread
//! gets one line of it.
//!
//! - **The platform's line**: no author, `via` `system`, in the session's
//!   thread only. "Asked in the session by <who>: “<what>”", what they
//!   typed on one line, at most [`TYPED_MAX`] characters. The page draws a
//!   system line as plain text, so what was typed is shown as written:
//!   no Markdown, no links, no mentions.
//! - **Once a turn.** The report that kept the turn's start posts it; the
//!   same report sent again is `Reported::Already`, and posts nothing.
//! - **Never a prompt.** It's posted straight to the room with no
//!   mentions, as the platform's other lines are: it never starts or
//!   prompts a session, and nobody is pushed for it. A session's first
//!   prompt leaves system lines out ([`super::hydrate`]).
//! - **Nothing to say:** a turn the agent woke itself for, or one from a
//!   box from before it said what was typed, posts nothing.

use anyhow::Result;
use arugula_control_wire::{chat::SYSTEM_VIA, work::TYPED_MAX};
use tracing::info;

use crate::{
    Platform,
    chat::session_card::threaded,
    db::sessions::{Event, Kind, Session, SessionDetail, State},
};

/// What the thread's line starts with.
pub const ASKED: &str = "Asked in the session";

/// The thread's line for a turn `by` typed in the block, starting `typed`;
/// none when nothing was typed.
pub fn line(by: Option<&str>, typed: &str) -> Option<String> {
    let mut what: String = typed.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(TYPED_MAX).collect();
    if what.is_empty() {
        return None;
    }
    if what.chars().count() == TYPED_MAX {
        what.push('…');
    }
    let by = by.map(str::trim).filter(|b| !b.is_empty());
    Some(match by {
        Some(who) => format!("{ASKED} by {who}: “{what}”"),
        None => format!("{ASKED}: “{what}”"),
    })
}

/// After session `s`'s box reported `events` (`session_live::report`): a
/// turn's start that came from its block says so in its thread (module
/// docs).
pub async fn reported(app: &Platform, s: &Session, events: &[Event]) -> Result<()> {
    for e in events.iter().filter(|e| e.kind == Kind::State && e.to == State::Working) {
        let d: SessionDetail = serde_json::from_str(&e.detail).unwrap_or_default();
        if !d.in_block {
            continue;
        }
        let Some(typed) = d.typed.as_deref() else { continue };
        let by = match d.by.as_deref() {
            Some(by) => Some(who(app, by).await),
            None => None,
        };
        let Some(text) = line(by.as_deref(), typed) else { continue };
        let Some(room) = app.db.chat_room(&s.room)? else { continue };
        let thread = if threaded(&room.kind) { s.parent_seq } else { 0 };
        let seq = super::review::post(app, &room, thread, "", SYSTEM_VIA, &text)?;
        info!(session = s.id, n = ?e.daemon_n, seq, "a turn typed in the session's block: said in its thread");
    }
    Ok(())
}

/// Who typed it, as people read it: the block says a login, or an account
/// (then the name the platform has for it).
async fn who(app: &Platform, by: &str) -> String {
    let account = app.cache.accounts(&*app.hub, &[by.to_owned()]).await.ok().and_then(|mut a| a.remove(by));
    account.map(|a| a.name).filter(|n| !n.is_empty()).unwrap_or_else(|| by.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_turns_line_is_one_short_line() {
        assert_eq!(
            line(Some("ann"), "  fix the\n\nREADME  ").as_deref(),
            Some("Asked in the session by ann: “fix the README”")
        );
        assert_eq!(line(None, "go on").as_deref(), Some("Asked in the session: “go on”"));
        assert_eq!(line(Some(" "), "go on").as_deref(), Some("Asked in the session: “go on”"));
        assert_eq!(line(Some("ann"), " \n "), None);
        let long = line(Some("ann"), &"é".repeat(TYPED_MAX + 5)).unwrap();
        assert!(long.ends_with(&format!("{}…”", "é".repeat(TYPED_MAX))), "{long}");
    }
}
