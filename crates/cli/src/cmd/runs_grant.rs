//! `arugula runs grant` (`cmd::runs` has the rest of `arugula runs`): a project's standing grant for runs on Arugula
//! control (M211, "One standing grant per project starts runs"). `show` says
//! whether the project has one and its past ones. An owner signs and
//! revokes one on control's page, from a browser or a phone: the platform
//! may then start and instruct runs on the project's hosted workspaces,
//! under the project's delegation, with every device asleep. This CLI
//! can't: its key is a file an agent on this machine can read, so neither
//! the platform nor any hosted machine takes a grant or a revoke it signed.
//! `sign` and `revoke` say so.

use anyhow::bail;
use arugula_e2e::now_ms;
use serde_json::Value;

use super::project::project_id;
use crate::{control::Enrolled, util::print_json};

#[derive(clap::Subcommand)]
pub enum GrantCmd {
    /// Whether the project has a standing grant, and its past ones.
    Show,
    /// Not here: an owner signs it from a browser or a phone, on control's
    /// page (this says so).
    Sign {
        /// How many days it lasts (1 to 366).
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u64).range(1..=366))]
        days: u64,
    },
    /// Not here: an owner revokes it from a browser or a phone (this says so).
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

/// What `sign` and `revoke` say.
const SIGN_ELSEWHERE: &str = "sign or revoke a project's standing grant from a browser or a phone of yours, on control's page \
     (Devices and machines, Runs on hosted workspaces): this CLI's key is a file an agent on this machine can read, so \
     the platform and its hosted machines don't take a grant or a revoke it signed";

pub fn run(cmd: Option<GrantCmd>, which: Which, json_out: bool) -> anyhow::Result<i32> {
    if matches!(cmd, Some(GrantCmd::Sign { .. } | GrantCmd::Revoke { .. })) {
        bail!("{SIGN_ELSEWHERE}");
    }
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
    let view = e.api("GET", &path, None)?;
    match cmd {
        GrantCmd::Show => {
            if json_out {
                print_json(&view);
            } else {
                print!("{}", shown(&view));
            }
        }
        GrantCmd::Sign { .. } | GrantCmd::Revoke { .. } => bail!("{SIGN_ELSEWHERE}"),
    }
    Ok(0)
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
                "It has hosted workspaces: an owner signs one on control's page, from a browser or a phone.\n",
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
            out.push_str("It names a delegation that isn't the project's live one: sign it again on control's page.\n");
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
    use serde_json::json;

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

    /// `sign` and `revoke` say where to do it, and do nothing here.
    #[test]
    fn sign_and_revoke_send_you_to_a_browser() {
        for cmd in [GrantCmd::Sign { days: 30 }, GrantCmd::Revoke { stop: true }] {
            let e = run(Some(cmd), Which { project: None }, false).unwrap_err().to_string();
            assert!(e.contains("from a browser or a phone"), "{e}");
        }
    }

    #[test]
    fn what_show_says() {
        let none = json!({ "grant": null, "past": [], "offer": true });
        assert!(shown(&none).contains("on control's page"));
        let g = json!({ "id": "abc", "by": "dev1", "made": 1, "expires": null });
        let stale = json!({ "grant": { "grant": g }, "past": [], "stale": true });
        assert!(shown(&stale).contains("sign it again"));
        let live = json!({ "grant": { "grant": g }, "past": [{ "grant": { "id": "old" }, "revoke": { "running": "stop" }, "revoked_at": 1 }] });
        let s = shown(&live);
        assert!(s.contains("Standing grant abc") && s.contains("device dev1") && s.contains("until revoked"), "{s}");
        assert!(s.contains("grant old, runs stopped"), "{s}");
    }
}
