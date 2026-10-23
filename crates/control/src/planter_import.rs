//! `arugula-control import planter` (M232, platform step 8, Arugula #30):
//! planter's accounts become account links (M229), from a backup of
//! planter's `DATA_DIR` (`arugula_platform::planter` reads it and says what
//! it holds).
//!
//! - **By numeric GitHub id only.** Each planter account is keyed by the
//!   person's GitHub id, and that is planter's own user id too. It links to
//!   the hub account whose GitHub identity has that number. A login is never
//!   matched: one planter knew as `jake` and an account that signs in as
//!   `jake` today are only the same person when the numbers are.
//! - **Nobody is made.** A planter account no hub account has the GitHub id
//!   of is reported as waiting for its first sign-in; a later run links it.
//! - **Imported, by the operator.** The link is client `planter`'s, like one
//!   planter makes itself, and audited `imported` by `operator`. A link is
//!   never authorization.
//! - **Nothing that's there changes.** The same link again is "already";
//!   either side linked elsewhere is reported; an account that was ever
//!   unlinked from planter is never linked again by an import; and an account
//!   planter says it linked to a different platform account is reported, not
//!   linked. Nothing is deleted. One transaction.
//! - **The backup is only read,** and `--dry-run` writes nothing.
//!
//! Then `arugula-platform import planter` links the repos, for the planters
//! linked here.

use std::{fmt::Write as _, path::PathBuf};

use anyhow::{Context as _, bail};
use arugula_e2e::now_ms;
use arugula_platform::planter::{Backup, CLIENT};
use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;

use crate::db::issuer::{self, ClientKind, Imported, Link};

#[derive(clap::Parser, Debug)]
struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// planter's accounts as account links, by numeric GitHub id (M232).
    /// Safe to run again; never deletes; reads the backup only.
    Planter {
        #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
        db: PathBuf,
        /// A copy of planter's DATA_DIR: its accounts.json (and boxes.json).
        #[arg(long)]
        backup: PathBuf,
        /// planter's issuer client (registered as `oidc`).
        #[arg(long, default_value = CLIENT)]
        client: String,
        /// Say what would change, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// The report as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// What became of one planter account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Linked now (or, dry, would be).
    Linked,
    /// Linked to this account already.
    Already,
    /// No hub account has this GitHub id: not on Arugula yet.
    Waiting,
    /// The account was unlinked from planter (ms): never linked again here.
    UnlinkedSince { at: u64 },
    /// planter's user is linked to another account.
    UserLinkedElsewhere { other: String },
    /// The account is linked to another planter user.
    AccountLinkedElsewhere { other: String },
    /// planter says it's linked to a different platform account.
    PlanterSaysOther { platform_account: String },
    /// Not a usable account (no numeric GitHub id).
    Skipped { why: String },
}

/// One planter account's line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    /// planter's key: the GitHub id, when it has one.
    pub user: String,
    pub login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// What the import did (or, dry, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub client: String,
    /// Whether the client is registered as an `oidc` client.
    pub client_registered: bool,
    pub accounts: Vec<Line>,
    /// planter's boxes: none is imported (`arugula-platform import planter`
    /// reports each).
    pub boxes: usize,
}

impl Report {
    pub fn count(&self, f: impl Fn(&Outcome) -> bool) -> usize {
        self.accounts.iter().filter(|l| f(&l.outcome)).count()
    }

