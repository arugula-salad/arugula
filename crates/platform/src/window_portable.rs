//! The second window's copy (M145, written in M144): the platform's tables
//! between a SQLite file and Postgres, either way, proven the same with a
//! row count and a **portable checksum** per table, one both databases
//! give for the same rows:
//!
//! ```text
//! arugula-platform copy --from /data/platform.db --to postgres://…   # the window
//! arugula-platform copy --from postgres://… --to /data/back.db       # rolling back after it
//! arugula-platform checksums --db postgres://…                       # portable, read-only
//! arugula-platform checksums --db /data/platform.db --portable       # the same numbers
//! ```
//!
//! **The portable checksum.** A row's digest is SHA-256 over its columns in
//! name order (this build's columns: Postgres's generated `search` isn't
//! one), each value as one of null, integer, real, text or bytes. A
//! boolean is the integer 0 or 1, as SQLite keeps it. SQLite's rowid is in
//! it only for the tables Postgres keeps it for, as a column named `rowid`
//! (`cloud_notifications`, `cloud_workspace_specs`: queries order by it).
//! A table's checksum is SHA-256 over its rows' digests sorted. It isn't
//! the SQLite-to-SQLite copy's checksum ([`super::sum`], which has every
//! rowid and SQLite's own types), so the first window's numbers stay as
//! they were.
//!
//! **The copy** reads one snapshot of the source (a read transaction;
//! `REPEATABLE READ` on Postgres), writes the target in one transaction,
//! and commits only if every table's count and portable checksum match.
//! It refuses what [`super::copy`] refuses: a target that holds rows
//! unless `--replace`, a source that isn't this build's tables. And what
//! Postgres can't hold the same: a value that would change type (a real
//! in an integer column), or a row whose foreign key points at nothing.
//! On Postgres the identities (`chat_messages.id`, `chat_events.seq`, the
//! two `rowid`s) are moved past the copied rows; on SQLite `chat_fts` is
//! rebuilt and checked.

use std::{collections::BTreeSet, fmt::Write as _, path::PathBuf, str::FromStr};

use anyhow::{Context as _, bail};
use rusqlite::Connection;
use sha2::{Digest as _, Sha256};

use crate::db::{
    Db,
    migrate::Dialect,
    sql::{Param, Sql, Value, params},
};

/// The tables whose SQLite rowid Postgres keeps, as a column named `rowid`
/// (M143).
pub const ROWID_TABLES: [&str; 2] = ["cloud_notifications", "cloud_workspace_specs"];

/// Rows written per `INSERT`.
const CHUNK: usize = 200;

/// Where a window's tool reads or writes: a SQLite file, or Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    File(PathBuf),
    Postgres(String),
}

impl FromStr for Place {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s.starts_with("postgres://") || s.starts_with("postgresql://") {
            Place::Postgres(s.to_owned())
        } else {
            Place::File(PathBuf::from(s))
        })
    }
}

impl std::fmt::Display for Place {
    /// As printed: a file's path, or Postgres's URL without its password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Place::File(p) => write!(f, "{}", p.display()),
            Place::Postgres(u) => match url::Url::parse(u) {
                Ok(mut u) => {
                    if u.password().is_some() {
                        let _ = u.set_password(Some("…"));
                    }
                    write!(f, "{u}")
                }
                Err(_) => f.write_str("postgres://(a URL that doesn't parse)"),
            },
        }
    }
}

/// What a portable copy did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub tables: Vec<super::Table>,
    /// Rows the target held in these tables before `--replace` emptied it.
    pub replaced: u64,
    /// `chat_fts`'s messages after its rebuild, when the target is SQLite.
    pub fts_rows: Option<u64>,
}

impl Report {
    pub fn rows(&self) -> u64 {
        self.tables.iter().map(|t| t.to_rows).sum()
    }

