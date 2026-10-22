//! `arugula runs grant` (`cmd::runs` has the rest of `arugula runs`): a project's standing grant for runs on Arugula
//! control (M211, "One standing grant per project starts runs"). An owner
//! signs it with this CLI's device key, as a browser signs it on control's
//! page: the platform may start and instruct runs on the project's hosted
//! workspaces, under the project's delegation, with every device asleep.
//! Each hosted machine checks the signature. A grant lasts 90 days unless
//! `--days` says otherwise (at most 366). `revoke` signs a revoke of every
//! grant the project has under its delegation, made by now: no new runs,
//! and with `--stop` the ones going stop too.
//!
//! An agent (CLAUDECODE or AI_AGENT set) can see the grant but not sign or
//! revoke one: this CLI signs as your device, and agents never approve.

use anyhow::bail;
use arugula_e2e::{
    hosted::Delegation,
    now_ms,
    project_grant::{GrantRevoke, ProjectGrant, Running},
};
use serde_json::{Value, json};

use super::project::project_id;
use crate::{control::Enrolled, http, util::print_json};

#[derive(clap::Subcommand)]
pub enum GrantCmd {
    /// Whether the project has a standing grant, and its past ones.
    Show,
    /// Sign the project's standing grant with this CLI's device.
    ///
    /// The platform may then start and instruct runs on every hosted
    /// workspace of the project, running whatever command the project's
    /// runs start, until it expires or is revoked. Deploys still ask every
    /// time.
    Sign {
        /// How many days it lasts (1 to 366).
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u64).range(1..=366))]
        days: u64,
    },
    /// Revoke it: no new runs. The ones going finish, unless `--stop`.
    Revoke {
        /// Stop the runs going too.
        #[arg(long)]
        stop: bool,
    },
}

#[derive(clap::Args)]
pub struct Which {
    /// The project: its id (prj_…) or name. Default: your personal one.
    #[arg(long, global = true)]
    project: Option<String>,
}

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

pub fn run(cmd: Option<GrantCmd>, which: Which, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let project = match &which.project {
        Some(p) if p.starts_with("prj_") => p.clone(),
        Some(p) => project_id(&e.api("GET", "/api/projects", None)?, p)?,
        None => "personal".into(),
    };
    let path = format!("/api/projects/{project}/runs/grant");
    let cmd = cmd.unwrap_or(GrantCmd::Show);
    if !matches!(cmd, GrantCmd::Show) && http::agent() {
        bail!(
            "an agent doesn't sign or revoke a project's standing grant: this CLI signs as your device, \
             and agents never approve. Ask the person to run it, or to use control's page"
        );
    }
    let view = e.api("GET", &path, None)?;
    match cmd {
        GrantCmd::Show => {
            if json_out {
                print_json(&view);
            } else {
                print!("{}", shown(&view));
            }
        }
        GrantCmd::Sign { days } => {
            if !view["grant"].is_null() && view["stale"] != true {
                bail!("this project has a standing grant: `arugula runs grant revoke` it before signing another");
            }
            let project = view["project"].as_str().unwrap_or(&project).to_owned();
            let d = delegation(&e, &project, view["owner"].as_str().unwrap_or_default())?;
            let now = now_ms();
            let mut g = ProjectGrant::new(&d, now, Some(now + days * DAY_MS));
            g.sign_with(e.keys());
            let kept = e.api("POST", &path, Some(&json!({ "grant": g })))?;
            if json_out {
                print_json(&kept);
            } else {
                println!(
                    "Signed the standing grant {}: the platform may start and instruct runs on every hosted workspace of \
                     the project for {days} days, unless it's revoked.",
                    g.id
                );
            }
        }
        GrantCmd::Revoke { stop } => {
            // Under the project's live delegation, as the hub says, and the
            // one the platform's live grant names if that's another: a
            // revoke names no grant, so the platform can't point it
            // elsewhere.
            let project = view["project"].as_str().unwrap_or(&project).to_owned();
            let owner = view["owner"].as_str().unwrap_or_default();
            let mut delegations: Vec<String> = delegation(&e, &project, owner).map(|d| d.id()).into_iter().collect();
            if let Some(d) = view["grant"]["grant"]["delegation"].as_str()
                && !delegations.iter().any(|x| x == d)
            {
                delegations.push(d.to_owned());
            }
            if delegations.is_empty() {
                bail!("this project has no delegation and no standing grant to revoke");
            }
            let running = if stop { Running::Stop } else { Running::Finish };
            let at = now_ms();
            let (mut revoked, mut told, mut last) = (0, 0, Value::Null);
            for d in &delegations {
                let r = GrantRevoke::new(&project, d, running, at, e.keys());
                match e.api("POST", &format!("{path}/revoke"), Some(&json!({ "revoke": r }))) {
                    Ok(done) => {
                        revoked += done["revoked"].as_u64().unwrap_or(0);
                        told += done["told"].as_u64().unwrap_or(0);
                        last = done;
                    }
                    // It covered no grant there: nothing to revoke under it.
                    Err(err) if format!("{err:#}").contains("covers no grant") => {}
                    Err(err) => return Err(err),
                }
            }
            if revoked == 0 {
                bail!("this project has no standing grant to revoke");
            }
            if json_out {
                print_json(&last);
            } else {
                let what = if stop { "; the runs going are to stop" } else { "; the runs going finish" };
                println!(
                    "Revoked the project's standing grant: no new runs{what} ({told} hosted machine(s) told now)."
                );
            }
        }
    }
    Ok(0)
}

