//! The platform's database handle: SQLite or Postgres (M143), with every
//! query written once for both ([`sql`]). In process it's a second
//! connection to the hub's SQLite file (WAL,
//! `arugula_control_common::sqlite`); M141 gives it a file of its own, and
//! hosted control runs it on Fly Managed Postgres from M145 ([`Args`]:
//! `ARUGULA_PLATFORM_DATABASE_URL`). Self-hosted control, dev, tests and
//! testnet stay on SQLite unless they ask. Either way it
//! never joins a hub table: what it needs from the hub comes through
//! [`HubPort`](crate::HubPort), so a query here that names one fails in
//! the platform's tests, whose database is its own.
//!
//! Each feature brings its own queries as it moves in (billing's in
//! [`billing`], M139; Cloud's in [`cloud`], M137; chat's in [`chat`], M138;
//! projects, team members and grants in [`projects`]; the forge mirror in
//! [`forge`], M162; native issues in [`issues`], M194), and flips their
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
pub mod content;
pub mod forge;
pub mod inbox;
pub mod issues;
pub mod jobs;
mod legacy;
pub mod migrate;
pub mod mirror;
pub mod objects;
pub mod pg;
pub mod projects;
pub mod sql;

pub use pg::Listening;
pub use sql::{Conn, Sql};

/// Where the platform's database is (M143): `--platform-database-url` or
/// `ARUGULA_PLATFORM_DATABASE_URL`, a `postgres://` URL or a SQLite file.
/// Without it, the file the caller names (in process, the hub's `--db`).
/// Control flattens it into its flags, and so does `arugula-platform`
/// (M140).
#[derive(clap::Args, Debug, Clone, Default)]
#[group(id = "platform_db")]
pub struct Args {
    /// The platform's database: `postgres://user:password@host/db` for
    /// Postgres (hosted control, from M145), or a SQLite file's path.
    #[arg(long = "platform-database-url", env = "ARUGULA_PLATFORM_DATABASE_URL", hide_env_values = true)]
    pub database_url: Option<String>,
}

impl Args {
    /// The database it names, or else the SQLite file at `file`.
    pub fn open(&self, file: &Path) -> anyhow::Result<Db> {
        match self.database_url.as_deref().filter(|u| !u.is_empty()) {
            Some(url) => Db::open_url(url),
            None => Db::open(file),
        }
    }

    /// Whether it names Postgres.
    pub fn postgres(&self) -> bool {
        self.database_url.as_deref().is_some_and(is_postgres)
    }
}

