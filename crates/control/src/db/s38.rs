//! S38 prototype (#979, throwaway): conversations kept in control as
//! ciphertext, and the lease on who runs each one's lead.
//!
//! What's here in the clear: the conversation's id, the account that owns
//! it, the record's version, each log entry's number, time, size and the
//! device that wrote it, the wrapped keys by device id, and the lease
//! (holder, fencing token, expiry, and the last entry its lead was shown).
//! The record (title, members, links) and every entry are sealed under the
//! conversation's key, which control never has (`arugula_e2e::conv`).

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::Db;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conv_heads (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    ver INTEGER NOT NULL,
    record BLOB NOT NULL,
    keys TEXT NOT NULL,
    updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS conv_log (
    conv TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at INTEGER NOT NULL,
    device TEXT NOT NULL,
    ct BLOB NOT NULL,
    PRIMARY KEY (conv, seq)
);
CREATE TABLE IF NOT EXISTS conv_leases (
    conv TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    token INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    cursor INTEGER NOT NULL
);
";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Head {
    pub id: String,
    pub account: String,
    pub ver: u64,
    /// Sealed, base64.
    pub record: String,
    /// Device id → the conversation key wrapped to it, base64 (JSON).
    pub keys: serde_json::Value,
    pub last: u64,
    pub lease: Option<Lease>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Lease {
    pub holder: String,
    /// The fencing token: one more each time the lease changes hands.
    pub token: u64,
    pub expires: u64,
    /// The last entry the lead was shown.
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub seq: u64,
    pub at: u64,
    pub device: String,
    /// Sealed, base64.
    pub ct: String,
}

/// Why a lease or a fenced write was refused.
#[derive(Debug)]
pub enum Refused {
    NoSuch,
    NotYours,
    /// Someone else holds it (or the token is stale): who, and their token.
    Held(Lease),
    /// The record changed since the version given.
    Stale(u64),
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

impl Db {
    /// Every conversation `account` owns, with its lease.
    pub fn s38_heads(&self, account: &str) -> anyhow::Result<Vec<Head>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT h.id, h.account, h.ver, h.record, h.keys,
                    (SELECT COALESCE(MAX(seq), 0) FROM conv_log WHERE conv = h.id),
                    l.holder, l.token, l.expires, l.cursor
             FROM conv_heads h LEFT JOIN conv_leases l ON l.conv = h.id WHERE h.account = ?1 ORDER BY h.id",
        )?;
        let rows = q.query_map(params![account], |r| {
            let keys: String = r.get(4)?;
            let record: Vec<u8> = r.get(3)?;
            let holder: Option<String> = r.get(6)?;
            Ok(Head {
                id: r.get(0)?,
                account: r.get(1)?,
                ver: r.get(2)?,
                record: b64(&record),
                keys: serde_json::from_str(&keys).unwrap_or_default(),
                last: r.get(5)?,
                lease: match holder {
                    Some(holder) => Some(Lease { holder, token: r.get(7)?, expires: r.get(8)?, cursor: r.get(9)? }),
                    None => None,
                },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn s38_account(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<String>> {
        c.query_row("SELECT account FROM conv_heads WHERE id = ?1", params![id], |r| r.get(0)).optional()
    }

    /// Put the record: a new conversation (`ver` 1), or version `ver` of
    /// one `account` owns, after `ver - 1`.
    pub fn s38_put(
        &self,
        account: &str,
        id: &str,
        ver: u64,
        record: &[u8],
        keys: &serde_json::Value,
        now: u64,
    ) -> anyhow::Result<Result<(), Refused>> {
        let c = self.c();
        let cur: Option<(String, u64)> = c
            .query_row("SELECT account, ver FROM conv_heads WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        match cur {
            Some((a, _)) if a != account => return Ok(Err(Refused::NotYours)),
            Some((_, v)) if v + 1 != ver => return Ok(Err(Refused::Stale(v))),
            None if ver != 1 => return Ok(Err(Refused::Stale(0))),
            _ => {}
        }
        c.execute(
            "INSERT INTO conv_heads (id, account, ver, record, keys, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET ver = ?3, record = ?4, keys = ?5, updated = ?6",
            params![id, account, ver, record, keys.to_string(), now],
        )?;
        Ok(Ok(()))
    }

    /// Append an entry. `fence`: the writer's lease token, for what only
    /// the lead's machine writes (the lead's replies, its links' events):
    /// refused unless `device` holds the lease now, with that token.
    pub fn s38_append(
        &self,
        account: &str,
        id: &str,
        device: &str,
        fence: Option<u64>,
        ct: &[u8],
        now: u64,
    ) -> anyhow::Result<Result<u64, Refused>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        match Self::s38_account(&tx, id)? {
            None => return Ok(Err(Refused::NoSuch)),
            Some(a) if a != account => return Ok(Err(Refused::NotYours)),
            _ => {}
        }
        if let Some(token) = fence {
            let l = Self::s38_lease_of(&tx, id)?;
            match l {
                Some(l) if l.holder == device && l.token == token && l.expires > now => {}
                Some(l) => return Ok(Err(Refused::Held(l))),
                None => return Ok(Err(Refused::Held(Lease { holder: String::new(), token: 0, expires: 0, cursor: 0 }))),
            }
        }
        let seq: u64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM conv_log WHERE conv = ?1", params![id], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO conv_log (conv, seq, at, device, ct) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, seq, now, device, ct],
        )?;
        tx.commit()?;
        Ok(Ok(seq))
    }

    /// Entries after `after`, at most `limit`.
    pub fn s38_log(&self, account: &str, id: &str, after: u64, limit: u64) -> anyhow::Result<Result<Vec<Entry>, Refused>> {
        let c = self.c();
        match Self::s38_account(&c, id)? {
            None => return Ok(Err(Refused::NoSuch)),
            Some(a) if a != account => return Ok(Err(Refused::NotYours)),
            _ => {}
        }
        let mut q =
            c.prepare("SELECT seq, at, device, ct FROM conv_log WHERE conv = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?;
        let rows = q.query_map(params![id, after, limit], |r| {
            let ct: Vec<u8> = r.get(3)?;
            Ok(Entry { seq: r.get(0)?, at: r.get(1)?, device: r.get(2)?, ct: b64(&ct) })
        })?;
        Ok(Ok(rows.collect::<Result<_, _>>()?))
    }

    fn s38_lease_of(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Lease>> {
        c.query_row("SELECT holder, token, expires, cursor FROM conv_leases WHERE conv = ?1", params![id], |r| {
            Ok(Lease { holder: r.get(0)?, token: r.get(1)?, expires: r.get(2)?, cursor: r.get(3)? })
        })
        .optional()
    }

    /// Take or renew the lead's lease for `device` (a machine of the
    /// conversation's account: the owner's, #978), for `ttl` ms.
    /// - Nobody holds it, or it expired: `device` takes it, and the token
    ///   goes up by one.
    /// - `device` holds it: renewed, if it gives the token it holds; its
    ///   `cursor` moves forward.
    /// - Someone else holds it, unexpired: refused, saying who.
    ///
    /// `release`: give it up now (its holder only), so another machine
    /// needn't wait out the expiry.
    #[allow(clippy::too_many_arguments)]
    pub fn s38_lease(
        &self,
        account: &str,
        id: &str,
        device: &str,
        token: Option<u64>,
        cursor: Option<u64>,
        release: bool,
        ttl: u64,
        now: u64,
    ) -> anyhow::Result<Result<Lease, Refused>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        match Self::s38_account(&tx, id)? {
            None => return Ok(Err(Refused::NoSuch)),
            Some(a) if a != account => return Ok(Err(Refused::NotYours)),
            _ => {}
        }
        let cur = Self::s38_lease_of(&tx, id)?;
        let next = match cur {
            Some(l) if l.holder == device && l.expires > now => {
                if token != Some(l.token) {
                    return Ok(Err(Refused::Held(l)));
                }
                if release {
                    Lease { expires: now, cursor: cursor.unwrap_or(l.cursor).max(l.cursor), ..l }
                } else {
                    Lease { expires: now + ttl, cursor: cursor.unwrap_or(l.cursor).max(l.cursor), ..l }
                }
            }
            Some(l) if l.expires > now => return Ok(Err(Refused::Held(l))),
            _ if release => return Ok(Err(Refused::NoSuch)),
            // Free, or expired: a new holder (or the same one, back after
            // losing it), with a new token. The cursor is the lead's, and
            // stays with the conversation.
            Some(l) => Lease { holder: device.to_owned(), token: l.token + 1, expires: now + ttl, cursor: l.cursor },
            None => Lease { holder: device.to_owned(), token: 1, expires: now + ttl, cursor: cursor.unwrap_or(0) },
        };
        tx.execute(
            "INSERT INTO conv_leases (conv, holder, token, expires, cursor) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (conv) DO UPDATE SET holder = ?2, token = ?3, expires = ?4, cursor = ?5",
            params![id, next.holder, next.token, next.expires, next.cursor],
        )?;
        tx.commit()?;
        Ok(Ok(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time_and_a_stale_token_writes_nothing() {
        let db = Db::memory();
        let keys = serde_json::json!({});
        db.s38_put("acct", "c-1", 1, b"sealed", &keys, 0).unwrap().unwrap();
        assert!(matches!(db.s38_put("other", "c-1", 2, b"x", &keys, 0).unwrap(), Err(Refused::NotYours)));
        assert!(matches!(db.s38_put("acct", "c-1", 3, b"x", &keys, 0).unwrap(), Err(Refused::Stale(1))));

        let a = db.s38_lease("acct", "c-1", "box-a", None, None, false, 15, 100).unwrap().unwrap();
        assert_eq!((a.holder.as_str(), a.token), ("box-a", 1));
        assert!(matches!(db.s38_lease("acct", "c-1", "box-b", None, None, false, 15, 110).unwrap(), Err(Refused::Held(_))));
        assert_eq!(db.s38_append("acct", "c-1", "box-a", Some(1), b"m1", 111).unwrap().unwrap(), 1);
        // box-a sleeps through its expiry; box-b takes it, with token 2.
        let b = db.s38_lease("acct", "c-1", "box-b", None, Some(1), false, 15, 120).unwrap().unwrap();
        assert_eq!((b.holder.as_str(), b.token, b.cursor), ("box-b", 2, 0), "the cursor is the conversation's");
        // box-a wakes: its writes and renewals are refused.
        assert!(matches!(db.s38_append("acct", "c-1", "box-a", Some(1), b"late", 121).unwrap(), Err(Refused::Held(_))));
        assert!(matches!(db.s38_lease("acct", "c-1", "box-a", Some(1), None, false, 15, 121).unwrap(), Err(Refused::Held(_))));
        // A person's post needs no lease.
        assert_eq!(db.s38_append("acct", "c-1", "phone", None, b"m2", 122).unwrap().unwrap(), 2);
        assert_eq!(db.s38_log("acct", "c-1", 0, 10).unwrap().unwrap().len(), 2);
    }
}