    pub fn render(&self, backup: &Backup) -> String {
        let mut out = String::new();
        let dry = self.dry_run;
        let _ = writeln!(
            out,
            "planter's backup at {} (read only): {} accounts, {} boxes{}",
            backup.dir.display(),
            self.accounts.len(),
            self.boxes,
            if dry { ", DRY RUN" } else { "" }
        );
        if !self.client_registered {
            let _ = writeln!(
                out,
                "warning: {} isn't a registered oidc client here: register it before the real run \
                 (arugula-control issuer add-client --kind oidc --id {})",
                self.client, self.client
            );
        }
        let _ = writeln!(out, "\naccounts, by numeric GitHub id (client {}):", self.client);
        for l in &self.accounts {
            let what = match &l.outcome {
                Outcome::Linked => format!("{} {}", if dry { "would link to" } else { "linked to" }, acct(l)),
                Outcome::Already => format!("already linked to {}", acct(l)),
                Outcome::Waiting => "waiting for their first sign-in to Arugula (with GitHub)".into(),
                Outcome::UnlinkedSince { at } => {
                    format!("not linked: {} unlinked planter at {at}, and stays unlinked", acct(l))
                }
                Outcome::UserLinkedElsewhere { other } => format!("not linked: this user is linked to {other}"),
                Outcome::AccountLinkedElsewhere { other } => {
                    format!("not linked: {} is linked to planter user {other}", acct(l))
                }
                Outcome::PlanterSaysOther { platform_account } => format!(
                    "not linked: planter says it's linked to {platform_account}, but GitHub id {} is {}",
                    l.user,
                    acct(l)
                ),
                Outcome::Skipped { why } => format!("skipped: {why}"),
            };
            let _ = writeln!(out, "  {:<12} {:<20} {what}", l.user, l.login);
        }
        let _ = writeln!(
            out,
            "\n{} {}, {} already, {} waiting, {} not linked, {} skipped{}",
            self.count(|o| *o == Outcome::Linked),
            if dry { "would be linked" } else { "linked" },
            self.count(|o| *o == Outcome::Already),
            self.count(|o| *o == Outcome::Waiting),
            self.count(|o| {
                matches!(
                    o,
                    Outcome::UnlinkedSince { .. }
                        | Outcome::UserLinkedElsewhere { .. }
                        | Outcome::AccountLinkedElsewhere { .. }
                        | Outcome::PlanterSaysOther { .. }
                )
            }),
            self.count(|o| matches!(o, Outcome::Skipped { .. })),
            if dry { "; nothing written" } else { "" }
        );
        let _ = writeln!(out, "then: arugula-platform import planter, for their repos and boxes");
        out
    }
}

fn acct(l: &Line) -> &str {
    l.account.as_deref().unwrap_or("?")
}

/// Import `backup`'s accounts into the hub's database `c` (the module's
/// docs), at `now`, in one transaction; nothing when `dry`.
pub fn import(c: &mut Connection, backup: &Backup, client: &str, dry: bool, now: u64) -> anyhow::Result<Report> {
    let registered = issuer::clients(c)?.iter().any(|k| k.id == client && k.kind == ClientKind::Oidc);
    if !registered && !dry {
        bail!(
            "{client} isn't a registered oidc client: register planter's client first \
             (arugula-control issuer add-client --kind oidc --id {client} …)"
        );
    }
    // Takes the write lock as it begins, waiting out the running hub's
    // (a deferred one that reads, then writes, fails at once if the hub
    // wrote in between).
    let behavior = if dry { TransactionBehavior::Deferred } else { TransactionBehavior::Immediate };
    let tx = c.transaction_with_behavior(behavior)?;
    let mut lines = Vec::new();
    for s in &backup.skipped {
        let outcome = Outcome::Skipped { why: s.why.clone() };
        lines.push(Line { user: s.key.clone(), login: s.login.clone(), account: None, outcome });
    }
    for a in &backup.accounts {
        let mut line = Line { user: a.user(), login: a.login.clone(), account: None, outcome: Outcome::Waiting };
        if let Some((account, _)) = issuer::account_by_github(&tx, a.github_id)? {
            line.account = Some(account.clone());
            line.outcome = match &a.platform_account {
                Some(said) if *said != account => Outcome::PlanterSaysOther { platform_account: said.clone() },
                _ => {
                    let name = Some(a.login.clone()).filter(|n| !n.is_empty() && n.len() <= crate::links::NAME_MAX);
                    let link = Link {
                        client: client.to_owned(),
                        user: a.user(),
                        account,
                        github_id: a.github_id,
                        name,
                        linked: now,
                    };
                    match issuer::import_link(&tx, &link, dry)? {
                        Imported::Linked => Outcome::Linked,
                        Imported::Already => Outcome::Already,
                        Imported::UnlinkedSince(at) => Outcome::UnlinkedSince { at },
                        Imported::UserLinkedElsewhere(other) => Outcome::UserLinkedElsewhere { other },
                        Imported::AccountLinkedElsewhere(other) => Outcome::AccountLinkedElsewhere { other },
                    }
                }
            };
        }
        lines.push(line);
    }
    if dry {
        tx.rollback()?;
    } else {
        tx.commit()?;
    }
    Ok(Report {
        dry_run: dry,
        client: client.to_owned(),
        client_registered: registered,
        accounts: lines,
        boxes: backup.boxes.len(),
    })
}

