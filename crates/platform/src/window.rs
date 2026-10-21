//! The windows' tools (M141, #1158): copy the platform's tables from one
//! SQLite file to another, prove the copy with a row count and a checksum
//! per table, and check that cloud secrets open with the key a process
//! has. Both binaries carry them, because the two Fly apps have separate
//! volumes and the file has to move between them:
//!
//! ```text
//! arugula-control platform copy --from /data/control.db --to /data/window.db   # on the hub
//! arugula-platform copy --from /data/window.db --to /data/platform.db          # on the platform
//! ```
//!
//! with `fly ssh sftp get` and `put` carrying `window.db` in between.
//! `docs/control-ops.md` ("The first window") has the runbook.
//!
//! **What's copied:** every table the platform's migrations make
//! ([`tables`]), which is `tables.toml`'s `process = "platform"` rows (a
//! test in control checks), but `platform_migrations`, which belongs to
//! each file's own migration runner: the target is brought up to this
//! build's migrations when it's opened, then filled. Each row keeps its
//! rowid, which some queries order by. `chat_fts` isn't copied: it's an
//! index over `chat_messages` (external content), rebuilt from the copied
//! rows and checked with FTS5's own `integrity-check`.
//!
//! **How it's proven:** inside the copy's one transaction, each table's
//! rows are counted and hashed on both sides. A row's digest is SHA-256
//! over its rowid and its columns in name order, each value with its
//! SQLite type, so a value that changed type changes it; a table's
//! checksum is SHA-256 over its rows' digests sorted, so neither row order
//! nor column order matters. Anything that differs rolls the whole copy
//! back and nothing is written.
//!
//! **Postgres (M145's window):** `copy` from or to a `postgres://` URL, and
//! `checksums`/`secrets-check` on one, go through [`portable`], whose
//! checksums both databases give alike.
//!
//! **What it refuses:** a target that already holds rows in any of those
//! tables, unless `--replace` (which empties them first, in the same
//! transaction); a source that isn't migrated to this build's tables; the
//! same file on both sides. The source is attached read-only, so it can be
//! the live hub's database: a copy reads one consistent snapshot of it.

use std::{collections::BTreeSet, fmt::Write as _, path::Path};

use anyhow::{Context as _, bail};
use clap::Parser;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, types::ValueRef};
use sha2::{Digest as _, Sha256};

#[path = "window_portable.rs"]
pub mod portable;

use portable::Place;

/// The FTS5 index over `chat_messages`, rebuilt rather than copied.
const FTS: &str = "chat_fts";

/// The tables the platform's migrations make, in the order they make them
/// (parents before the tables that refer to them): what a window copies.
/// Not `platform_migrations`, which is each file's own; not FTS5 tables
/// and their shadow tables, which are indexes.
pub fn tables() -> Vec<String> {
    let db = crate::db::Db::memory();
    let c = db.sqlite();
    let rows: Vec<(String, String)> = c
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let fts: Vec<&str> = rows
        .iter()
        .filter(|(_, sql)| sql.to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE"))
        .map(|(n, _)| n.as_str())
        .collect();
    rows.iter()
        .map(|(n, _)| n)
        .filter(|n| !n.starts_with("sqlite_") && *n != "platform_migrations")
        .filter(|n| !fts.iter().any(|f| *n == f || n.strip_prefix(f).is_some_and(|s| s.starts_with('_'))))
        .cloned()
        .collect()
}

/// One table on both sides of a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub from_rows: u64,
    pub to_rows: u64,
    pub from_sum: String,
    pub to_sum: String,
}

impl Table {
    pub fn matches(&self) -> bool {
        self.from_rows == self.to_rows && self.from_sum == self.to_sum
    }
}

/// What a copy did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub tables: Vec<Table>,
    /// `chat_fts` after its rebuild: the messages it indexes.
    pub fts_rows: u64,
    /// Rows the target held in these tables before `--replace` emptied it.
    pub replaced: u64,
}

impl Report {
    pub fn rows(&self) -> u64 {
        self.tables.iter().map(|t| t.to_rows).sum()
    }

    /// As the operator reads it: a line per table, then the verdict.
    pub fn render(&self, from: &Path, to: &Path) -> String {
        let mut out = format!("copied the platform's tables\n  from {}\n  to   {}\n\n", from.display(), to.display());
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
        let _ =
            writeln!(out, "{:<24} {:>9} {:>9}  rebuilt from chat_messages, integrity-check ok", FTS, "", self.fts_rows);
        if self.replaced > 0 {
            let _ = writeln!(out, "\n--replace: the target's {} earlier rows were deleted first", self.replaced);
        }
        let _ = writeln!(out, "\n{} tables, {} rows: every count and checksum matches", self.tables.len(), self.rows());
        out
    }
}

