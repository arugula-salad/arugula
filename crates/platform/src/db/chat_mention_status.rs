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
    /// With `asked`: the names it could have used (#576).
    pub names: Vec<String>,
    pub reply: Option<i64>,
    pub at: u64,
}

/// How far back a mention can still be answered.
pub const ANSWERABLE_MS: u64 = 60 * 60 * 1000;

/// How far back a mention that found its machine offline is sent again
/// when the machine connects (#419).
pub const REDELIVER_WITHIN_MS: u64 = 24 * 60 * 60 * 1000;

/// How long a mention may be `sending` before it counts as lost (control
/// restarted mid-delivery, #419): the daemon is given 5 s to answer.
pub const STALE_SENDING_MS: u64 = 2 * 60 * 1000;

/// The most mentions sent again at one connect (#419).
pub const REDELIVER_MAX: usize = 20;

impl Db {
    /// Set the status, replacing any the message had for the machine.
    pub fn set_chat_mention_status(&self, s: &MentionStatus) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_mention_status (room, seq, machine, machine_name, root, state, session, asked, reply, at, names)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (room, seq, machine) DO UPDATE SET machine_name = excluded.machine_name,
             root = excluded.root, state = excluded.state, session = excluded.session, asked = excluded.asked,
             reply = excluded.reply, at = excluded.at, names = excluded.names",
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
                s.at as i64,
                names_json(&s.names)
            ],
        )?;
        Ok(())
    }

    /// Settle a mention that was `sending`: the daemon's answer (`state`,
    /// `session`, `asked`, `names`, `at`). Only a `sending` one changes, so an agent's
    /// reply that got in first (`answered`) stays. Whether it changed.
    pub fn finish_chat_mention(&self, s: &MentionStatus) -> Result<bool> {
        let n = self.c().execute(
            "UPDATE chat_mention_status SET state = ?4, session = ?5, asked = ?6, at = ?7, names = ?8
             WHERE room = ?1 AND seq = ?2 AND machine = ?3 AND state = 'sending'",
            params![s.room, s.seq, s.machine, s.state, s.session, s.asked, s.at as i64, names_json(&s.names)],
        )?;
        Ok(n == 1)
    }

    /// Claim the mentions `machine` missed (#419): its `offline` ones, and
    /// its `sending` ones older than [`STALE_SENDING_MS`] (lost with a
    /// restart), from `since` on, oldest first, at most `max`. Each claimed
    /// one is `sending` (at `now`) when this returns, so no other connect
    /// takes it: the update only applies to a row still in the state it was
    /// read in.
    pub fn claim_offline_mentions(
        &self,
        machine: &str,
        since: u64,
        max: usize,
        now: u64,
    ) -> Result<Vec<MentionStatus>> {
        let stale = now.saturating_sub(STALE_SENDING_MS);
        let mut c = self.c();
        let tx = c.transaction()?;
        let rows: Vec<MentionStatus> = tx.query_map(
            "SELECT room, seq, machine, machine_name, root, state, session, asked, reply, at, names
             FROM chat_mention_status WHERE machine = ?1 AND at >= ?2
             AND (state = 'offline' OR (state = 'sending' AND at < ?4))
             ORDER BY at, room, seq LIMIT ?3",
            params![machine, since as i64, max as i64, stale as i64],
            status_row,
        )?;
        let mut claimed = vec![];
        for mut s in rows {
            let was = s.state.clone();
            let n = tx.execute(
                "UPDATE chat_mention_status SET state = 'sending', at = ?4
                 WHERE room = ?1 AND seq = ?2 AND machine = ?3 AND state = ?5 AND at < ?6",
                params![
                    s.room,
                    s.seq,
                    s.machine,
                    now as i64,
                    was,
                    if was == "sending" { stale } else { u64::MAX >> 1 } as i64
                ],
            )?;
            if n == 1 {
                s.state = "sending".into();
                s.at = now;
                claimed.push(s);
            }
        }
        tx.commit()?;
        Ok(claimed)
    }

    /// Mentions `sending` since before `cutoff` were lost with a restart
    /// (#419): they're `offline` again, to be sent when their machine
    /// connects. The machines they were for.
    pub fn release_stale_mentions(&self, cutoff: u64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let machines: Vec<String> = tx.query_map(
            "SELECT DISTINCT machine FROM chat_mention_status WHERE state = 'sending' AND at < ?1",
            params![cutoff as i64],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE chat_mention_status SET state = 'offline' WHERE state = 'sending' AND at < ?1",
            params![cutoff as i64],
        )?;
        tx.commit()?;
        Ok(machines)
    }

    /// The statuses of messages `from..=to` of a room, by message and then
    /// machine's name.
    pub fn chat_mention_statuses(&self, room: &str, from: i64, to: i64) -> Result<Vec<MentionStatus>> {
        self.c().query_map(
            "SELECT room, seq, machine, machine_name, root, state, session, asked, reply, at, names
             FROM chat_mention_status WHERE room = ?1 AND seq >= ?2 AND seq <= ?3
             ORDER BY seq, machine_name, machine",
            params![room, from, to],
            status_row,
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
             AND seq < ?4 AND at >= ?5 AND state NOT IN ('answered', 'chain') ORDER BY seq",
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

fn status_row(r: &super::sql::Row) -> Result<MentionStatus> {
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
        names: r.get::<Option<String>>(10)?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
    })
}

/// `names` as stored: a JSON array, or NULL for none.
fn names_json(names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| serde_json::to_string(names).unwrap_or_default())
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
            names: vec![],
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
