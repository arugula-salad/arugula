//! The platform's schema as numbered migrations (M142), the only way its
//! tables are made or changed. The hub keeps its own schema code.
//!
//! - Each migration is a file, `migrations/<dialect>/NNNN_name.sql` in this
//!   crate, listed below. A version means the same change in every
//!   dialect: SQLite here (in process, self-hosted, tests), and Postgres
//!   (M143), whose set is written where the two must differ (FTS5 against
//!   `tsvector`). [`pending`] is the same for both; only running them is
//!   the dialect's.
//! - `platform_migrations` records each one applied, with its checksum. A
//!   shipped migration is never edited: a changed checksum, or a database
//!   at a version this build doesn't know, stops the platform at start
//!   rather than running on a schema it wasn't built for.
//! - All of a start's migrations run in one write transaction: they all
//!   land, or nothing does.
//!
//! # Adopting a database made before migrations
//!
//! Until M142 each feature made its tables with `CREATE TABLE IF NOT EXISTS`
//! and patched old ones with column probes, inside the hub's file, so
//! production already has the platform's tables and no
//! `platform_migrations`. On such a database [`run`] finds the first version
//! whose schema the tables match exactly ([`Shape`]: columns, `CHECK`s,
//! foreign keys, indexes, triggers and FTS5 tables, compared table by
//! table), records it and the ones before it as applied without running
//! them (`adopted = 1`), and runs the rest. A database with none of the
//! platform's tables is new, and runs them all. One with some of them, or
//! with tables that match no version, is refused, naming each difference,
//! and left as it was: the platform won't guess at a schema.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, bail};
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

/// One numbered change to the platform's schema.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

impl Migration {
    /// What `platform_migrations` records, so an edited migration is
    /// caught: SHA-256 of its text, without carriage returns.
    pub fn checksum(&self) -> String {
        hex::encode(Sha256::digest(self.sql.replace('\r', "").as_bytes()))
    }

    /// Its file's name, `NNNN_name.sql`.
    pub fn file(&self) -> String {
        format!("{:04}_{}.sql", self.version, self.name)
    }
}

/// Which SQL a set of migrations is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

impl Dialect {
    pub fn migrations(self) -> &'static [Migration] {
        match self {
            Dialect::Sqlite => SQLITE,
            Dialect::Postgres => POSTGRES,
        }
    }

    /// Where its files are, under this crate.
    pub fn dir(self) -> &'static str {
        match self {
            Dialect::Sqlite => "migrations/sqlite",
            Dialect::Postgres => "migrations/postgres",
        }
    }
}

/// SQLite's: the baseline is production's schema as v0.53.2 made it.
const SQLITE: &[Migration] = &[
    Migration { version: 1, name: "baseline", sql: include_str!("../../migrations/sqlite/0001_baseline.sql") },
    Migration { version: 2, name: "hub_feed", sql: include_str!("../../migrations/sqlite/0002_hub_feed.sql") },
    Migration {
        version: 3,
        name: "personal_projects",
        sql: include_str!("../../migrations/sqlite/0003_personal_projects.sql"),
    },
];

/// Postgres's, from M143: the same versions and names as [`SQLITE`].
const POSTGRES: &[Migration] = &[];

/// The runner's own table, made before anything else.
pub const TABLE: &str = "platform_migrations";

const MAKE_TABLE: &str = "CREATE TABLE IF NOT EXISTS platform_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at INTEGER NOT NULL,
    adopted INTEGER NOT NULL DEFAULT 0
)";

/// A migration a database records as applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub version: u32,
    pub checksum: String,
}

/// The migrations still to run on a database that has applied `applied`
/// (in version order), or why this build can't run on it.
pub fn pending<'a>(set: &'a [Migration], applied: &[Applied]) -> anyhow::Result<&'a [Migration]> {
    let known = set.last().map_or(0, |m| m.version);
    for (i, a) in applied.iter().enumerate() {
        let Some(m) = set.get(i) else {
            bail!(
                "the platform's database is at migration {} and this build knows only up to {known}: \
                 run a build that has it, or restore the database from before it",
                applied.last().map_or(0, |a| a.version)
            );
        };
        if m.version != a.version {
            bail!("the platform's database records migration {} where this build has {}", a.version, m.version);
        }
        if m.checksum() != a.checksum {
            bail!(
                "migration {} has changed since it ran on this database: a shipped migration is never edited, \
                 add the next number instead",
                m.file()
            );
        }
    }
    Ok(&set[applied.len().min(set.len())..])
}

/// What a start did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The version an existing schema was adopted at; 0 for none.
    pub adopted: u32,
    /// The versions run.
    pub ran: Vec<u32>,
}

/// Brings the SQLite database on `conn` up to the latest migration.
pub fn run(conn: &mut Connection) -> anyhow::Result<Report> {
    run_to(conn, u32::MAX)
}

