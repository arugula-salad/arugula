//! Emoji reactions on chat messages (#104), in `chat_reactions`: a row per
//! (room, message, emoji, account, agent). `agent` is '' for a person and
//! the machine's id for its agent, who reacts as its owner, so the two are
//! separate rows. Who may react is decided in `crate::chat`; this is
//! storage. A deleted message, a dropped room and a deleted account take
//! their rows with them (`db::chat`).

use arugula_control_wire::chat::MAX_REACTIONS;

use super::{
    Db,
    sql::{Sql, params},
};
use anyhow::Result;

/// One reaction, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub room: String,
    pub seq: i64,
    pub emoji: String,
    pub account: String,
    pub agent: String,
    pub at: u64,
}

/// What adding a reaction did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Yes,
    /// The same one was there.
    Already,
    /// The message has [`MAX_REACTIONS`] other emoji.
    Full,
}

impl Db {
    /// Add the reaction; once is as good as twice.
    pub fn add_chat_reaction(&self, r: &Reaction) -> Result<Added> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let others: i64 = tx.query_row(
            "SELECT count(DISTINCT emoji) FROM chat_reactions WHERE room = ?1 AND seq = ?2 AND emoji != ?3",
            params![r.room, r.seq, r.emoji],
            |x| x.get(0),
        )?;
        if others as usize >= MAX_REACTIONS {
            return Ok(Added::Full);
        }
        let n = tx.execute(
            "INSERT INTO chat_reactions (room, seq, emoji, account, agent, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (room, seq, emoji, account, agent) DO NOTHING",
            params![r.room, r.seq, r.emoji, r.account, r.agent, r.at as i64],
        )?;
        tx.commit()?;
        Ok(if n == 0 { Added::Already } else { Added::Yes })
    }

    /// Take the reaction back; whether there was one.
    pub fn remove_chat_reaction(&self, room: &str, seq: i64, emoji: &str, account: &str, agent: &str) -> Result<bool> {
        let n = self.c().execute(
            "DELETE FROM chat_reactions WHERE room = ?1 AND seq = ?2 AND emoji = ?3 AND account = ?4 AND agent = ?5",
            params![room, seq, emoji, account, agent],
        )?;
        Ok(n > 0)
    }

    /// The reactions on messages `from..=to` of a room, first reaction
    /// first.
    pub fn chat_reactions(&self, room: &str, from: i64, to: i64) -> Result<Vec<Reaction>> {
        self.c().query_map(
            "SELECT room, seq, emoji, account, agent, at FROM chat_reactions
             WHERE room = ?1 AND seq >= ?2 AND seq <= ?3 ORDER BY at, emoji, account, agent",
            params![room, from, to],
            |r| {
                Ok(Reaction {
                    room: r.get(0)?,
                    seq: r.get(1)?,
                    emoji: r.get(2)?,
                    account: r.get(3)?,
                    agent: r.get(4)?,
                    at: r.get::<i64>(5)? as u64,
                })
            },
        )
    }
}

/// A message's reactions go with its content.
pub(super) fn drop_for_message(tx: &impl Sql, room: &str, seq: i64) -> Result<()> {
    tx.execute("DELETE FROM chat_reactions WHERE room = ?1 AND seq = ?2", params![room, seq])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::chat::{NewMessage, Room};

    fn room(db: &Db, id: &str) {
        let r = Room {
            id: id.into(),
            scope: "team:t".into(),
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

    fn post(db: &Db, room: &str) -> i64 {
        db.add_chat_message(
            room,
            &NewMessage {
                author: "a1",
                at: 1,
                text: "hi",
                quote: None,
                ref_machine: None,
                ref_pane: None,
                mentions: &[],
                agent: None,
                via: None,
                parent: None,
                also: false,
                agent_session: None,
            },
        )
        .unwrap()
    }

    fn react(room: &str, seq: i64, emoji: &str, account: &str, agent: &str, at: u64) -> Reaction {
        Reaction { room: room.into(), seq, emoji: emoji.into(), account: account.into(), agent: agent.into(), at }
    }

    #[test]
    fn adding_and_removing_are_idempotent() {
        let db = Db::test();
        room(&db, "r1");
        let s = post(&db, "r1");
        let r = react("r1", s, "👍", "a1", "", 5);
        assert_eq!(db.add_chat_reaction(&r).unwrap(), Added::Yes);
        assert_eq!(db.add_chat_reaction(&r).unwrap(), Added::Already);
        // An agent's is its owner's own, apart.
        assert_eq!(db.add_chat_reaction(&react("r1", s, "👍", "a1", "m1", 6)).unwrap(), Added::Yes);
        assert_eq!(db.chat_reactions("r1", s, s).unwrap().len(), 2);
        assert!(db.remove_chat_reaction("r1", s, "👍", "a1", "").unwrap());
        assert!(!db.remove_chat_reaction("r1", s, "👍", "a1", "").unwrap());
        let left = db.chat_reactions("r1", s, s).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].agent, "m1");
    }

    #[test]
    fn a_message_holds_a_few_kinds_of_emoji() {
        let db = Db::test();
        room(&db, "r1");
        let s = post(&db, "r1");
        for i in 0..MAX_REACTIONS {
            assert_eq!(db.add_chat_reaction(&react("r1", s, &format!("e{i}"), "a1", "", 1)).unwrap(), Added::Yes);
        }
        assert_eq!(db.add_chat_reaction(&react("r1", s, "new", "a1", "", 1)).unwrap(), Added::Full);
        // More people on one that's there is not another kind.
        assert_eq!(db.add_chat_reaction(&react("r1", s, "e0", "a2", "", 2)).unwrap(), Added::Yes);
    }

    #[test]
    fn deleting_a_message_or_a_room_drops_its_reactions() {
        let db = Db::test();
        room(&db, "r1");
        room(&db, "r2");
        let (a, b, other) = (post(&db, "r1"), post(&db, "r1"), post(&db, "r2"));
        for (r, s) in [("r1", a), ("r1", b), ("r2", other)] {
            db.add_chat_reaction(&react(r, s, "🎉", "a1", "", 1)).unwrap();
        }
        db.delete_chat_message("r1", a, 9).unwrap();
        assert!(db.chat_reactions("r1", a, a).unwrap().is_empty());
        assert_eq!(db.chat_reactions("r1", a, b).unwrap().len(), 1);
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::chat::erase_account(&tx, "a1", &[], &[]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.chat_reactions("r1", a, b).unwrap().is_empty());
        assert!(db.chat_reactions("r2", other, other).unwrap().is_empty());
    }
}
