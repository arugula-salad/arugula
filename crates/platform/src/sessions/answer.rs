//! A session's answers (thread = session, Arugula #129): each turn's last
//! message, posted in the session's thread, so what the agent said is in
//! the conversation and not only on the session's page.
//!
//! - **Every turn's** that carries one (`SessionDetail::reply`: the
//!   agent's last message of the turn), pushed or not, in any role but a
//!   review's: a review's reply is its review, which [`super::review`]
//!   posts. A box from before every turn sent it sends none, and its turns
//!   post nothing. An empty one posts nothing, unless the turn pushed.
//! - **Once.** The box's report that kept the turn posts it; the same
//!   report sent again (its answer was lost) is `Reported::Already`, and
//!   posts nothing. The turn's number in the session is the key.
//! - **In the thread only**: a reply in the session's thread, not also
//!   sent to the channel; in an object's thread (an issue's), a message in
//!   its list.
//! - **Whose:** the session's, not a person's (Arugula #215): no author,
//!   `via` `said:<ses>` (`said_via`), as a review's post is. The page
//!   draws it with the session's look, apart from people's and from the
//!   session's card (`via` `session:<ses>`). Its turns are still its
//!   asker's (aru-115), but the words are the agent's: they aren't
//!   counted as the asker's own anywhere (`chat::sessions::own`).
//! - **Cut from its start** to its last [`ANSWER_MAX`] characters (Arugula
//!   #215): a final message ends with its summary and caveats, which are
//!   what the thread must not lose. It starts with a line linking the
//!   session's page, where all of it is, then `…` and the rest, from a
//!   line's start (or a word's) near the cut. Not a summary asked of the
//!   agent: that's another turn, on the asker's key, after the turn that
//!   ended, and it could say something else.
//! - **Pushed:** a turn that pushed ends with one line saying so, how many
//!   commits to which branch, from its report; the agent can't know, since
//!   the box pushes after its turn. A turn that pushed and said nothing
//!   posts that line alone, so the thread always shows a push.
//! - **Plain text that wakes nobody.** It's posted straight to the room
//!   (`Db::add_chat_message`) with no mentions, never through chat's post
//!   (`chat::post_full`), as the platform's own lines are: an `@` the
//!   agent wrote names nobody and pushes nobody, and it never starts or
//!   prompts a session (`chat::sessions::posted` isn't called for it).

use anyhow::Result;
use arugula_control_wire::sessions::said_via;
use tracing::info;

use crate::{
    Platform,
    chat::session_card::threaded,
    db::sessions::{Event, Kind, Role, Session, SessionDetail},
};

/// The most characters of an answer the thread gets.
pub const ANSWER_MAX: usize = 2000;

/// What a cut answer starts with, before the session's link.
pub const THE_START: &str = "Its start is cut; all of it is in the session:";

/// How far past the cut an answer may start, to start on a line.
const LINE_NEAR: usize = ANSWER_MAX / 4;

/// The thread's text for `reply`: none when it's empty; past
/// [`ANSWER_MAX`] characters, its end, after a line linking `link`.
pub fn text(reply: &str, link: &str) -> Option<String> {
    let reply = reply.trim();
    if reply.is_empty() {
        return None;
    }
    let n = reply.chars().count();
    if n <= ANSWER_MAX {
        return Some(reply.to_owned());
    }
    let at = reply.char_indices().nth(n - ANSWER_MAX).map_or(0, |(at, _)| at);
    let end = &reply[at..];
    // The next line's start when it's near, else the next word's: no half
    // a word first.
    let near = |i: usize| end[..i].chars().count() <= LINE_NEAR;
    let from = match end.find('\n').filter(|&i| near(i)) {
        Some(i) => i,
        None => end.find(char::is_whitespace).filter(|&i| near(i)).unwrap_or(0),
    };
    Some(format!("{THE_START} {link}\n\n…{}", end[from..].trim_start()))
}

/// The line a turn that pushed ends its answer with: how many commits, to
/// which branch.
pub fn pushed(d: &SessionDetail) -> Option<String> {
    if d.pushed != Some(true) {
        return None;
    }
    let to = d.branch.as_deref().map(|b| format!(" to `{b}`")).unwrap_or_default();
    Some(match d.commits.len() {
        0 => format!("Pushed{to}."),
        1 => format!("Pushed 1 commit{to}."),
        n => format!("Pushed {n} commits{to}."),
    })
}

