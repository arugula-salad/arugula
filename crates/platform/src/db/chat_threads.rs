//! Reply threads on chat messages (#106), in `chat_messages.parent` and
//! `chat_thread_reads`. A reply is a message of the same room whose
//! `parent` is its thread's first message (threads are one deep), so it has
//! its own number, text, mentions and reactions like any message, and the
//! room's list leaves it out unless it was also sent to the channel
//! (`db::chat::MAIN`). Who may see a thread is who sees its room; that, and
//! who follows what, is decided in `crate::chat`. This is storage.
//!
//! - **Following** is a row of `chat_thread_reads` with `following = TRUE`,
//!   which also holds the follower's read mark in the thread (a room
//!   `seq`, only forward). A reply makes its author a follower, and the
//!   parent's author and whoever it mentions too, unless they unfollowed
//!   (their row says so, and stays). Following by hand marks what's there
//!   already as read: it begins from now.
//! - **Unread** in a thread is the replies after the follower's mark that
//!   aren't their own (their agents' are), as a room's is.
//! - Deleting the parent blanks it like any message; its replies stay.

use std::collections::HashMap;

use super::{
    Db,
    chat::{MSG_COLS, Message, NewMessage, Page, msg_row},
    migrate::Dialect,
    sql::{Sql, params},
};
use anyhow::Result;

/// How many repliers a thread names (their avatars).
pub const REPLIERS_SHOWN: usize = 5;

/// A thread to ask about: the reader's mark in it, and from when they see
/// the room (ms).
pub struct ThreadAsk<'a> {
    pub room: &'a str,
    pub parent: i64,
    pub read: i64,
    pub from: u64,
}

/// What a thread holds, for one reader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadStat {
    pub room: String,
    pub parent: i64,
    /// Replies not deleted, the newest's time and number.
    pub replies: u64,
    pub last_at: u64,
    pub last_seq: i64,
    /// Replies after the asked mark that aren't the reader's own, and how
    /// many of those mention them.
    pub unread: u64,
    pub mentions: u64,
}

impl Db {
    /// A page of one thread's replies, oldest first, and whether there are
    /// more that way (deleted ones are in it, blank, like the room's).
    pub fn chat_thread_messages(
        &self,
        room: &str,
        parent: i64,
        page: Page,
        limit: u32,
    ) -> Result<(Vec<Message>, bool)> {
        let c = self.c();
        let n = i64::from(limit) + 1;
        let mut ms: Vec<Message> = match page {
            Page::After(after) => c.query_map(
                &format!(
                    "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND parent = ?2 AND seq > ?3
                     ORDER BY seq LIMIT ?4"
                ),
                params![room, parent, after, n],
                msg_row,
            )?,
            Page::Before(before) => c.query_map(
                &format!(
                    "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND parent = ?2 AND seq < ?3
                     ORDER BY seq DESC LIMIT ?4"
                ),
                params![room, parent, before, n],
                msg_row,
            )?,
            Page::Newest => c.query_map(
                &format!(
                    "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND parent = ?2 ORDER BY seq DESC LIMIT ?3"
                ),
                params![room, parent, n],
                msg_row,
            )?,
        };
        let more = ms.len() > limit as usize;
        ms.truncate(limit as usize);
        if !matches!(page, Page::After(_)) {
            ms.reverse();
        }
        Ok((ms, more))
    }

