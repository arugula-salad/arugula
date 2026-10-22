//! `arugula grants`: what your machines may do with your things on Arugula
//! control (M130). `cloud.drive`: the agents on a machine drive your Cloud
//! workspaces; every machine you join gets it, and you revoke it from one,
//! and give it back, here. And (M197) what its agents may do in
//! conversations: `threads.read` and `threads.post` in a project's threads
//! (a team's channels are its project's) or your DMs, and `issues.write` on
//! a project's issues. Those are off until you `add` them, and only over
//! what you can read yourself. Through control, as this CLI's account
//! (`arugula login`).

use anyhow::bail;
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

/// The powers an agent gets over a scope (M197).
const AGENT_POWERS: [&str; 3] = ["threads.read", "threads.post", "issues.write"];

/// Where an agent power reaches: one of the two.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(multiple = false)]
pub struct Scope {
    /// A project, by its name or id (a team's channels are its project's).
    #[arg(long)]
    project: Option<String>,
    /// Your DMs.
    #[arg(long)]
    dms: bool,
}

#[derive(clap::Subcommand)]
pub enum GrantsCmd {
    /// Your machines' grants, live and revoked.
    Ls,
    /// Give a machine's agents a power over a project or your DMs.
    ///
    /// The powers are threads.read, threads.post or issues.write over a
    /// project (`--project`) or your DMs (`--dms`), or cloud.drive again.
    /// Every room it reaches says whose agents read and post there, and what
    /// they write is labelled as your agent's on that machine.
    Add {
        /// The machine: its name or id.
        machine: String,
        /// One or more of threads.read, threads.post, issues.write (or
        /// cloud.drive, with no scope).
        #[arg(required = true)]
        powers: Vec<String>,
        #[command(flatten)]
        scope: Scope,
    },
    /// Take a machine's grants back: all its live ones, or one power, over
    /// one scope.
    ///
    /// Its agents are refused from their next call.
    Revoke {
        /// The machine: its name or id.
        machine: String,
        /// Only this power.
        power: Option<String>,
        #[command(flatten)]
        scope: Scope,
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
    let show = |g: &Value| if json_out { print_json(g) } else { println!("{}", line(g)) };
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
        GrantsCmd::Add { machine, powers, scope } => {
            let id = machine_id(&grants, &machine)?;
            let scope = scope_of(&e, &scope)?;
            for power in &powers {
                let body = give(&id, power, scope.as_ref())?;
                show(&e.api("POST", "/api/grants", Some(&body))?);
            }
        }
        GrantsCmd::Revoke { machine, power, scope } => {
            let id = machine_id(&grants, &machine)?;
            let scope = scope_of(&e, &scope)?;
            let live: Vec<&Value> = grants
                .iter()
                .filter(|g| g["subject"]["id"] == id && g["live"] == true)
                .filter(|g| {
                    power.as_ref().is_none_or(|p| g["power"] == p.as_str()) && scope.as_ref().is_none_or(|s| over(g, s))
                })
                .collect();
            if live.is_empty() {
                println!("{machine} has no live grant like that.");
            }
            for g in live {
                let path = format!("/api/grants/{}/revoke", g["id"].as_str().unwrap_or_default());
                show(&e.api("POST", &path, None)?);
            }
        }
        GrantsCmd::Restore { machine } => {
            let id = machine_id(&grants, &machine)?;
            show(&e.api("POST", "/api/grants", Some(&json!({ "daemon": id, "power": "cloud.drive" })))?);
        }
    }
    Ok(0)
}

/// `POST /api/grants`'s body: an agent power needs a scope, cloud.drive
/// takes none.
fn give(daemon: &str, power: &str, scope: Option<&Value>) -> anyhow::Result<Value> {
    match (power, scope) {
        ("cloud.drive", None) => Ok(json!({ "daemon": daemon, "power": power })),
        ("cloud.drive", Some(_)) => bail!("cloud.drive is over your account: no --project or --dms"),
        (p, Some(s)) if AGENT_POWERS.contains(&p) => Ok(json!({ "daemon": daemon, "power": power, "scope": s })),
        (p, None) if AGENT_POWERS.contains(&p) => {
            bail!("{p} is over a project or your DMs: add --project NAME or --dms")
        }
        (p, _) => bail!("there's no power called {p}: threads.read, threads.post, issues.write or cloud.drive"),
    }
}