/// A `file:` URI for `path`, read-only, for `ATTACH`.
fn read_only_uri(path: &Path) -> anyhow::Result<String> {
    let abs = std::path::absolute(path)?;
    let u = url::Url::from_file_path(&abs)
        .map_err(|_| anyhow::anyhow!("{} isn't a path SQLite can open", abs.display()))?;
    Ok(format!("{u}?mode=ro"))
}

/// Copy the platform's tables from `from` to `to` (made, and brought up to
/// this build's migrations, if it isn't there), in one transaction, and
/// prove the copy. `replace` empties the target's tables first; without it
/// a target holding any of their rows is refused.
pub fn copy(from: &Path, to: &Path, replace: bool) -> anyhow::Result<Report> {
    if !from.exists() {
        bail!("{} isn't there", from.display());
    }
    if to.exists() && std::fs::canonicalize(from)? == std::fs::canonicalize(to)? {
        bail!("--from and --to are the same file");
    }
    let names = tables();
    // The target's own migration runner owns its schema and its
    // `platform_migrations`: a new file gets every migration, an old one
    // whatever it lacks.
    let db = crate::db::Db::open(to).with_context(|| format!("opening {}", to.display()))?;
    let mut c = db.sqlite();
    // Foreign keys hold in the source; rows go in table by table.
    c.pragma_update(None, "foreign_keys", "OFF")?;
    c.execute("ATTACH ?1 AS src", [read_only_uri(from)?]).with_context(|| format!("attaching {}", from.display()))?;
    let done = (|| -> anyhow::Result<Report> {
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // The source has every table, with the target's columns.
        let mut wrong = Vec::new();
        for t in &names {
            let want = columns(&tx, "main", t)?;
            let have = columns(&tx, "src", t)?;
            if have.is_empty() {
                wrong.push(format!("{t}: not in the source"));
            } else if have != want {
                let missing: Vec<_> = want.difference(&have).collect();
                let extra: Vec<_> = have.difference(&want).collect();
                wrong.push(format!("{t}: columns differ (the source lacks {missing:?}, has more: {extra:?})"));
            }
        }
        if !wrong.is_empty() {
            bail!(
                "{} doesn't hold this build's platform tables (start this build on it once, in process, to migrate it):\n  {}",
                from.display(),
                wrong.join("\n  ")
            );
        }
        let mut held = Vec::new();
        let mut replaced = 0;
        for t in &names {
            let n: u64 = tx.query_row(&format!("SELECT count(*) FROM main.\"{t}\""), [], |r| r.get(0))?;
            if n > 0 {
                held.push(format!("{t} ({n})"));
                replaced += n;
            }
        }
        if !held.is_empty() && !replace {
            bail!(
                "{} already holds the platform's rows: {}. Copy into an empty one, or pass --replace to delete them first",
                to.display(),
                held.join(", ")
            );
        }
        for t in names.iter().rev() {
            tx.execute(&format!("DELETE FROM main.\"{t}\""), [])?;
        }
        for t in &names {
            let mut cols: Vec<String> = columns(&tx, "main", t)?.into_iter().map(|c| format!("\"{c}\"")).collect();
            // The rowid too, unless a column already is it.
            if rowid_alias(&tx, t)?.is_none() {
                cols.insert(0, "rowid".into());
            }
            let cols = cols.join(", ");
            tx.execute(&format!("INSERT INTO main.\"{t}\" ({cols}) SELECT {cols} FROM src.\"{t}\""), [])?;
        }
        // The index, from the copied messages; FTS5 checks it against them.
        tx.execute(&format!("INSERT INTO main.{FTS} ({FTS}) VALUES ('rebuild')"), [])?;
        tx.execute(&format!("INSERT INTO main.{FTS} ({FTS}, rank) VALUES ('integrity-check', 1)"), [])
            .context("chat_fts's integrity-check after its rebuild")?;
        let fts_rows: u64 = tx.query_row(&format!("SELECT count(*) FROM main.{FTS}_docsize"), [], |r| r.get(0))?;
        let mut report = Report { tables: Vec::new(), fts_rows, replaced };
        for t in &names {
            let (from_rows, from_sum) = sum(&tx, "src", t)?;
            let (to_rows, to_sum) = sum(&tx, "main", t)?;
            report.tables.push(Table { name: t.clone(), from_rows, to_rows, from_sum, to_sum });
        }
        let messages = report.tables.iter().find(|t| t.name == "chat_messages").map_or(0, |t| t.to_rows);
        if report.fts_rows != messages {
            bail!("chat_fts indexes {} messages of {messages}", report.fts_rows);
        }
        let bad: Vec<&Table> = report.tables.iter().filter(|t| !t.matches()).collect();
        if !bad.is_empty() {
            bail!("the copy differs from its source, so nothing was written:\n{}", report.render(from, to));
        }
        // What the source's foreign keys allowed, the copy allows.
        for t in &names {
            let (a, b) = (dangling(&tx, "src", t)?, dangling(&tx, "main", t)?);
            if b > a {
                bail!("{t}: {b} rows refer to rows that aren't there (the source has {a})");
            }
        }
        tx.commit()?;
        Ok(report)
    })();
    let detached = c.execute("DETACH src", []);
    let report = done?;
    detached?;
    // The whole copy in the file itself, none of it left in the WAL, so the
    // file can be carried to the other machine as it is.
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(report)
}