/// [`run`], stopping after version `to` (tests: a database as an older
/// build left it).
pub(crate) fn run_to(conn: &mut Connection, to: u32) -> anyhow::Result<Report> {
    let set = Dialect::Sqlite.migrations();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(MAKE_TABLE)?;
    let mut report = Report::default();
    if applied(&tx)?.is_empty() {
        let n = adopt(&tx, set)?;
        for m in &set[..n] {
            record(&tx, m, true)?;
            report.adopted = m.version;
        }
    }
    for m in pending(set, &applied(&tx)?)?.iter().take_while(|m| m.version <= to) {
        tx.execute_batch(m.sql).with_context(|| format!("running migration {}", m.file()))?;
        record(&tx, m, false)?;
        report.ran.push(m.version);
    }
    tx.commit()?;
    Ok(report)
}

fn applied(c: &Connection) -> rusqlite::Result<Vec<Applied>> {
    let mut q = c.prepare("SELECT version, checksum FROM platform_migrations ORDER BY version")?;
    q.query_map([], |r| Ok(Applied { version: r.get(0)?, checksum: r.get(1)? }))?.collect()
}

fn record(c: &Connection, m: &Migration, adopted: bool) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO platform_migrations (version, name, checksum, applied_at, adopted) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![m.version, m.name, m.checksum(), arugula_e2e::now_ms(), adopted],
    )?;
    Ok(())
}

/// How many of `set` a database made before migrations already has: 0
/// when it has none of their tables. See the module's docs.
fn adopt(c: &Connection, set: &[Migration]) -> anyhow::Result<usize> {
    let steps = steps(set)?;
    let known: BTreeSet<&String> = steps.iter().flat_map(|s| s.0.keys()).collect();
    let present: Vec<String> = tables(c)?.into_iter().filter(|t| known.contains(t)).collect();
    if present.is_empty() {
        return Ok(0);
    }
    let here = Shape::of(c, &present)?;
    if let Some(i) = steps.iter().position(|s| *s == here) {
        return Ok(i + 1);
    }
    let (i, diff) = steps.iter().map(|s| here.diff(s)).enumerate().min_by_key(|(_, d)| d.len()).expect("a baseline");
    bail!(
        "the platform's tables are here but match no migration, so nothing was changed. \
         Against {} (the closest), they differ:\n  {}\n\
         Bring them to a migration's schema by hand, or restore the database.",
        set[i].file(),
        diff.join("\n  ")
    )
}

/// The schema after each of `set`, built in memory.
fn steps(set: &[Migration]) -> anyhow::Result<Vec<Shape>> {
    let c = Connection::open_in_memory()?;
    let mut out = Vec::new();
    for m in set {
        c.execute_batch(m.sql).with_context(|| format!("building migration {}", m.file()))?;
        out.push(Shape::of(&c, &tables(&c)?)?);
    }
    Ok(out)
}

/// The tables and virtual tables in `main`: not SQLite's own, not an FTS5
/// table's shadow tables, and not the runner's.
pub(crate) fn tables(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut q = c.prepare(
        "SELECT name FROM pragma_table_list WHERE schema = 'main' AND type IN ('table', 'virtual')
             AND name NOT LIKE 'sqlite_%' AND name != 'platform_migrations' ORDER BY name",
    )?;
    q.query_map([], |r| r.get(0))?.collect()
}

/// A schema, normalised so that two databases the same queries run the same
/// on compare equal, however each got there: table by table, each column
/// (by name, whatever its position: a probe's `ADD COLUMN` puts it last),
/// `CHECK`, foreign key, index (a `UNIQUE` or primary key's by its columns)
/// and trigger, and a virtual table's definition, with comments, quoting
/// and spacing ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shape(BTreeMap<String, BTreeMap<String, String>>);

