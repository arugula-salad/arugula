//! `arugula project`: a project's linked GitHub repos on Arugula control
//! (M163). A member of the project who is an admin or maintainer of the
//! repo on GitHub links it; its PRs and issues then live on the platform.
//! Unlinking stops that at once, and its rows go within a day. Through
//! control, as this CLI's account (`arugula login`).

use anyhow::bail;
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum ProjectCmd {
    /// The project's linked repos, and those unlinked and waiting to be
    /// purged.
    Ls {
        #[command(flatten)]
        which: Which,
    },
    /// Link a GitHub repo to the project.
    ///
    /// You need admin or maintain on it on GitHub, and Arugula's GitHub App
    /// installed where it lives: if it isn't, this says where to install it.
    Link {
        /// OWNER/NAME.
        repo: String,
        #[command(flatten)]
        which: Which,
    },
    /// Unlink a GitHub repo from the project.
    ///
    /// Nothing more is read from GitHub for it, and its rows go within a
    /// day unless it's linked again first.
    Unlink {
        /// OWNER/NAME.
        repo: String,
        #[command(flatten)]
        which: Which,
    },
}

#[derive(clap::Args)]
pub struct Which {
    /// The project: its id (prj_…) or name. Default: your personal one.
    #[arg(long)]
    project: Option<String>,
}

pub fn run(cmd: ProjectCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let project = |w: &Which| -> anyhow::Result<String> {
        match &w.project {
            None => Ok("personal".into()),
            Some(p) if p.starts_with("prj_") => Ok(p.clone()),
            Some(p) => project_id(&e.api("GET", "/api/projects", None)?, p),
        }
    };
    match cmd {
        ProjectCmd::Ls { which } => {
            let listed = e.api("GET", &format!("/api/projects/{}/repos", project(&which)?), None)?;
            let repos = listed["repos"].as_array().cloned().unwrap_or_default();
            if json_out {
                print_json(&listed);
            } else if repos.is_empty() {
                println!("No linked repos.");
            } else {
                for r in &repos {
                    println!("{}", line(r));
                }
            }
        }
        ProjectCmd::Link { repo, which } => {
            let path = format!("/api/projects/{}/repos", project(&which)?);
            let done = e.api("POST", &path, Some(&json!({ "repo": repo })))?;
            if json_out { print_json(&done) } else { println!("{}", line(&done)) }
        }
        ProjectCmd::Unlink { repo, which } => {
            let Some((owner, name)) = repo.split_once('/') else { bail!("repo: OWNER/NAME") };
            let path = format!("/api/projects/{}/repos/{owner}/{name}", project(&which)?);
            let done = e.api("DELETE", &path, None)?;
            if json_out { print_json(&done) } else { println!("{}", line(&done)) }
        }
    }
    Ok(0)
}

/// `acme/app  private  linked 2h ago`, or `unlinked`, and when it's purged.
fn line(r: &Value) -> String {
    let name = r["full_name"].as_str().unwrap_or_default();
    let private = if r["private"] == true { "private" } else { "public" };
    let state = match (r["unlinked_at"].as_u64(), r["purge_after"].as_u64()) {
        (Some(at), Some(purge)) => format!("unlinked {}, purged within {}", crate::hosts::ago(at), within(purge)),
        _ => format!("linked {}", r["linked_at"].as_u64().map(crate::hosts::ago).unwrap_or_default()),
    };
    format!("{name:<40} {private:<8} {state}")
}

/// How long until `ms`, roughly: `5h`, `20m`, or `0m` once it's past.
fn within(ms: u64) -> String {
    let left = ms.saturating_sub(arugula_e2e::now_ms()) / 60_000;
    if left >= 60 { format!("{}h", left.div_ceil(60)) } else { format!("{left}m") }
}

/// A project named on the command line, by its name (any case), among
/// the account's.
pub(super) fn project_id(listed: &Value, name: &str) -> anyhow::Result<String> {
    let ids: Vec<String> = listed["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("you have no project called {name}"),
        _ => bail!("more than one of your projects is called {name}: name one by its id ({})", ids.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn a_project_is_named_by_its_name_or_id() {
        let listed = json!({ "projects": [
            { "id": "prj_a", "name": "Personal" }, { "id": "prj_b", "name": "Acme" }, { "id": "prj_c", "name": "dup" },
            { "id": "prj_d", "name": "Dup" } ] });
        assert_eq!(project_id(&listed, "acme").unwrap(), "prj_b");
        assert!(project_id(&listed, "dup").unwrap_err().to_string().contains("prj_c, prj_d"));
        assert!(project_id(&listed, "nope").is_err());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "link", "acme/app"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "link", "acme/app", "--project", "Acme"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "unlink", "acme/app"]).is_ok());
        assert!(crate::Cli::try_parse_from(["arugula", "project", "ls"]).is_ok());
    }

    #[test]
    fn lines() {
        let r = json!({ "full_name": "acme/app", "private": true, "linked_at": 0 });
        assert!(line(&r).starts_with("acme/app"), "{}", line(&r));
        assert!(line(&r).contains("private"));
        let r =
            json!({ "full_name": "acme/app", "private": false, "linked_at": 0, "unlinked_at": 1, "purge_after": 2 });
        assert!(line(&r).contains("unlinked"), "{}", line(&r));
    }
}
