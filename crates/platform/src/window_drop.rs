//! After the first window (#1158) the hub's `control.db` kept its copies of
//! the platform's tables, read-only, as the rollback. `drop-copies` (#1196)
//! drops them, on the hub, once the platform has run on its own long
//! enough:
//!
//! ```text
//! arugula-control platform drop-copies --db /data/control.db --expect window-hub.txt --yes
//! ```
//!
//! **What it drops**, in one transaction: every table `--expect` names (the
//! window's `copy` or `checksums` output); every table this build's
//! platform makes ([`super::tables`]) that's in the file, which must be
//! empty if `--expect` doesn't name it; the platform's FTS5 indexes
//! (`chat_fts`), with their shadow tables; and `platform_migrations`.
//! Triggers and indexes go with their tables. Since the window, tables
//! the platform has stopped making (Cloud's `cloud_*`, M190) are still in
//! the hub's file: `--expect` names them, so they go too. Nothing else is
//! touched: every other table is the hub's.
//!
//! **What it refuses**, writing nothing: a table `--expect` names that
//! isn't there, or whose rows or checksum aren't the file's (something
//! wrote the copies, so they aren't the window's any more); a platform
//! table `--expect` doesn't name that holds rows; anything the hub keeps
//! (a foreign key, a view or a trigger on its own tables) that names a
//! table it would drop; a table `--expect` names that no platform
//! migration ever made (`migrate::ever_made`), so a hub table is never
//! one. The checksum is [`super::sum`]'s, over whatever
//! columns the table has, so it works for tables this build no longer
//! models; the window's `copy` prints its first 16 hex digits, `checksums`
//! all 64, and either is matched as a prefix. Without `--yes`, it checks
//! everything and says what it would drop. `run` also refuses when the
//! running hub runs the platform in process, whose tables those are.

use std::{collections::BTreeSet, fmt::Write as _, path::Path};

use anyhow::{Context as _, bail};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use super::sum;

/// One table's line in the window's output: `copy`'s source side, or
/// `checksums`'.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub name: String,
    pub rows: u64,
    /// Hex, 16 to 64 digits: a prefix of the table's checksum.
    pub sum: String,
}

/// The tables in the window's output: `copy`'s lines (`name rows rows sum
/// sum ok`) or `checksums`' (`name rows sum`), checked against the
/// output's own `N tables, M rows` line when it has one.
pub fn parse_expected(text: &str) -> anyhow::Result<Vec<Expected>> {
    let hex =
        |s: &str| s.len() >= 16 && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let name_ok = |s: &str| {
        s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && s.bytes().next().is_some_and(|b| !b.is_ascii_digit())
    };
    let mut out: Vec<Expected> = Vec::new();
    let mut total = None;
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| w.get(i).and_then(|s| s.parse::<u64>().ok());
        let e = match w.as_slice() {
            [name, _, _, from, to, verdict] if n(1).is_some() && n(2).is_some() && hex(from) && hex(to) => {
                if *verdict != "ok" || n(1) != n(2) || from != to {
                    bail!("the window's copy of {name} didn't match its source ({line:?}): that output proves nothing");
                }
                Expected { name: (*name).into(), rows: n(1).unwrap_or_default(), sum: (*from).into() }
            }
            [name, _, s] if n(1).is_some() && hex(s) => {
                Expected { name: (*name).into(), rows: n(1).unwrap_or_default(), sum: (*s).into() }
            }
            [_, "tables,", _, rows, ..] if n(0).is_some() && n(2).is_some() && rows.starts_with("rows") => {
                total = n(0).zip(n(2));
                continue;
            }
            _ => continue,
        };
        if !name_ok(&e.name) {
            bail!("{:?} isn't a table's name", e.name);
        }
        if out.iter().any(|o| o.name == e.name) {
            bail!("{} is in it twice", e.name);
        }
        out.push(e);
    }
    if out.is_empty() {
        bail!("no table's line in it: it should be the window's `copy` or `checksums` output");
    }
    if let Some((tables, rows)) = total {
        let (have, sum) = (out.len() as u64, out.iter().map(|e| e.rows).sum::<u64>());
        if (have, sum) != (tables, rows) {
            bail!("it says {tables} tables, {rows} rows, but lists {have} tables, {sum} rows: is it cut short?");
        }
    }
    Ok(out)
}

/// Why a table was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// `--expect` names it, and its rows and checksum are still those.
    Expected,
    /// This build's platform makes it, `--expect` doesn't name it, and it
    /// was empty.
    Unlisted,
    /// One of the platform's FTS5 indexes, with its shadow tables.
    Index,
    /// The platform's migration runner's own.
    Migrations,
}

