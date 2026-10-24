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
//!   pane or a session there), `account:<id>` (an archive, M124), or
//!   `project:<id>` (the thread on an object in that project, M196).
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
    /// The machine whose agent wrote it (M197); `author` is its owner.
    pub agent: Option<String>,
    /// The product that posted it for `author` (M237): a hud box's name.
    pub via: Option<String>,
}

/// How many of chat's live events are kept for pages that resume
/// (`Last-Event-ID`, M144): a page that missed more resyncs.
pub const CHAT_EVENTS_KEPT: u64 = 10_000;

/// How long a `typing` row is kept (#111): well past the 6 s it's worth
/// sending, so a machine that is a few seconds behind still finds it.
pub const TYPING_KEPT_MS: u64 = 15_000;

/// One of chat's live events, to add (M144): `live` with the event's JSON
/// in `body` (and `to`, whom only it goes to), `typing` (a `live` body that
/// only streams hear, and only while it's fresh), or `changed`, empty.
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
    pub at: u64,
}

/// One of an account's saved messages (#116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRow {
    pub room: String,
    pub seq: i64,
    pub saved_at: u64,
    pub done: Option<u64>,
    pub remind_at: Option<u64>,
    pub reminded: bool,
}

pub use arugula_control_wire::chat::SAVED_MAX;

pub struct NewMessage<'a> {
    pub author: &'a str,
    pub at: u64,
    pub text: &'a str,
    pub quote: Option<&'a str>,
    pub ref_machine: Option<&'a str>,
    pub ref_pane: Option<&'a str>,
    pub mentions: &'a [String],
    /// Written by an agent on this machine (M197), for `author`.
    pub agent: Option<&'a str>,
    /// Posted by this product for `author` (M237): a hud box's name.
    pub via: Option<&'a str>,
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
const MSG_COLS: &str =
    "room, seq, author, at, text, quote, ref_machine, ref_pane, mentions, edited, deleted, imported, agent, via";

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
        agent: r.get(12)?,
        via: r.get(13)?,
    })
}

fn marks(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// `col` is one of the texts in the JSON array that parameter `?n` holds:
/// one statement, prepared once, however long the list (a page of rooms'
/// reads, below), where `IN (?, ?, …)` is a statement for each length.
pub(super) fn in_json(c: &impl Sql, col: &str, n: u8) -> String {
    match c.dialect() {
        Dialect::Sqlite => format!("{col} IN (SELECT value FROM json_each(?{n}))"),
        Dialect::Postgres => format!("{col} IN (SELECT jsonb_array_elements_text(CAST(?{n} AS jsonb)))"),
    }
}

/// A list as the JSON array [`in_json`] takes.
pub(super) fn json_list<T: AsRef<str>>(v: &[T]) -> Result<String> {
    Ok(serde_json::to_string(&v.iter().map(AsRef::as_ref).collect::<Vec<_>>())?)
}

/// What a page of rooms needs for its unread counts, per room
/// ([`Db::chat_unread_in`]).
pub struct UnreadAsk<'a> {
    pub room: &'a str,
    /// The reader's mark.
    pub read: i64,
    /// From when they see the room (ms).
    pub from: u64,
}

