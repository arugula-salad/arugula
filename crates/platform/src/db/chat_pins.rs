//! A channel's topic and description (`chat_topics`), pinned messages
//! (`chat_pins`) and the system lines that tell of either
//! (`chat_system`), #119. Who may set or pin is decided in
//! `crate::chat::pins`; this is storage. A deleted message drops its pin,
//! a dropped room all of these (`db::chat`).

use super::{
    Db,
    chat::{NewMessage, in_json, json_list},
    sql::{Optional as _, Sql, params},
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// What people wrote about a channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topic {
    pub topic: String,
    pub description: String,
    /// Who last changed either.
    pub author: String,
    pub at: u64,
}

/// A pinned message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub seq: i64,
    pub author: String,
    pub at: u64,
}

/// What a system line tells of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct System {
    pub kind: String,
    /// The pinned message's number, for `pin` and `unpin`.
    pub target: Option<i64>,
}

impl Db {
    pub fn chat_topic(&self, room: &str) -> Result<Option<Topic>> {
        self.c()
            .query_row("SELECT topic, description, author, at FROM chat_topics WHERE room = ?1", params![room], |r| {
                Ok(Topic { topic: r.get(0)?, description: r.get(1)?, author: r.get(2)?, at: r.get::<i64>(3)? as u64 })
            })
            .optional()
    }

    /// The topics of these rooms that have one set, in one statement.
    pub fn chat_topics_of(&self, rooms: &[&str]) -> Result<HashMap<String, Topic>> {
        if rooms.is_empty() {
            return Ok(HashMap::new());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT room, topic, description, author, at FROM chat_topics
                 WHERE (topic != '' OR description != '') AND {}",
                in_json(&c, "room", 1)
            ),
            params![json_list(rooms)?],
            |r| {
                Ok((
                    r.get::<String>(0)?,
                    Topic { topic: r.get(1)?, description: r.get(2)?, author: r.get(3)?, at: r.get::<i64>(4)? as u64 },
                ))
            },
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Set a channel's topic and description, and say so in the room with
    /// one system line each for what changed (`says`: kind and text),
    /// all or none. The new lines' numbers.
    pub fn set_chat_topic(
        &self,
        room: &str,
        t: &Topic,
        agent: Option<&str>,
        says: &[(&str, &str)],
    ) -> Result<Vec<i64>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO chat_topics (room, topic, description, author, at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (room) DO UPDATE SET topic = excluded.topic, description = excluded.description,
             author = excluded.author, at = excluded.at",
            params![room, t.topic, t.description, t.author, t.at as i64],
        )?;
        let mut seqs = vec![];
        for (kind, text) in says {
            seqs.push(super::chat::add_message_in(&tx, room, &line(&t.author, t.at, agent, text), Some((kind, None)))?);
        }
        tx.commit()?;
        Ok(seqs)
    }

