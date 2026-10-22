//! `arugula runs grant`: a project's standing grant for runs on Arugula
//! control (M211, "One standing grant per project starts runs"). An owner
//! signs it with this CLI's device key, as a browser signs it on control's
//! page: the platform may start and instruct runs on the project's hosted
//! workspaces, under the project's delegation, with every device asleep.
//! Each hosted machine checks the signature. `revoke` signs its revoke:
//! no new runs, and with `--stop` the ones going stop too.
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
pub enum RunsCmd {
    /// The project's standing grant for runs on its hosted workspaces.
    ///
    /// `show` (the default) says whether there is one and who signed it,
    /// `sign` signs one with this CLI's device (an owner, once the project
    /// has a delegation for hosted machines), and `revoke` takes it back.
    Grant {
        #[command(subcommand)]
        cmd: Option<GrantCmd>,
        #[command(flatten)]
        which: Which,
    },
}

#[derive(clap::Subcommand)]
pub enum GrantCmd {
    /// Whether the project has a standing grant, and its past ones.
    Show,
    /// Sign the project's standing grant with this CLI's device.
    ///
    /// The platform may then start and instruct runs on the project's
    /// hosted workspaces, until it's revoked. Deploys still ask every time.
    Sign {
        /// Stop counting after this many days. Default: until revoked.
        #[arg(long)]
        days: Option<u64>,
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

pub fn run(cmd: RunsCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let RunsCmd::Grant { cmd, which } = cmd;
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
            if !view["grant"].is_null() {
                bail!("this project has a standing grant: `arugula runs grant revoke` it before signing another");
            }
            let project = view["project"].as_str().unwrap_or(&project).to_owned();
            let d = delegation(&e, &project, view["owner"].as_str().unwrap_or_default())?;
            let now = now_ms();
            let mut g = ProjectGrant::new(&d, now, days.map(|n| now + n * DAY_MS));
            g.sign_with(e.keys());
            let kept = e.api("POST", &path, Some(&json!({ "grant": g })))?;
            if json_out {
                print_json(&kept);
            } else {
                println!("Signed the standing grant {} for runs on the project's hosted workspaces.", g.id);
            }
        }
        GrantCmd::Revoke { stop } => {
            let Some(g) = view["grant"].get("grant").filter(|g| !g.is_null()) else {
                bail!("this project has no standing grant to revoke");
            };
            let g: ProjectGrant = serde_json::from_value(g.clone())?;
            let running = if stop { Running::Stop } else { Running::Finish };
            let r = GrantRevoke::new(&g, running, now_ms(), e.keys());
            let done = e.api("POST", &format!("{path}/revoke"), Some(&json!({ "revoke": r })))?;
            if json_out {
                print_json(&done);
            } else {
                let told = done["told"].as_u64().unwrap_or(0);
                let what = if stop { "; the runs going are to stop" } else { "; the runs going finish" };
                println!("Revoked the standing grant {}: no new runs{what} ({told} hosted machine(s) told now).", g.id);
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
            g["expires"].as_u64().map_or_else(|| "until revoked".to_owned(), |e| format!("expires {}", day(e)));
        out.push_str(&format!(
            "Standing grant {}: start and instruct runs on the project's hosted workspaces.\nSigned by device {} {}, {expires}.\n",
            g["id"].as_str().unwrap_or_default(),
            g["by"].as_str().unwrap_or_default(),
            g["made"].as_u64().map(day).unwrap_or_default(),
        ));
    }
    for p in v["past"].as_array().into_iter().flatten() {
        let stop = if p["revoke"]["running"] == "stop" { ", runs stopped" } else { "" };
        out.push_str(&format!(
            "Revoked {}: grant {}{stop}\n",
            p["revoked_at"].as_u64().map(day).unwrap_or_default(),
            p["grant"]["id"].as_str().unwrap_or_default()
        ));
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
            &["arugula", "runs", "grant", "revoke", "--stop", "--project", "Acme"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
    }

    #[test]
    fn what_show_says() {
        let none = json!({ "grant": null, "past": [], "offer": true });
        assert!(shown(&none).contains("arugula runs grant sign"));
        let g = json!({ "id": "abc", "by": "dev1", "made": 1, "expires": null });
        let live = json!({ "grant": { "grant": g }, "past": [{ "grant": { "id": "old" }, "revoke": { "running": "stop" }, "revoked_at": 1 }] });
        let s = shown(&live);
        assert!(s.contains("Standing grant abc") && s.contains("device dev1") && s.contains("until revoked"), "{s}");
        assert!(s.contains("grant old, runs stopped"), "{s}");
    }
}