fn is_postgres(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

pub struct Db {
    backend: Backend,
}

enum Backend {
    Sqlite(Mutex<Connection>),
    Postgres(Box<pg::Pool>),
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
        Ok(Self { backend: Backend::Sqlite(Mutex::new(conn)) })
    }

    /// Postgres at `url` (`postgres://` or `postgresql://`), else the
    /// SQLite file at that path.
    pub fn open_url(url: &str) -> anyhow::Result<Self> {
        if is_postgres(url) { Self::postgres(url, None) } else { Self::open(Path::new(url)) }
    }

    /// Postgres at `url`, brought up to the latest migration (M143). It's
    /// never adopted: Postgres starts empty, and M145 copies rows in.
    /// `schema`: its own schema rather than `public` (tests).
    fn postgres(url: &str, schema: Option<String>) -> anyhow::Result<Self> {
        let db = Self { backend: Backend::Postgres(Box::new(pg::Pool::connect(url, schema)?)) };
        let ran = migrate::run_postgres(&mut db.c()).context("the platform's schema")?;
        if !ran.is_empty() {
            tracing::info!(?ran, "the platform's schema, on Postgres");
        }
        Ok(db)
    }

    /// Postgres at `url` as it is, with no migration run: for the window's
    /// tools that only read (`checksums`, `secrets-check`, `migrations`;
    /// M145's nightly dump runs them on the live cluster), which check its
    /// migrations themselves.
    pub(crate) fn postgres_as_is(url: &str) -> anyhow::Result<Self> {
        Ok(Self { backend: Backend::Postgres(Box::new(pg::Pool::connect(url, None)?)) })
    }

    /// A database of its own in memory, for tests: nothing of the hub's is
    /// in it.
    pub fn memory() -> Self {
        let mut conn = sqlite::memory();
        migrate::run(&mut conn).expect("the platform's schema");
        Self { backend: Backend::Sqlite(Mutex::new(conn)) }
    }

    /// A test's database: in memory, or, when `ARUGULA_TEST_POSTGRES_URL`
    /// names a Postgres, a schema of its own there, dropped with it. The
    /// platform's suite runs on both (`just test-platform-postgres`).
    #[cfg(any(test, feature = "contract"))]
    pub fn test() -> Self {
        match std::env::var("ARUGULA_TEST_POSTGRES_URL") {
            Ok(url) if !url.is_empty() => Self::test_on(&url),
            _ => Self::memory(),
        }
    }

    /// A schema of its own on the Postgres at `url`, dropped with it (a
    /// bench reaching Postgres through a proxy of its own).
    #[cfg(any(test, feature = "contract"))]
    pub fn test_on(url: &str) -> Self {
        let schema = format!("t_{}", hex::encode(arugula_e2e::random::<8>()));
        Self::postgres(url, Some(schema)).expect("a test schema on the test Postgres")
    }

    pub fn dialect(&self) -> migrate::Dialect {
        match self.backend {
            Backend::Sqlite(_) => migrate::Dialect::Sqlite,
            Backend::Postgres(_) => migrate::Dialect::Postgres,
        }
    }

    /// The connection held for listening to the platform's other machines
    /// and for the leader's lock (M144, [`pg::Pool::listen`]): `None` on
    /// SQLite, which is one machine, always its own leader.
    pub fn listen(&self) -> Option<Listening> {
        match &self.backend {
            Backend::Sqlite(_) => None,
            Backend::Postgres(p) => Some(p.listen()),
        }
    }

    /// Another handle on this test's database (M144: two machines on one
    /// database): the same schema on Postgres, which the first handle
    /// drops; `None` on SQLite in memory, which only one connection sees.
    #[cfg(any(test, feature = "contract"))]
    pub fn second(&self) -> Option<Self> {
        match &self.backend {
            Backend::Sqlite(_) => None,
            Backend::Postgres(p) => Some(Self { backend: Backend::Postgres(Box::new(p.second().ok()?)) }),
        }
    }

    /// A URL that reaches this test's Postgres schema
    /// (`ARUGULA_TEST_POSTGRES_URL` with its `search_path`), for what opens
    /// Postgres by URL (the window's tools); `None` on SQLite.
    #[cfg(any(test, feature = "contract"))]
    pub fn test_url(&self) -> Option<String> {
        let Backend::Postgres(p) = &self.backend else { return None };
        let url = std::env::var("ARUGULA_TEST_POSTGRES_URL").ok()?;
        let sep = if url.contains('?') { '&' } else { '?' };
        Some(format!("{url}{sep}options=-c%20search_path%3D{}", p.schema()?))
    }

    /// A connection, held until it's dropped ([`sql`]).
    pub fn c(&self) -> Conn<'_> {
        crate::timing::waited(|| match &self.backend {
            Backend::Sqlite(c) => Conn(sql::Inner::Sqlite(c.lock().unwrap())),
            Backend::Postgres(p) => Conn(sql::Inner::Postgres(p.lease())),
        })
    }

    /// SQLite's connection itself, for what only SQLite has (the hub's
    /// checks of its file). Panics on Postgres.
    pub fn sqlite(&self) -> MutexGuard<'_, Connection> {
        match &self.backend {
            Backend::Sqlite(c) => c.lock().unwrap(),
            Backend::Postgres(_) => panic!("the platform's database is Postgres"),
        }
    }

    /// Every column of every table here that has `needle` in it, as
    /// `table.column` (tests: nothing of a deleted account is left).
    #[cfg(any(test, feature = "contract"))]
    pub fn mentions(&self, needle: &str) -> Vec<String> {
        let c = self.c();
        let columns: Vec<(String, String)> = match c.dialect() {
            migrate::Dialect::Sqlite => c.query_map(
                "SELECT m.name, p.name FROM sqlite_master m, pragma_table_info(m.name) p
                 WHERE m.type = 'table' AND m.name != 'platform_migrations'
                 ORDER BY 1, p.cid",
                sql::params![],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            migrate::Dialect::Postgres => c.query_map(
                "SELECT table_name::text, column_name::text FROM information_schema.columns
                 WHERE table_schema = current_schema() AND table_name != 'platform_migrations'
                 ORDER BY table_name, ordinal_position",
                sql::params![],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
        }
        .unwrap();
        let mut out = Vec::new();
        for (t, col) in columns {
            let n: u64 = c
                .query_row(
                    &format!("SELECT COUNT(*) FROM {t} WHERE CAST({col} AS TEXT) LIKE '%' || ?1 || '%'"),
                    sql::params![needle],
                    |r| r.get(0),
                )
                .unwrap();
            if n > 0 {
                out.push(format!("{t}.{col}"));
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

fn taken(e: anyhow::Error) -> anyhow::Error {
    let sqlite = matches!(
        e.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(f, _)) if f.code == rusqlite::ErrorCode::ConstraintViolation
    );
    if sqlite || pg::is_constraint(&e) { Taken.into() } else { e }
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
        let n: i64 = db.sqlite().query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0)).unwrap();
        assert!(n >= 1, "the same file");
        let mem = super::Db::memory();
        let n: i64 = mem
            .sqlite()
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'accounts'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "a test's database has nothing of the hub's");
        drop((hub, db));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }
}