    /// Pin message `seq`, or unpin it, and say so; `None` when it already
    /// was (or wasn't). Else the system line's number.
    pub fn set_chat_pin(
        &self,
        room: &str,
        seq: i64,
        on: bool,
        (author, agent): (&str, Option<&str>),
        at: u64,
    ) -> Result<Option<i64>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = if on {
            tx.execute(
                "INSERT INTO chat_pins (room, seq, author, at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (room, seq) DO NOTHING",
                params![room, seq, author, at as i64],
            )?
        } else {
            tx.execute("DELETE FROM chat_pins WHERE room = ?1 AND seq = ?2", params![room, seq])?
        };
        if n == 0 {
            return Ok(None);
        }
        let kind = if on { "pin" } else { "unpin" };
        let said = super::chat::add_message_in(&tx, room, &line(author, at, agent, ""), Some((kind, Some(seq))))?;
        tx.commit()?;
        Ok(Some(said))
    }

    /// A room's pins, the newest first.
    pub fn chat_pins(&self, room: &str) -> Result<Vec<Pin>> {
        self.c().query_map(
            "SELECT seq, author, at FROM chat_pins WHERE room = ?1 ORDER BY at DESC, seq DESC",
            params![room],
            |r| Ok(Pin { seq: r.get(0)?, author: r.get(1)?, at: r.get::<i64>(2)? as u64 }),
        )
    }

    /// How many pins each of these rooms has (those with some).
    pub fn chat_pin_counts(&self, rooms: &[&str]) -> Result<HashMap<String, u32>> {
        if rooms.is_empty() {
            return Ok(HashMap::new());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT room, count(*) FROM chat_pins WHERE {} GROUP BY room", in_json(&c, "room", 1)),
            params![json_list(rooms)?],
            |r| Ok((r.get::<String>(0)?, r.get::<i64>(1)? as u32)),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Which of the messages `from..=to` of a room are system lines, and of what.
    pub fn chat_system(&self, room: &str, from: i64, to: i64) -> Result<HashMap<i64, System>> {
        let rows = self.c().query_map(
            "SELECT seq, kind, target FROM chat_system WHERE room = ?1 AND seq >= ?2 AND seq <= ?3",
            params![room, from, to],
            |r| Ok((r.get::<i64>(0)?, System { kind: r.get(1)?, target: r.get(2)? })),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// The pinned messages of a room that are among `seqs`.
    pub fn chat_pinned_among(&self, room: &str, from: i64, to: i64) -> Result<HashSet<i64>> {
        let rows: Vec<i64> = self.c().query_map(
            "SELECT seq FROM chat_pins WHERE room = ?1 AND seq >= ?2 AND seq <= ?3",
            params![room, from, to],
            |r| r.get(0),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Whether a team's owners alone set its channels' topics.
    /// For a page of rooms, in one statement: the topics of those that have
    /// one, their pin counts, and which of `teams` let only owners set a
    /// topic (`GET /api/chat`'s budget, `hub_calls_tests`).
    #[allow(clippy::type_complexity)]
    pub fn chat_room_extras(
        &self,
        rooms: &[&str],
        teams: &[&str],
    ) -> Result<(HashMap<String, Topic>, HashMap<String, u32>, HashSet<String>)> {
        let (mut topics, mut pins, mut owners) = (HashMap::new(), HashMap::new(), HashSet::new());
        if rooms.is_empty() {
            return Ok((topics, pins, owners));
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT 't', room, topic, description, author, at, 0 FROM chat_topics
                 WHERE (topic != '' OR description != '') AND {rooms}
                 UNION ALL
                 SELECT 'p', room, '', '', '', 0, count(*) FROM chat_pins WHERE {rooms} GROUP BY room
                 UNION ALL
                 SELECT 'o', team, '', '', '', 0, 1 FROM chat_team_settings WHERE topic_owners AND {teams}",
                rooms = in_json(&c, "room", 1),
                teams = in_json(&c, "team", 2),
            ),
            params![json_list(rooms)?, json_list(teams)?],
            |r| {
                Ok((
                    r.get::<String>(0)?,
                    r.get::<String>(1)?,
                    Topic { topic: r.get(2)?, description: r.get(3)?, author: r.get(4)?, at: r.get::<i64>(5)? as u64 },
                    r.get::<i64>(6)?,
                ))
            },
        )?;
        for (kind, key, topic, n) in rows {
            match kind.as_str() {
                "t" => {
                    topics.insert(key, topic);
                }
                "p" => {
                    pins.insert(key, n as u32);
                }
                _ => {
                    owners.insert(key);
                }
            }
        }
        Ok((topics, pins, owners))
    }

    pub fn chat_topic_owners(&self, team: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT topic_owners FROM chat_team_settings WHERE team = ?1", params![team], |r| r.get(0))
            .optional()?
            .unwrap_or(false))
    }
}

/// A system line's message: `author` did it (`agent` on its machine, for
/// its owner), and `text` says what.
fn line<'a>(author: &'a str, at: u64, agent: Option<&'a str>, text: &'a str) -> NewMessage<'a> {
    NewMessage { author, at, text, quote: None, ref_machine: None, ref_pane: None, mentions: &[], agent, via: None }
}

/// A message's pin goes with its content.
pub(super) fn drop_for_message(tx: &impl Sql, room: &str, seq: i64) -> Result<()> {
    tx.execute("DELETE FROM chat_pins WHERE room = ?1 AND seq = ?2", params![room, seq])?;
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

    /// A system line is numbered like a message, but isn't unread, isn't
    /// found, and doesn't move its author's read mark over what they
    /// haven't read.
    #[test]
    fn system_lines_are_numbered_but_never_unread() {
        let db = Db::test();
        room(&db, "r1");
        let said = post(&db, "r1", "a2");
        let pinned = db.set_chat_pin("r1", said, true, ("a1", None), 5).unwrap().unwrap();
        assert_eq!(pinned, said + 1);
        let topics = db
            .set_chat_topic(
                "r1",
                &Topic { topic: "t".into(), author: "a1".into(), at: 6, ..Default::default() },
                None,
                &[("topic", "t")],
            )
            .unwrap();
        assert_eq!(topics, [said + 2]);
        assert_eq!(db.chat_unread("r1", "a1", 0, 0).unwrap(), (1, 0), "only a2's message");
        assert_eq!(db.chat_mark("r1", "a1").unwrap().0, 0, "a1 has read nothing");
        let asks = [crate::db::chat::UnreadAsk { room: "r1", read: 0, from: 0 }];
        assert_eq!(db.chat_unread_in("a1", &asks).unwrap()["r1"], (1, 0));
        let sys = db.chat_system("r1", 0, 9).unwrap();
        assert_eq!(sys.len(), 2);
        assert_eq!(sys[&pinned], System { kind: "pin".into(), target: Some(said) });
        // Once is as good as twice.
        assert_eq!(db.set_chat_pin("r1", said, true, ("a1", None), 7).unwrap(), None);
        assert_eq!(db.chat_pin_counts(&["r1", "r2"]).unwrap()["r1"], 1);
    }

    /// Deleting a message, a room or an account takes the pins and topics.
    #[test]
    fn deleting_drops_pins_and_rooms_drop_topics() {
        let db = Db::test();
        room(&db, "r1");
        room(&db, "r2");
        let (a, b) = (post(&db, "r1", "a1"), post(&db, "r1", "a2"));
        let c = post(&db, "r2", "a2");
        for (r, s, who) in [("r1", a, "a2"), ("r1", b, "a2"), ("r2", c, "a1")] {
            db.set_chat_pin(r, s, true, (who, None), 5).unwrap();
        }
        db.set_chat_topic(
            "r1",
            &Topic { topic: "x".into(), author: "a1".into(), at: 1, ..Default::default() },
            None,
            &[],
        )
        .unwrap();
        db.delete_chat_message("r1", a, 9).unwrap();
        assert_eq!(db.chat_pins("r1").unwrap().iter().map(|p| p.seq).collect::<Vec<_>>(), [b]);
        assert_eq!(db.chat_topics_of(&["r1", "r2"]).unwrap().keys().collect::<Vec<_>>(), ["r1"]);
        let mut c = db.c();
        let tx = c.transaction().unwrap();
        crate::db::chat::erase_account(&tx, "a1", &[], &[]).unwrap();
        tx.commit().unwrap();
        drop(c);
        assert!(db.chat_pins("r2").unwrap().is_empty(), "a pin by an erased account goes");
        assert_eq!(db.chat_topic("r1").unwrap().unwrap().author, "", "a topic keeps no author");
        assert_eq!(db.chat_pins("r1").unwrap().len(), 1);
    }
}
