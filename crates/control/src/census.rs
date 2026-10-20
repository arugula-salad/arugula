//! `arugula-control census` (M132): the daemons control has seen lately,
//! counted by the version they last said they run. It sizes the version
//! skew a change to control has to survive.
//!
//! It opens the database read-only, so it can run beside the live control
//! (`just control-census` runs it on the hosted one over `fly ssh`). A
//! daemon's version is what it last sent on `GET /api/daemon/trust` (M100,
//! daemons from 0.43.0 on); an older daemon never says, and counts as
//! `unknown`. "Seen" is `last_seen`: the last time it was on the relay.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use rusqlite::{Connection, OpenFlags, params};

const DAY_MS: u64 = 86_400_000;

#[derive(Parser, Debug)]
#[command(name = "arugula-control census", about = "Count the daemons control saw lately, by version (read-only)")]
pub struct Args {
    /// Control's database, opened read-only.
    #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
    /// How far back to look.
    #[arg(long, default_value_t = 30)]
    days: u64,
}

/// One version's daemons: how many, and the oldest and newest last-seen
/// among them (ms since the epoch).
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub version: Option<String>,
    pub daemons: u64,
    pub oldest: u64,
    pub newest: u64,
}

/// The database at `path`, read-only: it can't be written through, and
/// opening it makes no file.
pub fn open(path: &Path) -> anyhow::Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {} read-only", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.pragma_update(None, "query_only", "ON")?;
    Ok(c)
}

/// The daemons last seen at or after `since` (ms), by version: newest
/// version first, `unknown` last.
pub fn count(c: &Connection, since: u64) -> anyhow::Result<Vec<Line>> {
    let mut q = c.prepare(
        "SELECT version, COUNT(*), MIN(last_seen), MAX(last_seen) FROM daemons
         WHERE last_seen >= ?1 GROUP BY version",
    )?;
    let mut lines = q
        .query_map(params![since], |r| {
            Ok(Line { version: r.get(0)?, daemons: r.get(1)?, oldest: r.get(2)?, newest: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    lines.sort_by(|a, b| match (&a.version, &b.version) {
        (Some(x), Some(y)) => key(y).cmp(&key(x)).then_with(|| y.cmp(x)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    Ok(lines)
}

/// A version's numbers, for ordering: `0.52.0` before `0.9.1`.
fn key(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

/// The census as text: a line per version and a total.
pub fn render(lines: &[Line], days: u64, now: u64) -> String {
    let mut out = format!("Daemons seen by control in the {days} days to {}, by version\n\n", date(now));
    out += &format!("{:<16} {:>7}  {:<17}  {:<17}\n", "version", "daemons", "oldest last seen", "newest last seen");
    for l in lines {
        out += &format!(
            "{:<16} {:>7}  {:<17}  {:<17}\n",
            l.version.as_deref().unwrap_or("unknown (<0.43)"),
            l.daemons,
            date(l.oldest),
            date(l.newest)
        );
    }
    out += &format!("{:<16} {:>7}\n", "total", lines.iter().map(|l| l.daemons).sum::<u64>());
    out
}

/// `2026-10-20 13:47` (UTC) for ms since the epoch.
fn date(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

/// `arugula-control census [--db PATH] [--days N]`, with `args` after the
/// word `census`.
pub fn run(args: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control census".to_owned()).chain(args));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64;
    let c = open(&a.db)?;
    let lines = count(&c, now.saturating_sub(a.days * DAY_MS))?;
    print!("{}", render(&lines, a.days, now));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn counts_recent_daemons_by_version_read_only() {
        let path = std::env::temp_dir().join(format!("arugula-census-{}.db", hex::encode(arugula_e2e::random::<16>())));
        let now = 100 * DAY_MS;
        {
            let db = Db::open(&path).unwrap();
            let daemon = |id: &str, seen: u64, version: Option<&str>| {
                db.put_daemon("acct", id, id, &[]).unwrap();
                db.seen(id, None, seen).unwrap();
                if let Some(v) = version {
                    db.set_daemon_machine(id, v, "linux", "x86_64").unwrap();
                }
            };
            daemon("a", now - DAY_MS, Some("0.52.0"));
            daemon("b", now - 3 * DAY_MS, Some("0.52.0"));
            daemon("c", now - 2 * DAY_MS, Some("0.9.1"));
            daemon("d", now - 5 * DAY_MS, None);
            // Seen too long ago, and never seen: not counted.
            daemon("e", now - 31 * DAY_MS, Some("0.52.0"));
            db.put_daemon("acct", "f", "f", &[]).unwrap();
        }
        let c = open(&path).unwrap();
        let lines = count(&c, now - 30 * DAY_MS).unwrap();
        assert_eq!(
            lines,
            vec![
                Line { version: Some("0.52.0".into()), daemons: 2, oldest: now - 3 * DAY_MS, newest: now - DAY_MS },
                Line { version: Some("0.9.1".into()), daemons: 1, oldest: now - 2 * DAY_MS, newest: now - 2 * DAY_MS },
                Line { version: None, daemons: 1, oldest: now - 5 * DAY_MS, newest: now - 5 * DAY_MS },
            ]
        );
        assert!(c.execute("DELETE FROM daemons", []).is_err(), "the census can't write");
        let text = render(&lines, 30, now);
        assert!(text.contains("0.52.0                 2  1970-04-08 00:00"), "{text}");
        assert!(text.contains("unknown (<0.43)"), "{text}");
        assert!(text.contains("total                  4"), "{text}");
        drop(c);
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }

    #[test]
    fn dates_are_utc() {
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(1_792_503_000_000), "2026-10-20 13:30");
        assert_eq!(date(951_782_400_000), "2000-02-29 00:00");
    }
}
