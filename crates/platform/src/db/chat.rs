//! Chat (M122): rooms, their messages in plain text, read marks, an FTS5
//! index, and who may read each pane's or session's thread as its daemon
//! last said. Who sees what is decided in `crate::chat`; this is storage.
//! The platform's since M138: it moved here from control's `db/chat.rs`,
//! and never joins a hub table (a team channel's project is the team's
//! default one, `crate::projects::team_project`). Search is FTS5's
//! (`chat_fts`) on SQLite and a `tsvector` column on Postgres (M143),
//! matching the same words ([`Search`]). The tables, with notes on their
//! columns, are made by the platform's migrations (`migrations/`).
//!
//! - A room's `scope` is what it belongs to, and what deleting takes it
//!   with: `team:<id>` (a channel or a DM), `machine:<id>` (the thread on a
//!   pane or a session there), or `account:<id>` (an archive, M124).
//! - Control numbers messages per room (`seq`), in the same transaction
//!   that moves the room's `last_seq`. M124's import numbers old messages
//!   0 and below, before the room's own.
//! - A deleted message keeps its row and number, with no text, quote, pane
//!   or mentions, so numbers and read marks hold.

use super::{
    Db,
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, list, params},
    taken,
};
use anyhow::Result;
use arugula_control_wire::chat::ChatReader;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub scope: String,
    pub kind: String,
    pub name: String,
    pub machine: Option<String>,
    pub target: Option<String>,
    pub created_by: String,
    pub created: u64,
    pub archived: Option<u64>,
    pub last_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub room: String,
    pub seq: i64,
    pub author: String,
    pub at: u64,
    pub text: String,
    pub quote: Option<String>,
    pub ref_machine: Option<String>,
    pub ref_pane: Option<String>,
    pub mentions: Vec<String>,
    pub edited: Option<u64>,
    pub deleted: Option<u64>,
    pub imported: Option<String>,
}

/// How many of chat's live events are kept for pages that resume
/// (`Last-Event-ID`, M144): a page that missed more resyncs.
pub const CHAT_EVENTS_KEPT: u64 = 10_000;

/// One of chat's live events, to add (M144): `live` with the event's JSON
/// in `body` (and `to`, whom only it goes to), or `changed`, empty.
pub struct NewChatEvent<'a> {
    pub kind: &'a str,
    pub body: &'a str,
    pub to: Option<&'a str>,
    pub at: u64,
}

/// One of chat's live events, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    pub seq: u64,
    pub kind: String,
    pub body: String,
    pub to: Option<String>,
}

pub struct NewMessage<'a> {
    pub author: &'a str,
    pub at: u64,
    pub text: &'a str,
    pub quote: Option<&'a str>,
    pub ref_machine: Option<&'a str>,
    pub ref_pane: Option<&'a str>,
    pub mentions: &'a [String],
}

/// A message brought in from a machine (M124): its number there, and the
/// name its author had there.
pub struct OldMessage<'a> {
    pub id: u64,
    pub name: &'a str,
    pub m: NewMessage<'a>,
}

/// Which page of a room's messages.
#[derive(Debug, Clone, Copy)]
pub enum Page {
    After(i64),
    Before(i64),
    Newest,
}

/// A word searched for: messages match when they have it (or, `prefix`,
/// a word starting with it). Words are split where FTS5's unicode61 splits
/// them, at anything that isn't a letter or a digit: `main.rs` is the two
/// words `main` and `rs`, next to each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub prefix: bool,
}

/// What a search asks for, beside the rooms.
#[derive(Debug, Default)]
pub struct Search<'a> {
    /// Words a message has all of; none: any message.
    pub words: &'a [Word],
    pub author: Option<&'a str>,
    pub has_quote: bool,
}

const ROOM_COLS: &str = "id, scope, kind, name, machine, target, created_by, created, archived, last_seq";
const MSG_COLS: &str = "room, seq, author, at, text, quote, ref_machine, ref_pane, mentions, edited, deleted, imported";

fn room_row(r: &Row) -> Result<Room> {
    Ok(Room {
        id: r.get(0)?,
        scope: r.get(1)?,
        kind: r.get(2)?,
        name: r.get(3)?,
        machine: r.get(4)?,
        target: r.get(5)?,
        created_by: r.get(6)?,
        created: r.get(7)?,
        archived: r.get(8)?,
        last_seq: r.get(9)?,
    })
}

fn msg_row(r: &Row) -> Result<Message> {
    let mentions: String = r.get(8)?;
    Ok(Message {
        room: r.get(0)?,
        seq: r.get(1)?,
        author: r.get(2)?,
        at: r.get(3)?,
        text: r.get(4)?,
        quote: r.get(5)?,
        ref_machine: r.get(6)?,
        ref_pane: r.get(7)?,
        mentions: serde_json::from_str(&mentions).unwrap_or_default(),
        edited: r.get(9)?,
        deleted: r.get(10)?,
        imported: r.get(11)?,
    })
}

