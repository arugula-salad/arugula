//! Reply threads on a message (#106). A reply is a message of the same
//! room with a `parent`, the number of its thread's first message (never a
//! reply: threads are one deep). Whoever sees the room sees its threads,
//! each from their floor, and may reply where they may post.
//!
//! - **The list:** the room's own list leaves replies out, unless a reply
//!   was also sent to the channel ([`crate::db::chat`]'s `MAIN`). A thread
//!   is read apart: `messages?thread=N`.
//! - **A parent shows its thread** ([`ChatThread`]): how many replies, the
//!   newest's time, who replied, and for the reader whether they follow and
//!   what's unread ([`Shower`] asks [`summaries`] for a page at once).
//! - **Following** ([`crate::db::chat_threads`]): the parent's author, whoever
//!   replies and whoever a reply mentions follow it; anyone can follow or
//!   stop. A thread has its own read mark and unread count, apart from the
//!   room's, and the Threads view ([`followed`]) lists them, but for the
//!   done ones: a thread whose agent session's PR merged (M265).
//! - **Push** ([`super::push`]): a follower gets each reply at their level
//!   for followed threads (`notify_prefs.threads`), unless the room is set
//!   to nothing for them. A mention in a reply is pushed as anywhere.
//! - **Live:** a reply is announced as [`ChatLive::Reply`], not as a new
//!   message of the room (its list didn't change), and as a message too when
//!   it was also sent to the channel. A move of one's own mark or following
//!   is [`ChatLive::ThreadRead`], to their other pages.
//! - Deleting the parent blanks it like any message; its thread stays.

use super::*;
use crate::db::chat_threads::{ThreadAsk, ThreadStat};
use arugula_control_wire::chat::{ChatFollow, ChatThread, ChatThreadItem, ChatThreadRead, ChatThreads, THREADS_LISTED};

/// What a parent holds for one reader, before names are put to it.
#[derive(Debug, Clone)]
pub(super) struct Summary {
    pub stat: ThreadStat,
    /// Accounts, the newest replier first.
    pub repliers: Vec<String>,
    pub following: bool,
    pub read: i64,
}

impl Summary {
    /// As the reader sees it; `named` puts a name to an account.
    pub(super) fn shown(&self, named: impl Fn(&str) -> ChatPerson, agent: bool) -> ChatThread {
        ChatThread {
            replies: self.stat.replies.min(u64::from(u32::MAX)) as u32,
            last: self.stat.last_at,
            repliers: self.repliers.iter().map(|a| named(a)).collect(),
            // An agent follows nothing and has no marks: its owner's aren't
            // its to read.
            following: self.following && !agent,
            read: if agent { 0 } else { self.read },
            unread: if self.following && !agent { self.stat.unread } else { 0 },
            mentions: if self.following && !agent { self.stat.mentions } else { 0 },
        }
    }
}

/// The threads that start at these messages, for `reader`, by (room, seq):
/// one statement for each of the replies' authors, the reader's marks and
/// the counts, per room.
pub(super) fn summaries(db: &Db, reader: &str, ms: &[&Message]) -> anyhow::Result<HashMap<(String, i64), Summary>> {
    let mut span: HashMap<&str, (i64, i64)> = HashMap::new();
    for m in ms.iter().filter(|m| m.parent.is_none() && m.imported.is_none()) {
        let e = span.entry(m.room.as_str()).or_insert((m.seq, m.seq));
        *e = (e.0.min(m.seq), e.1.max(m.seq));
    }
    let mut out = HashMap::new();
    for (room, (from, to)) in span {
        let mut repliers = db.chat_thread_repliers(room, from, to)?;
        if repliers.is_empty() {
            continue;
        }
        let marks = db.chat_thread_marks(reader, room, from, to)?;
        let asks: Vec<ThreadAsk<'_>> = repliers
            .keys()
            .map(|p| ThreadAsk { room, parent: *p, read: marks.get(p).map_or(0, |m| m.1), from: 0 })
            .collect();
        for stat in db.chat_thread_stats(reader, &asks)? {
            let Some(who) = repliers.remove(&stat.parent) else { continue };
            let (following, read) = marks.get(&stat.parent).copied().unwrap_or((false, 0));
            if stat.replies > 0 {
                out.insert((room.to_owned(), stat.parent), Summary { stat, repliers: who, following, read });
            }
        }
    }
    Ok(out)
}