/// What `drop-copies` dropped, or would drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// Each table, its rows and why, in the order dropped.
    pub tables: Vec<(String, u64, Why)>,
    /// The tables left: the hub's own.
    pub kept: usize,
    /// Whether they're gone, or this was a check (no `--yes`).
    pub done: bool,
}

impl Dropped {
    pub fn render(&self, db: &Path) -> String {
        let verb = if self.done { "dropped" } else { "would drop" };
        let mut out = format!("{verb} the platform's copies in {}\n\n", db.display());
        let _ = writeln!(out, "{:<24} {:>9}  why", "table", "rows");
        for (t, n, why) in &self.tables {
            let why = match why {
                Why::Expected => "the window's copy, unchanged",
                Why::Unlisted => "this build's platform table, empty",
                Why::Index => "the platform's index, and its shadow tables",
                Why::Migrations => "the platform's migrations",
            };
            let _ = writeln!(out, "{t:<24} {n:>9}  {why}");
        }
        let rows: u64 = self.tables.iter().map(|t| t.1).sum();
        let _ = writeln!(
            out,
            "\n{} tables, {rows} rows, in one transaction; the hub's {} tables {}",
            self.tables.len(),
            self.kept,
            if self.done { "are as they were" } else { "would stay as they are" }
        );
        if !self.done {
            out.push_str("nothing was written: pass --yes to drop them\n");
        }
        out
    }
}

/// An FTS5 table's shadow tables' suffixes.
const SHADOWS: [&str; 5] = ["_data", "_idx", "_content", "_docsize", "_config"];

/// The FTS5 tables this build's platform makes, and `chat_fts`, which it
/// always has.
fn platform_indexes() -> BTreeSet<String> {
    let db = crate::db::Db::memory();
    let c = db.sqlite();
    let mut q = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND upper(sql) LIKE 'CREATE VIRTUAL TABLE%'")
        .expect("sqlite_master");
    let mut out: BTreeSet<String> = q.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    out.insert(super::FTS.into());
    out
}

/// Whether `sql` names `table` as a word (SQL's names ignore case).
fn names(sql: &str, table: &str) -> bool {
    let (sql, table) = (sql.to_ascii_lowercase(), table.to_ascii_lowercase());
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    sql.match_indices(&table)
        .any(|(at, _)| !word(sql[..at].chars().next_back()) && !word(sql[at + table.len()..].chars().next()))
}