    /// What these threads hold for `account`, each with its own mark and
    /// floor, in one statement; a thread with no replies is in the answer
    /// with zeros.
    pub fn chat_thread_stats(&self, account: &str, asks: &[ThreadAsk<'_>]) -> Result<Vec<ThreadStat>> {
        if asks.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let list = serde_json::to_string(
            &asks
                .iter()
                .map(|a| serde_json::json!({"room": a.room, "parent": a.parent, "since": a.read, "floor": a.from}))
                .collect::<Vec<_>>(),
        )?;
        let (want, mentioned) = match c.dialect() {
            Dialect::Sqlite => (
                "SELECT json_extract(value, '$.room') AS room, json_extract(value, '$.parent') AS parent,
                     json_extract(value, '$.since') AS since, json_extract(value, '$.floor') AS floor
                 FROM json_each(?2)",
                "EXISTS (SELECT 1 FROM json_each(m.mentions) WHERE value = ?1)",
            ),
            Dialect::Postgres => (
                "SELECT * FROM jsonb_to_recordset(CAST(?2 AS jsonb))
                 AS w(room text, parent bigint, since bigint, floor bigint)",
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text(m.mentions::jsonb) v WHERE v = ?1)",
            ),
        };
        // Someone else's reply after the mark; imported messages count as read.
        let theirs = "m.seq > w.since AND (m.author != ?1 OR m.agent IS NOT NULL) AND m.imported_id IS NULL";
        c.query_map(
            &format!(
                "SELECT w.room, w.parent, count(m.seq), CAST(coalesce(max(m.at), 0) AS BIGINT),
                     CAST(coalesce(max(m.seq), 0) AS BIGINT),
                     CAST(coalesce(sum(CASE WHEN {theirs} THEN 1 ELSE 0 END), 0) AS BIGINT),
                     CAST(coalesce(sum(CASE WHEN {theirs} AND {mentioned} THEN 1 ELSE 0 END), 0) AS BIGINT)
                 FROM ({want}) w
                 LEFT JOIN chat_messages m ON m.room = w.room AND m.parent = w.parent
                     AND m.deleted IS NULL AND m.at >= w.floor
                 GROUP BY w.room, w.parent"
            ),
            params![account, list],
            |r| {
                Ok(ThreadStat {
                    room: r.get(0)?,
                    parent: r.get(1)?,
                    replies: r.get(2)?,
                    last_at: r.get::<i64>(3)? as u64,
                    last_seq: r.get(4)?,
                    unread: r.get(5)?,
                    mentions: r.get(6)?,
                })
            },
        )
    }

    /// What every thread `account` follows holds for them, floors aside, in
    /// one statement (`GET /api/chat`'s badge): as [`chat_thread_stats`]
    /// (Self::chat_thread_stats) with each thread's own mark.
    pub fn chat_followed_stats(&self, account: &str) -> Result<Vec<ThreadStat>> {
        let c = self.c();
        let mentioned = match c.dialect() {
            Dialect::Sqlite => "EXISTS (SELECT 1 FROM json_each(m.mentions) WHERE value = ?1)",
            Dialect::Postgres => "EXISTS (SELECT 1 FROM jsonb_array_elements_text(m.mentions::jsonb) v WHERE v = ?1)",
        };
        let theirs = "m.seq > w.seq AND (m.author != ?1 OR m.agent IS NOT NULL) AND m.imported_id IS NULL";
        c.query_map(
            &format!(
                "SELECT w.room, w.parent, count(m.seq), CAST(coalesce(max(m.at), 0) AS BIGINT),
                     CAST(coalesce(max(m.seq), 0) AS BIGINT),
                     CAST(coalesce(sum(CASE WHEN {theirs} THEN 1 ELSE 0 END), 0) AS BIGINT),
                     CAST(coalesce(sum(CASE WHEN {theirs} AND {mentioned} THEN 1 ELSE 0 END), 0) AS BIGINT)
                 FROM chat_thread_reads w
                 LEFT JOIN chat_messages m ON m.room = w.room AND m.parent = w.parent AND m.deleted IS NULL
                 WHERE w.account = ?1 AND w.following = TRUE
                 GROUP BY w.room, w.parent"
            ),
            params![account],
            |r| {
                Ok(ThreadStat {
                    room: r.get(0)?,
                    parent: r.get(1)?,
                    replies: r.get(2)?,
                    last_at: r.get::<i64>(3)? as u64,
                    last_seq: r.get(4)?,
                    unread: r.get(5)?,
                    mentions: r.get(6)?,
                })
            },
        )
    }

