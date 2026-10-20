//! Opening control's SQLite database. The hub opens it, and in process the
//! platform opens a second connection to the same file beside it (#1153):
//! WAL lets one read while the other writes, and the busy timeout waits out
//! the other's write lock (and Litestream's checkpoints, #174) rather than
//! failing.

use std::{path::Path, time::Duration};

use anyhow::Context as _;
use rusqlite::Connection;

/// How long a write waits for another connection's lock.
pub const BUSY: Duration = Duration::from_secs(5);

/// A connection to the database at `path`, made if it isn't there: WAL,
/// foreign keys on, and [`BUSY`] to wait for a lock.
pub fn open(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.busy_timeout(BUSY)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// A database in memory, for tests. No pragmas: it has one connection, and
/// control's tests have always run without foreign keys.
pub fn memory() -> Connection {
    Connection::open_in_memory().expect("an in-memory database")
}

#[cfg(test)]
mod tests {
    /// Two connections to one file, as the hub and the platform have in
    /// process: what one commits, the other reads, even while it holds a
    /// read open.
    #[test]
    fn two_connections_share_one_file() {
        let path = std::env::temp_dir().join(format!("arugula-common-{}.db", hex::encode(arugula_e2e::random::<8>())));
        let hub = super::open(&path).unwrap();
        let platform = super::open(&path).unwrap();
        let mode: String = platform.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        hub.execute_batch("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1);").unwrap();
        let reading = platform.unchecked_transaction().unwrap();
        let count =
            |c: &rusqlite::Connection| -> i64 { c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap() };
        assert_eq!(count(&reading), 1);
        hub.execute("INSERT INTO t VALUES (2)", []).unwrap();
        assert_eq!(count(&reading), 1, "a read sees the snapshot it started with");
        drop(reading);
        assert_eq!(count(&platform), 2);
        drop((hub, platform));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }
}