    pub fn render(&self, from: &Place, to: &Place) -> String {
        let mut out = format!("copied the platform's tables (portable checksums)\n  from {from}\n  to   {to}\n\n");
        let _ = writeln!(
            out,
            "{:<24} {:>9} {:>9}  {:<16}  {:<16}",
            "table", "rows from", "rows to", "sha256 from", "sha256 to"
        );
        for t in &self.tables {
            let _ = writeln!(
                out,
                "{:<24} {:>9} {:>9}  {:<16}  {:<16}  {}",
                t.name,
                t.from_rows,
                t.to_rows,
                &t.from_sum[..16],
                &t.to_sum[..16],
                if t.matches() { "ok" } else { "DIFFERS" }
            );
        }
        match self.fts_rows {
            Some(n) => {
                let _ = writeln!(
                    out,
                    "{:<24} {:>9} {n:>9}  rebuilt from chat_messages, integrity-check ok",
                    "chat_fts", ""
                );
            }
            None => {
                let _ = writeln!(out, "{:<24} {:>9} {:>9}  Postgres's generated column", "search", "", "");
            }
        }
        if self.replaced > 0 {
            let _ = writeln!(out, "\n--replace: the target's {} earlier rows were deleted first", self.replaced);
        }
        let _ = writeln!(out, "\n{} tables, {} rows: every count and checksum matches", self.tables.len(), self.rows());
        out
    }
}

/// Each table's columns as the portable checksum reads them: this build's
/// (its SQLite migrations'), in name order, with `rowid` for
/// [`ROWID_TABLES`].
fn shapes() -> Vec<(String, Vec<String>)> {
    let db = Db::memory();
    let c = db.sqlite();
    super::tables()
        .into_iter()
        .map(|t| {
            let mut cols: Vec<String> =
                super::columns(&c, "main", &t).expect("this build's columns").into_iter().collect();
            if ROWID_TABLES.contains(&t.as_str()) {
                cols.push("rowid".into());
                cols.sort();
            }
            (t, cols)
        })
        .collect()
}

/// A column as a `SELECT` names it: SQLite's rowid isn't quoted, so it's
/// the rowid on SQLite and the column on Postgres.
fn select(cols: &[String]) -> String {
    cols.iter()
        .map(|c| if c == "rowid" { "rowid".to_owned() } else { format!("\"{c}\"") })
        .collect::<Vec<_>>()
        .join(", ")
}

fn rows(c: &impl Sql, table: &str, cols: &[String]) -> anyhow::Result<Vec<Vec<Value>>> {
    c.query_map(&format!("SELECT {} FROM \"{table}\"", select(cols)), params![], |r| {
        (0..cols.len()).map(|i| r.get::<Value>(i)).collect()
    })
    .with_context(|| format!("reading {table}"))
}

/// A table's row count and portable checksum (the module's docs say how).
pub fn sum(cols: &[String], rows: &[Vec<Value>]) -> (u64, String) {
    let mut digests: Vec<[u8; 32]> = rows
        .iter()
        .map(|row| {
            let mut h = Sha256::new();
            for (name, v) in cols.iter().zip(row) {
                h.update(name.as_bytes());
                h.update([0]);
                let (tag, bytes): (u8, Vec<u8>) = match v {
                    Value::Null => (0, Vec::new()),
                    Value::Int(n) => (1, n.to_le_bytes().to_vec()),
                    Value::Bool(b) => (1, i64::from(*b).to_le_bytes().to_vec()),
                    Value::Real(f) => (2, f.to_bits().to_le_bytes().to_vec()),
                    Value::Text(t) => (3, t.as_bytes().to_vec()),
                    Value::Blob(b) => (4, b.clone()),
                };
                h.update([tag]);
                h.update((bytes.len() as u64).to_le_bytes());
                h.update(&bytes);
            }
            h.finalize().into()
        })
        .collect();
    digests.sort_unstable();
    let mut h = Sha256::new();
    for d in &digests {
        h.update(d);
    }
    (digests.len() as u64, hex::encode(h.finalize()))
}

/// Each of the platform's tables at `at`, read-only (one snapshot): its
/// row count and portable checksum. On a file, a table it doesn't have is
/// left out.
pub fn checksums(at: &Place) -> anyhow::Result<Vec<(String, u64, String)>> {
    let mut out = Vec::new();
    let mut add = |t: String, cols: &[String], rows: &[Vec<Value>]| {
        let (n, s) = sum(cols, rows);
        out.push((t, n, s));
    };
    match at {
        Place::File(path) => {
            let c = super::read_only(path)?;
            c.execute_batch("BEGIN")?;
            for (t, cols) in shapes() {
                if !super::columns(&c, "main", &t)?.is_empty() {
                    let r = rows(&c, &t, &cols)?;
                    add(t, &cols, &r);
                }
            }
            c.execute_batch("COMMIT")?;
        }
        Place::Postgres(url) => {
            let db = Db::open_url(url)?;
            let c = db.c();
            c.execute_batch("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY")?;
            for (t, cols) in shapes() {
                let r = rows(&c, &t, &cols)?;
                add(t, &cols, &r);
            }
            c.execute_batch("COMMIT")?;
        }
    }
    Ok(out)
}

