//! `arugula-control account-reset`: support's reset of an account's
//! devices (aru-119, #1582), for a person none of whose devices can
//! approve and who has no recovery code left. Support checks who's asking
//! first, by its own means; this is the step after.
//!
//! Without `--yes` it only says what it would do. With it, in one
//! transaction (`Db::reset_account_devices`): every device and revocation
//! of the account is archived in `account_resets` with who ran it and why,
//! and goes from the account; its root is cleared. The next browser the
//! person signs in with enrolls as the account's first device and gets new
//! recovery codes. Their machines keep their pin of the old root, so
//! control drops them, and each is joined again on the machine.
//!
//! ```text
//! fly ssh console -a illogical-control -C "/arugula-control account-reset --account ACCOUNT --by NAME --why 'ticket 123: lost every device' --db /data/control.db"
//! ```
//!
//! At most one reset an account a week, unless `--again`.

use std::path::PathBuf;

use anyhow::Context as _;
use arugula_e2e::Cert;
use clap::Parser;

use crate::db::Db;

/// How long after a reset another is refused without `--again`.
pub const AGAIN_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Parser, Debug)]
#[command(
    name = "arugula-control account-reset",
    about = "Reset an account's devices so its next browser enrolls as the first (aru-119)"
)]
pub struct Args {
    /// Control's database: the hub's file, never a new one.
    #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
    /// The account's id.
    #[arg(long)]
    account: String,
    /// Who runs it, for the record.
    #[arg(long)]
    by: String,
    /// Why, for the record: the ticket and how support checked who asked.
    #[arg(long)]
    why: String,
    /// Do it. Without this, only say what it would do.
    #[arg(long)]
    yes: bool,
    /// Reset even though the account was reset in the last week.
    #[arg(long)]
    again: bool,
}

/// What a reset would take away, for the operator to read first.
pub fn plan(db: &Db, account: &str) -> anyhow::Result<String> {
    let a = db.account(account)?.with_context(|| format!("no account {account}"))?;
    let (certs, waiting) = db.devices(account)?;
    let revoked = db.revocations(account)?.len();
    let mut out = format!("Account {account}, root {}\n", a.root.as_deref().unwrap_or("none"));
    out += &format!("Devices it loses ({}):\n", certs.len() + waiting.len());
    let line = |c: &Cert, note: &str| format!("  {}  {:<8} {}{note}\n", c.device, c.kind.as_str(), c.name);
    for c in &certs {
        out += &line(c, "");
    }
    for c in &waiting {
        out += &line(c, "  (waiting)");
    }
    out += &format!("Revocations archived: {revoked}\n");
    out += "Its machines are dropped until each is joined again on the machine.\n";
    Ok(out)
}