fn marks(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// The words as an FTS5 query: each a phrase (`"mean"`), or a prefix
/// (`"mea"*`), so FTS5's syntax is only ever text inside one.
pub fn fts5_query(words: &[Word]) -> String {
    let phrase = |w: &Word| {
        let t: String = w.text.chars().filter(|c| *c != '"').collect();
        if w.prefix { format!("\"{t}\"*") } else { format!("\"{t}\"") }
    };
    words.iter().map(phrase).collect::<Vec<_>>().join(" ")
}

/// The words as Postgres's `tsquery` (for `to_tsquery('simple', ...)`):
/// each word's parts in a row (`'main' <-> 'rs'`), the last one a prefix
/// (`:*`) when it is, and every word (`&`). Only letters and digits are
/// left, so its syntax is never the person's. `None`: no word has a part,
/// and nothing matches (as FTS5's empty phrase).
pub fn tsquery(words: &[Word]) -> Option<String> {
    let mut all = Vec::new();
    for w in words {
        let parts: Vec<&str> = w.text.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty()).collect();
        let last = parts.len().checked_sub(1)?;
        let parts: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, p)| if w.prefix && i == last { format!("'{p}':*") } else { format!("'{p}'") })
            .collect();
        all.push(parts.join(" <-> "));
    }
    Some(all.join(" & "))
}