/// `schema.table`'s column names.
fn columns(c: &Connection, schema: &str, table: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut q = c.prepare("SELECT name FROM pragma_table_info(?1, ?2)")?;
    let cols = q.query_map([table, schema], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(cols)
}

/// The column that is `table`'s rowid (an `INTEGER PRIMARY KEY`), if one is.
fn rowid_alias(c: &Connection, table: &str) -> anyhow::Result<Option<String>> {
    let mut q = c.prepare("SELECT name, type FROM pragma_table_info(?1, 'main') WHERE pk > 0")?;
    let pk: Vec<(String, String)> = q.query_map([table], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(match pk.as_slice() {
        [(name, ty)] if ty.eq_ignore_ascii_case("INTEGER") => Some(name.clone()),
        _ => None,
    })
}

/// Rows of `schema.table` whose foreign keys point at nothing.
fn dangling(c: &Connection, schema: &str, table: &str) -> anyhow::Result<u64> {
    let mut q = c.prepare(&format!("PRAGMA {schema}.foreign_key_check(\"{table}\")"))?;
    let n = q.query_map([], |_| Ok(()))?.count();
    Ok(n as u64)
}

/// `schema.table`'s row count and checksum (the module's docs say how).
pub fn sum(c: &Connection, schema: &str, table: &str) -> anyhow::Result<(u64, String)> {
    let cols: Vec<String> = columns(c, schema, table)?.into_iter().collect();
    let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
    let mut q = c.prepare(&format!("SELECT rowid, {list} FROM {schema}.\"{table}\""))?;
    let mut rows = q.query([])?;
    let mut digests: Vec<[u8; 32]> = Vec::new();
    while let Some(r) = rows.next()? {
        let mut h = Sha256::new();
        for i in 0..=cols.len() {
            h.update(if i == 0 { "rowid" } else { cols[i - 1].as_str() }.as_bytes());
            h.update([0]);
            let (tag, bytes): (u8, Vec<u8>) = match r.get_ref(i)? {
                ValueRef::Null => (0, Vec::new()),
                ValueRef::Integer(n) => (1, n.to_le_bytes().to_vec()),
                ValueRef::Real(f) => (2, f.to_bits().to_le_bytes().to_vec()),
                ValueRef::Text(t) => (3, t.to_vec()),
                ValueRef::Blob(b) => (4, b.to_vec()),
            };
            h.update([tag]);
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(&bytes);
        }
        digests.push(h.finalize().into());
    }
    digests.sort_unstable();
    let mut h = Sha256::new();
    for d in &digests {
        h.update(d);
    }
    Ok((digests.len() as u64, hex::encode(h.finalize())))
}

/// Each of the platform's tables in the file at `path`, read-only: its row
/// count and checksum, the same as [`copy`] prints. A table the file
/// doesn't have is left out.
pub fn checksums(path: &Path) -> anyhow::Result<Vec<(String, u64, String)>> {
    let c = read_only(path)?;
    let mut out = Vec::new();
    for t in tables() {
        if columns(&c, "main", &t)?.is_empty() {
            continue;
        }
        let (n, s) = sum(&c, "main", &t)?;
        out.push((t, n, s));
    }
    Ok(out)
}

fn read_only(path: &Path) -> anyhow::Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {} read-only", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.pragma_update(None, "query_only", "ON")?;
    Ok(c)
}

/// Cloud secrets in the file at `path` that open with `key`, and the
/// `(account, name)` of each that doesn't. Values are never kept.
pub fn secrets_open(path: &Path, key: &str) -> anyhow::Result<(u64, Vec<(String, String)>)> {
    let cipher = crate::cloud::secrets::SecretCipher::new(key)?;
    let c = read_only(path)?;
    let mut q = c.prepare("SELECT account, name, sealed FROM cloud_secrets ORDER BY account, name")?;
    let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
    let (mut opened, mut failed) = (0, Vec::new());
    for row in rows {
        let (account, name, sealed) = row?;
        match cipher.open(&account, &name, &sealed) {
            Ok(_) => opened += 1,
            Err(_) => failed.push((account, name)),
        }
    }
    Ok((opened, failed))
}

/// `var` as the running server (this same binary, not this command) has
/// it in its environment: on a Fly machine, `fly ssh console` doesn't
/// always carry the app's secrets, and the server's are the ones that
/// matter. `None` off Linux, or when there's no such process.
fn running_env(var: &str) -> Option<(u32, String)> {
    let me = std::process::id();
    let exe = std::env::current_exe().ok()?;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == me || std::fs::read_link(e.path().join("exe")).ok().as_deref() != Some(exe.as_path()) {
            continue;
        }
        // Another operator's command, not the server.
        let cmdline = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        if cmdline.split(|b| *b == 0).nth(1).is_some_and(|a| COMMANDS.iter().any(|c| c.as_bytes() == a)) {
            continue;
        }
        let environ = std::fs::read(e.path().join("environ")).ok()?;
        let prefix = format!("{var}=");
        let value = environ.split(|b| *b == 0).find_map(|kv| kv.strip_prefix(prefix.as_bytes()))?;
        return Some((pid, String::from_utf8_lossy(value).into_owned()));
    }
    None
}