/// Drop the platform's copies from the hub's file at `path` (the module's
/// docs say which, and what's refused), in one transaction; with `commit`
/// false, check everything and drop nothing.
pub fn drop_copies(path: &Path, expected: &[Expected], commit: bool) -> anyhow::Result<Dropped> {
    if !path.exists() {
        bail!("{} isn't there", path.display());
    }
    // Only what a platform migration made: never one of the hub's.
    let ever = crate::db::migrate::ever_made()?;
    let strangers: Vec<&str> = expected.iter().map(|e| e.name.as_str()).filter(|n| !ever.contains(*n)).collect();
    if !strangers.is_empty() {
        bail!("no platform migration ever made {}: refusing to drop them", strangers.join(", "));
    }
    // Not the platform's `Db`, which would migrate the copies: a plain
    // connection, without foreign keys, so a table's drop deletes no row
    // anywhere else first.
    let mut c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(30))?;
    c.pragma_update(None, "foreign_keys", "OFF")?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let all: Vec<(String, String, String, String)> = tx
        .prepare("SELECT type, name, tbl_name, coalesce(sql, '') FROM sqlite_master")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let tables: BTreeSet<&str> = all.iter().filter(|o| o.0 == "table").map(|o| o.1.as_str()).collect();
    let migrations = tables.contains(crate::db::migrate::TABLE);

    // Gone already, or not all there.
    let missing: Vec<&str> = expected.iter().map(|e| e.name.as_str()).filter(|n| !tables.contains(n)).collect();
    if missing.len() == expected.len() && !migrations {
        bail!(
            "none of the window's tables is in {}, nor platform_migrations: the copies are gone already, so \
             nothing was done",
            path.display()
        );
    }
    if !missing.is_empty() {
        bail!(
            "{} lacks the window's {}: these aren't the window's copies, so nothing was dropped",
            path.display(),
            missing.join(", ")
        );
    }

    // Each still the window's.
    let mut drop: Vec<(String, u64, Why)> = Vec::new();
    let mut differ = Vec::new();
    for e in expected {
        let (rows, sum) = sum(&tx, "main", &e.name)?;
        if rows != e.rows || !sum.starts_with(&e.sum) {
            differ.push(format!("{:<24} {:>9} {:>9}  {:<16}  {:<16}", e.name, e.rows, rows, e.sum, &sum[..16]));
        }
        drop.push((e.name.clone(), rows, Why::Expected));
    }
    if !differ.is_empty() {
        bail!(
            "something wrote the platform's copies since the window, so nothing was dropped:\n{:<24} {:>9} {:>9}  \
             {:<16}  {:<16}\n{}",
            "table",
            "rows then",
            "rows now",
            "sha256 then",
            "sha256 now",
            differ.join("\n")
        );
    }

    // This build's platform tables the window didn't have: only empty.
    let mut held = Vec::new();
    for t in super::tables() {
        if !tables.contains(t.as_str()) || drop.iter().any(|d| d.0 == t) {
            continue;
        }
        let n: u64 = tx.query_row(&format!("SELECT count(*) FROM main.\"{t}\""), [], |r| r.get(0))?;
        if n > 0 {
            held.push(format!("{t} ({n})"));
        }
        drop.push((t, n, Why::Unlisted));
    }
    if !held.is_empty() {
        bail!(
            "{} holds rows in platform tables the window didn't copy: {}. Nothing was dropped",
            path.display(),
            held.join(", ")
        );
    }

    // The indexes, then the migrations' table.
    let mut gone: BTreeSet<String> = drop.iter().map(|d| d.0.clone()).collect();
    let mut indexes: Vec<String> = Vec::new();
    for f in platform_indexes() {
        let Some(o) = all.iter().find(|o| o.0 == "table" && o.1 == f) else { continue };
        if !o.3.to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE") {
            bail!("{f} in {} isn't an FTS5 index: nothing was dropped", path.display());
        }
        let n: u64 = match tables.contains(format!("{f}_docsize").as_str()) {
            true => tx.query_row(&format!("SELECT count(*) FROM main.\"{f}_docsize\""), [], |r| r.get(0))?,
            false => 0,
        };
        for s in SHADOWS {
            gone.insert(format!("{f}{s}"));
        }
        gone.insert(f.clone());
        indexes.push(f.clone());
        drop.push((f, n, Why::Index));
    }
    if migrations {
        let t = crate::db::migrate::TABLE;
        let n: u64 = tx.query_row(&format!("SELECT count(*) FROM main.\"{t}\""), [], |r| r.get(0))?;
        gone.insert(t.into());
        drop.push((t.into(), n, Why::Migrations));
    }

    // Nothing the hub keeps names them: its foreign keys, its views, its
    // tables' triggers.
    let mut named = Vec::new();
    for (ty, name, on, sql) in &all {
        if gone.contains(on) || gone.contains(name) || name.starts_with("sqlite_") {
            continue;
        }
        if ty == "table" {
            let mut q = tx.prepare("SELECT DISTINCT \"table\" FROM pragma_foreign_key_list(?1)")?;
            let to: Vec<String> = q.query_map([name], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for t in to.iter().filter(|t| gone.contains(*t)) {
                named.push(format!("table {name} has a foreign key to {t}"));
            }
        } else if ty == "view" || ty == "trigger" {
            for t in gone.iter().filter(|t| names(sql, t)) {
                named.push(format!("{ty} {name} names {t}"));
            }
        }
    }
    if !named.is_empty() {
        bail!("the hub's own schema names tables this would drop, so nothing was dropped:\n  {}", named.join("\n  "));
    }

    let kept = tables.iter().filter(|t| !gone.contains(**t) && !t.starts_with("sqlite_")).count();
    if !commit {
        return Ok(Dropped { tables: drop, kept, done: false });
    }
    // The indexes first: an FTS5 table's drop takes its shadow tables.
    for f in &indexes {
        tx.execute(&format!("DROP TABLE main.\"{f}\""), [])?;
    }
    for (t, _, why) in &drop {
        if *why != Why::Index {
            tx.execute(&format!("DROP TABLE main.\"{t}\""), [])?;
        }
    }
    // Every table, index and trigger of theirs went with them.
    let left: Vec<String> = tx
        .prepare("SELECT type, name, tbl_name FROM sqlite_master")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, name, on)| gone.contains(name) || gone.contains(on))
        .map(|(ty, name, _)| format!("{ty} {name}"))
        .collect();
    if !left.is_empty() {
        bail!("after the drops, {} is still there: nothing was dropped", left.join(", "));
    }
    tx.commit()?;
    Ok(Dropped { tables: drop, kept, done: true })
}

/// Whether `ARUGULA_PLATFORM`'s value is a platform out of process (an
/// `http(s)://` URL), not the hub's own (`in-process`, the default, or
/// unset).
pub fn out_of_process(platform: &str) -> bool {
    let p = platform.trim();
    p.starts_with("http://") || p.starts_with("https://")
}

#[cfg(test)]
#[path = "window_drop_tests.rs"]
mod tests;
