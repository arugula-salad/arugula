//! S41 prototype (throwaway): chat held by control, in plain text. Rooms
//! are a team's channels and DMs, and the threads about a pane or a
//! session; messages are numbered per room by control.
use super::Db;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

pub(super) const SCHEMA: &str = "
-- A place to talk. `scope` is a team id, or `account:<id>` for a pane or
-- session thread on a machine in no team. `kind` is 'channel', 'dm',
-- 'pane' or 'session'. A pane or session room names its machine and its
-- pane (or session) id; a channel names itself.
CREATE TABLE IF NOT EXISTS chat_rooms (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    machine TEXT,
    target TEXT,
    created_by TEXT NOT NULL,
    created INTEGER NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0,
    last_seq INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_target ON chat_rooms (kind, machine, target) WHERE machine IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_channel ON chat_rooms (scope, name) WHERE kind = 'channel';
-- A DM's two people (channels are the whole roster's).
CREATE TABLE IF NOT EXISTS chat_dm_members (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    PRIMARY KEY (room, account)
);
CREATE TABLE IF NOT EXISTS chat_messages (
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    author TEXT NOT NULL,
    at INTEGER NOT NULL,
    text TEXT NOT NULL,
    ref_machine TEXT,
    ref_pane TEXT,
    quote TEXT,
    mentions TEXT NOT NULL DEFAULT '[]',
    imported TEXT,
    PRIMARY KEY (room, seq)
);
CREATE INDEX IF NOT EXISTS chat_messages_author ON chat_messages (author);
CREATE VIRTUAL TABLE IF NOT EXISTS chat_fts USING fts5 (text, quote, content='chat_messages', content_rowid='rowid');
CREATE TRIGGER IF NOT EXISTS chat_fts_in AFTER INSERT ON chat_messages BEGIN
    INSERT INTO chat_fts (rowid, text, quote) VALUES (new.rowid, new.text, coalesce(new.quote, ''));
END;
CREATE TRIGGER IF NOT EXISTS chat_fts_out AFTER DELETE ON chat_messages BEGIN
    INSERT INTO chat_fts (chat_fts, rowid, text, quote) VALUES ('delete', old.rowid, old.text, coalesce(old.quote, ''));
END;
CREATE TABLE IF NOT EXISTS chat_reads (
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (room, account)
);
-- What control pushed for chat, and how it went (the prototype's record).
CREATE TABLE IF NOT EXISTS chat_pushes (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    why TEXT NOT NULL,
    subs INTEGER NOT NULL,
    at INTEGER NOT NULL
);
";

#[derive(Debug, Clone, Serialize)]
pub struct Room {
    pub id: String,
    pub scope: String,
    pub kind: String,
    pub name: String,
    pub machine: Option<String>,
    pub target: Option<String>,
    pub created_by: String,
    pub created: u64,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub room: String,
    pub seq: u64,
    pub author: String,
    pub at: u64,
    pub text: String,
    pub ref_machine: Option<String>,
    pub ref_pane: Option<String>,
    pub quote: Option<String>,
    pub mentions: Vec<String>,
    pub imported: Option<String>,
}

pub struct NewMessage<'a> {
    pub author: &'a str,
    pub at: u64,
    pub text: &'a str,
    pub ref_machine: Option<&'a str>,
    pub ref_pane: Option<&'a str>,
    pub quote: Option<&'a str>,
    pub mentions: &'a [String],
    pub imported: Option<&'a str>,
}

const ROOM_COLS: &str = "id, scope, kind, name, machine, target, created_by, created, last_seq";
const MSG_COLS: &str = "room, seq, author, at, text, ref_machine, ref_pane, quote, mentions, imported";

fn room_row(r: &rusqlite::Row) -> rusqlite::Result<Room> {
    Ok(Room {
        id: r.get(0)?,
        scope: r.get(1)?,
        kind: r.get(2)?,
        name: r.get(3)?,
        machine: r.get(4)?,
        target: r.get(5)?,
        created_by: r.get(6)?,
        created: r.get(7)?,
        last_seq: r.get(8)?,
    })
}