/// The first words that make either binary run one of these instead of
/// serving (and `platform`, control's prefix for them).
pub const COMMANDS: [&str; 5] = ["copy", "checksums", "secrets-check", "migrations", "platform"];

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// Copy the platform's tables from one database to another, and prove
    /// it: file to file (the first window), or to or from Postgres (the
    /// second: portable checksums).
    Copy {
        /// The file to copy from (the hub's control.db, or a copy of it),
        /// opened read-only; or a `postgres://` URL.
        #[arg(long)]
        from: Place,
        /// The file to copy into, made if it isn't there; or a
        /// `postgres://` URL, migrated to this build if it isn't.
        #[arg(long)]
        to: Place,
        /// Delete the target's rows in these tables first. Without it, a
        /// target that holds any is refused.
        #[arg(long)]
        replace: bool,
    },
    /// Each platform table's row count and checksum, read-only.
    Checksums {
        /// A file, or a `postgres://` URL (portable checksums).
        #[arg(long)]
        db: Place,
        /// A file's portable checksums, as Postgres's are, rather than the
        /// first window's (rowids and SQLite's types).
        #[arg(long)]
        portable: bool,
        /// Postgres only: read in the snapshot another session exported
        /// with `pg_export_snapshot()` (and still holds), as a
        /// `pg_dump --snapshot` of it does: the nightly dump's record.
        #[arg(long)]
        snapshot: Option<String>,
    },
    /// The migrations a database has applied, against this build's,
    /// read-only: whether this build would run any, and refused when it
    /// can't run on it at all.
    Migrations {
        /// A file, or a `postgres://` URL.
        #[arg(long)]
        db: Place,
    },
    /// Whether every cloud secret in the database opens with the cloud
    /// secret key: the running server's, else ARUGULA_CLOUD_SECRET_KEY
    /// here. Prints no value.
    SecretsCheck {
        /// A file, or a `postgres://` URL.
        #[arg(long)]
        db: Place,
    },
}