    /// Who replied in threads `from..=to` of a room: each thread's
    /// repliers, the newest first, at most [`REPLIERS_SHOWN`]. Deleted
    /// replies don't count.
    pub fn chat_thread_repliers(&self, room: &str, from: i64, to: i64) -> Result<HashMap<i64, Vec<String>>> {
        let rows: Vec<(i64, String)> = self.c().query_map(
            "SELECT parent, author FROM (
                 SELECT parent, author, max(seq) AS newest FROM chat_messages
                 WHERE room = ?1 AND parent >= ?2 AND parent <= ?3 AND deleted IS NULL AND author != ''
                 GROUP BY parent, author
             ) t ORDER BY parent, newest DESC",
            params![room, from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for (parent, author) in rows {
            let v = out.entry(parent).or_default();
            if v.len() < REPLIERS_SHOWN {
                v.push(author);
            }
        }
        Ok(out)
    }

    /// `account`'s place in threads `from..=to` of a room: whether they
    /// follow, and their read mark.
    pub fn chat_thread_marks(
        &self,
        account: &str,
        room: &str,
        from: i64,
        to: i64,
    ) -> Result<HashMap<i64, (bool, i64)>> {
        let rows = self.c().query_map(
            "SELECT parent, following, seq FROM chat_thread_reads
             WHERE room = ?1 AND account = ?2 AND parent >= ?3 AND parent <= ?4",
            params![room, account, from, to],
            |r| Ok((r.get::<i64>(0)?, (r.get::<bool>(1)?, r.get::<i64>(2)?))),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// The threads `account` follows, as (room, parent, read mark).
    pub fn chat_followed(&self, account: &str) -> Result<Vec<(String, i64, i64)>> {
        self.c().query_map(
            "SELECT room, parent, seq FROM chat_thread_reads WHERE account = ?1 AND following = TRUE",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// Whoever follows a thread, to push and to count for.
    pub fn chat_thread_followers(&self, room: &str, parent: i64) -> Result<Vec<String>> {
        self.c().query_map(
            "SELECT account FROM chat_thread_reads WHERE room = ?1 AND parent = ?2 AND following = TRUE",
            params![room, parent],
            |r| r.get(0),
        )
    }

    /// Follow a thread, or stop. Following begins from now: what's in the
    /// thread already is read.
    pub fn set_chat_thread_following(&self, room: &str, parent: i64, account: &str, following: bool) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let newest: i64 = tx.query_row(
            "SELECT CAST(coalesce(max(seq), 0) AS BIGINT) FROM chat_messages WHERE room = ?1 AND parent = ?2",
            params![room, parent],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO chat_thread_reads (room, parent, account, following, seq) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (room, parent, account) DO UPDATE SET following = excluded.following,
                 seq = CASE WHEN excluded.seq > chat_thread_reads.seq AND excluded.following = TRUE
                            THEN excluded.seq ELSE chat_thread_reads.seq END",
            params![room, parent, account, following, newest],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Move a thread read mark forward (never back). Whether it moved; a
    /// reader with no place in the thread has no mark to move.
    pub fn set_chat_thread_read(&self, room: &str, parent: i64, account: &str, seq: i64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE chat_thread_reads SET seq = ?4 WHERE room = ?1 AND parent = ?2 AND account = ?3 AND seq < ?4",
            params![room, parent, account, seq],
        )? > 0)
    }
}

/// A reply was added: its author follows the thread and has read it up to
/// here (unless an agent wrote it: its owner hasn't), and the parent's
/// author and whom it mentions follow too, with this reply unread, unless
/// they unfollowed.
pub(super) fn replied(tx: &impl Sql, room: &str, parent: i64, seq: i64, m: &NewMessage) -> Result<()> {
    if m.agent.is_none() {
        tx.execute(
            "INSERT INTO chat_thread_reads (room, parent, account, following, seq) VALUES (?1, ?2, ?3, TRUE, ?4)
             ON CONFLICT (room, parent, account) DO UPDATE SET following = TRUE,
                 seq = CASE WHEN excluded.seq > chat_thread_reads.seq THEN excluded.seq ELSE chat_thread_reads.seq END",
            params![room, parent, m.author, seq],
        )?;
    }
    let mut others: Vec<String> = tx.query_map(
        "SELECT author FROM chat_messages WHERE room = ?1 AND seq = ?2 AND author != ''",
        params![room, parent],
        |r| r.get(0),
    )?;
    // The agents' mentions (`machine:<id>`) aren't accounts.
    others.extend(m.mentions.iter().filter(|a| !a.contains(':')).cloned());
    for who in others {
        tx.execute(
            "INSERT INTO chat_thread_reads (room, parent, account, following, seq) VALUES (?1, ?2, ?3, TRUE, ?4)
             ON CONFLICT (room, parent, account) DO NOTHING",
            params![room, parent, who, seq - 1],
        )?;
    }
    Ok(())
}