type Readers = std::collections::HashMap<(String, String, String), Vec<ChatReader>>;

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

    /// The thread on an object (M196) in a project's scope, by the
    /// object's global id: one per project, so a repo relinked to another
    /// project gets a new one there.
    pub fn chat_room_on(&self, scope: &str, target: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(
                &format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = 'object' AND scope = ?1 AND target = ?2"),
                params![scope, target],
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
    /// (`crate::projects::team_project`), and an object's thread to the
    /// object's (M196); DMs and pane threads to none. Fails with
    /// [`super::Taken`] for a channel name, or a pane's or an object's room,
    /// already there.
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

    /// Every room in these scopes (team channels and DMs), on these
    /// machines (pane and session threads), with `account` a member of it
    /// by row (its DMs and archives), or on an object in a project of
    /// `account`'s or of these scopes' teams (M196), each once, in no
    /// particular order. One statement: a chat list's first.
    pub fn chat_rooms_seen(&self, scopes: &[String], machines: &[String], account: &str) -> Result<Vec<Room>> {
        let c = self.c();
        c.query_map(
            &format!(
                // A UNION, not ORs: each part keeps its index.
                "SELECT {ROOM_COLS} FROM chat_rooms WHERE {}
                 UNION SELECT {ROOM_COLS} FROM chat_rooms WHERE {}
                 UNION SELECT {ROOM_COLS} FROM chat_rooms WHERE id IN (SELECT room FROM chat_members WHERE account = ?3)
                 UNION SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = 'object' AND scope IN
                     (SELECT 'project:' || id FROM projects WHERE account_id = ?3 OR {})",
                in_json(&c, "scope", 1),
                in_json(&c, "machine", 2),
                in_json(&c, "('team:' || team_id)", 1)
            ),
            params![json_list(scopes)?, json_list(machines)?, account],
            room_row,
        )
    }

    /// The rooms an agent grant over a scope reaches (#122), when it's
    /// given: the channels of the team whose default project it is, or the
    /// DMs of the account. Not object threads (a grant over a project
    /// reaches every issue's: they show their agents when opened).
    pub fn chat_rooms_reached(&self, scope_kind: &str, scope_id: &str) -> Result<Vec<Room>> {
        let c = self.c();
        let (sql, arg) = match scope_kind {
            "project" => (
                format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = 'channel' AND archived IS NULL AND scope IN
                     (SELECT 'team:' || team_id FROM projects WHERE id = ?1 AND is_default = TRUE)"
                ),
                scope_id,
            ),
            "account" => (
                format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = 'dm' AND archived IS NULL
                     AND id IN (SELECT room FROM chat_members WHERE account = ?1)"
                ),
                scope_id,
            ),
            _ => return Ok(vec![]),
        };
        c.query_map(&sql, params![arg], room_row)
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

    /// Append a message: control numbers it, and its author has read it
    /// (unless an agent wrote it: its owner hasn't, M197). Its number.
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
            "INSERT INTO chat_messages (room, seq, author, at, text, quote, ref_machine, ref_pane, mentions, agent, via)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                room,
                seq,
                m.author,
                m.at,
                m.text,
                m.quote,
                m.ref_machine,
                m.ref_pane,
                serde_json::to_string(m.mentions)?,
                m.agent,
                m.via
            ],
        )?;
        if m.agent.is_some() || m.author.is_empty() {
            tx.commit()?;
            return Ok(seq);
        }
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

    /// [`chat_messages`](Self::chat_messages), of only those that mention
    /// `who` (an account, or a machine's agents as `machine:<id>`, M198).
    pub fn chat_mentioning(&self, room: &str, who: &str, page: Page, limit: u32) -> Result<(Vec<Message>, bool)> {
        let c = self.c();
        let n = i64::from(limit) + 1;
        let (op, order, at) = match page {
            Page::After(after) => (">", "", after),
            Page::Before(before) => ("<", " DESC", before),
            Page::Newest => ("<", " DESC", i64::MAX),
        };
        let mentioned = match c.dialect() {
            Dialect::Sqlite => "EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?4)",
            Dialect::Postgres => "EXISTS (SELECT 1 FROM jsonb_array_elements_text(mentions::jsonb) v WHERE v = ?4)",
        };
        let mut ms = c.query_map(
            &format!(
                "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq {op} ?2 AND {mentioned}
                 ORDER BY seq{order} LIMIT ?3"
            ),
            params![room, at, n, who],
            msg_row,
        )?;
        let more = ms.len() > limit as usize;
        ms.truncate(limit as usize);
        if !matches!(page, Page::After(_)) {
            ms.reverse();
        }
        Ok((ms, more))
    }

    /// The newest message in each of these rooms that mentions `who` (as
    /// [`chat_mentioning`](Self::chat_mentioning)), by room: one statement.
    pub fn chat_last_mentions(&self, rooms: &[&str], who: &str) -> Result<std::collections::HashMap<String, i64>> {
        if rooms.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let mentioned = match c.dialect() {
            Dialect::Sqlite => "EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?2)",
            Dialect::Postgres => "EXISTS (SELECT 1 FROM jsonb_array_elements_text(mentions::jsonb) v WHERE v = ?2)",
        };
        let rows = c.query_map(
            &format!(
                "SELECT room, max(seq) FROM chat_messages WHERE {} AND deleted IS NULL AND {mentioned} GROUP BY room",
                in_json(&c, "room", 1)
            ),
            params![json_list(rooms)?, who],
            |r| Ok((r.get::<String>(0)?, r.get::<i64>(1)?)),
        )?;
        Ok(rows.into_iter().collect())
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
        super::chat_reactions::drop_for_message(&tx, room, seq)?;
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
            "SELECT seq, kind, body, to_account, at FROM chat_events WHERE seq > ?1 ORDER BY seq LIMIT ?2",
            params![after, limit],
            |r| Ok(ChatEvent { seq: r.get(0)?, kind: r.get(1)?, body: r.get(2)?, to: r.get(3)?, at: r.get(4)? }),
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

    /// Drop typing rows older than [`TYPING_KEPT_MS`]: they're only ever
    /// worth a few seconds, so they go far sooner than the window of real
    /// events and never crowd it. How many went.
    pub fn prune_typing(&self, now: u64) -> Result<usize> {
        self.c().execute(
            "DELETE FROM chat_events WHERE kind = 'typing' AND at < ?1",
            params![now.saturating_sub(TYPING_KEPT_MS)],
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

    /// [`chat_mark`](Self::chat_mark) for a page of rooms, in one
    /// statement: the rooms with a mark (the rest are `(0, false)`), with
    /// the reader's own notification level for each where they chose one
    /// (#113), so a page needn't read those apart.
    #[allow(clippy::type_complexity)]
    pub fn chat_marks(
        &self,
        rooms: &[&str],
        account: &str,
    ) -> Result<std::collections::HashMap<String, (i64, bool, Option<arugula_control_wire::chat::NotifyLevel>)>> {
        if rooms.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT room, seq, muted, level FROM chat_reads WHERE account = ?1 AND {}",
                in_json(&c, "room", 2)
            ),
            params![account, json_list(rooms)?],
            |r| {
                let level: Option<String> = r.get(3)?;
                Ok((r.get::<String>(0)?, (r.get(1)?, r.get(2)?, level.as_deref().map(crate::db::notify::level))))
            },
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Move a read mark forward (never back). Whether it moved.
    pub fn set_chat_read(&self, room: &str, account: &str, seq: i64) -> Result<bool> {
        Ok(self.c().execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET seq = excluded.seq WHERE excluded.seq > chat_reads.seq",
            params![room, account, seq],
        )? > 0)
    }

    /// Move a read mark to `seq`, forward or back (mark as unread, #116).
    pub fn set_chat_read_to(&self, room: &str, account: &str, seq: i64) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET seq = excluded.seq",
            params![room, account, seq],
        )?;
        Ok(())
    }

    /// Save a message for `account` (#116), or save it again: `done` is
    /// undone, the reminder replaced and armed. Past [`SAVED_MAX`] the
    /// oldest done items go, then the oldest.
    pub fn save_chat_message(
        &self,
        account: &str,
        room: &str,
        seq: i64,
        at: u64,
        remind_at: Option<u64>,
    ) -> Result<()> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.execute(
            "INSERT INTO chat_saved (account, room, seq, saved_at, remind_at, reminded) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (account, room, seq) DO UPDATE
             SET done_at = NULL, remind_at = excluded.remind_at, reminded = excluded.reminded",
            params![account, room, seq, at, remind_at, false],
        )?;
        tx.execute(
            "DELETE FROM chat_saved WHERE account = ?1 AND (room, seq) IN (
                 SELECT room, seq FROM chat_saved WHERE account = ?1
                 ORDER BY (done_at IS NOT NULL), saved_at DESC LIMIT 1000000 OFFSET ?2)",
            params![account, SAVED_MAX as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Mark a saved message done or not. Whether it was saved.
    pub fn set_chat_saved_done(&self, account: &str, room: &str, seq: i64, done: Option<u64>) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE chat_saved SET done_at = ?4 WHERE account = ?1 AND room = ?2 AND seq = ?3",
            params![account, room, seq, done],
        )? > 0)
    }

    /// Stop keeping a saved message. Whether it was saved.
    pub fn unsave_chat_message(&self, account: &str, room: &str, seq: i64) -> Result<bool> {
        Ok(self.c().execute(
            "DELETE FROM chat_saved WHERE account = ?1 AND room = ?2 AND seq = ?3",
            params![account, room, seq],
        )? > 0)
    }

    /// An account's saved messages: to do first, newest saved first, then
    /// the done ones, newest done first.
    pub fn chat_saved(&self, account: &str) -> Result<Vec<SavedRow>> {
        self.c().query_map(
            "SELECT room, seq, saved_at, done_at, remind_at, reminded FROM chat_saved WHERE account = ?1
             ORDER BY (done_at IS NOT NULL), coalesce(done_at, saved_at) DESC, seq DESC",
            params![account],
            |r| {
                Ok(SavedRow {
                    room: r.get(0)?,
                    seq: r.get(1)?,
                    saved_at: r.get(2)?,
                    done: r.get(3)?,
                    remind_at: r.get(4)?,
                    reminded: r.get(5)?,
                })
            },
        )
    }

    /// Reminders due by `now` that nobody has sent yet, claimed: each is
    /// marked reminded in the same statement that returns it, so of two
    /// machines running the check one gets it. Done items don't remind.
    pub fn claim_chat_reminders(&self, now: u64, limit: u32) -> Result<Vec<(String, String, i64)>> {
        self.c().query_map(
            "UPDATE chat_saved SET reminded = ?3
             WHERE (account, room, seq) IN (
                 SELECT account, room, seq FROM chat_saved
                 WHERE remind_at IS NOT NULL AND remind_at <= ?1 AND reminded = ?4 AND done_at IS NULL
                 ORDER BY remind_at LIMIT ?2)
             AND reminded = ?4
             RETURNING account, room, seq",
            params![now, limit, true, false],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    pub fn set_chat_muted(&self, room: &str, account: &str, muted: bool) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_reads (room, account, muted) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET muted = excluded.muted",
            params![room, account, muted],
        )?;
        Ok(())
    }

    /// The words that notify `account` as a mention does (#114).
    pub fn chat_keywords(&self, account: &str) -> Result<Vec<String>> {
        let words: Option<String> = self
            .c()
            .query_row("SELECT words FROM chat_keywords WHERE account = ?1", params![account], |r| r.get(0))
            .optional()?;
        Ok(words.and_then(|w| serde_json::from_str(&w).ok()).unwrap_or_default())
    }

    /// [`chat_keywords`](Self::chat_keywords) for a room's audience, those
    /// who have any.
    pub fn chat_keywords_of(&self, accounts: &[String]) -> Result<Vec<(String, Vec<String>)>> {
        if accounts.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT account, words FROM chat_keywords WHERE {}", in_json(&c, "account", 1)),
            params![json_list(accounts)?],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )?;
        Ok(rows
            .into_iter()
            .filter_map(|(a, w)| Some((a, serde_json::from_str::<Vec<String>>(&w).ok().filter(|w| !w.is_empty())?)))
            .collect())
    }

    pub fn set_chat_keywords(&self, account: &str, words: &[String]) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_keywords (account, words) VALUES (?1, ?2)
             ON CONFLICT (account) DO UPDATE SET words = excluded.words",
            params![account, serde_json::to_string(words)?],
        )?;
        Ok(())
    }

    /// Whether `@channel` and `@here` are for a team's owners only (#114).
    pub fn chat_broadcast_owners(&self, team: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT broadcast_owners FROM chat_team_settings WHERE team = ?1", params![team], |r| r.get(0))
            .optional()?
            .unwrap_or(false))
    }

    pub fn set_chat_broadcast_owners(&self, team: &str, owners_only: bool) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_team_settings (team, broadcast_owners) VALUES (?1, ?2)
             ON CONFLICT (team) DO UPDATE SET broadcast_owners = excluded.broadcast_owners",
            params![team, owners_only],
        )?;
        Ok(())
    }

    /// Others' messages after `read` (from `from` ms on), and how many of
    /// them mention `account`. Messages brought in from a machine (M124)
    /// count as read; the account's own agents' (M197) don't.
    pub fn chat_unread(&self, room: &str, account: &str, read: i64, from: u64) -> Result<(u64, u64)> {
        let c = self.c();
        let mentioned = match c.dialect() {
            Dialect::Sqlite => "EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?3)",
            Dialect::Postgres => "EXISTS (SELECT 1 FROM jsonb_array_elements_text(mentions::jsonb) v WHERE v = ?3)",
        };
        c.query_row(
            &format!(
                "SELECT count(*), CAST(coalesce(sum(CASE WHEN {mentioned} THEN 1 ELSE 0 END), 0) AS BIGINT)
                 FROM chat_messages WHERE room = ?1 AND seq > ?2 AND (author != ?3 OR agent IS NOT NULL) AND deleted IS NULL AND at >= ?4
                 AND imported_id IS NULL AND (via IS NULL OR via != 'system')"
            ),
            params![room, read, account, from],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// [`chat_unread`](Self::chat_unread) for a page of rooms, each with
    /// its own mark and floor, in one statement: (unread, mentions) by room.
    pub fn chat_unread_in(
        &self,
        account: &str,
        rooms: &[UnreadAsk<'_>],
    ) -> Result<std::collections::HashMap<String, (u64, u64)>> {
        if rooms.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let asks = serde_json::to_string(
            &rooms
                .iter()
                .map(|a| serde_json::json!({"room": a.room, "since": a.read, "floor": a.from}))
                .collect::<Vec<_>>(),
        )?;
        // The keys are the columns' names (`jsonb_to_recordset` matches by name).
        let (want, mentioned) = match c.dialect() {
            Dialect::Sqlite => (
                "SELECT json_extract(value, '$.room') AS room, json_extract(value, '$.since') AS since,
                     json_extract(value, '$.floor') AS floor FROM json_each(?2)",
                "EXISTS (SELECT 1 FROM json_each(m.mentions) WHERE value = ?1)",
            ),
            Dialect::Postgres => (
                "SELECT * FROM jsonb_to_recordset(CAST(?2 AS jsonb)) AS w(room text, since bigint, floor bigint)",
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text(m.mentions::jsonb) v WHERE v = ?1)",
            ),
        };
        let rows = c.query_map(
            &format!(
                "SELECT w.room, count(m.seq),
                     CAST(coalesce(sum(CASE WHEN m.seq IS NOT NULL AND {mentioned} THEN 1 ELSE 0 END), 0) AS BIGINT)
                 FROM ({want}) w
                 LEFT JOIN chat_messages m ON m.room = w.room AND m.seq > w.since AND (m.author != ?1 OR m.agent IS NOT NULL)
                     AND m.deleted IS NULL AND m.at >= w.floor AND m.imported_id IS NULL
                     AND (m.via IS NULL OR m.via != 'system')
                 GROUP BY w.room"
            ),
            params![account, asks],
            |r| Ok((r.get::<String>(0)?, (r.get(1)?, r.get(2)?))),
        )?;
        Ok(rows.into_iter().collect())
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

    /// Every pane's and session's readers on these machines, as their
    /// daemons last said, in one statement: by (machine, kind, target). A
    /// target with no row isn't there.
    pub fn chat_readers_on(&self, machines: &[String]) -> Result<Readers> {
        if machines.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT machine, kind, target, readers FROM chat_readers WHERE {}", in_json(&c, "machine", 1)),
            params![json_list(machines)?],
            |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?), r.get::<String>(3)?)),
        )?;
        rows.into_iter().map(|(k, body)| Ok((k, serde_json::from_str(&body)?))).collect()
    }

    /// [`chat_members`](Self::chat_members) for a page of rooms, in one
    /// statement: by room, each in account order (a room with none isn't
    /// there).
    pub fn chat_members_of(&self, rooms: &[&str]) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut out: std::collections::HashMap<String, Vec<String>> = Default::default();
        if rooms.is_empty() {
            return Ok(out);
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT room, account FROM chat_members WHERE {} ORDER BY room, account", in_json(&c, "room", 1)),
            params![json_list(rooms)?],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )?;
        for (room, account) in rows {
            out.entry(room).or_default().push(account);
        }
        Ok(out)
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
pub(super) fn drop_rooms_where(tx: &impl Sql, cond: &str, arg: impl super::sql::Param) -> Result<()> {
    // Which first: `cond` may read `chat_members`, which goes too.
    let ids: Vec<String> =
        tx.query_map(&format!("SELECT id FROM chat_rooms WHERE {cond}"), params![arg], |r| r.get(0))?;
    for id in ids {
        for table in ["chat_messages", "chat_reads", "chat_members", "chat_reactions", "chat_saved"] {
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
        tx.execute("DELETE FROM chat_team_settings WHERE team = ?1", params![t])?;
    }
    // Its machines' old conversations, archived under it (M124).
    drop_rooms_where(tx, "scope = 'account:' || ?1", account)?;
    // The threads on objects in projects that went with it (its personal
    // ones, its disbanded teams'): projects are erased first.
    drop_rooms_where(tx, "kind = ?1 AND scope NOT IN (SELECT 'project:' || id FROM projects)", "object")?;
    // Reactions on the messages it blanks, and those it made.
    tx.execute(
        "DELETE FROM chat_reactions WHERE account = ?1 OR EXISTS (SELECT 1 FROM chat_messages m
         WHERE m.room = chat_reactions.room AND m.seq = chat_reactions.seq AND m.author = ?1)",
        params![account],
    )?;
    tx.execute(
        "UPDATE chat_messages SET author = '', text = '', quote = NULL, ref_machine = NULL, ref_pane = NULL,
         mentions = '[]', imported = NULL, agent = NULL, via = NULL, deleted = coalesce(deleted, ?2) WHERE author = ?1",
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
    tx.execute("DELETE FROM chat_keywords WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM notify_prefs WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_saved WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_members WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_presence WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_sidebars WHERE account = ?1", params![account])?;
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
            &NewMessage {
                author,
                at: 1,
                text,
                quote,
                ref_machine: None,
                ref_pane: None,
                mentions,
                agent: None,
                via: None,
            },
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
                    agent: None,
                    via: None,
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