/// The project's live delegation for hosted machines, from control (the
/// hub keeps them, M178): what the grant names.
fn delegation(e: &Enrolled, project: &str, owner: &str) -> anyhow::Result<Delegation> {
    let listed = e.api("GET", "/api/hosted/delegations", None)?;
    let key = listed["key"].as_str().unwrap_or_default();
    let live = listed["delegations"].as_array().into_iter().flatten().find(|l| {
        let d = &l["delegation"];
        d["project"] == project && d["owner"] == owner && d["key"] == key && l["revoke"].is_null()
    });
    match live {
        Some(l) => Ok(serde_json::from_value(l["delegation"].clone())?),
        None => bail!(
            "this project has no delegation for hosted machines: an owner allows them first (Devices and machines, \
             Hosted machines, on control's page)"
        ),
    }
}

fn day(ms: u64) -> String {
    crate::hosts::ago(ms)
}

/// A time to come, as days from now.
fn when(ms: u64) -> String {
    let days = ms.saturating_sub(now_ms()).div_ceil(DAY_MS);
    if days == 1 { "tomorrow".into() } else { format!("{days} days from now") }
}

/// The grant, in a few lines.
fn shown(v: &Value) -> String {
    let mut out = String::new();
    let g = &v["grant"]["grant"];
    if g.is_null() {
        out.push_str("No standing grant: runs don't start on this project's hosted workspaces.\n");
        if v["offer"] == true {
            out.push_str(
                "It has hosted workspaces: an owner signs one with `arugula runs grant sign`, or on control's page.\n",
            );
        }
    } else {
        let expires =
            g["expires"].as_u64().map_or_else(|| "until revoked".to_owned(), |e| format!("until {}", when(e)));
        out.push_str(&format!(
            "Standing grant {}: the platform may start and instruct runs (any command the project's runs start) on \
             every hosted workspace of the project, {expires}.\nSigned by device {} {}.\n",
            g["id"].as_str().unwrap_or_default(),
            g["by"].as_str().unwrap_or_default(),
            g["made"].as_u64().map(day).unwrap_or_default(),
        ));
        if v["stale"] == true {
            out.push_str(
                "It names a delegation that isn't the project's live one: sign it again (`arugula runs grant sign`).\n",
            );
        } else if v["renew"] == true {
            out.push_str("It expires soon: revoke it and sign it again to keep runs starting.\n");
        }
    }
    for p in v["past"].as_array().into_iter().flatten() {
        let id = p["grant"]["id"].as_str().unwrap_or_default();
        if let Some(at) = p["revoked_at"].as_u64() {
            let stop = if p["revoke"]["running"] == "stop" { ", runs stopped" } else { "" };
            out.push_str(&format!("Revoked {}: grant {id}{stop}\n", day(at)));
        } else if let Some(at) = p["superseded_at"].as_u64() {
            out.push_str(&format!("Replaced {}: grant {id}\n", day(at)));
        } else {
            out.push_str(&format!("Expired: grant {id}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn the_commands_parse() {
        for argv in [
            &["arugula", "runs", "grant"][..],
            &["arugula", "runs", "grant", "show", "--project", "Acme"],
            &["arugula", "runs", "grant", "--project", "prj_1", "sign", "--days", "30"],
            &["arugula", "runs", "grant", "sign"],
            &["arugula", "runs", "grant", "revoke", "--stop", "--project", "Acme"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
        for argv in [
            &["arugula", "runs", "grant", "sign", "--days", "0"][..],
            &["arugula", "runs", "grant", "sign", "--days", "400"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_err(), "{argv:?}");
        }
    }

    #[test]
    fn what_show_says() {
        let none = json!({ "grant": null, "past": [], "offer": true });
        assert!(shown(&none).contains("arugula runs grant sign"));
        let g = json!({ "id": "abc", "by": "dev1", "made": 1, "expires": null });
        let stale = json!({ "grant": { "grant": g }, "past": [], "stale": true });
        assert!(shown(&stale).contains("sign it again"));
        let live = json!({ "grant": { "grant": g }, "past": [{ "grant": { "id": "old" }, "revoke": { "running": "stop" }, "revoked_at": 1 }] });
        let s = shown(&live);
        assert!(s.contains("Standing grant abc") && s.contains("device dev1") && s.contains("until revoked"), "{s}");
        assert!(s.contains("grant old, runs stopped"), "{s}");
    }
}