/// The thread's text for turn `d`: its reply ([`text`]), then the line
/// saying what it pushed ([`pushed`]); that line alone for a turn that
/// pushed and said nothing; none for one that did neither.
pub fn answer(d: &SessionDetail, link: &str) -> Option<String> {
    let said = d.reply.as_deref().and_then(|r| text(r, link));
    match (said, pushed(d)) {
        (Some(said), Some(line)) => Some(format!("{said}\n\n{line}")),
        (said, line) => said.or(line),
    }
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
        let Some(text) = answer(&d, &link) else { continue };
        let Some(room) = app.db.chat_room(&s.room)? else { continue };
        let thread = if threaded(&room.kind) { s.parent_seq } else { 0 };
        let seq = super::review::post(app, &room, thread, "", &said_via(&s.id), &text)?;
        info!(session = s.id, n = ?e.daemon_n, seq, "a session's turn: its answer posted in its thread");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_answer_keeps_its_end_after_the_sessions_link() {
        let link = "https://app.example/p/prj_1/sessions/ses_1";
        assert_eq!(text("  \n ", link), None);
        assert_eq!(text(" The README is current.\n", link).as_deref(), Some("The README is current."));
        let exact = "é".repeat(ANSWER_MAX);
        assert_eq!(text(&exact, link).as_deref(), Some(exact.as_str()), "at the limit: whole");
        // No line or word to start on: from the cut, on a character's edge.
        let long = format!("xyz{}", "é".repeat(ANSWER_MAX - 1));
        let out = text(&long, link).unwrap();
        let (head, kept) = out.split_once("\n\n").unwrap();
        assert_eq!(head, format!("{THE_START} {link}"));
        assert_eq!(kept, format!("…z{}", "é".repeat(ANSWER_MAX - 1)));
        // Words: from a word's start, and the end (its caveat) is kept.
        let words = format!("{}Caveat: not on the web yet.", "word ".repeat(ANSWER_MAX));
        let kept = text(&words, link).unwrap().split_once("\n\n").unwrap().1.to_owned();
        assert!(kept.starts_with("…word word") && kept.ends_with("Caveat: not on the web yet."), "{kept}");
        assert!(kept.chars().count() <= ANSWER_MAX + 1, "{}", kept.chars().count());
        // A line's start near the cut: from it.
        let lines = format!("{}\n\n**Plan**\n{}\nDone.", "a".repeat(ANSWER_MAX), "b".repeat(ANSWER_MAX - 100));
        let kept = text(&lines, link).unwrap().split_once("\n\n").unwrap().1.to_owned();
        assert_eq!(kept, format!("…**Plan**\n{}\nDone.", "b".repeat(ANSWER_MAX - 100)));
    }

    #[test]
    fn a_pushed_turn_says_what_it_pushed() {
        let turn = |pushed: Option<bool>, commits: usize| SessionDetail {
            branch: Some("arugula/1234abcd-menu".into()),
            pushed,
            commits: (0..commits).map(|i| format!("abc123{i} step {i}")).collect(),
            ..Default::default()
        };
        assert_eq!(pushed(&turn(Some(true), 3)).as_deref(), Some("Pushed 3 commits to `arugula/1234abcd-menu`."));
        assert_eq!(pushed(&turn(Some(true), 1)).as_deref(), Some("Pushed 1 commit to `arugula/1234abcd-menu`."));
        assert_eq!(pushed(&turn(Some(true), 0)).as_deref(), Some("Pushed to `arugula/1234abcd-menu`."));
        assert_eq!(pushed(&turn(Some(false), 2)), None, "unpushed: no line");
        assert_eq!(pushed(&turn(None, 2)), None, "an older box's: no line");
    }

    /// The answer: the reply, then the pushed line; a turn that pushed and
    /// said nothing posts the line alone, so the thread shows the push.
    #[test]
    fn a_pushed_turn_that_said_nothing_posts_the_pushed_line() {
        let link = "https://app.example/p/prj_1/sessions/ses_1";
        let turn = |pushed: bool, reply: Option<&str>| SessionDetail {
            branch: Some("arugula/1234abcd-menu".into()),
            pushed: Some(pushed),
            commits: vec!["abc1234 the menu".into(), "def5678 its test".into()],
            reply: reply.map(str::to_owned),
            ..Default::default()
        };
        let line = "Pushed 2 commits to `arugula/1234abcd-menu`.";
        assert_eq!(answer(&turn(true, Some("Done.")), link), Some(format!("Done.\n\n{line}")));
        assert_eq!(answer(&turn(true, None), link).as_deref(), Some(line), "no reply: the line alone");
        assert_eq!(answer(&turn(true, Some(" \n ")), link).as_deref(), Some(line), "an empty reply: the line alone");
        assert_eq!(answer(&turn(false, Some("Done.")), link).as_deref(), Some("Done."));
        assert_eq!(answer(&turn(false, None), link), None, "neither: nothing");
    }
}