/// `arugula-control import …`.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    use clap::Parser as _;
    let a = Args::parse_from(std::iter::once("arugula-control import".to_owned()).chain(args));
    let What::Planter { db, backup, client, dry_run, json } = a.what;
    let backup = Backup::read(&backup)?;
    let mut c = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", db.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    let r = import(&mut c, &backup, &client, dry_run, now_ms())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", r.render(&backup));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::db::{Db, issuer::Client};

    /// The made-up backup the platform's tests read too.
    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../platform/fixtures/planter-backup")
    }

    fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name().into_string().unwrap(), std::fs::read(e.path()).unwrap())
            })
            .collect();
        out.sort();
        out
    }

    /// A hub where jake (GitHub 4242, signing in as `jake-gh`) and alice
    /// (5151) have accounts, with planter's client registered.
    fn hub() -> Db {
        let db = Db::memory();
        db.account_for("github", "4242", "jake-gh", "acct-jake", 1).unwrap();
        db.account_for("github", "5151", "alice", "acct-alice", 1).unwrap();
        db.put_issuer_client(&Client {
            id: CLIENT.into(),
            kind: ClientKind::Oidc,
            name: "planter".into(),
            redirect_uris: vec!["https://planter.example/auth/arugula/callback".into()],
            created: 1,
        })
        .unwrap();
        db
    }

    /// Every link and audit row.
    fn rows(db: &Db) -> Vec<String> {
        let c = db.conn();
        let mut out = Vec::new();
        for sql in [
            "SELECT client || ' ' || product_user || ' ' || account || ' ' || github_id || ' ' || \
             COALESCE(name, '') || ' ' || linked FROM account_links ORDER BY 1",
            "SELECT account || ' ' || client || ' ' || product_user || ' ' || event || ' ' || by || ' ' || at \
             FROM account_link_events ORDER BY id",
        ] {
            let mut q = c.prepare(sql).unwrap();
            out.extend(q.query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap));
        }
        out
    }

    fn outcomes(r: &Report) -> Vec<(&str, Option<&str>, &Outcome)> {
        r.accounts.iter().map(|l| (l.user.as_str(), l.account.as_deref(), &l.outcome)).collect()
    }

    fn run(db: &Db, backup: &Backup, dry: bool, now: u64) -> anyhow::Result<Report> {
        import(&mut db.conn(), backup, CLIENT, dry, now)
    }

    #[test]
    fn accounts_link_by_github_id_and_a_second_run_changes_nothing() {
        let db = hub();
        let backup = Backup::read(&fixture()).unwrap();
        let before = files(&fixture());

        let r = run(&db, &backup, true, 10).unwrap();
        let want = [
            ("ghost", None, &Outcome::Skipped { why: "no numeric GitHub id".into() }),
            // A login that changed hands: planter's jake-gh 1111 is not the
            // account that signs in as jake-gh today (4242).
            ("1111", None, &Outcome::Waiting),
            ("4242", Some("acct-jake"), &Outcome::Linked),
            ("5151", Some("acct-alice"), &Outcome::Linked),
            ("7777", None, &Outcome::Waiting),
        ];
        assert_eq!(outcomes(&r), want);
        assert!(rows(&db).is_empty(), "a dry run writes nothing");
        let text = r.render(&backup);
        assert!(text.contains("would link to acct-jake") && text.contains("nothing written"), "{text}");
        assert!(text.contains("waiting for their first sign-in"), "{text}");

        let r = run(&db, &backup, false, 20).unwrap();
        assert_eq!(outcomes(&r), want);
        let jake = db.account_link(CLIENT, "4242").unwrap().unwrap();
        assert_eq!((jake.account.as_str(), jake.github_id, jake.name.as_deref()), ("acct-jake", 4242, Some("jake-gh")));
        assert!(db.account_link(CLIENT, "1111").unwrap().is_none(), "never by login");
        let audit: Vec<(String, String)> =
            db.account_link_events("acct-jake").unwrap().into_iter().map(|e| (e.event, e.by)).collect();
        assert_eq!(audit, [("imported".to_owned(), "operator".to_owned())]);
        // The platform page lists it like a link the product made.
        let products = db.account_products("acct-jake").unwrap();
        assert_eq!(products.len(), 1);
        assert_eq!((products[0].name.as_str(), products[0].granted), ("planter", None));
        let done = rows(&db);

        // Again: nothing changes.
        let r = run(&db, &backup, false, 30).unwrap();
        assert_eq!(r.count(|o| *o == Outcome::Linked), 0);
        assert_eq!(r.count(|o| *o == Outcome::Already), 2);
        assert_eq!(rows(&db), done);

        // Someone not on Arugula yet is picked up once they sign in.
        db.account_for("github", "7777", "newbie", "acct-new", 40).unwrap();
        let r = run(&db, &backup, false, 50).unwrap();
        assert_eq!(outcomes(&r)[4], ("7777", Some("acct-new"), &Outcome::Linked));
        assert_eq!(r.count(|o| *o == Outcome::Linked), 1);
        assert_eq!(files(&fixture()), before, "the backup is only read");
    }

    #[test]
    fn an_unlink_sticks_and_nothing_there_is_changed() {
        let db = hub();
        let backup = Backup::read(&fixture()).unwrap();
        run(&db, &backup, false, 10).unwrap();
        // alice unlinks planter on the platform page.
        assert!(db.unlink_product("acct-alice", CLIENT, 20).unwrap());
        let r = run(&db, &backup, false, 30).unwrap();
        assert_eq!(outcomes(&r)[3], ("5151", Some("acct-alice"), &Outcome::UnlinkedSince { at: 20 }));
        assert!(db.account_link(CLIENT, "5151").unwrap().is_none());

        // A link planter's user or the account has elsewhere is left.
        let db = hub();
        let other = Link {
            client: CLIENT.into(),
            user: "9999".into(),
            account: "acct-jake".into(),
            github_id: 4242,
            name: None,
            linked: 5,
        };
        assert_eq!(db.link_account(&other, 1).unwrap(), Ok(true));
        let r = run(&db, &backup, false, 10).unwrap();
        assert_eq!(
            outcomes(&r)[2],
            ("4242", Some("acct-jake"), &Outcome::AccountLinkedElsewhere { other: "9999".into() })
        );
        assert_eq!(db.account_link(CLIENT, "9999").unwrap().unwrap().account, "acct-jake");

        // planter saying another platform account is reported, not linked.
        let db = hub();
        let dir = std::env::temp_dir().join(format!("planter-says-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let accounts = serde_json::json!({ "5151": { "id": 5151, "login": "alice", "platformAccount": "acct-other" } });
        std::fs::write(dir.join("accounts.json"), accounts.to_string()).unwrap();
        let r = run(&db, &Backup::read(&dir).unwrap(), false, 10).unwrap();
        let said = Outcome::PlanterSaysOther { platform_account: "acct-other".into() };
        assert_eq!(outcomes(&r), [("5151", Some("acct-alice"), &said)]);
        assert!(db.account_link(CLIENT, "5151").unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn planters_client_is_registered_first() {
        let db = Db::memory();
        db.account_for("github", "4242", "jake-gh", "acct-jake", 1).unwrap();
        let backup = Backup::read(&fixture()).unwrap();
        let e = run(&db, &backup, false, 10).unwrap_err();
        assert!(format!("{e:#}").contains("isn't a registered oidc client"), "{e:#}");
        let r = run(&db, &backup, true, 10).unwrap();
        assert!(!r.client_registered && r.render(&backup).contains("warning"));
        assert!(rows(&db).is_empty());
    }
}