impl Shape {
    /// The shape of `tables` in the database on `c`.
    pub fn of(c: &Connection, tables: &[String]) -> rusqlite::Result<Self> {
        let mut out = BTreeMap::new();
        for t in tables {
            let mut facts = BTreeMap::new();
            let sql: String =
                c.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |r| r.get(0))?;
            let sql = normalise(&sql);
            if sql.to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE") {
                facts.insert("virtual table".into(), sql.splitn(4, ' ').nth(3).unwrap_or_default().to_owned());
            }
            for check in checks(&sql) {
                facts.insert(format!("check {check}"), String::new());
            }
            let mut q = c.prepare(r#"SELECT name, type, "notnull", dflt_value, pk FROM pragma_table_info(?1)"#)?;
            let cols = q.query_map([t], |r| {
                let (name, ty, notnull, default, pk): (String, String, bool, Option<String>, u32) =
                    (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
                let mut d = ty;
                if notnull {
                    d.push_str(" NOT NULL");
                }
                if let Some(v) = default {
                    d.push_str(&format!(" DEFAULT {v}"));
                }
                if pk > 0 {
                    d.push_str(&format!(" PRIMARY KEY #{pk}"));
                }
                Ok((format!("column {name}"), d))
            })?;
            for col in cols {
                let (k, v) = col?;
                facts.insert(k, v);
            }
            let mut q = c.prepare(
                r#"SELECT "from", "table", "to", on_update, on_delete, "match" FROM pragma_foreign_key_list(?1)"#,
            )?;
            let fks = q.query_map([t], |r| {
                let (from, table, to): (String, String, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?);
                let (up, del, m): (String, String, String) = (r.get(3)?, r.get(4)?, r.get(5)?);
                Ok(format!(
                    "foreign key {from} -> {table}({}) on update {up} on delete {del} match {m}",
                    to.unwrap_or_default()
                ))
            })?;
            for fk in fks {
                facts.insert(fk?, String::new());
            }
            let mut q = c.prepare(
                r#"SELECT l.name, l."unique", l.origin, m.sql FROM pragma_index_list(?1) l
                   LEFT JOIN sqlite_master m ON m.type = 'index' AND m.name = l.name"#,
            )?;
            let indexes = q
                .query_map([t], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get(3)?))
                })?
                .collect::<rusqlite::Result<Vec<(String, bool, String, Option<String>)>>>()?;
            for (name, unique, origin, sql) in indexes {
                match sql {
                    Some(sql) => facts.insert(format!("index {name}"), normalise(&sql)),
                    // A `UNIQUE` or primary key's own: its name is made up
                    // from the table's, so it's known by its columns.
                    None => {
                        let mut q = c.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
                        let cols: Vec<String> = q.query_map([&name], |r| r.get(0))?.collect::<Result<_, _>>()?;
                        let kind = if origin == "pk" { "primary key" } else { "unique" };
                        facts.insert(format!("{kind} ({})", cols.join(",")), format!("unique={unique}"))
                    }
                };
            }
            let mut q = c.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1")?;
            let triggers = q.query_map([t], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for trigger in triggers {
                let (name, sql) = trigger?;
                facts.insert(format!("trigger {name}"), normalise(&sql));
            }
            out.insert(t.clone(), facts);
        }
        Ok(Self(out))
    }

    /// How this differs from `want`, a line each.
    pub fn diff(&self, want: &Shape) -> Vec<String> {
        let mut out = Vec::new();
        let names: BTreeSet<&String> = self.0.keys().chain(want.0.keys()).collect();
        for t in names {
            let (have, wanted) = match (self.0.get(t), want.0.get(t)) {
                (Some(h), Some(w)) => (h, w),
                (None, _) => {
                    out.push(format!("table {t} is missing"));
                    continue;
                }
                (_, None) => {
                    out.push(format!("table {t} isn't in it"));
                    continue;
                }
            };
            let facts: BTreeSet<&String> = have.keys().chain(wanted.keys()).collect();
            for f in facts {
                match (have.get(f), wanted.get(f)) {
                    (Some(h), Some(w)) if h != w => out.push(format!("{t}: {f} is `{h}`, not `{w}`")),
                    (None, Some(w)) => out.push(format!("{t}: {f} is missing ({w})").replace(" ()", "")),
                    (Some(h), None) => out.push(format!("{t}: {f} isn't in it ({h})").replace(" ()", "")),
                    _ => {}
                }
            }
        }
        out
    }
}

/// `sql` with its comments, identifier quotes and spacing dropped: one
/// space between words, none before `(`, `)` or `,` or after `(` or `,`,
/// and no `;` at the end. String literals are kept as they are.
fn normalise(sql: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut chars = sql.chars().peekable();
    let end = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                let mut lit = String::from(ch);
                while let Some(s) = chars.next() {
                    lit.push(s);
                    if s == '\'' {
                        match chars.peek() {
                            Some('\'') => lit.push(chars.next().unwrap_or('\'')),
                            _ => break,
                        }
                    }
                }
                word.push_str(&lit);
            }
            '-' if chars.peek() == Some(&'-') => {
                end(&mut word, &mut tokens);
                chars.by_ref().find(|&s| s == '\n');
            }
            '"' | '`' | '[' | ']' => {}
            '(' | ')' | ',' | ';' => {
                end(&mut word, &mut tokens);
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => end(&mut word, &mut tokens),
            c => word.push(c),
        }
    }
    end(&mut word, &mut tokens);
    while tokens.last().is_some_and(|t| t == ";") {
        tokens.pop();
    }
    let mut out = String::new();
    for t in &tokens {
        let glued = out.is_empty() || out.ends_with(['(', ',']) || matches!(t.as_str(), "(" | ")" | "," | ";");
        if !glued {
            out.push(' ');
        }
        out.push_str(t);
    }
    out
}

/// The `CHECK (...)` constraints in a normalised `CREATE TABLE`.
fn checks(sql: &str) -> Vec<String> {
    let upper = sql.to_ascii_uppercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = upper[from..].find("CHECK(").map(|i| i + from) {
        let open = at + "CHECK".len();
        let mut depth = 0;
        let mut end = open;
        for (i, ch) in sql[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(sql[at..end].to_owned());
        from = end.max(open + 1);
    }
    out
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