/// The source's rows, read in one snapshot, with their portable sums.
struct Read {
    tables: Vec<(String, Vec<String>, Vec<Vec<Value>>)>,
}

fn read_source(from: &Place) -> anyhow::Result<Read> {
    let shapes = shapes();
    let mut tables = Vec::new();
    match from {
        Place::File(path) => {
            if !path.exists() {
                bail!("{} isn't there", path.display());
            }
            let c = super::read_only(path)?;
            c.execute_batch("BEGIN")?;
            let mut wrong = Vec::new();
            let mut dangling = Vec::new();
            for (t, cols) in &shapes {
                let have = super::columns(&c, "main", t)?;
                let want: BTreeSet<String> = cols.iter().filter(|c| *c != "rowid").cloned().collect();
                if have.is_empty() {
                    wrong.push(format!("{t}: not in the source"));
                } else if have != want {
                    wrong.push(format!("{t}: columns differ (has {have:?}, this build has {want:?})"));
                } else if super::dangling(&c, "main", t)? > 0 {
                    dangling.push(t.clone());
                }
            }
            if !wrong.is_empty() {
                bail!(
                    "{} doesn't hold this build's platform tables (start this build on it once to migrate it):\n  {}",
                    path.display(),
                    wrong.join("\n  ")
                );
            }
            if !dangling.is_empty() {
                bail!(
                    "{}: rows in {} refer to rows that aren't there, which Postgres's foreign keys refuse",
                    path.display(),
                    dangling.join(", ")
                );
            }
            for (t, cols) in shapes {
                let r = rows(&c, &t, &cols)?;
                tables.push((t, cols, r));
            }
            c.execute_batch("COMMIT")?;
        }
        Place::Postgres(url) => {
            // Opening it brings it to this build's migrations, as starting
            // the platform on it would.
            let db = Db::open_url(url)?;
            let c = db.c();
            c.execute_batch("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY")?;
            for (t, cols) in shapes {
                let r = rows(&c, &t, &cols)?;
                tables.push((t, cols, r));
            }
            c.execute_batch("COMMIT")?;
        }
    }
    Ok(Read { tables })
}

/// Write `read` into `tx` (emptied first with `replace`), and check it.
fn write(tx: &impl Sql, read: &Read, replace: bool, to: &Place) -> anyhow::Result<(Vec<super::Table>, u64)> {
    let mut held = Vec::new();
    let mut replaced = 0;
    for (t, _, _) in &read.tables {
        let n: u64 = tx.query_row(&format!("SELECT count(*) FROM \"{t}\""), params![], |r| r.get(0))?;
        if n > 0 {
            held.push(format!("{t} ({n})"));
            replaced += n;
        }
    }
    if !held.is_empty() && !replace {
        bail!(
            "{to} already holds the platform's rows: {}. Copy into an empty one, or pass --replace to delete them first",
            held.join(", ")
        );
    }
    for (t, _, _) in read.tables.iter().rev() {
        tx.execute(&format!("DELETE FROM \"{t}\""), params![])?;
    }
    for (t, cols, rows) in &read.tables {
        let names = select(cols);
        for chunk in rows.chunks(CHUNK) {
            let mut n = 0;
            let values: Vec<String> = chunk
                .iter()
                .map(|_| {
                    let marks: Vec<String> = cols
                        .iter()
                        .map(|_| {
                            n += 1;
                            format!("?{n}")
                        })
                        .collect();
                    format!("({})", marks.join(", "))
                })
                .collect();
            let p: Vec<&dyn Param> = chunk.iter().flatten().map(|v| v as &dyn Param).collect();
            tx.execute(&format!("INSERT INTO \"{t}\" ({names}) VALUES {}", values.join(", ")), &p).with_context(
                || format!("writing {t} (a value Postgres can't keep as it is, such as a real in an integer column?)"),
            )?;
        }
    }
    let mut out = Vec::new();
    for (t, cols, from_rows) in &read.tables {
        let (from_n, from_sum) = sum(cols, from_rows);
        let (to_n, to_sum) = sum(cols, &rows(tx, t, cols)?);
        out.push(super::Table { name: t.clone(), from_rows: from_n, to_rows: to_n, from_sum, to_sum });
    }
    Ok((out, replaced))
}