impl Db {
    pub fn chat_room(&self, id: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(&format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE id = ?1"), params![id], room_row)
            .optional()
    }

    /// The thread on a pane or session.
    pub fn chat_room_for(&self, kind: &str, machine: &str, target: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(
                &format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = ?1 AND machine = ?2 AND target = ?3"),
                params![kind, machine, target],
                room_row,
            )
            .optional()
    }

    /// The DM between two people in a scope.
    pub fn chat_dm(&self, scope: &str, a: &str, b: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms r WHERE scope = ?1 AND kind = 'dm'
                     AND EXISTS (SELECT 1 FROM chat_members WHERE room = r.id AND account = ?2)
                     AND EXISTS (SELECT 1 FROM chat_members WHERE room = r.id AND account = ?3)"
                ),
                params![scope, a, b],
                room_row,
            )
            .optional()
    }

    /// A new room, with its members (a DM's two people), in `project`: a
    /// team's channel belongs to the team's default project
    /// (`crate::projects::team_project`); DMs and threads to none. Fails with
    /// [`super::Taken`] for a channel name or a pane's room already there.
    pub fn add_chat_room(&self, room: &Room, members: &[&str], project: Option<&str>) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO chat_rooms (id, scope, kind, name, machine, target, created_by, created, project_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                room.id,
                room.scope,
                room.kind,
                room.name,
                room.machine,
                room.target,
                room.created_by,
                room.created,
                project
            ],
        )
        .map_err(taken)?;
        for a in members {
            tx.execute(
                "INSERT INTO chat_members (room, account) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                params![room.id, a],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Channels and DMs in these scopes, and the threads on these machines.
    pub fn chat_rooms_in(&self, scopes: &[String], machines: &[String]) -> Result<Vec<Room>> {
        let c = self.c();
        let mut out = Vec::new();
        if !scopes.is_empty() {
            out.extend(c.query_map(
                &format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms WHERE scope IN ({}) ORDER BY created",
                    marks(scopes.len())
                ),
                &list(scopes),
                room_row,
            )?);
        }
        if !machines.is_empty() {
            out.extend(c.query_map(
                &format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms WHERE machine IN ({}) ORDER BY created",
                    marks(machines.len())
                ),
                &list(machines),
                room_row,
            )?);
        }
        out.dedup_by(|a, b| a.id == b.id);
        Ok(out)
    }

    /// Rooms an account is a member of by row: its DMs and archives.
    pub fn chat_rooms_member(&self, account: &str) -> Result<Vec<Room>> {
        self.c().query_map(
            &format!(
                "SELECT {ROOM_COLS} FROM chat_rooms WHERE id IN (SELECT room FROM chat_members WHERE account = ?1)
                 ORDER BY created"
            ),
            params![account],
            room_row,
        )
    }

    pub fn chat_members(&self, room: &str) -> Result<Vec<String>> {
        self.c()
            .query_map("SELECT account FROM chat_members WHERE room = ?1 ORDER BY account", params![room], |r| r.get(0))
    }

    /// How many channels a scope has.
    pub fn chat_channels(&self, scope: &str) -> Result<u64> {
        self.c().query_row(
            "SELECT count(*) FROM chat_rooms WHERE scope = ?1 AND kind = 'channel'",
            params![scope],
            |r| r.get(0),
        )
    }

    /// How many rooms a scope has (pane threads on a machine).
    pub fn chat_rooms_count(&self, scope: &str) -> Result<u64> {
        self.c().query_row("SELECT count(*) FROM chat_rooms WHERE scope = ?1", params![scope], |r| r.get(0))
    }

    /// The text and quotes a scope's rooms hold.
    pub fn chat_bytes(&self, scope: &str) -> Result<u64> {
        self.c().query_row(
            "SELECT CAST(coalesce(sum(bytes), 0) AS BIGINT) FROM chat_rooms WHERE scope = ?1",
            params![scope],
            |r| r.get(0),
        )
    }

    pub fn rename_chat_room(&self, id: &str, name: &str) -> Result<()> {
        self.c().execute("UPDATE chat_rooms SET name = ?2 WHERE id = ?1", params![id, name]).map_err(taken)?;
        Ok(())
    }

    pub fn archive_chat_room(&self, id: &str, at: Option<u64>) -> Result<()> {
        self.c().execute("UPDATE chat_rooms SET archived = ?2 WHERE id = ?1", params![id, at])?;
        Ok(())
    }

    /// Append a message: control numbers it, and its author has read it.
    /// Its number.
    pub fn add_chat_message(&self, room: &str, m: &NewMessage) -> Result<i64> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let bytes = (m.text.len() + m.quote.map_or(0, str::len)) as i64;
        let seq: i64 = tx.query_row(
            "UPDATE chat_rooms SET last_seq = last_seq + 1, bytes = bytes + ?2 WHERE id = ?1 RETURNING last_seq",
            params![room, bytes],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO chat_messages (room, seq, author, at, text, quote, ref_machine, ref_pane, mentions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                room,
                seq,
                m.author,
                m.at,
                m.text,
                m.quote,
                m.ref_machine,
                m.ref_pane,
                serde_json::to_string(m.mentions)?
            ],
        )?;
        tx.execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE
             SET seq = CASE WHEN excluded.seq > chat_reads.seq THEN excluded.seq ELSE chat_reads.seq END",
            params![room, m.author, seq],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    /// A page of a room's messages, oldest first, and whether there are
    /// more that way.
    pub fn chat_messages(&self, room: &str, page: Page, limit: u32) -> Result<(Vec<Message>, bool)> {
        let c = self.c();
        let n = i64::from(limit) + 1;
        let mut ms: Vec<Message> = match page {
            Page::After(after) => c.query_map(
                &format!("SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"),
                params![room, after, n],
                msg_row,
            )?,
            Page::Before(before) => c.query_map(
                &format!(
                    "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT ?3"
                ),
                params![room, before, n],
                msg_row,
            )?,
            Page::Newest => c.query_map(
                &format!("SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 ORDER BY seq DESC LIMIT ?2"),
                params![room, n],
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

    /// Bring in a machine's old messages (M124), each once by its old
    /// number (`imported_id`): those the room has already are left out.
    /// While the room holds nothing of control's own, they keep their
    /// numbers. Once it does, every message brought in goes before
    /// control's, numbered up to 0 in their old order (S41 q6), so a later
    /// part of an import still lands after the earlier ones. How many were
    /// added, and how many it had.
    pub fn import_chat_messages(&self, room: &str, old: &[OldMessage]) -> Result<(usize, usize)> {
        // Far below any number a room gives, while they're put in order.
        const PARKED: i64 = -(1 << 50);
        let mut c = self.c();
        let tx = c.transaction()?;
        // One import or post in the room at a time (M144): both move the
        // room's row.
        tx.query_row(&format!("SELECT id FROM chat_rooms WHERE id = ?1{}", tx.for_update()), params![room], |r| {
            r.get::<String>(0)
        })?;
        let have: std::collections::HashSet<i64> = tx
            .query_map(
                "SELECT imported_id FROM chat_messages WHERE room = ?1 AND imported_id IS NOT NULL",
                params![room],
                |r| r.get(0),
            )?
            .into_iter()
            .collect();
        let mut new: Vec<&OldMessage> = old.iter().filter(|o| !have.contains(&(o.id as i64))).collect();
        new.sort_by_key(|o| o.id);
        new.dedup_by_key(|o| o.id);
        let already = old.len() - new.len();
        if new.is_empty() {
            return Ok((0, already));
        }
        let own: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM chat_messages WHERE room = ?1 AND imported_id IS NULL)",
            params![room],
            |r| r.get(0),
        )?;
        if own {
            tx.execute(
                "UPDATE chat_messages SET seq = ?2 - imported_id WHERE room = ?1 AND imported_id IS NOT NULL",
                params![room, PARKED],
            )?;
        }
        let mut bytes = 0i64;
        for o in &new {
            let m = &o.m;
            let id = o.id as i64;
            tx.execute(
                "INSERT INTO chat_messages (room, seq, author, at, text, quote, ref_machine, ref_pane, mentions,
                 imported, imported_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    room,
                    if own { PARKED - id } else { id },
                    m.author,
                    m.at,
                    m.text,
                    m.quote,
                    m.ref_machine,
                    m.ref_pane,
                    serde_json::to_string(m.mentions)?,
                    o.name,
                    id
                ],
            )?;
            bytes += (m.text.len() + m.quote.map_or(0, str::len)) as i64;
        }
        let last = if own {
            // All of them, in their old order, up to 0.
            tx.execute(
                "UPDATE chat_messages SET seq = (
                     SELECT n FROM (
                         SELECT id, row_number() OVER (ORDER BY imported_id)
                             - (SELECT count(*) FROM chat_messages WHERE room = ?1 AND imported_id IS NOT NULL) AS n
                         FROM chat_messages WHERE room = ?1 AND imported_id IS NOT NULL
                     ) x WHERE x.id = chat_messages.id)
                 WHERE room = ?1 AND imported_id IS NOT NULL",
                params![room],
            )?;
            0
        } else {
            new.last().map_or(0, |o| o.id as i64)
        };
        tx.execute(
            "UPDATE chat_rooms SET last_seq = CASE WHEN last_seq > ?2 THEN last_seq ELSE ?2 END, bytes = bytes + ?3
             WHERE id = ?1",
            params![room, last, bytes],
        )?;
        tx.commit()?;
        Ok((new.len(), already))
    }

    /// An archive's reader (M124), from when they read it (ms).
    pub fn set_chat_member_from(&self, room: &str, account: &str, from: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_members (room, account, from_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET from_ms = excluded.from_ms",
            params![room, account, from as i64],
        )?;
        Ok(())
    }

    /// From when a member of a room reads it (ms); `None`: not a member.
    pub fn chat_member_from(&self, room: &str, account: &str) -> Result<Option<u64>> {
        self.c()
            .query_row(
                "SELECT from_ms FROM chat_members WHERE room = ?1 AND account = ?2",
                params![room, account],
                |r| r.get(0),
            )
            .optional()
    }

    pub fn chat_message(&self, room: &str, seq: i64) -> Result<Option<Message>> {
        self.c()
            .query_row(
                &format!("SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq = ?2"),
                params![room, seq],
                msg_row,
            )
            .optional()
    }

    /// The author's new text, and whom it mentions now.
    pub fn edit_chat_message(&self, room: &str, seq: i64, text: &str, mentions: &[String], at: u64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let old: i64 = tx.query_row(
            &format!("SELECT octet_length(text) FROM chat_messages WHERE room = ?1 AND seq = ?2{}", tx.for_update()),
            params![room, seq],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE chat_messages SET text = ?3, mentions = ?4, edited = ?5 WHERE room = ?1 AND seq = ?2",
            params![room, seq, text, serde_json::to_string(mentions)?, at],
        )?;
        tx.execute(
            "UPDATE chat_rooms SET bytes = CASE WHEN bytes - ?2 + ?3 > 0 THEN bytes - ?2 + ?3 ELSE 0 END WHERE id = ?1",
            params![room, old, text.len() as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a message: its row and number stay, with nothing in them.
    pub fn delete_chat_message(&self, room: &str, seq: i64, at: u64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let old: i64 = tx.query_row(
            &format!(
                "SELECT octet_length(text) + coalesce(octet_length(quote), 0) FROM chat_messages
                 WHERE room = ?1 AND seq = ?2{}",
                tx.for_update()
            ),
            params![room, seq],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE chat_messages SET text = '', quote = NULL, ref_machine = NULL, ref_pane = NULL,
             mentions = '[]', deleted = ?3 WHERE room = ?1 AND seq = ?2",
            params![room, seq, at],
        )?;
        tx.execute(
            "UPDATE chat_rooms SET bytes = CASE WHEN bytes > ?2 THEN bytes - ?2 ELSE 0 END WHERE id = ?1",
            params![room, old],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Add one of chat's live events (M144), numbered after every other,
    /// and wake the platform's other machines to read it. One at a time
    /// (the `chat_events` lock, on Postgres), so they commit in order and
    /// a reader that has read up to a number never misses an earlier one.
    pub fn add_chat_event(&self, e: &NewChatEvent) -> Result<u64> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.lock("chat_events")?;
        let seq: u64 = tx.query_row(
            "INSERT INTO chat_events (kind, body, to_account, at) VALUES (?1, ?2, ?3, ?4) RETURNING seq",
            params![e.kind, e.body, e.to, e.at],
            |r| r.get(0),
        )?;
        tx.notify(&format!("chat {seq}"))?;
        tx.commit()?;
        Ok(seq)
    }

    /// Chat's live events after `after`, oldest first, at most `limit`.
    pub fn chat_events_after(&self, after: u64, limit: u32) -> Result<Vec<ChatEvent>> {
        self.c().query_map(
            "SELECT seq, kind, body, to_account FROM chat_events WHERE seq > ?1 ORDER BY seq LIMIT ?2",
            params![after, limit],
            |r| Ok(ChatEvent { seq: r.get(0)?, kind: r.get(1)?, body: r.get(2)?, to: r.get(3)? }),
        )
    }

    /// The oldest and newest live events kept: `(0, 0)` with none.
    pub fn chat_event_bounds(&self) -> Result<(u64, u64)> {
        self.c().query_row("SELECT coalesce(min(seq), 0), coalesce(max(seq), 0) FROM chat_events", params![], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
    }

    /// Keep only the newest [`CHAT_EVENTS_KEPT`] live events (never the
    /// newest, so a number isn't given twice). How many went.
    pub fn prune_chat_events(&self) -> Result<usize> {
        self.c().execute(
            "DELETE FROM chat_events WHERE seq <= (SELECT max(seq) FROM chat_events) - ?1",
            params![CHAT_EVENTS_KEPT],
        )
    }

    /// An account's read mark in a room, and whether it's muted.
    pub fn chat_mark(&self, room: &str, account: &str) -> Result<(i64, bool)> {
        Ok(self
            .c()
            .query_row(
                "SELECT seq, muted FROM chat_reads WHERE room = ?1 AND account = ?2",
                params![room, account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, false)))
    }

    /// Move a read mark forward (never back). Whether it moved.
    pub fn set_chat_read(&self, room: &str, account: &str, seq: i64) -> Result<bool> {
        Ok(self.c().execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET seq = excluded.seq WHERE excluded.seq > chat_reads.seq",
            params![room, account, seq],
        )? > 0)
    }

    pub fn set_chat_muted(&self, room: &str, account: &str, muted: bool) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_reads (room, account, muted) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET muted = excluded.muted",
            params![room, account, muted],
        )?;
        Ok(())
    }

    /// Others' messages after `read` (from `from` ms on), and how many of
    /// them mention `account`. Messages brought in from a machine (M124)
    /// count as read.
    pub fn chat_unread(&self, room: &str, account: &str, read: i64, from: u64) -> Result<(u64, u64)> {
        let c = self.c();
        let mentioned = match c.dialect() {
            Dialect::Sqlite => "EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?3)",
            Dialect::Postgres => "EXISTS (SELECT 1 FROM jsonb_array_elements_text(mentions::jsonb) v WHERE v = ?3)",
        };
        c.query_row(
            &format!(
                "SELECT count(*), CAST(coalesce(sum(CASE WHEN {mentioned} THEN 1 ELSE 0 END), 0) AS BIGINT)
                 FROM chat_messages WHERE room = ?1 AND seq > ?2 AND author != ?3 AND deleted IS NULL AND at >= ?4
                 AND imported_id IS NULL"
            ),
            params![room, read, account, from],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// Messages in these rooms that match, newest first.
    pub fn chat_search(&self, rooms: &[String], s: &Search, limit: u32) -> Result<Vec<Message>> {
        if rooms.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let cols = MSG_COLS.split(", ").map(|c| format!("m.{c}")).collect::<Vec<_>>().join(", ");
        let mut sql = format!("SELECT {cols} FROM chat_messages m");
        let mut args: Vec<String> = Vec::new();
        let mut wh = vec![format!("m.room IN ({})", marks(rooms.len())), "m.deleted IS NULL".to_owned()];
        if !s.words.is_empty() {
            match c.dialect() {
                Dialect::Sqlite => {
                    sql.push_str(" JOIN chat_fts f ON f.rowid = m.id");
                    wh.insert(0, "chat_fts MATCH ?".into());
                    args.push(fts5_query(s.words));
                }
                Dialect::Postgres => match tsquery(s.words) {
                    Some(q) => {
                        wh.insert(0, "m.search @@ to_tsquery('simple', ?)".into());
                        args.push(q);
                    }
                    None => return Ok(vec![]),
                },
            }
        }
        args.extend(rooms.iter().cloned());
        if let Some(a) = s.author {
            wh.push("m.author = ?".into());
            args.push(a.to_owned());
        }
        if s.has_quote {
            wh.push("m.quote IS NOT NULL AND m.quote != ''".into());
        }
        sql.push_str(&format!(" WHERE {} ORDER BY m.at DESC, m.id DESC LIMIT {limit}", wh.join(" AND ")));
        c.query_map(&sql, &list(&args), msg_row)
    }

    /// Who may read the thread on a pane or session, as its machine last
    /// said; `None` before it has said.
    pub fn chat_readers(&self, machine: &str, kind: &str, target: &str) -> Result<Option<Vec<ChatReader>>> {
        let body: Option<String> = self
            .c()
            .query_row(
                "SELECT readers FROM chat_readers WHERE machine = ?1 AND kind = ?2 AND target = ?3",
                params![machine, kind, target],
                |r| r.get(0),
            )
            .optional()?;
        Ok(body.map(|b| serde_json::from_str(&b)).transpose()?)
    }

    pub fn set_chat_readers(
        &self,
        machine: &str,
        kind: &str,
        target: &str,
        readers: &[ChatReader],
        at: u64,
    ) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_readers (machine, kind, target, readers, at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (machine, kind, target) DO UPDATE SET readers = excluded.readers, at = excluded.at",
            params![machine, kind, target, serde_json::to_string(readers)?, at],
        )?;
        Ok(())
    }
}

/// Every room in a scope, and all that's in them.
fn drop_rooms_where(tx: &impl Sql, cond: &str, arg: &str) -> Result<()> {
    // Which first: `cond` may read `chat_members`, which goes too.
    let ids: Vec<String> =
        tx.query_map(&format!("SELECT id FROM chat_rooms WHERE {cond}"), params![arg], |r| r.get(0))?;
    for id in ids {
        for table in ["chat_messages", "chat_reads", "chat_members"] {
            tx.execute(&format!("DELETE FROM {table} WHERE room = ?1"), params![id])?;
        }
        tx.execute("DELETE FROM chat_rooms WHERE id = ?1", params![id])?;
    }
    Ok(())
}

/// What deleting an account takes from chat (S41 decision 6), inside the
/// platform's transaction once the hub has deleted it
/// (`PlatformPort::account_deleted`):
/// - its DMs, both sides, and the threads on its machines;
/// - the rooms of the teams that go with it;
/// - its messages elsewhere keep their rows and numbers, blank and with no
///   author ("a deleted account");
/// - it leaves no trace in others' mentions, read marks or reader lists.
pub(crate) fn erase_account(tx: &impl Sql, account: &str, machines: &[String], disband: &[String]) -> Result<()> {
    drop_rooms_where(tx, "kind = 'dm' AND id IN (SELECT room FROM chat_members WHERE account = ?1)", account)?;
    for m in machines {
        drop_rooms_where(tx, "scope = 'machine:' || ?1", m)?;
        tx.execute("DELETE FROM chat_readers WHERE machine = ?1", params![m])?;
    }
    for t in disband {
        drop_rooms_where(tx, "scope = 'team:' || ?1", t)?;
    }
    // Its machines' old conversations, archived under it (M124).
    drop_rooms_where(tx, "scope = 'account:' || ?1", account)?;
    tx.execute(
        "UPDATE chat_messages SET author = '', text = '', quote = NULL, ref_machine = NULL, ref_pane = NULL,
         mentions = '[]', imported = NULL, deleted = coalesce(deleted, ?2) WHERE author = ?1",
        params![account, arugula_e2e::now_ms()],
    )?;
    // Out of others' mentions and reader lists (JSON lists in text).
    let (mentions, readers) = match tx.dialect() {
        Dialect::Sqlite => (
            "UPDATE chat_messages SET mentions = (SELECT json_group_array(value) FROM json_each(mentions) WHERE value != ?1)
             WHERE EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?1)",
            "UPDATE chat_readers SET readers = (SELECT json_group_array(json(value)) FROM json_each(readers)
                 WHERE json_extract(value, '$.account') != ?1)
             WHERE EXISTS (SELECT 1 FROM json_each(readers) WHERE json_extract(value, '$.account') = ?1)",
        ),
        Dialect::Postgres => (
            "UPDATE chat_messages SET mentions = (SELECT coalesce(jsonb_agg(v ORDER BY n), '[]'::jsonb)::text
                 FROM jsonb_array_elements_text(mentions::jsonb) WITH ORDINALITY e(v, n) WHERE v != ?1)
             WHERE EXISTS (SELECT 1 FROM jsonb_array_elements_text(mentions::jsonb) v WHERE v = ?1)",
            "UPDATE chat_readers SET readers = (SELECT coalesce(jsonb_agg(v ORDER BY n), '[]'::jsonb)::text
                 FROM jsonb_array_elements(readers::jsonb) WITH ORDINALITY e(v, n) WHERE v->>'account' != ?1)
             WHERE EXISTS (SELECT 1 FROM jsonb_array_elements(readers::jsonb) v WHERE v->>'account' = ?1)",
        ),
    };
    tx.execute(mentions, params![account])?;
    tx.execute(readers, params![account])?;
    tx.execute("UPDATE chat_rooms SET created_by = '' WHERE created_by = ?1", params![account])?;
    tx.execute("DELETE FROM chat_reads WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_members WHERE account = ?1", params![account])?;
    // Its own live events (its read marks) keep their numbers, naming no one.
    tx.execute(
        "UPDATE chat_events SET kind = 'changed', body = '', to_account = NULL WHERE to_account = ?1",
        params![account],
    )?;
    // Rooms' byte counts after the blanking.
    tx.execute(
        "UPDATE chat_rooms SET bytes = (SELECT CAST(coalesce(sum(octet_length(text) + coalesce(octet_length(quote), 0)), 0) AS BIGINT)
         FROM chat_messages WHERE room = chat_rooms.id)",
        params![],
    )?;
    // FTS5's old segments still hold the blanked words until they're
    // merged: merge them now (deleting an account is rare). Postgres's
    // `search` column is made again with the row, and the dead rows and
    // their index entries go at the next vacuum.
    if tx.dialect() == Dialect::Sqlite {
        tx.execute("INSERT INTO chat_fts (chat_fts) VALUES ('optimize')", params![])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(db: &Db, id: &str, scope: &str, kind: &str, members: &[&str]) {
        let r = Room {
            id: id.into(),
            scope: scope.into(),
            kind: kind.into(),
            name: id.into(),
            machine: None,
            target: None,
            created_by: "a1".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&r, members, None).unwrap();
    }

    fn post(db: &Db, room: &str, author: &str, text: &str, quote: Option<&str>, mentions: &[String]) -> i64 {
        db.add_chat_message(
            room,
            &NewMessage { author, at: 1, text, quote, ref_machine: None, ref_pane: None, mentions },
        )
        .unwrap()
    }

    /// The numbers of the messages matching `q`'s words (`dep*`: a prefix).
    fn found(db: &Db, rooms: &[&str], q: &str) -> Vec<i64> {
        let rooms: Vec<String> = rooms.iter().map(|s| s.to_string()).collect();
        let words: Vec<Word> = q
            .split_whitespace()
            .map(|w| Word { text: w.trim_end_matches('*').into(), prefix: w.ends_with('*') })
            .collect();
        db.chat_search(&rooms, &Search { words: &words, ..Default::default() }, 50)
            .unwrap()
            .iter()
            .map(|m| m.seq)
            .collect()
    }

    #[test]
    fn numbers_read_marks_and_pages() {
        let db = Db::test();
        room(&db, "r1", "team:t", "channel", &[]);
        for i in 1..=5 {
            assert_eq!(post(&db, "r1", if i % 2 == 0 { "a2" } else { "a1" }, &format!("m{i}"), None, &[]), i);
        }
        // Your own post marks you read; others' are unread.
        assert_eq!(db.chat_mark("r1", "a1").unwrap(), (5, false));
        assert_eq!(db.chat_mark("r1", "a2").unwrap().0, 4);
        assert_eq!(db.chat_unread("r1", "a2", 4, 0).unwrap(), (1, 0));
        // Marks only move forward.
        assert!(!db.set_chat_read("r1", "a1", 2).unwrap());
        assert_eq!(db.chat_mark("r1", "a1").unwrap().0, 5);
        let (ms, more) = db.chat_messages("r1", Page::Newest, 2).unwrap();
        assert_eq!((ms.iter().map(|m| m.seq).collect::<Vec<_>>(), more), (vec![4, 5], true));
        let (ms, more) = db.chat_messages("r1", Page::Before(4), 10).unwrap();
        assert_eq!((ms.iter().map(|m| m.seq).collect::<Vec<_>>(), more), (vec![1, 2, 3], false));
        let (ms, more) = db.chat_messages("r1", Page::After(3), 1).unwrap();
        assert_eq!((ms.iter().map(|m| m.seq).collect::<Vec<_>>(), more), (vec![4], true));
    }

    fn import(db: &Db, room: &str, ids: &[u64]) -> (usize, usize) {
        let texts: Vec<String> = ids.iter().map(|i| format!("old {i}")).collect();
        let old: Vec<OldMessage> = ids
            .iter()
            .zip(&texts)
            .map(|(id, text)| OldMessage {
                id: *id,
                name: "Jake",
                m: NewMessage {
                    author: "a1",
                    at: *id,
                    text,
                    quote: None,
                    ref_machine: None,
                    ref_pane: None,
                    mentions: &[],
                },
            })
            .collect();
        db.import_chat_messages(room, &old).unwrap()
    }

    fn numbered(db: &Db, room: &str) -> Vec<(i64, String)> {
        db.chat_messages(room, Page::After(i64::MIN), 200).unwrap().0.into_iter().map(|m| (m.seq, m.text)).collect()
    }

    #[test]
    fn imports_keep_numbers_or_go_before_controls_own() {
        let db = Db::test();
        let n = |seq: i64, t: &str| (seq, t.to_owned());
        // A room with nothing of its own: old numbers kept, in parts, and
        // each once.
        room(&db, "fresh", "machine:m", "pane", &[]);
        assert_eq!(import(&db, "fresh", &[1, 2]), (2, 0));
        assert_eq!(import(&db, "fresh", &[2, 3]), (1, 1));
        assert_eq!(numbered(&db, "fresh"), [n(1, "old 1"), n(2, "old 2"), n(3, "old 3")]);
        assert_eq!(db.chat_room("fresh").unwrap().unwrap().last_seq, 3);
        assert_eq!(post(&db, "fresh", "a2", "new", None, &[]), 4);
        assert_eq!(db.chat_unread("fresh", "a3", 0, 0).unwrap(), (1, 0), "old messages count as read");

        // A room used in control first: they go before its own, and a later
        // part still lands after the earlier ones.
        room(&db, "used", "machine:m", "session", &[]);
        post(&db, "used", "a2", "new 1", None, &[]);
        assert_eq!(import(&db, "used", &[1, 2]), (2, 0));
        assert_eq!(import(&db, "used", &[3]), (1, 0));
        assert_eq!(numbered(&db, "used"), [n(-2, "old 1"), n(-1, "old 2"), n(0, "old 3"), n(1, "new 1")]);
        assert_eq!(post(&db, "used", "a2", "new 2", None, &[]), 2);
        assert_eq!(db.chat_bytes("machine:m").unwrap() as usize, "old 1".len() * 6 + "new".len() + "new 1".len() * 2);
    }

    #[test]
    fn the_index_follows_edits_and_deletes() {
        let db = Db::test();
        room(&db, "r1", "team:t", "channel", &[]);
        post(&db, "r1", "a1", "the mean test fails", Some("FAILED mean"), &[]);
        post(&db, "r1", "a1", "deploy done", None, &[]);
        assert_eq!(found(&db, &["r1"], "mean"), vec![1]);
        assert_eq!(found(&db, &["r1"], "FAILED"), vec![1], "quotes are searched too");
        assert_eq!(found(&db, &["r1"], "dep*"), vec![2]);
        db.edit_chat_message("r1", 2, "rollback done", &[], 2).unwrap();
        assert!(found(&db, &["r1"], "deploy").is_empty(), "an edit leaves no old words in the index");
        assert_eq!(found(&db, &["r1"], "rollback"), vec![2]);
        db.delete_chat_message("r1", 1, 3).unwrap();
        assert!(found(&db, &["r1"], "mean").is_empty());
        assert!(found(&db, &["r1"], "FAILED").is_empty());
        let m = db.chat_message("r1", 1).unwrap().unwrap();
        assert_eq!((m.text.as_str(), m.quote, m.deleted), ("", None, Some(3)), "the number stays");
        assert!(found(&db, &["other"], "rollback").is_empty(), "only the rooms asked");
    }

    /// Both databases split words where FTS5's unicode61 does, and fold
    /// case (M143): what a person finds doesn't depend on which one holds
    /// the messages. (Accents are the difference: FTS5 drops them, so `cafe`
    /// finds `café` on SQLite and not on Postgres.) Hits come newest first
    /// on both.
    #[test]
    fn words_match_alike_on_both_databases() {
        let db = Db::test();
        room(&db, "r1", "team:t", "channel", &[]);
        post(&db, "r1", "a1", "see src/main.rs:12 for the fix", None, &[]);
        post(&db, "r1", "a1", "Ünïcode names_with_underscores", None, &[]);
        post(&db, "r1", "a1", "the FIX is in, and fixed again", None, &[]);
        assert_eq!(found(&db, &["r1"], "main"), vec![1]);
        assert_eq!(found(&db, &["r1"], "main.rs"), vec![1], "a word with a dot is its parts in a row");
        assert!(found(&db, &["r1"], "rs.main").is_empty(), "in that order");
        assert_eq!(found(&db, &["r1"], "ünïcode"), vec![2], "case folds past ASCII");
        assert_eq!(found(&db, &["r1"], "underscores"), vec![2]);
        assert_eq!(found(&db, &["r1"], "fix"), vec![3, 1], "newest first");
        assert_eq!(found(&db, &["r1"], "fix*"), vec![3, 1]);
        assert_eq!(found(&db, &["r1"], "fix again"), vec![3], "every word");
        assert!(found(&db, &["r1"], "--").is_empty(), "a word with no letters finds nothing");
    }

    #[test]
    fn erasing_an_account_leaves_nothing_of_it_in_chat() {
        let db = Db::test();
        room(&db, "ch", "team:t", "channel", &[]);
        room(&db, "dm", "team:t", "dm", &["gone1", "b2"]);
        room(&db, "gone-team", "team:t2", "channel", &[]);
        let r = Room {
            id: "pane".into(),
            scope: "machine:box".into(),
            kind: "pane".into(),
            name: "pane 1".into(),
            machine: Some("box".into()),
            target: Some("1".into()),
            created_by: "gone1".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&r, &[], None).unwrap();
        room(&db, "archive", "account:gone1", "archive", &["gone1", "b2"]);
        post(&db, "ch", "gone1", "secret plans", Some("quoted"), &["b2".into()]);
        post(&db, "ch", "b2", "@gonzo hi", None, &["gone1".into()]);
        post(&db, "dm", "b2", "just us", None, &[]);
        post(&db, "pane", "b2", "on its box", None, &[]);
        db.set_chat_readers(
            "other-box",
            "pane",
            "3",
            &[
                ChatReader { account: "gone1".into(), from: None, watching: false },
                ChatReader { account: "b2".into(), from: Some(5), watching: false },
            ],
            1,
        )
        .unwrap();
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        erase_account(&tx, "gone1", &["box".into()], &["t2".into()]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.chat_room("dm").unwrap().is_none(), "its DMs go, both sides");
        assert!(db.chat_room("pane").unwrap().is_none(), "the threads on its machines go");
        assert!(db.chat_room("gone-team").unwrap().is_none(), "a disbanded team's rooms go");
        assert!(db.chat_room("archive").unwrap().is_none(), "its machines' old conversations go");
        let m = db.chat_message("ch", 1).unwrap().unwrap();
        assert_eq!((m.author.as_str(), m.text.as_str(), m.quote.as_deref()), ("", "", None));
        assert!(m.deleted.is_some());
        assert_eq!(db.chat_message("ch", 2).unwrap().unwrap().mentions, Vec::<String>::new());
        assert_eq!(
            db.chat_readers("other-box", "pane", "3").unwrap().unwrap(),
            vec![ChatReader { account: "b2".into(), from: Some(5), watching: false }]
        );
        assert!(found(&db, &["ch"], "secret").is_empty() && found(&db, &["ch"], "quoted").is_empty());
        assert!(db.mentions("gone1").is_empty(), "{:?}", db.mentions("gone1"));
        assert_eq!(db.chat_bytes("team:t").unwrap(), "@gonzo hi".len() as u64);
    }
}
