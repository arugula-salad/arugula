//! Files and images in chat (#107), in `chat_files`: the bytes themselves,
//! with the name, the sniffed type and, once a message is posted with it,
//! the room and number it belongs to. Who may fetch a file is decided in
//! `crate::chat::files`; this is storage. A file goes with its message
//! when that's deleted, with its room, and with its uploader's account;
//! one never attached is swept after [`UNATTACHED_MS`].

use super::{
    Db,
    sql::{Optional as _, Row, Sql, list, params},
};
use anyhow::Result;
use std::collections::HashMap;

/// How long an upload waits for its message.
pub const UNATTACHED_MS: u64 = 60 * 60 * 1000;

/// A file's row, without its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatFileRow {
    pub id: String,
    pub room: String,
    pub seq: Option<i64>,
    pub account: String,
    pub scope: String,
    pub name: String,
    pub mime: String,
    pub kind: String,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub at: u64,
}

pub struct NewChatFile<'a> {
    pub id: &'a str,
    pub room: &'a str,
    pub account: &'a str,
    pub scope: &'a str,
    pub name: &'a str,
    pub mime: &'a str,
    pub kind: &'a str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub at: u64,
    pub data: &'a [u8],
}

const COLS: &str = "id, room, seq, account, scope, name, mime, kind, size, width, height, at";

fn row(r: &Row) -> Result<ChatFileRow> {
    Ok(ChatFileRow {
        id: r.get(0)?,
        room: r.get(1)?,
        seq: r.get(2)?,
        account: r.get(3)?,
        scope: r.get(4)?,
        name: r.get(5)?,
        mime: r.get(6)?,
        kind: r.get(7)?,
        size: r.get(8)?,
        width: r.get(9)?,
        height: r.get(10)?,
        at: r.get(11)?,
    })
}

