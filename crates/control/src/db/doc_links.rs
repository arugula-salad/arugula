//! Shared Docs (#1064): a link's row and its encrypted parts. Control holds
//! ciphertext only and never reads a part; it counts bytes and keeps the
//! limits, all of them inside one transaction with the write they guard, so
//! a refused part leaves no row behind.
use super::Db;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

/// A link's snapshot, in ciphertext bytes (a Doc can be about 43 MB).
pub const MAX_LINK: u64 = 48 << 20;
/// What one account keeps in all, and how many links it may have.
pub const MAX_ACCOUNT: u64 = 200 << 20;
pub const MAX_LINKS: u64 = 100;
/// Parts in one snapshot: each is at most 1 MB.
pub const MAX_PARTS: u32 = 48;

pub(super) const SCHEMA: &str = "
-- A link `version` > 0 is being served; 0 is a first upload not yet
-- committed. `size` and `parts` are the served version's.
CREATE TABLE IF NOT EXISTS doc_links (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    daemon TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    parts INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    made INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    opens INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS doc_links_account ON doc_links (account);
CREATE INDEX IF NOT EXISTS doc_links_daemon ON doc_links (daemon);
-- Ciphertext, at most 1 MB a row. `at` is when it came, for sweeping a
-- version nobody committed.
CREATE TABLE IF NOT EXISTS doc_link_parts (
    id TEXT NOT NULL,
    ver INTEGER NOT NULL,
    n INTEGER NOT NULL,
    data BLOB NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (id, ver, n)
);
";

/// Why a request about a link was refused.
#[derive(Debug)]
pub enum DocErr {
    /// No such link, or it's another machine's or account's: the same answer.
    Gone,
    /// The version is not newer than the one served.
    Stale,
    /// Over the 48 MB a link may hold.
    TooBig,
    /// Over the account's 200 MB.
    Quota,
    /// The account has all the links it may.
    Links,
    /// The parts aren't the ones the commit says: what is wrong.
    Incomplete(String),
    Db(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for DocErr {
    fn from(e: E) -> Self {
        DocErr::Db(e.into())
    }
}

type R<T> = Result<T, DocErr>;

/// The machine making a link, and its account.
#[derive(Clone, Copy)]
pub struct Maker<'a> {
    pub account: &'a str,
    pub daemon: &'a str,
}

/// One of an account's links, as its list shows it.
#[derive(Debug, Serialize)]
pub struct DocLink {
    pub id: String,
    pub daemon: String,
    pub size: u64,
    pub made: u64,
    pub updated: u64,
    pub opens: u64,
}

/// What an account holds, in served bytes and links (rows, counting a first
/// upload not yet committed).
pub struct DocUse {
    pub used: u64,
    pub links: u64,
}

/// The served version of a link.
pub struct Served {
    pub ver: u64,
    pub parts: u32,
    pub size: u64,
}

/// A link's owner: `(account, daemon, served version)`.
fn owner(tx: &Transaction, id: &str) -> rusqlite::Result<Option<(String, String, u64)>> {
    tx.query_row("SELECT account, daemon, version FROM doc_links WHERE id = ?1", params![id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .optional()
}

/// The link, if `account` and `daemon` (when given) own it.
fn owned(tx: &Transaction, id: &str, account: &str, daemon: Option<&str>) -> R<u64> {
    match owner(tx, id)? {
        Some((a, d, v)) if a == account && daemon.is_none_or(|x| x == d) => Ok(v),
        _ => Err(DocErr::Gone),
    }
}

/// Served bytes of the account's links other than `except`.
fn served_bytes(tx: &Transaction, account: &str, except: &str) -> rusqlite::Result<u64> {
    tx.query_row(
        "SELECT COALESCE(SUM(size), 0) FROM doc_links WHERE account = ?1 AND version > 0 AND id != ?2",
        params![account, except],
        |r| r.get(0),
    )
}

impl Db {
    /// Keep one part of a version. Everything that could refuse it is
    /// checked first, in the same transaction: nothing is written for a
    /// part that is refused. The first part of an id makes the link, and
    /// records the machine that made it.
    pub fn put_doc_part(&self, id: &str, by: Maker, ver: u64, n: u32, data: &[u8], now: u64) -> R<()> {
        let Maker { account, daemon } = by;
        let mut c = self.c();
        let tx = c.transaction()?;
        let len = data.len() as u64;
        let exists = match owner(&tx, id)? {
            Some((a, d, v)) => {
                if a != account || d != daemon {
                    return Err(DocErr::Gone);
                }
                if ver <= v {
                    return Err(DocErr::Stale);
                }
                true
            }
            None => {
                let rows: u64 =
                    tx.query_row("SELECT COUNT(*) FROM doc_links WHERE account = ?1", params![account], |r| r.get(0))?;
                if rows >= MAX_LINKS {
                    return Err(DocErr::Links);
                }
                false
            }
        };
        // What this link has uploaded and not committed, without the part
        // this one replaces.
        let waiting: u64 = tx.query_row(
            "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM doc_link_parts WHERE id = ?1 AND NOT (ver = ?2 AND n = ?3)",
            params![id, ver, n],
            |r| r.get(0),
        )?;
        if waiting + len > MAX_LINK {
            return Err(DocErr::TooBig);
        }
        // The account's served bytes and every uncommitted byte of every
        // link it has: many links half uploaded must not add up to more
        // than the account may keep.
        let others: u64 = tx.query_row(
            "SELECT COALESCE(SUM(LENGTH(p.data)), 0) FROM doc_link_parts p JOIN doc_links l ON l.id = p.id
             WHERE l.account = ?1 AND NOT (p.id = ?2 AND p.ver = ?3 AND p.n = ?4)",
            params![account, id, ver, n],
            |r| r.get(0),
        )?;
        let served: u64 = tx.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM doc_links WHERE account = ?1 AND version > 0",
            params![account],
            |r| r.get(0),
        )?;
        if served + others + len > MAX_ACCOUNT {
            return Err(DocErr::Quota);
        }
        if !exists {
            tx.execute(
                "INSERT INTO doc_links (id, account, daemon, made, updated) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, account, daemon, now],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO doc_link_parts (id, ver, n, data, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, ver, n, data, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Serve `ver`: it must be newer than the one served, hold exactly the
    /// parts `0..parts` that add up to `size`, and fit the limits (again,
    /// whatever got in). Then, atomically, it is the served version and
    /// every other version's parts go.
    pub fn commit_doc_link(&self, id: &str, by: Maker, ver: u64, parts: u32, size: u64, now: u64) -> R<()> {
        let Maker { account, daemon } = by;
        let mut c = self.c();
        let tx = c.transaction()?;
        let served = owned(&tx, id, account, Some(daemon))?;
        if ver <= served {
            return Err(DocErr::Stale);
        }
        if size > MAX_LINK {
            return Err(DocErr::TooBig);
        }
        let stored: Vec<(u32, u64)> = {
            let mut q =
                tx.prepare("SELECT n, LENGTH(data) FROM doc_link_parts WHERE id = ?1 AND ver = ?2 ORDER BY n")?;
            q.query_map(params![id, ver], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        if let Some(extra) = stored.iter().find(|(n, _)| *n >= parts) {
            return Err(DocErr::Incomplete(format!("part {} is beyond the {parts} parts of the commit", extra.0)));
        }
        if let Some(n) = (0..parts).find(|n| !stored.iter().any(|(m, _)| m == n)) {
            return Err(DocErr::Incomplete(format!("part {n} was never uploaded")));
        }
        let sum: u64 = stored.iter().map(|(_, l)| l).sum();
        if sum != size {
            return Err(DocErr::Incomplete(format!("the parts add up to {sum} bytes, not {size}")));
        }
        if served_bytes(&tx, account, id)? + size > MAX_ACCOUNT {
            return Err(DocErr::Quota);
        }
        if served == 0 {
            let links: u64 = tx.query_row(
                "SELECT COUNT(*) FROM doc_links WHERE account = ?1 AND version > 0",
                params![account],
                |r| r.get(0),
            )?;
            if links >= MAX_LINKS {
                return Err(DocErr::Links);
            }
        }
        tx.execute(
            "UPDATE doc_links SET version = ?2, parts = ?3, size = ?4, updated = ?5 WHERE id = ?1",
            params![id, ver, parts, size, now],
        )?;
        tx.execute("DELETE FROM doc_link_parts WHERE id = ?1 AND ver != ?2", params![id, ver])?;
        tx.commit()?;
        Ok(())
    }

    /// Unshare: the link and its parts. `daemon` when the machine does it
    /// (only the one that made the link may); a session does it for any link
    /// of its account.
    pub fn delete_doc_link(&self, id: &str, account: &str, daemon: Option<&str>) -> R<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        owned(&tx, id, account, daemon)?;
        tx.execute("DELETE FROM doc_link_parts WHERE id = ?1", params![id])?;
        tx.execute("DELETE FROM doc_links WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// An account's links that are being served, newest update first.
    pub fn doc_links(&self, account: &str) -> anyhow::Result<Vec<DocLink>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT id, daemon, size, made, updated, opens FROM doc_links
             WHERE account = ?1 AND version > 0 ORDER BY updated DESC, id",
        )?;
        let rows = q.query_map(params![account], |r| {
            Ok(DocLink {
                id: r.get(0)?,
                daemon: r.get(1)?,
                size: r.get(2)?,
                made: r.get(3)?,
                updated: r.get(4)?,
                opens: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn doc_use(&self, account: &str) -> anyhow::Result<DocUse> {
        let c = self.c();
        let (used, links) = c.query_row(
            "SELECT COALESCE(SUM(size), 0), COUNT(*) FROM doc_links WHERE account = ?1",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(DocUse { used, links })
    }

    /// The version a link serves, if it has one.
    pub fn doc_served(&self, id: &str) -> anyhow::Result<Option<Served>> {
        Ok(self
            .c()
            .query_row("SELECT version, parts, size FROM doc_links WHERE id = ?1 AND version > 0", params![id], |r| {
                Ok(Served { ver: r.get(0)?, parts: r.get(1)?, size: r.get(2)? })
            })
            .optional()?)
    }

    /// One part of a version.
    pub fn doc_part(&self, id: &str, ver: u64, n: u32) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self
            .c()
            .query_row(
                "SELECT data FROM doc_link_parts WHERE id = ?1 AND ver = ?2 AND n = ?3",
                params![id, ver, n],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Opens counted since the last flush, added to the links' totals.
    pub fn add_doc_opens(&self, opens: &std::collections::HashMap<String, u64>) -> anyhow::Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for (id, n) in opens {
            tx.execute("UPDATE doc_links SET opens = opens + ?2 WHERE id = ?1", params![id, n])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Parts of a version nobody committed, older than `before` (ms), and
    /// the links that never got a committed version. The parts of a served
    /// version are never swept. How many parts went.
    pub fn sweep_doc_links(&self, before: u64) -> anyhow::Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let n = tx.execute(
            "DELETE FROM doc_link_parts WHERE at < ?1
             AND ver > COALESCE((SELECT version FROM doc_links WHERE id = doc_link_parts.id), 0)",
            params![before],
        )?;
        tx.execute(
            "DELETE FROM doc_links WHERE version = 0 AND NOT EXISTS (SELECT 1 FROM doc_link_parts WHERE id = doc_links.id)",
            [],
        )?;
        tx.commit()?;
        Ok(n)
    }

    /// Parts held for a link, with their versions (tests).
    #[cfg(test)]
    pub fn doc_part_keys(&self, id: &str) -> Vec<(u64, u32)> {
        let c = self.c();
        let mut q = c.prepare("SELECT ver, n FROM doc_link_parts WHERE id = ?1 ORDER BY ver, n").unwrap();
        q.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
    }

    /// Rows in both tables, for every link (tests).
    #[cfg(test)]
    pub fn doc_rows(&self) -> (u64, u64) {
        let c = self.c();
        let n = |t: &str| c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
        (n("doc_links"), n("doc_link_parts"))
    }

    /// A part or link as an older build or a higher limit might have let in:
    /// no checks (tests of what commit checks again).
    #[cfg(test)]
    pub fn seed_doc(
        &self,
        id: &str,
        account: &str,
        daemon: &str,
        version: u64,
        size: u64,
        part: Option<(u64, u32, &[u8])>,
    ) {
        let c = self.c();
        c.execute(
            "INSERT OR IGNORE INTO doc_links (id, account, daemon, version, parts, size, made, updated)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, 1, 1)",
            params![id, account, daemon, version, size],
        )
        .unwrap();
        if let Some((ver, n, data)) = part {
            c.execute(
                "INSERT OR REPLACE INTO doc_link_parts (id, ver, n, data, at) VALUES (?1, ?2, ?3, ?4, 1)",
                params![id, ver, n, data],
            )
            .unwrap();
        }
    }
}
