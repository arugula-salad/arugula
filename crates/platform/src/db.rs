//! The platform's database handle. In process it's a second connection to
//! the hub's SQLite file (WAL, `arugula_control_common::sqlite`); M141
//! gives it a file of its own, and M143 moves it to Postgres. Either way it
//! never joins a hub table: what it needs from the hub comes through
//! [`HubPort`](crate::HubPort), so a query here that names one fails in
//! the platform's tests, whose database is its own.
//!
//! Each feature brings its own tables and queries as it moves in (Cloud's
//! from `crates/control/src/db/cloud.rs`, chat's from `db/chat.rs`), and
//! flips their `process` in `crates/control/tables.toml` to `platform`.

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use arugula_control_common::sqlite;
use rusqlite::Connection;

pub mod billing;

pub struct Db {
    conn: Mutex<Connection>,
}

/// The platform's tables, made if they aren't there: each feature's own.
const SCHEMA: &[&str] = &[billing::SCHEMA];

impl Db {
    /// The database at `path`: in process, the hub's file.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = sqlite::open(path)?;
        for schema in SCHEMA {
            conn.execute_batch(schema)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// A database of its own in memory, for tests: nothing of the hub's is
    /// in it.
    pub fn memory() -> Self {
        let conn = sqlite::memory();
        for schema in SCHEMA {
            conn.execute_batch(schema).expect("the platform's schema");
        }
        Self { conn: Mutex::new(conn) }
    }

    pub fn c(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn opens_beside_the_hub() {
        let path = std::env::temp_dir()
            .join(format!("arugula-platform-{}.db", u64::from_le_bytes(arugula_e2e::random::<8>())));
        let hub = arugula_control_common::sqlite::open(&path).unwrap();
        hub.execute_batch("CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY)").unwrap();
        let db = super::Db::open(&path).unwrap();
        let n: i64 = db.c().query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0)).unwrap();
        assert!(n >= 1, "the same file");
        let mem = super::Db::memory();
        let n: i64 =
            mem.c().query_row("SELECT count(*) FROM sqlite_master WHERE name = 'accounts'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a test's database has nothing of the hub's");
        drop((hub, db));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }
}