/// The scope asked for, as `POST /api/grants` takes it: a project named by
/// its name or id among yours (`GET /api/projects`), or your DMs.
fn scope_of(e: &Enrolled, s: &Scope) -> anyhow::Result<Option<Value>> {
    if s.dms {
        return Ok(Some(json!({ "kind": "account" })));
    }
    let Some(want) = &s.project else { return Ok(None) };
    let projects = e.api("GET", "/api/projects", None)?;
    Ok(Some(json!({ "kind": "project", "id": project_id(&projects, want)? })))
}

/// A project named on the command line, by its id or name (any case).
fn project_id(projects: &Value, want: &str) -> anyhow::Result<String> {
    let all = projects["projects"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = all
        .iter()
        .filter(|p| p["id"] == want || p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(want)))
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("you have no project called {want} (the platform page lists yours)"),
        _ => bail!("more than one project is called {want}: name one by its id ({})", ids.join(", ")),
    }
}

/// Whether a grant is over this scope.
fn over(g: &Value, scope: &Value) -> bool {
    match scope["kind"].as_str() {
        Some("account") => g["scope"]["kind"] == "account" && g["power"] != "cloud.drive",
        _ => g["scope"]["kind"] == "project" && g["scope"]["id"] == scope["id"],
    }
}

/// `geek  threads.post  project acme  live  grt_…`, or `revoked` and when.
fn line(g: &Value) -> String {
    let name = g["subject"]["name"].as_str().or(g["subject"]["id"].as_str()).unwrap_or_default();
    let state = match g["revoked_at"].as_u64() {
        Some(at) => format!("revoked {}", crate::hosts::ago(at)),
        None => "live".into(),
    };
    let scope = match (g["scope"]["kind"].as_str(), g["power"].as_str()) {
        (Some("project"), _) => {
            format!("project {}", g["scope"]["name"].as_str().or(g["scope"]["id"].as_str()).unwrap_or_default())
        }
        (_, Some("cloud.drive")) => "your account".into(),
        _ => "your DMs".into(),
    };
    format!(
        "{name:<20} {:<13} {scope:<24} {state:<16} {}",
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
    ids.sort();
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

    /// `add` takes one or more powers and one scope; an agent power needs
    /// one, cloud.drive none.
    #[test]
    fn an_agent_power_is_given_over_a_project_or_the_dms() {
        let parse = |args: &[&str]| crate::Cli::try_parse_from([&["arugula", "grants"], args].concat());
        assert!(parse(&["add", "geek", "threads.read", "threads.post", "--project", "acme"]).is_ok());
        assert!(parse(&["add", "geek", "threads.read", "--dms"]).is_ok());
        assert!(parse(&["add", "geek", "threads.read", "--dms", "--project", "acme"]).is_err(), "one scope");
        assert!(parse(&["add", "geek"]).is_err(), "a power");
        assert!(parse(&["revoke", "geek", "threads.post", "--project", "acme"]).is_ok());

        let dms = json!({ "kind": "account" });
        assert_eq!(give("d1", "threads.post", Some(&dms)).unwrap()["scope"], dms);
        assert_eq!(give("d1", "cloud.drive", None).unwrap(), json!({ "daemon": "d1", "power": "cloud.drive" }));
        assert!(give("d1", "threads.post", None).unwrap_err().to_string().contains("--project NAME or --dms"));
        assert!(give("d1", "cloud.drive", Some(&dms)).is_err());
        assert!(give("d1", "pane.drive", Some(&dms)).is_err());

        let projects = json!({ "projects": [
            { "id": "prj_1", "name": "Personal" }, { "id": "prj_2", "name": "Acme" }, { "id": "prj_3", "name": "acme" },
        ] });
        assert_eq!(project_id(&projects, "personal").unwrap(), "prj_1");
        assert_eq!(project_id(&projects, "prj_3").unwrap(), "prj_3");
        assert!(project_id(&projects, "acme").unwrap_err().to_string().contains("prj_2, prj_3"));
        assert!(project_id(&projects, "none").is_err());

        let g = |power: &str, kind: &str, id: &str| json!({ "power": power, "scope": { "kind": kind, "id": id } });
        assert!(over(&g("threads.read", "account", "a"), &dms));
        assert!(!over(&g("cloud.drive", "account", "a"), &dms), "--dms isn't cloud.drive");
        assert!(over(&g("threads.read", "project", "prj_2"), &json!({ "kind": "project", "id": "prj_2" })));
        assert!(!over(&g("threads.read", "project", "prj_3"), &json!({ "kind": "project", "id": "prj_2" })));
        assert!(
            line(&json!({ "subject": { "name": "geek" }, "power": "threads.post",
            "scope": { "kind": "project", "id": "prj_2", "name": "Acme" }, "id": "grt_1" }))
            .contains("project Acme")
        );
    }
}
