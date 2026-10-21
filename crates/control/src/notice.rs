//! `arugula-control notice` (M141): a notice for every account, such as
//! an announced window a day ahead. It's the notice control already shows
//! once (#206): a row per account in `notices`, which `GET /api/teams`
//! hands the page and the page shows in a dialog until it's dismissed
//! (`POST /api/me/notices/{id}/seen`). Unseen ones go after 30 days. Run
//! on the hub's machine, beside the running hub:
//!
//! ```text
//! fly ssh sftp put notice.txt /tmp/notice.txt -a illogical-control
//! fly ssh console -a illogical-control -C "/arugula-control notice post --db /data/control.db --from /tmp/notice.txt"
//! ```
//!
//! `--from` is a file whose first line is the title and the rest the body,
//! so nothing has to survive `fly ssh console -C`'s quoting; `--title` and
//! `--body` say it on the command line instead.
//!
//! An account made after the post doesn't get it. `withdraw` takes back
//! the ones nobody has dismissed yet (a window that moved), by title.

use std::{path::PathBuf, time::Duration};

use anyhow::Context as _;
use clap::Parser;
use rusqlite::params;

#[derive(Parser, Debug)]
#[command(name = "arugula-control notice", about = "Tell every account something, once, on control's page")]
struct Args {
    #[command(subcommand)]
    what: What,
    /// Control's database.
    #[arg(long, global = true, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// A notice for every account there is now.
    Post {
        #[arg(long, required_unless_present = "from", conflicts_with = "from")]
        title: Option<String>,
        #[arg(long, required_unless_present = "from", conflicts_with = "from")]
        body: Option<String>,
        /// A file: its first line is the title, the rest the body.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Say how many accounts would get it, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Take back the notices with this title that haven't been dismissed.
    Withdraw {
        #[arg(long, required_unless_present = "from", conflicts_with = "from")]
        title: Option<String>,
        /// The file the notice was posted from: its first line is the title.
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// The notices waiting to be seen, by title.
    List,
}

/// A notice's title and body: from the flags, or from a file whose first
/// line is the title.
fn said(title: Option<String>, body: Option<String>, from: Option<PathBuf>) -> anyhow::Result<(String, String)> {
    Ok(match from {
        Some(f) => {
            let text = std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
            let (title, body) = text.split_once('\n').unwrap_or((&text, ""));
            (title.trim().to_owned(), body.trim().to_owned())
        }
        None => (title.unwrap_or_default().trim().to_owned(), body.unwrap_or_default().trim().to_owned()),
    })
}

/// `arugula-control notice post|withdraw|list`.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control notice".to_owned()).chain(args));
    // The hub's file, never a new one.
    let c = rusqlite::Connection::open_with_flags(
        &a.db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", a.db.display()))?;
    c.busy_timeout(Duration::from_secs(5))?;
    match a.what {
        What::Post { title, body, from, dry_run } => {
            let (title, body) = said(title, body, from)?;
            anyhow::ensure!(!title.is_empty() && !body.is_empty(), "a notice needs a title and a body");
            anyhow::ensure!(
                title.len() <= 200 && body.len() <= 2000,
                "keep it short: 200 bytes of title, 2000 of body"
            );
            let accounts: u64 = c.query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))?;
            if dry_run {
                println!("would tell {accounts} accounts: {title}\n\n{body}");
                return Ok(());
            }
            let n = c.execute(
                "INSERT INTO notices (account, title, body, created) SELECT id, ?1, ?2, ?3 FROM accounts",
                params![title, body, arugula_e2e::now_ms()],
            )?;
            println!("posted to {n} accounts: each sees it once, on control's page, until they dismiss it");
        }
        What::Withdraw { title, from } => {
            let (title, _) = said(title, None, from)?;
            let n = c.execute("DELETE FROM notices WHERE title = ?1", params![title])?;
            println!("withdrew {n} notices nobody had dismissed");
        }
        What::List => {
            let mut q = c.prepare("SELECT title, count(*), min(created) FROM notices GROUP BY title ORDER BY 3")?;
            let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?;
            for row in rows {
                let (title, n) = row?;
                println!("{n:>6} unseen  {title}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every account gets it, once, as the page reads notices; withdrawing
    /// takes back the ones still unseen.
    #[test]
    fn a_notice_reaches_every_account() {
        let dir = std::env::temp_dir().join(format!("arugula-notice-{}", hex::encode(arugula_e2e::random::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.db");
        let cmd = |args: &[&str]| {
            run(args.iter().map(|s| (*s).to_owned()).chain(["--db".into(), path.display().to_string()]))
        };
        assert!(cmd(&["list"]).is_err(), "no database there");
        assert!(!path.exists(), "and none made");
        let db = crate::db::Db::open(&path).unwrap();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("INSERT INTO accounts (id, created) VALUES ('acct-a', 1), ('acct-b', 2)")
            .unwrap();
        let (title, body) = ("Chat and Cloud: maintenance", "Down a few minutes. Reload open pages after.");
        cmd(&["post", "--title", title, "--body", body, "--dry-run"]).unwrap();
        assert!(db.notices("acct-a").unwrap().is_empty(), "a dry run writes nothing");
        assert!(cmd(&["post", "--title", " ", "--body", body]).is_err());
        let file = dir.join("notice.txt");
        std::fs::write(&file, format!("{title}\n\n{body}\n")).unwrap();
        cmd(&["post", "--from", &file.display().to_string()]).unwrap();
        for a in ["acct-a", "acct-b"] {
            let n = db.notices(a).unwrap();
            assert_eq!(n.len(), 1, "{a}");
            assert_eq!((n[0].1.as_str(), n[0].2.as_str()), (title, body));
        }
        assert!(db.drop_notice("acct-a", db.notices("acct-a").unwrap()[0].0).unwrap(), "dismissed as the page does");
        cmd(&["list"]).unwrap();
        cmd(&["withdraw", "--from", &file.display().to_string()]).unwrap();
        assert!(db.notices("acct-b").unwrap().is_empty());
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
