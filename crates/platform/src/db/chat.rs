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
//!   that moves the room's `last_seq`. M124's import numbered old messages
//!   0 and below, before the room's own; it's gone (native #203), and what
//!   it brought in stays as it was (`imported`, `imported_id`).
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

/// What a channel or DM has beside its [`Room`] (#117, #118), read apart so
/// the rooms of other kinds stay as they were.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomMeta {
    /// An invite-only channel.
    pub private: bool,
    /// Whoever joins the team joins it.
    pub auto_join: bool,
    /// A DM's people, sorted and joined by `,` when it was made.
    pub dm_key: Option<String>,
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
    /// A reply: the `seq` of its thread's first message (#106).
    pub parent: Option<i64>,
    /// A reply that was also sent to the channel: the room's list has it.
    pub also: bool,
}

impl Message {
    /// Whether the room's own list shows it: not a reply, or one that was
    /// sent to the channel too.
    pub fn in_main(&self) -> bool {
        self.parent.is_none() || self.also
    }
}

/// How many of chat's live events are kept for pages that resume
/// (`Last-Event-ID`, M144): a page that missed more resyncs.
pub const CHAT_EVENTS_KEPT: u64 = 10_000;

/// How long a `typing` row is kept (#111): well past the 6 s it's worth
/// sending, so a machine that is a few seconds behind still finds it.
pub const TYPING_KEPT_MS: u64 = 15_000;

/// At most one warm (M262) per project in this long, across machines.
pub const WARM_GAP_MS: u64 = 60_000;

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
    /// A reply in the thread of this message (#106), and whether it is
    /// also sent to the channel.
    pub parent: Option<i64>,
    pub also: bool,
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

/// A condition on `chat_messages` rows: not a system line (#119), which
/// nobody has to read, search for or be mentioned by.
/// Not a system line: neither #119's (a row of `chat_system`) nor #122's
/// (no author, `via` "system").
const NOT_SYSTEM: &str = "(chat_messages.via IS NULL OR chat_messages.via != 'system') AND NOT EXISTS (SELECT 1 FROM chat_system s WHERE s.room = chat_messages.room AND s.seq = chat_messages.seq)";

const ROOM_COLS: &str = "id, scope, kind, name, machine, target, created_by, created, archived, last_seq";
pub(super) const MSG_COLS: &str = "room, seq, author, at, text, quote, ref_machine, ref_pane, mentions, edited, deleted, imported, agent, via, parent, also";

/// What the room's own list holds: not the replies, unless sent to the
/// channel too (#106).
pub(super) const MAIN: &str = "(parent IS NULL OR also = TRUE)";

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

