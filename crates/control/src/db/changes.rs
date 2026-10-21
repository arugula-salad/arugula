//! The hub's outbox (#1153): one row for each change the platform's caches
//! follow (a new account, an account's name or its deletion, a revoked
//! device, a roster or a lock, a machine joining, moving, leaving or being
//! shared, a Labs flag), written in the same transaction as the change, and
//! read by the platform through `HubPort::changes`. Its kinds are
//! `ChangeKind`'s.
//!
//! **Every write that changes what `HubPort` answers writes a row here**:
//! the platform keeps those answers for as long as it reads this feed, not
//! for a fixed time (`arugula_platform::cache`), so a write without its row
//! would be stale there until something else drops it. Only billing's
//! meters aren't covered (the platform keeps them a cache tick).
//!
//! `seq` is AUTOINCREMENT, so it's never reused, even after the newest rows
//! are pruned. Rows go after [`KEEP_MS`], and beyond the newest
//! [`KEEP_ROWS`]: a platform that was away longer sees a gap and drops its
//! caches. Deleting an account takes every row naming it, its devices, its
//! machines and the teams it disbanded, and leaves one row that names
//! nobody (#173).

use std::collections::HashSet;

use arugula_platform::hub::{Change, ChangeKind, ChangePage};
use rusqlite::{Connection, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hub_changes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    subject TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS hub_changes_subject ON hub_changes (subject);
";

/// How long a change is kept: the platform reads the feed every few
/// seconds, so a week covers any outage worth riding out.
pub const KEEP_MS: u64 = 7 * 24 * 3600 * 1000;
/// And at most this many, the newest.
pub const KEEP_ROWS: u64 = 100_000;

/// Note a change, inside the transaction that makes it.
pub(super) fn changed(c: &Connection, kind: ChangeKind, subject: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO hub_changes (at, kind, subject) VALUES (?1, ?2, ?3)",
        params![arugula_e2e::now_ms(), kind.as_str(), subject],
    )?;
    Ok(())
}

/// Forget the changes about these subjects (an account being deleted).
pub(super) fn forget(c: &Connection, subjects: &HashSet<String>) -> rusqlite::Result<()> {
    for s in subjects {
        c.execute("DELETE FROM hub_changes WHERE subject = ?1", [s])?;
    }
    Ok(())
}

impl Db {
    /// The changes after `after`, oldest first, at most `limit`.
    pub fn changes(&self, after: u64, limit: u32) -> anyhow::Result<ChangePage> {
        let c = self.c();
        let mut q = c.prepare("SELECT seq, at, kind, subject FROM hub_changes WHERE seq > ?1 ORDER BY seq LIMIT ?2")?;
        let changes = q
            .query_map(params![after, limit], |r| {
                Ok(Change {
                    seq: r.get(0)?,
                    at: r.get(1)?,
                    kind: ChangeKind::parse(&r.get::<_, String>(2)?),
                    subject: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let last: u64 =
            c.query_row("SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'hub_changes'), 0)", [], |r| {
                r.get(0)
            })?;
        let oldest: Option<u64> = c.query_row("SELECT min(seq) FROM hub_changes", [], |r| r.get(0))?;
        let gap = after.saturating_add(1) < oldest.unwrap_or(last + 1);
        Ok(ChangePage { changes, last, gap })
    }

    /// The newest change written (0 for none): what the hub tells the
    /// platform with each call (`arugula_platform::cache::SEQ_HEADER`), so
    /// a call after a write reads it.
    pub fn last_change(&self) -> anyhow::Result<u64> {
        Ok(self.c().query_row(
            "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'hub_changes'), 0)",
            [],
            |r| r.get(0),
        )?)
    }

    /// Drop changes older than [`KEEP_MS`] and beyond the newest
    /// [`KEEP_ROWS`]; how many went.
    pub fn prune_changes(&self, now: u64) -> anyhow::Result<usize> {
        let c = self.c();
        let old = c.execute("DELETE FROM hub_changes WHERE at < ?1", params![now.saturating_sub(KEEP_MS)])?;
        let many = c.execute(
            "DELETE FROM hub_changes WHERE seq <= (SELECT max(seq) FROM hub_changes) - ?1",
            params![KEEP_ROWS],
        )?;
        Ok(old + many)
    }
}

#[cfg(test)]
mod tests {
    use arugula_platform::hub::ChangeKind;

    use super::super::Db;

    /// A change that fails takes its row with it: the roster version was
    /// taken, so nothing changed.
    #[test]
    fn written_with_the_change_or_not_at_all() {
        let db = Db::memory();
        let t = super::super::Team {
            id: "t1".into(),
            name: "one".into(),
            founder: "a".into(),
            founder_root: "r".into(),
            locked: false,
        };
        db.add_team(&t, 1, r#"{"members":[]}"#, 1).unwrap();
        let after = db.changes(0, 0).unwrap().last;
        assert!(db.add_roster("t1", 1, r#"{"members":[]}"#).is_err(), "version 1 is taken");
        assert!(db.changes(after, 10).unwrap().changes.is_empty());
        db.add_roster("t1", 2, r#"{"members":[]}"#).unwrap();
        let page = db.changes(after, 10).unwrap();
        assert_eq!(page.changes.len(), 1);
        assert_eq!((page.changes[0].kind, page.changes[0].subject.as_str()), (ChangeKind::TeamRoster, "t1"));
    }

    #[test]
    fn pruned_by_age_and_by_count() {
        let db = Db::memory();
        for i in 0..5 {
            db.c()
                .execute("INSERT INTO hub_changes (at, kind, subject) VALUES (?1, 'labs.flag', 'x')", [i * 1000])
                .unwrap();
        }
        assert_eq!(db.prune_changes(super::KEEP_MS + 2_500).unwrap(), 3, "older than a week");
        let page = db.changes(0, 10).unwrap();
        assert_eq!(page.changes.iter().map(|c| c.seq).collect::<Vec<_>>(), [4, 5]);
        assert!(page.gap, "1 to 3 went unread");
        assert!(!db.changes(3, 10).unwrap().gap);
        db.c()
            .execute(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
                 INSERT INTO hub_changes (at, kind, subject) SELECT ?2, 'labs.flag', 'x' FROM n",
                rusqlite::params![super::KEEP_ROWS, super::KEEP_MS + 2_500],
            )
            .unwrap();
        assert_eq!(db.prune_changes(super::KEEP_MS + 2_500).unwrap(), 2, "the newest 100,000 stay");
        let page = db.changes(0, 1).unwrap();
        assert_eq!((page.changes[0].seq, page.last), (6, super::KEEP_ROWS + 5));
    }
}
