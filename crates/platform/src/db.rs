//! The platform's database handle. In process it's a second connection to
//! the hub's SQLite file (WAL, `arugula_control_common::sqlite`); M141
//! gives it a file of its own, and M143 moves it to Postgres. Either way it
//! never joins a hub table: what it needs from the hub comes through
//! [`HubPort`](crate::HubPort), so a query here that names one fails in
//! the platform's tests, whose database is its own.
//!
//! Each feature brings its own queries as it moves in (billing's in
//! [`billing`], M139; Cloud's in [`cloud`], M137; chat's in [`chat`], M138;
//! projects, team members and grants in [`projects`]), and flips their
//! `process` in `crates/control/tables.toml` to `platform`. Their tables
//! are made and changed only by numbered migrations ([`migrate`], M142).

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use anyhow::Context as _;
use arugula_control_common::sqlite;
use rusqlite::Connection;

pub mod billing;
pub mod chat;
pub mod cloud;
mod legacy;
pub mod migrate;
pub mod projects;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// The database at `path` (in process, the hub's file), brought up to
    /// the latest migration ([`migrate`]): a database made before
    /// migrations is adopted, and one this build can't run on is refused.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut conn = sqlite::open(path)?;
        let report = migrate::run(&mut conn).context("the platform's schema")?;
        if report != migrate::Report::default() {
            tracing::info!(adopted = report.adopted, upgraded = report.upgraded, ran = ?report.ran, "the platform's schema");
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// A database of its own in memory, for tests: nothing of the hub's is
    /// in it.
    pub fn memory() -> Self {
        let mut conn = sqlite::memory();
        migrate::run(&mut conn).expect("the platform's schema");
        Self { conn: Mutex::new(conn) }
    }

    pub fn c(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    /// Every column of every table here that has `needle` in it, as
    /// `table.column` (tests: nothing of a deleted account is left).
    #[cfg(any(test, feature = "contract"))]
    pub fn mentions(&self, needle: &str) -> Vec<String> {
        let c = self.c();
        let tables: Vec<String> = c
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name != 'platform_migrations'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut out = Vec::new();
        for t in tables {
            let cols: Vec<String> = c
                .prepare(&format!("PRAGMA table_info({t})"))
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            for col in cols {
                let n: u64 = c
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {t} WHERE CAST({col} AS TEXT) LIKE '%' || ?1 || '%'"),
                        [needle],
                        |r| r.get(0),
                    )
                    .unwrap();
                if n > 0 {
                    out.push(format!("{t}.{col}"));
                }
            }
        }
        out
    }
}

/// A row that's already there (a unique key), as opposed to anything else
/// going wrong.
#[derive(Debug)]
pub struct Taken;

impl std::fmt::Display for Taken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("already there")
    }
}

impl std::error::Error for Taken {}

fn taken(e: rusqlite::Error) -> anyhow::Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Taken.into(),
        e => e.into(),
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