pub(super) fn msg_row(r: &Row) -> Result<Message> {
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
        parent: r.get(14)?,
        also: r.get(15)?,
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

    /// The DM from before group DMs between two people in a scope: only
    /// theirs (no `dm_key`), and nobody else in it.
    pub fn chat_dm(&self, scope: &str, a: &str, b: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms r WHERE scope = ?1 AND kind = 'dm' AND dm_key IS NULL
                     AND EXISTS (SELECT 1 FROM chat_members WHERE room = r.id AND account = ?2)
                     AND EXISTS (SELECT 1 FROM chat_members WHERE room = r.id AND account = ?3)
                     AND (SELECT count(*) FROM chat_members WHERE room = r.id) = 2"
                ),
                params![scope, a, b],
                room_row,
            )
            .optional()
    }

    /// The DM of these people (sorted, joined by `,`: [`dm_key`]) in a
    /// scope, whoever of them is still in it.
    pub fn chat_dm_of(&self, scope: &str, key: &str) -> Result<Option<Room>> {
        self.c()
            .query_row(
                &format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE scope = ?1 AND kind = 'dm' AND dm_key = ?2"),
                params![scope, key],
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
        self.add_chat_room_with(room, members, project, &RoomMeta::default())
    }

    /// [`add_chat_room`](Self::add_chat_room) for a channel or DM with its
    /// [`RoomMeta`]. A DM's `dm_key` is unique in its scope: [`super::Taken`]
    /// when someone made the same DM just now.
    pub fn add_chat_room_with(
        &self,
        room: &Room,
        members: &[&str],
        project: Option<&str>,
        meta: &RoomMeta,
    ) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO chat_rooms (id, scope, kind, name, machine, target, created_by, created, project_id,
                                     private, auto_join, dm_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                room.id,
                room.scope,
                room.kind,
                room.name,
                room.machine,
                room.target,
                room.created_by,
                room.created,
                project,
                meta.private as i64,
                meta.auto_join as i64,
                meta.dm_key
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

    /// A room's members with when each reads from (ms), in account order.
    pub fn chat_member_rows(&self, room: &str) -> Result<Vec<(String, u64)>> {
        self.c().query_map(
            "SELECT account, from_ms FROM chat_members WHERE room = ?1 ORDER BY account",
            params![room],
            |r| Ok((r.get::<String>(0)?, r.get::<u64>(1)?)),
        )
    }

    /// Add `account` to a channel or DM, reading from `from` (ms): a
    /// channel's from the start (a DM it came back to:
    /// [`add_chat_member_from_now`](Self::add_chat_member_from_now)).
    /// `false`: it already was.
    pub fn add_chat_member(&self, room: &str, account: &str, from: u64) -> Result<bool> {
        Ok(self.c().execute(
            "INSERT INTO chat_members (room, account, from_ms) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
            params![room, account, from as i64],
        )? > 0)
    }

    /// Add `account` back to a DM it left, reading from `now` (ms), or from
    /// just after the last message said there when that's later: one said
    /// in the millisecond it came back, before it did, isn't one it reads
    /// (Arugula #294). `false`: it already was.
    pub fn add_chat_member_from_now(&self, room: &str, account: &str, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "INSERT INTO chat_members (room, account, from_ms)
             SELECT CAST(?1 AS TEXT), CAST(?2 AS TEXT),
                 CASE WHEN COALESCE(MAX(at), -1) >= CAST(?3 AS BIGINT) THEN MAX(at) + 1 ELSE CAST(?3 AS BIGINT) END
             FROM chat_messages WHERE room = ?1
             ON CONFLICT DO NOTHING",
            params![room, account, now as i64],
        )? > 0)
    }

    /// Take `account` out of a room's members. `false`: it wasn't one.
    pub fn remove_chat_member(&self, room: &str, account: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM chat_members WHERE room = ?1 AND account = ?2", params![room, account])? > 0)
    }

    /// [`RoomMeta`] of these rooms (a room that has none of the kinds that
    /// use it is there with the defaults), in one statement.
    pub fn chat_room_meta(&self, rooms: &[&str]) -> Result<std::collections::HashMap<String, RoomMeta>> {
        let mut out = std::collections::HashMap::new();
        if rooms.is_empty() {
            return Ok(out);
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT id, private, auto_join, dm_key FROM chat_rooms WHERE {}", in_json(&c, "id", 1)),
            params![json_list(rooms)?],
            |r| {
                Ok((
                    r.get::<String>(0)?,
                    RoomMeta { private: r.get::<i64>(1)? != 0, auto_join: r.get::<i64>(2)? != 0, dm_key: r.get(3)? },
                ))
            },
        )?;
        out.extend(rows);
        Ok(out)
    }

    /// A team's channels, public and private, by name: the browser reads
    /// the public ones.
    pub fn chat_team_channels(&self, scope: &str) -> Result<Vec<Room>> {
        self.c().query_map(
            &format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE scope = ?1 AND kind = 'channel' ORDER BY name"),
            params![scope],
            room_row,
        )
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
        let seq = add_message_in(&tx, room, m, None)?;
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
                &format!("SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq > ?2 AND {MAIN} ORDER BY seq LIMIT ?3"),
                params![room, after, n],
                msg_row,
            )?,
            Page::Before(before) => c.query_map(
                &format!(
                    "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq < ?2 AND {MAIN} ORDER BY seq DESC LIMIT ?3"
                ),
                params![room, before, n],
                msg_row,
            )?,
            Page::Newest => c.query_map(
                &format!("SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND {MAIN} ORDER BY seq DESC LIMIT ?2"),
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

    /// The platform's open asks with `via` in `room` (Arugula #214), in the
    /// thread under `parent` (the room's own messages for `None`), answered:
    /// they mention nobody now, so they stop counting as anyone's mention.
    /// Their numbers and parents.
    pub fn close_chat_asks(&self, room: &str, parent: Option<i64>, via: &str) -> Result<Vec<(i64, Option<i64>)>> {
        let c = self.c();
        let open = "author = '' AND via = ?2 AND mentions <> '[]'";
        match parent {
            Some(p) => c.query_map(
                &format!(
                    "UPDATE chat_messages SET mentions = '[]' WHERE room = ?1 AND parent = ?3 AND {open}
                     RETURNING seq, parent"
                ),
                params![room, via, p],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            None => c.query_map(
                &format!(
                    "UPDATE chat_messages SET mentions = '[]' WHERE room = ?1 AND parent IS NULL AND {open}
                     RETURNING seq, parent"
                ),
                params![room, via],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
        }
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
        super::chat_files::drop_for_message(&tx, room, seq)?;
        super::chat_pins::drop_for_message(&tx, room, seq)?;
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

    /// Drop typing rows older than [`TYPING_KEPT_MS`], and warm rows older
    /// than [`WARM_GAP_MS`]: they're only ever worth a few seconds (or a
    /// minute), so they go far sooner than the window of real events and
    /// never crowd it. How many went.
    pub fn prune_typing(&self, now: u64) -> Result<usize> {
        self.c().execute(
            "DELETE FROM chat_events WHERE (kind = 'typing' AND at < ?1) OR (kind = 'warm' AND at < ?2)",
            params![now.saturating_sub(TYPING_KEPT_MS), now.saturating_sub(WARM_GAP_MS)],
        )
    }

    /// Whether `project`'s agents box may be warmed now (M262): true at
    /// most once a [`WARM_GAP_MS`] per project, whichever platform machine
    /// asks. It's a `warm` row of `chat_events` (`body` the project), taken
    /// under the table's lock as a live event is added, so two machines
    /// never both take it. Nothing streams it and nobody is told: it's
    /// only the mark, pruned with typing.
    pub fn take_warm(&self, project: &str, now: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.lock("chat_events")?;
        let recent: i64 = tx.query_row(
            "SELECT count(*) FROM chat_events WHERE kind = 'warm' AND body = ?1 AND at > ?2",
            params![project, now.saturating_sub(WARM_GAP_MS)],
            |r| r.get(0),
        )?;
        if recent > 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO chat_events (kind, body, to_account, at) VALUES ('warm', ?1, NULL, ?2)",
            params![project, now],
        )?;
        tx.commit()?;
        Ok(true)
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

    pub fn set_chat_team_settings(&self, team: &str, s: arugula_control_wire::chat::ChatTeamSettings) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_team_settings (team, broadcast_owners, topic_owners) VALUES (?1, ?2, ?3)
             ON CONFLICT (team) DO UPDATE SET broadcast_owners = excluded.broadcast_owners,
             topic_owners = excluded.topic_owners",
            params![team, s.broadcast_owners, s.topic_owners],
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
                 AND imported_id IS NULL AND {NOT_SYSTEM} AND {MAIN}"
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
                     AND NOT EXISTS (SELECT 1 FROM chat_system s WHERE s.room = m.room AND s.seq = m.seq)
                     AND (m.parent IS NULL OR m.also = TRUE)
                 GROUP BY w.room"
            ),
            params![account, asks],
            |r| Ok((r.get::<String>(0)?, (r.get(1)?, r.get(2)?))),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Messages in these rooms that match, newest first: by their words,
    /// or (#107) by a file's name that has all the words.
    pub fn chat_search(&self, rooms: &[String], s: &Search, limit: u32) -> Result<Vec<Message>> {
        let mut hits = self.chat_search_text(rooms, s, limit)?;
        if s.words.is_empty() || s.has_quote {
            return Ok(hits);
        }
        let words: Vec<String> = s.words.iter().map(|w| w.text.clone()).collect();
        for (room, seq) in self.chat_file_name_hits(rooms, &words, limit)? {
            if hits.iter().any(|m| m.room == room && m.seq == seq) {
                continue;
            }
            if let Some(m) = self.chat_message(&room, seq)?
                && m.deleted.is_none()
                && s.author.is_none_or(|a| m.author == a)
            {
                hits.push(m);
            }
        }
        hits.sort_by_key(|m| std::cmp::Reverse(m.at));
        hits.truncate(limit as usize);
        Ok(hits)
    }

    fn chat_search_text(&self, rooms: &[String], s: &Search, limit: u32) -> Result<Vec<Message>> {
        if rooms.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let cols = MSG_COLS.split(", ").map(|c| format!("m.{c}")).collect::<Vec<_>>().join(", ");
        let mut sql = format!("SELECT {cols} FROM chat_messages m");
        let mut args: Vec<String> = Vec::new();
        let mut wh = vec![
            format!("m.room IN ({})", marks(rooms.len())),
            "m.deleted IS NULL".to_owned(),
            NOT_SYSTEM.replace("chat_messages.", "m."),
        ];
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

    /// [`chat_member_rows`](Self::chat_member_rows) and
    /// [`chat_room_meta`](Self::chat_room_meta) for a page of rooms, in
    /// one statement (a chat list's budget is a few): the members by room,
    /// each in account order (a room with none isn't there), and every
    /// room's meta.
    #[allow(clippy::type_complexity)]
    pub fn chat_members_of(
        &self,
        rooms: &[&str],
    ) -> Result<(std::collections::HashMap<String, Vec<(String, u64)>>, std::collections::HashMap<String, RoomMeta>)>
    {
        let mut members: std::collections::HashMap<String, Vec<(String, u64)>> = Default::default();
        let mut metas: std::collections::HashMap<String, RoomMeta> = Default::default();
        if rooms.is_empty() {
            return Ok((members, metas));
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT r.id, m.account, m.from_ms, r.private, r.auto_join, r.dm_key
                 FROM chat_rooms r LEFT JOIN chat_members m ON m.room = r.id
                 WHERE {} ORDER BY r.id, m.account",
                in_json(&c, "r.id", 1)
            ),
            params![json_list(rooms)?],
            |r| {
                let meta =
                    RoomMeta { private: r.get::<i64>(3)? != 0, auto_join: r.get::<i64>(4)? != 0, dm_key: r.get(5)? };
                let member =
                    r.get::<Option<String>>(1)?.map(|a| Ok::<_, anyhow::Error>((a, r.get::<u64>(2)?))).transpose()?;
                Ok((r.get::<String>(0)?, member, meta))
            },
        )?;
        for (room, member, meta) in rows {
            if let Some(m) = member {
                members.entry(room.clone()).or_default().push(m);
            }
            metas.insert(room, meta);
        }
        Ok((members, metas))
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

/// [`Db::add_chat_message`] inside a transaction. `system` makes it a
/// system line (#119): what happened, and to which message.
pub(super) fn add_message_in(
    tx: &impl Sql,
    room: &str,
    m: &NewMessage,
    system: Option<(&str, Option<i64>)>,
) -> Result<i64> {
    let bytes = (m.text.len() + m.quote.map_or(0, str::len)) as i64;
    let seq: i64 = tx.query_row(
        "UPDATE chat_rooms SET last_seq = last_seq + 1, bytes = bytes + ?2 WHERE id = ?1 RETURNING last_seq",
        params![room, bytes],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO chat_messages (room, seq, author, at, text, quote, ref_machine, ref_pane, mentions, agent, via, parent, also)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            m.via,
            m.parent,
            m.also
        ],
    )?;
    if let Some(parent) = m.parent {
        super::chat_threads::replied(tx, room, parent, seq, m)?;
        // A reply read in its thread says nothing of the room's list: only
        // one sent to the channel too is read there as well.
        if !m.also {
            return Ok(seq);
        }
    }
    if let Some((kind, target)) = system {
        tx.execute(
            "INSERT INTO chat_system (room, seq, kind, target) VALUES (?1, ?2, ?3, ?4)",
            params![room, seq, kind, target],
        )?;
    }
    // A system line reads nothing: whoever pinned hasn't read what came
    // before, and a grant's line (#122) has no author to have read it.
    if m.agent.is_none() && system.is_none() && !m.author.is_empty() {
        tx.execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE
             SET seq = CASE WHEN excluded.seq > chat_reads.seq THEN excluded.seq ELSE chat_reads.seq END",
            params![room, m.author, seq],
        )?;
    }
    Ok(seq)
}

/// Every room in a scope, and all that's in them.
pub(super) fn drop_rooms_where(tx: &impl Sql, cond: &str, arg: impl super::sql::Param) -> Result<()> {
    // Which first: `cond` may read `chat_members`, which goes too.
    let ids: Vec<String> =
        tx.query_map(&format!("SELECT id FROM chat_rooms WHERE {cond}"), params![arg], |r| r.get(0))?;
    for id in ids {
        for table in [
            "chat_messages",
            "chat_reads",
            "chat_members",
            "chat_reactions",
            "chat_saved",
            "chat_files",
            "chat_topics",
            "chat_pins",
            "chat_system",
            "chat_thread_reads",
        ] {
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
    // A group DM goes on for the rest (#117); one left with two or fewer
    // goes, both sides.
    drop_rooms_where(
        tx,
        "kind = 'dm' AND id IN (SELECT room FROM chat_members WHERE account = ?1)
         AND (SELECT count(*) FROM chat_members m WHERE m.room = chat_rooms.id) <= 2",
        account,
    )?;
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
    // Files on the messages it blanks, and any it uploaded and never posted.
    tx.execute(
        "DELETE FROM chat_files WHERE account = ?1 OR EXISTS (SELECT 1 FROM chat_messages m
         WHERE m.room = chat_files.room AND m.seq = chat_files.seq AND m.author = ?1)",
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
    tx.execute("UPDATE chat_topics SET author = '' WHERE author = ?1", params![account])?;
    tx.execute("DELETE FROM chat_pins WHERE author = ?1", params![account])?;
    tx.execute("DELETE FROM chat_reads WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM chat_thread_reads WHERE account = ?1", params![account])?;
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
                parent: None,
                also: false,
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

    /// A group DM (#117) goes on for the rest when one of its people is
    /// erased; one left with two or fewer goes, as a 1:1 always did.
    #[test]
    fn erasing_an_account_leaves_a_group_dm_to_the_rest() {
        let db = Db::test();
        room(&db, "trio", "team:t", "dm", &["gone1", "b2", "c3"]);
        room(&db, "pair", "team:t", "dm", &["gone1", "b2"]);
        room(&db, "other", "team:t", "dm", &["b2", "c3"]);
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        erase_account(&tx, "gone1", &[], &[]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert_eq!(db.chat_members("trio").unwrap(), ["b2", "c3"]);
        assert!(db.chat_room("pair").unwrap().is_none());
        assert!(db.chat_room("other").unwrap().is_some());
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