/// The first message of the thread `parent` in a room, for a reader whose
/// floor is `from`: it must be there, not itself a reply, and not from
/// before they see the room. Deleted is fine: its thread stays.
pub(super) fn parent_in(app: &Platform, room: &Room, from: u64, parent: i64) -> Result<Message, ApiError> {
    let m = message_in(app, room, parent)?;
    if m.at < from {
        return Err(err(StatusCode::NOT_FOUND, "no such message"));
    }
    if m.parent.is_some() {
        return Err(err(StatusCode::BAD_REQUEST, "a thread starts at the message it replies to, not at a reply"));
    }
    Ok(m)
}

/// A page of a thread's replies, oldest first, and the message that starts
/// it (`messages?thread=N`). The messages are the caller's to show.
pub(super) fn page(
    app: &Platform,
    room: &Room,
    from: u64,
    parent: i64,
    page: Page,
    limit: u32,
) -> Result<(Message, Vec<Message>, bool), ApiError> {
    let first = parent_in(app, room, from, parent)?;
    let (ms, more) = app.db.chat_thread_messages(&room.id, parent, page, limit)?;
    Ok((first, ms.into_iter().filter(|m| m.at >= from).collect(), more))
}

/// Tell open pages a reply changed (edited, deleted, reacted to), so a
/// thread open anywhere reads it again. Nothing for a message that isn't a
/// reply: the room's own events say it. A new reply is announced where it's
/// written ([`super::post_full`]).
pub(super) fn announce(app: &Platform, m: &Message) {
    if let Some(parent) = m.parent {
        app.chat.publish(&app.db, ChatLive::Reply { room: m.room.clone(), parent, seq: m.seq }, None);
    }
}

/// The threads `account` follows in rooms they see (not the threads on
/// objects: those are read on their objects), with the stats the reader
/// has: replies, newest, unread. Threads whose parent is from before their
/// floor, and those without a reply left, are left out.
async fn followed_stats(app: &Platform, account: &str, seen: &[(Room, u64)]) -> anyhow::Result<Vec<(ThreadStat, u64)>> {
    let floors: HashMap<&str, u64> =
        seen.iter().filter(|(r, _)| r.kind != "object").map(|(r, f)| (r.id.as_str(), *f)).collect();
    // One statement for the lot; only threads in a room the reader sees from
    // a floor (a pane's, "from now") are counted again with it.
    let all = app.db.chat_followed_stats(account)?;
    let (mut out, mut floored) = (Vec::new(), Vec::new());
    for s in all.into_iter().filter(|s| s.replies > 0) {
        match floors.get(s.room.as_str()) {
            Some(0) => out.push((s, 0)),
            Some(from) => floored.push((s, *from)),
            None => {}
        }
    }
    let asks: Vec<ThreadAsk<'_>> =
        floored.iter().map(|(s, from)| ThreadAsk { room: &s.room, parent: s.parent, read: 0, from: *from }).collect();
    if !asks.is_empty() {
        // Their marks, for the unread.
        let marks: HashMap<(String, i64), i64> =
            app.db.chat_followed(account)?.into_iter().map(|(r, p, m)| ((r, p), m)).collect();
        let asks: Vec<ThreadAsk<'_>> = asks
            .into_iter()
            .map(|a| ThreadAsk { read: marks.get(&(a.room.to_owned(), a.parent)).copied().unwrap_or(0), ..a })
            .collect();
        let from_of: HashMap<(String, i64), u64> =
            floored.iter().map(|(s, f)| ((s.room.clone(), s.parent), *f)).collect();
        for s in app.db.chat_thread_stats(account, &asks)?.into_iter().filter(|s| s.replies > 0) {
            let from = from_of.get(&(s.room.clone(), s.parent)).copied().unwrap_or(0);
            out.push((s, from));
        }
    }
    // A thread whose session's PR merged is done (M265): it leaves the
    // list, and its unread the badge.
    let mut rooms: Vec<&str> = out.iter().map(|(s, _)| s.room.as_str()).collect();
    rooms.sort_unstable();
    rooms.dedup();
    let done = app.db.done_threads(&rooms)?;
    if !done.is_empty() {
        out.retain(|(s, _)| u64::try_from(s.parent).map_or(true, |p| !done.contains(&(s.room.clone(), p))));
    }
    Ok(out)
}

