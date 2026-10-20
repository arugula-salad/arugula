//! `arugula grants`: what your machines may do with your things on Arugula
//! control (M130). For now one power, `cloud.drive`: the agents on a machine
//! drive your Cloud workspaces. Every machine you join gets it; revoke it
//! from one, and give it back, here. Through control, as this CLI's
//! account (`arugula login`).

use anyhow::bail;
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum GrantsCmd {
    /// Your machines' grants, live and revoked.
    Ls,
    /// Take a machine's cloud.drive grant back: its agents can't drive your
    /// Cloud workspaces until it's restored.
    Revoke {
        /// The machine: its name or id.
        machine: String,
    },
    /// Give a machine cloud.drive again.
    Restore {
        /// The machine: its name or id.
        machine: String,
    },
}

pub fn run(cmd: Option<GrantsCmd>, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let listed = e.api("GET", "/api/grants", None)?;
    let grants = listed["grants"].as_array().cloned().unwrap_or_default();
    match cmd.unwrap_or(GrantsCmd::Ls) {
        GrantsCmd::Ls => {
            if json_out {
                print_json(&listed);
            } else if grants.is_empty() {
                println!("No grants.");
            } else {
                for g in &grants {
                    println!("{}", line(g));
                }
            }
        }
        GrantsCmd::Revoke { machine } => {
            let id = machine_id(&grants, &machine)?;
            let live: Vec<&Value> = grants.iter().filter(|g| g["subject"]["id"] == id && g["live"] == true).collect();
            if live.is_empty() {
                println!("{machine} has no live grant.");
            }
            for g in live {
                let path = format!("/api/grants/{}/revoke", g["id"].as_str().unwrap_or_default());
                let done = e.api("POST", &path, None)?;
                if json_out { print_json(&done) } else { println!("{}", line(&done)) }
            }
        }
        GrantsCmd::Restore { machine } => {
            let id = machine_id(&grants, &machine)?;
            let done = e.api("POST", "/api/grants", Some(&json!({ "daemon": id, "power": "cloud.drive" })))?;
            if json_out { print_json(&done) } else { println!("{}", line(&done)) }
        }
    }
    Ok(0)
}

/// `geek  cloud.drive  live  grt_…`, or `revoked` and when.
fn line(g: &Value) -> String {
    let name = g["subject"]["name"].as_str().or(g["subject"]["id"].as_str()).unwrap_or_default();
    let state = match g["revoked_at"].as_u64() {
        Some(at) => format!("revoked {}", crate::hosts::ago(at)),
        None => "live".into(),
    };
    format!(
        "{name:<20} {:<12} {state:<16} {}",
        g["power"].as_str().unwrap_or_default(),
        g["id"].as_str().unwrap_or_default()
    )
}

/// A machine named on the command line, by its name or id, among those
/// with grants (every machine on the account has had one since it joined).
fn machine_id(grants: &[Value], machine: &str) -> anyhow::Result<String> {
    let mut ids: Vec<String> = grants
        .iter()
        .filter(|g| g["subject"]["id"] == machine || g["subject"]["name"] == machine)
        .filter_map(|g| g["subject"]["id"].as_str().map(String::from))
        .collect();
    ids.dedup();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("no machine on this account is called {machine} (`arugula grants` lists them)"),
        _ => bail!("more than one machine is called {machine}: name one by its id ({})", ids.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn a_machine_is_named_by_its_name_or_id_once() {
        let g = |id: &str, name: &str, live: bool| json!({ "subject": { "kind": "daemon", "id": id, "name": name }, "live": live });
        let grants = [g("d1", "geek", true), g("d1", "geek", false), g("d2", "box", true), g("d3", "box", true)];
        assert_eq!(machine_id(&grants, "geek").unwrap(), "d1");
        assert_eq!(machine_id(&grants, "d2").unwrap(), "d2");
        assert!(machine_id(&grants, "box").unwrap_err().to_string().contains("d2, d3"));
        assert!(machine_id(&grants, "mini").is_err());
        assert!(crate::Cli::try_parse_from(["arugula", "grants", "revoke", "geek"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "grants"]).is_ok());
    }
}