impl Db {
    /// Keep an upload, unattached. `Ok(false)`: the scope's files would pass
    /// `scope_max` bytes (checked in the same transaction as the insert, so
    /// two uploads can't both fit the last megabyte).
    pub fn add_chat_file(&self, f: &NewChatFile, scope_max: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let used: i64 = tx.query_row(
            "SELECT CAST(coalesce(sum(size), 0) AS BIGINT) FROM chat_files WHERE scope = ?1",
            params![f.scope],
            |r| r.get(0),
        )?;
        if used as u64 + f.data.len() as u64 > scope_max {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO chat_files (id, room, seq, account, scope, name, mime, kind, size, width, height, at, data)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                f.id,
                f.room,
                f.account,
                f.scope,
                f.name,
                f.mime,
                f.kind,
                f.data.len() as i64,
                f.width,
                f.height,
                f.at,
                f.data.to_vec()
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The row of file `id`, without its bytes.
    pub fn chat_file(&self, id: &str) -> Result<Option<ChatFileRow>> {
        self.c().query_row(&format!("SELECT {COLS} FROM chat_files WHERE id = ?1"), params![id], row).optional()
    }

    /// The bytes of file `id`.
    pub fn chat_file_data(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.c().query_row("SELECT data FROM chat_files WHERE id = ?1", params![id], |r| r.get::<Vec<u8>>(0)).optional()
    }

    /// Files `account` uploaded to `room` that no message has: the ones a
    /// post may take, in `ids`' order. Fewer than asked: some aren't theirs
    /// to attach.
    pub fn chat_files_to_attach(&self, room: &str, account: &str, ids: &[String]) -> Result<Vec<ChatFileRow>> {
        let mut out = Vec::new();
        for id in ids {
            if let Some(f) = self.chat_file(id)?
                && f.room == room
                && f.account == account
                && f.seq.is_none()
            {
                out.push(f);
            }
        }
        Ok(out)
    }

    /// Attach uploads to message `seq`, in the order given. Those another
    /// post took meanwhile stay theirs.
    pub fn attach_chat_files(&self, room: &str, seq: i64, account: &str, ids: &[String]) -> Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let mut n = 0;
        for (i, id) in ids.iter().enumerate() {
            n += tx.execute(
                "UPDATE chat_files SET seq = ?3, ord = ?5 WHERE id = ?1 AND room = ?2 AND account = ?4
                 AND seq IS NULL",
                params![id, room, seq, account, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// The files on messages `from..=to` of a room, by (room, seq), in the
    /// order they were attached.
    pub fn chat_files_in(&self, room: &str, from: i64, to: i64) -> Result<HashMap<(String, i64), Vec<ChatFileRow>>> {
        let rows = self.c().query_map(
            &format!("SELECT {COLS} FROM chat_files WHERE room = ?1 AND seq >= ?2 AND seq <= ?3 ORDER BY seq, ord, id"),
            params![room, from, to],
            row,
        )?;
        let mut out: HashMap<(String, i64), Vec<ChatFileRow>> = HashMap::new();
        for f in rows {
            out.entry((f.room.clone(), f.seq.unwrap_or_default())).or_default().push(f);
        }
        Ok(out)
    }

    /// Messages in these rooms with a file whose name has all these words
    /// in it (any case), newest first: `chat_search` adds them to the
    /// text's matches.
    pub fn chat_file_name_hits(&self, rooms: &[String], words: &[String], limit: u32) -> Result<Vec<(String, i64)>> {
        if rooms.is_empty() || words.is_empty() {
            return Ok(vec![]);
        }
        let mut wh = vec![format!("room IN ({})", vec!["?"; rooms.len()].join(",")), "seq IS NOT NULL".to_owned()];
        let mut args: Vec<String> = rooms.to_vec();
        for w in words {
            wh.push("lower(name) LIKE ? ESCAPE '\\'".into());
            let esc = w.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            args.push(format!("%{esc}%"));
        }
        self.c().query_map(
            &format!(
                "SELECT room, seq FROM chat_files WHERE {} GROUP BY room, seq ORDER BY max(at) DESC LIMIT {limit}",
                wh.join(" AND ")
            ),
            &list(&args),
            |r| Ok((r.get::<String>(0)?, r.get::<i64>(1)?)),
        )
    }

    /// Bytes a scope's files take.
    pub fn chat_file_bytes(&self, scope: &str) -> Result<u64> {
        self.c().query_row(
            "SELECT CAST(coalesce(sum(size), 0) AS BIGINT) FROM chat_files WHERE scope = ?1",
            params![scope],
            |r| r.get(0),
        )
    }

    /// Forget uploads nobody posted, older than [`UNATTACHED_MS`] before
    /// `now`.
    pub fn sweep_chat_files(&self, now: u64) -> Result<usize> {
        self.c()
            .execute("DELETE FROM chat_files WHERE seq IS NULL AND at < ?1", params![now.saturating_sub(UNATTACHED_MS)])
    }
}

/// A message's files go with its content.
pub(super) fn drop_for_message(tx: &impl Sql, room: &str, seq: i64) -> Result<()> {
    tx.execute("DELETE FROM chat_files WHERE room = ?1 AND seq = ?2", params![room, seq])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chat::{NewMessage, Room};

    fn room(db: &Db, id: &str, scope: &str) {
        let r = Room {
            id: id.into(),
            scope: scope.into(),
            kind: "channel".into(),
            name: id.into(),
            machine: None,
            target: None,
            created_by: "a1".into(),
            created: 1,
            archived: None,
            last_seq: 0,
        };
        db.add_chat_room(&r, &[], None).unwrap();
    }

    fn post(db: &Db, room: &str, author: &str) -> i64 {
        db.add_chat_message(
            room,
            &NewMessage {
                author,
                at: 1,
                text: "hi",
                quote: None,
                ref_machine: None,
                ref_pane: None,
                mentions: &[],
                agent: None,
                via: None,
            },
        )
        .unwrap()
    }

    fn file<'a>(id: &'a str, room: &'a str, account: &'a str, scope: &'a str) -> NewChatFile<'a> {
        NewChatFile {
            id,
            room,
            account,
            scope,
            name: "n.bin",
            mime: "application/octet-stream",
            kind: "file",
            width: None,
            height: None,
            at: 1,
            data: b"abc",
        }
    }

    #[test]
    fn a_room_or_an_account_takes_its_files() {
        let db = Db::test();
        room(&db, "r1", "team:t");
        room(&db, "r2", "team:t2");
        let (a, b) = (post(&db, "r1", "a1"), post(&db, "r1", "b2"));
        let other = post(&db, "r2", "b2");
        for (id, r, acc, scope, seq) in [
            ("f1", "r1", "a1", "team:t", Some(a)),
            ("f2", "r1", "b2", "team:t", Some(b)),
            ("f3", "r2", "b2", "team:t2", Some(other)),
            // Never posted, and uploaded by the one who leaves.
            ("f4", "r1", "a1", "team:t", None),
        ] {
            assert!(db.add_chat_file(&file(id, r, acc, scope), 1 << 20).unwrap());
            if let Some(seq) = seq {
                db.attach_chat_files(r, seq, acc, &[id.to_owned()]).unwrap();
            }
        }
        assert_eq!(db.chat_file_bytes("team:t").unwrap(), 9);

        // Erasing a1 takes the files on its messages and those it never posted.
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::chat::erase_account(&tx, "a1", &[], &[]).unwrap();
        tx.commit().unwrap();
        drop(c);
        let left: Vec<bool> = ["f1", "f2", "f3", "f4"].iter().map(|f| db.chat_file(f).unwrap().is_some()).collect();
        assert_eq!(left, [false, true, true, false]);

        // Disbanding a team drops its rooms, and their files with them.
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::chat::erase_account(&tx, "zz", &[], &["t2".into()]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.chat_file("f3").unwrap().is_none());
        assert!(db.chat_file("f2").unwrap().is_some());
    }

    #[test]
    fn only_the_uploader_attaches_in_order() {
        let db = Db::test();
        room(&db, "r1", "team:t");
        let s = post(&db, "r1", "a1");
        for id in ["x", "y", "z"] {
            db.add_chat_file(&file(id, "r1", "a1", "team:t"), 1 << 20).unwrap();
        }
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(db.chat_files_to_attach("r1", "b2", &ids(&["x"])).unwrap().is_empty(), "not theirs");
        assert!(db.chat_files_to_attach("r2", "a1", &ids(&["x"])).unwrap().is_empty(), "not this room's");
        assert_eq!(db.attach_chat_files("r1", s, "b2", &ids(&["x"])).unwrap(), 0);
        assert_eq!(db.attach_chat_files("r1", s, "a1", &ids(&["z", "x", "y"])).unwrap(), 3);
        let got = db.chat_files_in("r1", s, s).unwrap();
        let names: Vec<&str> = got[&("r1".to_owned(), s)].iter().map(|f| f.id.as_str()).collect();
        assert_eq!(names, ["z", "x", "y"]);
        assert_eq!(db.attach_chat_files("r1", s, "a1", &ids(&["x"])).unwrap(), 0, "once");
    }
}