/// Copy the platform's tables from `from` to `to`, at least one of them
/// Postgres, and prove it with portable checksums (the module's docs).
pub fn copy(from: &Place, to: &Place, replace: bool) -> anyhow::Result<Report> {
    if from == to {
        bail!("--from and --to are the same");
    }
    let read = read_source(from)?;
    let report = match to {
        Place::Postgres(url) => {
            let db = Db::open_url(url).context("opening the target")?;
            let mut c = db.c();
            let tx = c.transaction()?;
            tx.lock("window")?;
            let (tables, replaced) = write(&tx, &read, replace, to)?;
            let report = Report { tables, replaced, fts_rows: None };
            check(&report, from, to)?;
            // Identities go on after the copied rows.
            let identities: Vec<(String, String)> = tx.query_map(
                "SELECT table_name::text, column_name::text FROM information_schema.columns
                 WHERE table_schema = current_schema() AND is_identity = 'YES' ORDER BY 1",
                params![],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            for (t, col) in identities {
                tx.execute(
                    &format!(
                        "SELECT setval(pg_get_serial_sequence('\"{t}\"', '{col}'),
                             coalesce((SELECT max(\"{col}\") FROM \"{t}\"), 1),
                             (SELECT max(\"{col}\") FROM \"{t}\") IS NOT NULL)"
                    ),
                    params![],
                )?;
            }
            tx.commit()?;
            report
        }
        Place::File(path) => {
            let db = Db::open(path).with_context(|| format!("opening {}", path.display()))?;
            let mut c = db.c();
            let tx = c.immediate()?;
            let (tables, replaced) = write(&tx, &read, replace, to)?;
            tx.execute_batch(&format!("INSERT INTO {0} ({0}) VALUES ('rebuild')", super::FTS))?;
            tx.execute_batch(&format!("INSERT INTO {0} ({0}, rank) VALUES ('integrity-check', 1)", super::FTS))
                .context("chat_fts's integrity-check after its rebuild")?;
            let fts: u64 =
                tx.query_row(&format!("SELECT count(*) FROM {}_docsize", super::FTS), params![], |r| r.get(0))?;
            let report = Report { tables, replaced, fts_rows: Some(fts) };
            check(&report, from, to)?;
            let messages = report.tables.iter().find(|t| t.name == "chat_messages").map_or(0, |t| t.to_rows);
            if fts != messages {
                bail!("chat_fts indexes {fts} messages of {messages}");
            }
            tx.commit()?;
            drop(c);
            let conn: std::sync::MutexGuard<'_, Connection> = db.sqlite();
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            report
        }
    };
    Ok(report)
}

fn check(report: &Report, from: &Place, to: &Place) -> anyhow::Result<()> {
    if report.tables.iter().any(|t| !t.matches()) {
        bail!("the copy differs from its source, so nothing was written:\n{}", report.render(from, to));
    }
    Ok(())
}

/// Cloud secrets at `at` that open with `key`, and the `(account, name)`
/// of each that doesn't, as [`super::secrets_open`] does for a file.
pub fn secrets_open(at: &Place, key: &str) -> anyhow::Result<(u64, Vec<(String, String)>)> {
    match at {
        Place::File(path) => super::secrets_open(path, key),
        Place::Postgres(url) => {
            let cipher = crate::cloud::secrets::SecretCipher::new(key)?;
            let db = Db::open_url(url)?;
            anyhow::ensure!(db.dialect() == Dialect::Postgres, "not Postgres");
            let rows: Vec<(String, String, Vec<u8>)> = db.c().query_map(
                "SELECT account, name, sealed FROM cloud_secrets ORDER BY account, name",
                params![],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let (mut opened, mut failed) = (0, Vec::new());
            for (account, name, sealed) in rows {
                match cipher.open(&account, &name, &sealed) {
                    Ok(_) => opened += 1,
                    Err(_) => failed.push((account, name)),
                }
            }
            Ok((opened, failed))
        }
    }
}
