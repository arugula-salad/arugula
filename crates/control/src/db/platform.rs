//! What the hub keeps about the platform (M140):
//! - `maintenance`: the maintenance switch (M141), one row while it's on.
//!   The operator sets it with `arugula-control maintenance` on the hub's
//!   machine; the running hub reads it every second (`maintenance.rs`), so
//!   it holds across a restart, and setting it restarts nothing.
//! - `untold_deletions`: accounts the hub deleted that the platform hasn't
//!   confirmed erasing yet. Written in the deletion's own transaction, and
//!   gone once the platform says it's done: out of process the two can't
//!   share a transaction, and the change feed names nobody (#173), so this
//!   is how a deletion survives the platform being away.

use arugula_platform_api::Deleted;
use rusqlite::{Connection, OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS maintenance (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    retry_after INTEGER NOT NULL,
    pause INTEGER NOT NULL DEFAULT 0,
    since INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS untold_deletions (
    account TEXT PRIMARY KEY,
    deleted TEXT NOT NULL,
    at INTEGER NOT NULL
);
";

/// The maintenance switch, while it's on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Maintenance {
    /// What `Retry-After` says, in seconds.
    pub retry_after: u64,
    /// The in-process platform's background work is held too.
    pub pause: bool,
    /// Since when (ms).
    pub since: u64,
}

/// The switch, from a connection of its own (the operator's command).
pub fn maintenance(c: &Connection) -> rusqlite::Result<Option<Maintenance>> {
    c.query_row("SELECT retry_after, pause, since FROM maintenance WHERE id = 1", [], |r| {
        Ok(Maintenance { retry_after: r.get(0)?, pause: r.get::<_, i64>(1)? != 0, since: r.get(2)? })
    })
    .optional()
}

/// Turn the switch on (or change it), or off with `None`.
pub fn set_maintenance(c: &Connection, m: Option<Maintenance>) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    match m {
        Some(m) => c.execute(
            "INSERT INTO maintenance (id, retry_after, pause, since) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET retry_after = ?1, pause = ?2",
            params![m.retry_after, i64::from(m.pause), m.since],
        )?,
        None => c.execute("DELETE FROM maintenance", [])?,
    };
    Ok(())
}

/// Note a deletion the platform hasn't been told of, inside the
/// deletion's transaction.
pub(super) fn untold(c: &Connection, d: &Deleted, now: u64) -> anyhow::Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO untold_deletions (account, deleted, at) VALUES (?1, ?2, ?3)",
        params![d.account, serde_json::to_string(d)?, now],
    )?;
    Ok(())
}

impl Db {
    pub fn maintenance(&self) -> anyhow::Result<Option<Maintenance>> {
        Ok(maintenance(&self.c())?)
    }

    pub fn set_maintenance(&self, m: Option<Maintenance>) -> anyhow::Result<()> {
        Ok(set_maintenance(&self.c(), m)?)
    }

    /// Deletions the platform hasn't confirmed, oldest first.
    pub fn untold_deletions(&self) -> anyhow::Result<Vec<Deleted>> {
        let c = self.c();
        let mut q = c.prepare("SELECT deleted FROM untold_deletions ORDER BY at")?;
        let rows: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        rows.iter().map(|j| Ok(serde_json::from_str(j)?)).collect()
    }

    /// The platform erased this account.
    pub fn told_deletion(&self, account: &str) -> anyhow::Result<()> {
        self.c().execute("DELETE FROM untold_deletions WHERE account = ?1", params![account])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_and_the_untold() {
        let db = Db::memory();
        assert_eq!(db.maintenance().unwrap(), None);
        let on = Maintenance { retry_after: 30, pause: true, since: 5 };
        db.set_maintenance(Some(on)).unwrap();
        assert_eq!(db.maintenance().unwrap(), Some(on));
        db.set_maintenance(Some(Maintenance { retry_after: 60, pause: false, since: 9 })).unwrap();
        let m = db.maintenance().unwrap().unwrap();
        assert_eq!((m.retry_after, m.pause, m.since), (60, false, 5), "changed, not restarted");
        db.set_maintenance(None).unwrap();
        assert_eq!(db.maintenance().unwrap(), None);

        let d = Deleted { account: "acct-a".into(), machines: vec!["dmn-1".into()], disbanded: vec![] };
        untold(&db.c(), &d, 1).unwrap();
        assert_eq!(db.untold_deletions().unwrap(), vec![d]);
        db.told_deletion("acct-a").unwrap();
        assert!(db.untold_deletions().unwrap().is_empty());
    }
}
