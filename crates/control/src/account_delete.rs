//! `arugula-control account-delete`: an operator's delete of an account,
//! for one that can't (or won't) delete itself from Settings: a test bot's,
//! or a person who asked support. The same delete as Settings → *Delete
//! account* (`account::delete_account`), run on the hub's database with no
//! server.
//!
//! Without `--yes` it only says what it would do. With it: the account's
//! hub rows go in one transaction (`Db::delete_account`), teammates who'd
//! be told get a notice in the app (no push from here), and the deletion
//! waits in `untold_deletions` until the running hub tells the platform,
//! which erases its part (chat, billing, projects and grants) within
//! `TELL_EVERY`.
//!
//! ```text
//! fly ssh console -a illogical-control -C "/arugula-control account-delete --account ACCOUNT --db /data/control.db"
//! ```
//!
//! What Settings refuses, this refuses too (an owner leaves its teams
//! first). Billing is the platform's, so this can't see a plan: check the
//! account pays for none before `--yes`.

use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;

use crate::{account, db::Db};

#[derive(Parser, Debug)]
#[command(name = "arugula-control account-delete", about = "Delete an account, as Settings → Delete account does")]
pub struct Args {
    /// Control's database: the hub's file, never a new one.
    #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
    /// The account's id.
    #[arg(long)]
    account: String,
    /// Do it. Without this, only say what it would do.
    #[arg(long)]
    yes: bool,
}

/// What a delete would do, for the operator to read first.
pub fn preview(db: &Db, account: &str, p: &account::Plan) -> anyhow::Result<String> {
    let a = db.account(account)?.with_context(|| format!("no account {account}"))?;
    let (certs, waiting) = db.devices(account)?;
    let machines = db.daemons(account)?;
    let mut out = format!("Account {account} ({})\n", p.confirm);
    if !a.name.is_empty() && a.name != p.confirm {
        out += &format!("  name {}\n", a.name);
    }
    out += &format!("Devices: {}, machines: {}\n", certs.len() + waiting.len(), machines.len());
    for m in &machines {
        out += &format!("  machine {}  {}\n", m.id, m.name);
    }
    for (team, name, others) in &p.disband {
        out += &format!("Disbands {name} ({team}), {others} others in it\n");
    }
    for (team, name, _) in &p.tell {
        out += &format!("Leaves {name} ({team}): its owners are told\n");
    }
    for b in &p.blockers {
        out += &format!("Refused: {b}\n");
    }
    out += "Billing is the platform's: check the account pays for no plan first.\n";
    Ok(out)
}

/// Delete it: the hub's rows, the notices, and the deletion the running
/// hub tells the platform of. Returns the machines that went.
pub fn delete(db: &Db, account: &str, p: &account::Plan) -> anyhow::Result<Vec<String>> {
    if let Some(why) = p.blockers.first() {
        anyhow::bail!("refused, as Settings would: {why}");
    }
    let me = db.account(account)?.map(|a| a.name).filter(|n| !n.is_empty()).unwrap_or_else(|| "A member".into());
    let mut told: Vec<(Vec<String>, String, String)> = Vec::new();
    for (team, name, _) in &p.disband {
        if let Some(body) = db.latest_roster(team)?
            && let Ok(r) = serde_json::from_str::<arugula_e2e::team::Roster>(&body)
        {
            let others = r.members.into_iter().map(|m| m.account).filter(|a| a != account).collect();
            told.push((others, format!("{name} was deleted"), "Its founder's account was deleted.".into()));
        }
    }
    for (_, name, owners) in &p.tell {
        told.push((
            owners.clone(),
            format!("{me} left {name}"),
            "Their account was deleted. Remove them from the roster when you next change it.".into(),
        ));
    }
    let disband: Vec<String> = p.disband.iter().map(|(t, _, _)| t.clone()).collect();
    let daemons = db.delete_account(&crate::db::Erase { account, disband: &disband, retain_for: &p.retain_for })?;
    let now = arugula_e2e::now_ms();
    for (to, title, body) in told {
        for a in &to {
            db.add_notice(a, &title, &body, now)?;
        }
    }
    Ok(daemons)
}

/// `arugula-control account-delete --account ID [--yes] [--db PATH]`.
pub fn run(args: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control account-delete".to_owned()).chain(args));
    anyhow::ensure!(a.db.is_file(), "no database at {}", a.db.display());
    let db = Db::open(&a.db)?;
    let p = account::plan(&db, &a.account).with_context(|| format!("no account {}", a.account))?;
    print!("{}", preview(&db, &a.account, &p)?);
    if !a.yes {
        println!("Nothing changed: --yes to delete.");
        return Ok(());
    }
    let machines = delete(&db, &a.account, &p)?;
    println!(
        "Deleted {} ({} machines). The running hub tells the platform within {} s.",
        a.account,
        machines.len(),
        account::TELL_EVERY.as_secs()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{
        Cert, DeviceKeys, Kind,
        team::{Member, Roster, TeamRole},
    };

    use super::*;
    use crate::db::Team;

    fn person(db: &Db, id: &str, login: &str) -> (DeviceKeys, Cert) {
        db.account_for("github", &format!("gh-{id}"), login, id, 1).unwrap();
        let keys = DeviceKeys::generate();
        let mut c = Cert::new(&keys, id, Kind::Browser, "laptop");
        c.sign_with(&keys);
        db.put_device(&c, true, 1).unwrap();
        (keys, c)
    }

    #[test]
    fn an_editor_goes_and_the_owner_hears() {
        let db = Db::memory();
        let (ok, oc) = person(&db, "acct-owner", "owner");
        let (_, bc) = person(&db, "acct-bot", "bot");
        let mut r = Roster {
            v: 1,
            team: "t1".into(),
            name: "Team".into(),
            version: 1,
            at: 1,
            members: vec![
                Member {
                    account: "acct-owner".into(),
                    root: oc.device.clone(),
                    role: TeamRole::Owner,
                    name: "owner".into(),
                },
                Member {
                    account: "acct-bot".into(),
                    root: bc.device.clone(),
                    role: TeamRole::Editor,
                    name: "bot".into(),
                },
            ],
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(&ok);
        let t = Team {
            id: "t1".into(),
            name: "Team".into(),
            founder: "acct-owner".into(),
            founder_root: oc.device.clone(),
            locked: false,
        };
        db.add_team(&t, 1, &serde_json::to_string(&r).unwrap(), 1).unwrap();

        let p = account::plan(&db, "acct-bot").unwrap();
        let said = preview(&db, "acct-bot", &p).unwrap();
        assert!(said.contains("Leaves Team (t1)"), "{said}");
        assert!(db.account("acct-bot").unwrap().is_some(), "a preview changes nothing");

        delete(&db, "acct-bot", &p).unwrap();
        assert!(db.account("acct-bot").unwrap().is_none());
        assert_eq!(db.untold_deletions().unwrap().len(), 1, "the running hub tells the platform");
        let n = db.notices("acct-owner").unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].1, "bot left Team");
    }
}