/// `<prog> copy|checksums|secrets-check …`: `prog` is how the operator
/// called it (`arugula-platform`, or `arugula-control platform`).
pub fn run(prog: &str, args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once(prog.to_owned()).chain(args));
    match a.what {
        What::Copy { from: Place::File(from), to: Place::File(to), replace } => {
            let r = copy(&from, &to, replace)?;
            print!("{}", r.render(&from, &to));
        }
        What::Copy { from, to, replace } => {
            let r = portable::copy(&from, &to, replace)?;
            print!("{}", r.render(&from, &to));
        }
        What::Checksums { db, portable, snapshot } => {
            let (sums, kind) = match &db {
                Place::File(path) if !portable && snapshot.is_none() => (checksums(path)?, ""),
                _ => (portable::checksums_in(&db, snapshot.as_deref())?, ", portable checksums"),
            };
            println!("the platform's tables in {db} (read-only{kind})\n");
            println!("{:<24} {:>9}  sha256", "table", "rows");
            for (t, n, s) in &sums {
                println!("{t:<24} {n:>9}  {s}");
            }
            println!("\n{} tables, {} rows", sums.len(), sums.iter().map(|s| s.1).sum::<u64>());
        }
        What::Migrations { db } => {
            let m = portable::migrations(&db)?;
            print!("{}", m.render(&db));
            if !m.pending.is_empty() {
                bail!("this build would migrate {db}: it's at {}", m.applied.last().map_or(0, |a| a.0));
            }
        }
        What::SecretsCheck { db } => {
            const VAR: &str = "ARUGULA_CLOUD_SECRET_KEY";
            let (key, whose) = match running_env(VAR) {
                Some((pid, k)) => (k, format!("the running server's (pid {pid})")),
                None => match std::env::var(VAR) {
                    Ok(k) if !k.is_empty() => (k, format!("{VAR} in this shell")),
                    _ => bail!("no {VAR}: neither the running server nor this shell has one"),
                },
            };
            let (opened, failed) = portable::secrets_open(&db, &key)?;
            println!("cloud secrets in {db}, with {whose} key:");
            println!("  {opened} open");
            for (account, name) in &failed {
                println!("  doesn't open: {account} {name}");
            }
            if !failed.is_empty() {
                bail!("{} of {} cloud secrets don't open with this key", failed.len(), opened + failed.len() as u64);
            }
            if opened == 0 {
                println!("  (there are none, so this proves nothing about the key)");
            }
        }
    }
    Ok(())
}

/// The cloud secret key [`sample`]'s secret is sealed with (tests).
#[cfg(any(test, feature = "contract"))]
pub const SAMPLE_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

/// A few rows in several of the platform's tables, the way the platform
/// writes them (tests, here and in control): rooms, members and messages
/// that search finds, a read mark, projects, a grant, the feed's position,
/// cloud notifications with a gap in their rowids, and a cloud secret
/// sealed with [`SAMPLE_KEY`]. Accounts `acct-a` and `acct-b`.
#[cfg(any(test, feature = "contract"))]
pub fn sample(db: &crate::db::Db) {
    let cipher = crate::cloud::secrets::SecretCipher::new(SAMPLE_KEY).unwrap();
    let c = db.sqlite();
    c.execute_batch(
        "INSERT INTO projects (id, team_id, account_id, name, is_default, created_at)
             VALUES ('prj_t', 'team-1', NULL, 'acme', 1, 5), ('prj_a', NULL, 'acct-a', 'Personal', 1, 6);
         INSERT INTO chat_rooms (id, scope, kind, name, created_by, created, last_seq, project_id)
             VALUES ('r1', 'team:team-1', 'channel', 'general', 'acct-a', 10, 3, 'prj_t'),
                    ('r2', 'dm:acct-a:acct-b', 'dm', '', 'acct-a', 11, 1, NULL);
         INSERT INTO chat_members (room, account) VALUES ('r2', 'acct-a'), ('r2', 'acct-b');
         INSERT INTO chat_messages (id, room, seq, author, at, text, quote)
             VALUES (7, 'r1', 1, 'acct-a', 20, 'the rehearsal went well', NULL),
                    (9, 'r1', 2, 'acct-b', 21, 'ship it', 'the rehearsal went well'),
                    (12, 'r1', 3, 'acct-a', 22, 'a real number 0.5', NULL),
                    (40, 'r2', 1, 'acct-b', 23, 'psst', NULL);
         INSERT INTO chat_reads (room, account, seq) VALUES ('r1', 'acct-a', 3);
         INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
             VALUES ('g1', 'daemon', 'd1', 'cloud.drive', 'account', 'acct-a', 'acct-a', 30);
         INSERT INTO hub_feed (reader, seq) VALUES ('projects', 1234);
         INSERT INTO cloud_notifications (conversation, account, request, next_attempt)
             VALUES ('c2', 'acct-a', 'q2', 1), ('c1', 'acct-a', 'q1', 1);
         DELETE FROM cloud_notifications WHERE conversation = 'c2';
         INSERT INTO cloud_notifications (conversation, account, request, next_attempt) VALUES ('c3', 'acct-a', 'q3', 2.5);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO cloud_secrets (account, name, sealed, project_id) VALUES ('acct-a', 'GITHUB_TOKEN', ?1, 'prj_a')",
        [cipher.seal("acct-a", "GITHUB_TOKEN", "ghp_never_printed").unwrap()],
    )
    .unwrap();
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