fn msg_row(r: &rusqlite::Row) -> rusqlite::Result<Message> {
    let mentions: String = r.get(8)?;
    Ok(Message {
        room: r.get(0)?,
        seq: r.get(1)?,
        author: r.get(2)?,
        at: r.get(3)?,
        text: r.get(4)?,
        ref_machine: r.get(5)?,
        ref_pane: r.get(6)?,
        quote: r.get(7)?,
        mentions: serde_json::from_str(&mentions).unwrap_or_default(),
        imported: r.get(9)?,
    })
}

impl Db {
    pub fn chat_room(&self, id: &str) -> Result<Option<Room>> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE id = ?1 AND archived = 0"), params![id], room_row)
            .optional()?)
    }

    pub fn chat_room_for(&self, kind: &str, machine: &str, target: &str) -> Result<Option<Room>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE kind = ?1 AND machine = ?2 AND target = ?3"),
                params![kind, machine, target],
                room_row,
            )
            .optional()?)
    }

    pub fn chat_dm(&self, scope: &str, a: &str, b: &str) -> Result<Option<Room>> {
        Ok(self
            .c()
            .query_row(
                &format!(
                    "SELECT {ROOM_COLS} FROM chat_rooms r WHERE scope = ?1 AND kind = 'dm'
                     AND EXISTS (SELECT 1 FROM chat_dm_members WHERE room = r.id AND account = ?2)
                     AND EXISTS (SELECT 1 FROM chat_dm_members WHERE room = r.id AND account = ?3)"
                ),
                params![scope, a, b],
                room_row,
            )
            .optional()?)
    }

    pub fn add_chat_room(&self, room: &Room, dm: &[&str]) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO chat_rooms (id, scope, kind, name, machine, target, created_by, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![room.id, room.scope, room.kind, room.name, room.machine, room.target, room.created_by, room.created],
        )?;
        for a in dm {
            tx.execute("INSERT INTO chat_dm_members (room, account) VALUES (?1, ?2)", params![room.id, a])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rooms in these scopes (a person's teams and own account), and the
    /// pane and session rooms on these machines.
    pub fn chat_rooms(&self, scopes: &[String], machines: &[String]) -> Result<Vec<Room>> {
        let c = self.c();
        let mut q = c.prepare(&format!("SELECT {ROOM_COLS} FROM chat_rooms WHERE archived = 0 ORDER BY created"))?;
        let all = q.query_map([], room_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(all
            .into_iter()
            .filter(|r| match r.kind.as_str() {
                "pane" | "session" => r.machine.as_ref().is_some_and(|m| machines.contains(m)),
                _ => scopes.contains(&r.scope),
            })
            .collect())
    }

    pub fn chat_dm_members(&self, room: &str) -> Result<Vec<String>> {
        let c = self.c();
        let mut q = c.prepare("SELECT account FROM chat_dm_members WHERE room = ?1 ORDER BY account")?;
        Ok(q.query_map(params![room], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Append a message: control numbers it. Returns its number.
    pub fn add_chat_message(&self, room: &str, m: &NewMessage) -> Result<u64> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let seq: u64 = tx.query_row(
            "UPDATE chat_rooms SET last_seq = last_seq + 1 WHERE id = ?1 RETURNING last_seq",
            params![room],
            |r| r.get(0),
        )?;
        tx.execute(
            &format!("INSERT INTO chat_messages ({MSG_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"),
            params![
                room,
                seq,
                m.author,
                m.at,
                m.text,
                m.ref_machine,
                m.ref_pane,
                m.quote,
                serde_json::to_string(m.mentions)?,
                m.imported
            ],
        )?;
        // Your own message is read.
        tx.execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET seq = max(seq, excluded.seq)",
            params![room, m.author, seq],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    pub fn chat_messages(&self, room: &str, after: u64, limit: u32) -> Result<Vec<Message>> {
        let c = self.c();
        let mut q = c.prepare(&format!(
            "SELECT {MSG_COLS} FROM chat_messages WHERE room = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"
        ))?;
        Ok(q.query_map(params![room, after, limit], msg_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn chat_read(&self, room: &str, account: &str) -> Result<u64> {
        Ok(self
            .c()
            .query_row("SELECT seq FROM chat_reads WHERE room = ?1 AND account = ?2", params![room, account], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    pub fn set_chat_read(&self, room: &str, account: &str, seq: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_reads (room, account, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT (room, account) DO UPDATE SET seq = max(seq, excluded.seq)",
            params![room, account, seq],
        )?;
        Ok(())
    }

    /// Unread messages after `read`, and how many of them mention `account`.
    pub fn chat_unread(&self, room: &str, account: &str, read: u64) -> Result<(u64, u64)> {
        Ok(self.c().query_row(
            "SELECT count(*), coalesce(sum(EXISTS (SELECT 1 FROM json_each(mentions) WHERE value = ?3)), 0)
             FROM chat_messages WHERE room = ?1 AND seq > ?2 AND author != ?3",
            params![room, read, account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Full-text search in these rooms, newest first.
    pub fn chat_search(&self, rooms: &[String], query: &str, limit: u32) -> Result<Vec<Message>> {
        if rooms.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let marks = vec!["?"; rooms.len()].join(",");
        let sql = format!(
            "SELECT {} FROM chat_messages m JOIN chat_fts f ON f.rowid = m.rowid
             WHERE chat_fts MATCH ?1 AND m.room IN ({marks}) ORDER BY m.at DESC LIMIT {limit}",
            MSG_COLS.split(", ").map(|c| format!("m.{c}")).collect::<Vec<_>>().join(", ")
        );
        let mut q = c.prepare(&sql)?;
        let mut args: Vec<&dyn rusqlite::ToSql> = vec![&query];
        for r in rooms {
            args.push(r);
        }
        Ok(q.query_map(args.as_slice(), msg_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn note_chat_push(&self, account: &str, room: &str, seq: u64, why: &str, subs: usize, at: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_pushes (account, room, seq, why, subs, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account, room, seq, why, subs as i64, at],
        )?;
        Ok(())
    }

    /// What deleting an account takes from chat: its DMs and its
    /// messages' text (its place in the room stays, as "deleted").
    pub fn erase_chat_account(&self, account: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM chat_messages WHERE room IN (SELECT room FROM chat_dm_members WHERE account = ?1)",
            params![account],
        )?;
        tx.execute(
            "DELETE FROM chat_rooms WHERE id IN (SELECT room FROM chat_dm_members WHERE account = ?1)",
            params![account],
        )?;
        tx.execute(
            "DELETE FROM chat_dm_members WHERE room NOT IN (SELECT id FROM chat_rooms)",
            [],
        )?;
        tx.execute(
            "UPDATE chat_messages SET text = '', quote = NULL, mentions = '[]' WHERE author = ?1",
            params![account],
        )?;
        tx.execute("DELETE FROM chat_reads WHERE account = ?1", params![account])?;
        tx.commit()?;
        Ok(())
    }

    /// What deleting a team takes: all its rooms and their messages.
    pub fn erase_chat_scope(&self, scope: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM chat_messages WHERE room IN (SELECT id FROM chat_rooms WHERE scope = ?1)", params![scope])?;
        tx.execute("DELETE FROM chat_reads WHERE room IN (SELECT id FROM chat_rooms WHERE scope = ?1)", params![scope])?;
        tx.execute(
            "DELETE FROM chat_dm_members WHERE room IN (SELECT id FROM chat_rooms WHERE scope = ?1)",
            params![scope],
        )?;
        tx.execute("DELETE FROM chat_rooms WHERE scope = ?1", params![scope])?;
        tx.commit()?;
        Ok(())
    }
}
