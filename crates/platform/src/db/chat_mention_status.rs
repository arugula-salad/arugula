//! Where an `@machine` mention in chat got to (#502), in
//! `chat_mention_status`: a row per (message, machine). Control writes it
//! when the machine's daemon answers the push, and again when the machine's
//! agent replies in the thread. Who may see it is decided in `crate::chat`
//! (it comes with the message); this is storage. A deleted message, a
//! dropped room and a deleted account take their rows with them
//! (`db::chat`).

use super::{
    Db,
    sql::{Sql, params},
};
use anyhow::Result;

/// One mention's status, as stored. `state` is a
/// `ChatMentionState`'s name (snake case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionStatus {
    pub room: String,
    pub seq: i64,
    pub machine: String,
    pub machine_name: String,
    /// The message's thread: its parent, or itself.
    pub root: i64,
    pub state: String,
    pub session: Option<String>,
    pub asked: Option<String>,
    pub reply: Option<i64>,
    pub at: u64,
}

/// How far back a mention can still be answered.
pub const ANSWERABLE_MS: u64 = 60 * 60 * 1000;

impl Db {
    /// Set the status, replacing any the message had for the machine.
    pub fn set_chat_mention_status(&self, s: &MentionStatus) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_mention_status (room, seq, machine, machine_name, root, state, session, asked, reply, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (room, seq, machine) DO UPDATE SET machine_name = excluded.machine_name,
             root = excluded.root, state = excluded.state, session = excluded.session, asked = excluded.asked,
             reply = excluded.reply, at = excluded.at",
            params![
                s.room,
                s.seq,
                s.machine,
                s.machine_name,
                s.root,
                s.state,
                s.session,
                s.asked,
                s.reply,
                s.at as i64
            ],
        )?;
        Ok(())
    }

    /// The statuses of messages `from..=to` of a room, by message and then
    /// machine's name.
    pub fn chat_mention_statuses(&self, room: &str, from: i64, to: i64) -> Result<Vec<MentionStatus>> {
        self.c().query_map(
            "SELECT room, seq, machine, machine_name, root, state, session, asked, reply, at
             FROM chat_mention_status WHERE room = ?1 AND seq >= ?2 AND seq <= ?3
             ORDER BY seq, machine_name, machine",
            params![room, from, to],
            |r| {
                Ok(MentionStatus {
                    room: r.get(0)?,
                    seq: r.get(1)?,
                    machine: r.get(2)?,
                    machine_name: r.get(3)?,
                    root: r.get(4)?,
                    state: r.get(5)?,
                    session: r.get(6)?,
                    asked: r.get(7)?,
                    reply: r.get(8)?,
                    at: r.get::<i64>(9)? as u64,
                })
            },
        )
    }

    /// `machine` replied at message `reply`, in the thread of `root`: every
    /// mention of it in that thread, from `since` on and before the reply,
    /// that wasn't answered yet, is. The numbers of the messages changed.
    pub fn answer_chat_mentions(
        &self,
        room: &str,
        root: i64,
        machine: &str,
        reply: i64,
        since: u64,
    ) -> Result<Vec<i64>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let seqs: Vec<i64> = tx.query_map(
            "SELECT seq FROM chat_mention_status WHERE room = ?1 AND root = ?2 AND machine = ?3
             AND seq < ?4 AND at >= ?5 AND state != 'answered' ORDER BY seq",
            params![room, root, machine, reply, since as i64],
            |r| r.get(0),
        )?;
        for seq in &seqs {
            tx.execute(
                "UPDATE chat_mention_status SET state = 'answered', reply = ?4
                 WHERE room = ?1 AND seq = ?2 AND machine = ?3",
                params![room, seq, machine, reply],
            )?;
        }
        tx.commit()?;
        Ok(seqs)
    }
}

/// A message's statuses go with its content.
pub(super) fn drop_for_message(tx: &impl Sql, room: &str, seq: i64) -> Result<()> {
    tx.execute("DELETE FROM chat_mention_status WHERE room = ?1 AND seq = ?2", params![room, seq])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(seq: i64, root: i64, machine: &str, state: &str, at: u64) -> MentionStatus {
        MentionStatus {
            room: "r1".into(),
            seq,
            machine: machine.into(),
            machine_name: machine.into(),
            root,
            state: state.into(),
            session: None,
            asked: None,
            reply: None,
            at,
        }
    }

    #[test]
    fn a_status_is_replaced_and_a_reply_answers_the_threads_mentions_once() {
        let db = Db::test();
        db.set_chat_mention_status(&status(5, 5, "m1", "waiting", 1_000)).unwrap();
        db.set_chat_mention_status(&status(5, 5, "m1", "delivered", 1_100)).unwrap();
        db.set_chat_mention_status(&status(6, 5, "m1", "delivered", 1_200)).unwrap();
        db.set_chat_mention_status(&status(7, 5, "m2", "delivered", 1_200)).unwrap();
        db.set_chat_mention_status(&status(9, 9, "m1", "delivered", 1_300)).unwrap();
        let all = db.chat_mention_statuses("r1", 1, 20).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].state, "delivered");
        // Only that thread's, that machine's, from `since`, before the reply.
        assert_eq!(db.answer_chat_mentions("r1", 5, "m1", 8, 1_150).unwrap(), vec![6]);
        assert_eq!(db.answer_chat_mentions("r1", 5, "m1", 8, 0).unwrap(), vec![5]);
        assert!(db.answer_chat_mentions("r1", 5, "m1", 8, 0).unwrap().is_empty());
        let after = db.chat_mention_statuses("r1", 1, 20).unwrap();
        assert_eq!(after.iter().filter(|s| s.state == "answered" && s.reply == Some(8)).count(), 2);
        assert!(after.iter().any(|s| s.machine == "m2" && s.state == "delivered"));
        assert!(after.iter().any(|s| s.seq == 9 && s.state == "delivered"));
    }
}