/// Unread replies in the followed threads, and how many mention the reader:
/// the Threads view's badge (`GET /api/chat`).
pub(super) async fn totals(app: &Platform, account: &str, seen: &[(Room, u64)]) -> anyhow::Result<(u64, u64)> {
    let stats = followed_stats(app, account, seen).await?;
    Ok(stats.iter().fold((0, 0), |(u, m), (s, _)| (u + s.unread, m + s.mentions)))
}

/// `GET /api/chat/threads`: the threads I follow, the most recently active
/// first, with their unread.
pub async fn followed(State(app): State<Arc<Platform>>, s: Person) -> J<ChatThreads> {
    gate(&app, &s.account).await?;
    let (seen, _) = rooms_seen(&app, &s.account).await?;
    let mut stats = followed_stats(&app, &s.account, &seen).await?;
    let (unread, mentions) = stats.iter().fold((0, 0), |(u, m), (s, _)| (u + s.unread, m + s.mentions));
    stats.sort_by_key(|s| std::cmp::Reverse((s.0.last_at, s.0.last_seq)));
    let mut parents = Vec::new();
    let mut latest = Vec::new();
    for (st, from) in stats.iter().take(THREADS_LISTED) {
        let (Some(p), l) = (
            app.db.chat_message(&st.room, st.parent)?.filter(|p| p.at >= *from),
            app.db.chat_message(&st.room, st.last_seq)?,
        ) else {
            continue;
        };
        parents.push(p);
        latest.push(l);
    }
    let all: Vec<&Message> = parents.iter().chain(latest.iter().flatten()).collect();
    let shower = Shower::of(&app, &s.account, &all).await?;
    let threads = parents
        .iter()
        .zip(&latest)
        .map(|(p, l)| ChatThreadItem {
            room: p.room.clone(),
            parent: shower.show(p),
            latest: l.as_ref().map(|l| shower.show(l)),
        })
        .collect();
    Ok(Json(ChatThreads { threads, unread, mentions }))
}

/// `POST /api/chat/rooms/{id}/threads/{seq}/read`: read the thread up to
/// `seq` (only forward). Answers its first message.
pub async fn read(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, parent)): Path<(String, i64)>,
    Json(b): Json<ChatThreadRead>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    let first = parent_in(&app, &room, from, parent)?;
    let seq = b.seq.min(room.last_seq);
    if app.db.set_chat_thread_read(&room.id, parent, &s.account, seq)? {
        app.chat.publish(&app.db, ChatLive::ThreadRead { room: room.id.clone(), parent, seq }, Some(s.account.clone()));
    }
    Ok(Json(shown(&app, &s.account, &first).await?))
}

/// `POST /api/chat/rooms/{id}/threads/{seq}/follow`: follow the thread, or
/// stop. Answers its first message.
pub async fn follow(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, parent)): Path<(String, i64)>,
    Json(b): Json<ChatFollow>,
) -> J<ChatMessage> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    let first = parent_in(&app, &room, from, parent)?;
    app.db.set_chat_thread_following(&room.id, parent, &s.account, b.following)?;
    app.chat.publish(
        &app.db,
        ChatLive::ThreadRead { room: room.id.clone(), parent, seq: room.last_seq },
        Some(s.account.clone()),
    );
    Ok(Json(shown(&app, &s.account, &first).await?))
}