/// `arugula-control account-reset --account ID --by NAME --why TEXT [--yes] [--again] [--db PATH]`.
pub fn run(args: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control account-reset".to_owned()).chain(args));
    anyhow::ensure!(a.db.is_file(), "no database at {}", a.db.display());
    anyhow::ensure!(!a.by.trim().is_empty() && !a.why.trim().is_empty(), "--by and --why are for the record: say both");
    let db = Db::open(&a.db)?;
    print!("{}", plan(&db, &a.account)?);
    let now = arugula_e2e::now_ms();
    if let Some((at, by)) = db.last_reset(&a.account)?
        && now.saturating_sub(at) < AGAIN_AFTER_MS
        && !a.again
    {
        anyhow::bail!("{by} reset this account less than a week ago: --again to do it anyway");
    }
    if !a.yes {
        println!("Nothing changed: --yes to reset.");
        return Ok(());
    }
    anyhow::ensure!(db.reset_account_devices(&a.account, &a.by, &a.why, now)?, "no account {}", a.account);
    println!("Reset. The next browser {} signs in with enrolls as the first device.", a.account);
    Ok(())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{DeviceKeys, Kind, Revocation};

    use super::*;

    /// An account with a root browser, a second browser it approved (later
    /// revoked) and a machine.
    fn account(db: &Db) -> (Cert, Cert, Cert) {
        db.account_for("github", "1", "lex00", "acct", 1).unwrap();
        let rk = DeviceKeys::generate();
        let mut root = Cert::new(&rk, "acct", Kind::Browser, "Chrome on Mac");
        root.sign_with(&rk);
        // Signed by itself and approved: the account's root.
        db.put_device(&root, true, 1).unwrap();
        let mut safari = Cert::new(&DeviceKeys::generate(), "acct", Kind::Browser, "Safari on Mac");
        safari.sign_with(&rk);
        db.put_device(&safari, true, 2).unwrap();
        let mut mac = Cert::new(&DeviceKeys::generate(), "acct", Kind::Daemon, "MacBook-Pro.local");
        mac.sign_with(&rk);
        db.put_device(&mac, true, 3).unwrap();
        db.add_revocation(&Revocation::new("acct", &safari.device, &rk)).unwrap();
        (root, safari, mac)
    }

    #[test]
    fn a_reset_archives_every_device_and_clears_the_root() {
        let db = Db::memory();
        let (root, safari, mac) = account(&db);
        assert_eq!(db.account("acct").unwrap().unwrap().root.as_deref(), Some(root.device.as_str()));
        let said = plan(&db, "acct").unwrap();
        for c in [&root, &safari, &mac] {
            assert!(said.contains(&c.device), "{said}");
        }
        assert!(said.contains("Revocations archived: 1"), "{said}");

        assert!(db.reset_account_devices("acct", "support", "ticket 1", 10).unwrap());
        assert_eq!(db.account("acct").unwrap().unwrap().root, None);
        let (certs, waiting) = db.devices("acct").unwrap();
        assert!(certs.is_empty() && waiting.is_empty());
        assert!(db.revocations("acct").unwrap().is_empty());
        assert_eq!(db.last_reset("acct").unwrap(), Some((10, "support".to_owned())));

        // The archive holds the certificates and revocations as they were.
        let (devices, revocations): (String, String) = db
            .conn()
            .query_row("SELECT devices, revocations FROM account_resets WHERE account = 'acct'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        let kept: Vec<Cert> = serde_json::from_str(&devices).unwrap();
        assert_eq!(kept.iter().map(|c| c.device.clone()).collect::<Vec<_>>(), [root.device, safari.device, mac.device]);
        assert_eq!(serde_json::from_str::<Vec<Revocation>>(&revocations).unwrap().len(), 1);
    }

    /// Over HTTP: before the reset a new browser waits for an approval
    /// nobody can give; after it, a browser that signs its own certificate
    /// is the account's first device, and the machine is no longer one of
    /// its devices (it joins again).
    #[tokio::test]
    async fn after_a_reset_the_next_browser_is_the_first_device() {
        use crate::routing_wire::{control, daemon, person, session};
        let c = control(|_| {}).await;
        person(&c.app, "lex00", "a1");
        let mac = daemon(&c.app, "a1", "MacBook-Pro.local");
        let cookie = session(&c.app, "a1");
        let fresh = |name: &str| {
            let k = DeviceKeys::generate();
            let mut cert = Cert::new(&k, "a1", Kind::Browser, name);
            cert.sign_with(&k);
            cert
        };

        let stranded = fresh("Chrome on Mac");
        let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(serde_json::json!({ "cert": stranded }))).await;
        assert_eq!((st, &v["approved"]), (200, &serde_json::json!(false)), "{v}");

        assert!(c.app.db.reset_account_devices("a1", "support", "ticket 4", arugula_e2e::now_ms()).unwrap());
        assert!(c.app.db.device("a1", &mac.id()).unwrap().is_none());

        let first = fresh("Chrome on Mac");
        let (st, v) = c.as_person(&cookie, "POST", "/api/devices", Some(serde_json::json!({ "cert": first }))).await;
        assert_eq!(
            (st, &v["approved"], v["root"].as_str()),
            (200, &serde_json::json!(true), Some(first.device.as_str())),
            "{v}"
        );
        assert_eq!(c.app.db.account("a1").unwrap().unwrap().root, Some(first.device.clone()));
    }

    #[test]
    fn no_such_account_resets_nothing() {
        let db = Db::memory();
        assert!(!db.reset_account_devices("nobody", "support", "ticket 2", 10).unwrap());
        assert!(plan(&db, "nobody").is_err());
    }

    #[test]
    fn deleting_the_account_takes_its_resets() {
        let db = Db::memory();
        account(&db);
        db.reset_account_devices("acct", "support", "ticket 3", 10).unwrap();
        let n =
            |db: &Db| db.conn().query_row("SELECT COUNT(*) FROM account_resets", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(n(&db), 1);
        db.delete_account(&crate::db::Erase { account: "acct", disband: &[], retain_for: &[] }).unwrap();
        assert_eq!(n(&db), 0);
    }
}
