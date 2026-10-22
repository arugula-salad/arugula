//! `arugula cloud`: a project's hosted workspaces on Arugula control
//! (M179). Each is `arugulad` in a box at the platform's compute provider,
//! in a project: it enrolls into the project and shows in its machines.
//! Through control, as this CLI's account (`arugula login`).

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum CloudCmd {
    /// Make a hosted workspace.
    ///
    /// Its box is made from the current release's template and enrolls
    /// into the project. Needs owner or editor on the project.
    Create {
        /// Its name in the project: letters, digits, `-`, `_` and `.`.
        name: String,
        /// An HTTPS clone URL to check out. Repeatable.
        #[arg(long = "repo")]
        repos: Vec<String>,
        /// A package to install. Repeatable.
        #[arg(long = "package")]
        packages: Vec<String>,
        /// A setup script to run after both: a file, or `-` for stdin.
        #[arg(long)]
        setup: Option<String>,
        /// The name of an env var it takes from its secrets. Repeatable.
        #[arg(long = "env")]
        env: Vec<String>,
        /// Wait until it's ready (or failed).
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// The project's hosted workspaces (every project's, without
    /// `--project`).
    #[command(visible_alias = "list")]
    Ls {
        #[arg(long)]
        project: Option<String>,
    },
    /// Open a hosted workspace: wakes it if it's paused, and says its
    /// machine.
    Open {
        /// Its id (hw_…) or name.
        workspace: String,
        /// Wait until it's ready (or failed).
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// Make a hosted workspace's box again from its spec.
    ///
    /// Anything on its disk that isn't pushed is lost.
    Rebuild {
        workspace: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a hosted workspace and its box.
    #[command(visible_alias = "rm")]
    Delete {
        workspace: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// The project's own inference keys, which its hosted agents run on.
    ///
    /// `set anthropic` or `set openai` reads the key from stdin (asked for
    /// without echo on a terminal): an owner of the project. Control keeps
    /// it sealed and puts it in each box's egress rules, so the compute
    /// provider's proxy sets it on the box's requests to the provider: it's
    /// never in the box, and never shown again. `clear` removes it; `ls`
    /// says which are set.
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
}

#[derive(clap::Subcommand)]
pub enum KeyCmd {
    /// Set the project's key for a provider, from stdin.
    Set {
        #[arg(value_parser = ["anthropic", "openai"])]
        provider: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Remove the project's key for a provider.
    #[command(visible_alias = "rm")]
    Clear {
        #[arg(value_parser = ["anthropic", "openai"])]
        provider: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Which of the project's keys are set.
    #[command(visible_alias = "list")]
    Ls {
        #[arg(long)]
        project: Option<String>,
    },
}

/// How long `--wait` waits.
const WAIT: Duration = Duration::from_secs(300);

pub fn run(cmd: CloudCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let show = |w: &Value| if json_out { print_json(w) } else { println!("{}", line(w)) };
    match cmd {
        CloudCmd::Create { name, repos, packages, setup, env, wait, project } => {
            let setup = match setup.as_deref() {
                None => String::new(),
                Some("-") => std::io::read_to_string(std::io::stdin()).context("reading the setup script")?,
                Some(f) => std::fs::read_to_string(f).with_context(|| format!("reading {f}"))?,
            };
            let project = project_of(&e, project.as_deref())?;
            let body =
                json!({ "name": name, "spec": { "repos": repos, "packages": packages, "setup": setup, "env": env } });
            let w = e.api("POST", &format!("/api/projects/{project}/workspaces"), Some(&body))?;
            let w = if wait { wait_ready(&e, &project, &w)? } else { w };
            show(&w);
            return Ok(if w["state"] == "failed" { 1 } else { 0 });
        }
        CloudCmd::Ls { project } => {
            let projects = match project {
                Some(p) => vec![project_of(&e, Some(&p))?],
                None => all_projects(&e)?,
            };
            let mut all = vec![];
            for p in projects {
                let listed = e.api("GET", &format!("/api/projects/{p}/workspaces"), None)?;
                all.extend(listed["workspaces"].as_array().cloned().unwrap_or_default());
            }
            if json_out {
                print_json(&json!({ "workspaces": all }));
            } else if all.is_empty() {
                println!("No hosted workspaces.");
            } else {
                for w in &all {
                    println!("{}", line(w));
                }
            }
        }
        CloudCmd::Open { workspace, wait, project } => {
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            let w = e.api("GET", &path(&project, &w), None)?;
            let w = if wait { wait_ready(&e, &project, &w)? } else { w };
            show(&w);
            return Ok(if w["state"] == "failed" { 1 } else { 0 });
        }
        CloudCmd::Rebuild { workspace, project } => {
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            show(&e.api("POST", &format!("{}/rebuild", path(&project, &w)), None)?);
        }
        CloudCmd::Delete { workspace, project } => {
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            show(&e.api("DELETE", &path(&project, &w), None)?);
        }
        CloudCmd::Key { cmd } => {
            let keys = match cmd {
                KeyCmd::Set { provider, project } => {
                    let project = project_of(&e, project.as_deref())?;
                    let value = super::studio::secret_input(&format!("{provider} API key: "))?;
                    let body = json!({ "value": value });
                    e.api("PUT", &format!("/api/projects/{project}/inference-keys/{provider}"), Some(&body))?
                }
                KeyCmd::Clear { provider, project } => {
                    let project = project_of(&e, project.as_deref())?;
                    e.api("DELETE", &format!("/api/projects/{project}/inference-keys/{provider}"), None)?
                }
                KeyCmd::Ls { project } => {
                    let project = project_of(&e, project.as_deref())?;
                    e.api("GET", &format!("/api/projects/{project}/inference-keys"), None)?
                }
            };
            if json_out {
                print_json(&keys);
            } else {
                for k in keys["keys"].as_array().into_iter().flatten() {
                    println!("{}", key_line(k));
                }
            }
        }
    }
    Ok(0)
}

/// `anthropic  set by alice` or `openai  not set`.
fn key_line(k: &Value) -> String {
    let provider = k["provider"].as_str().unwrap_or_default();
    match (k["set"] == true, k["set_by"].as_str()) {
        (true, Some(by)) => format!("{provider:<10} set by {by}"),
        (true, None) => format!("{provider:<10} set"),
        _ => format!("{provider:<10} not set"),
    }
}

fn path(project: &str, w: &Value) -> String {
    format!("/api/projects/{project}/workspaces/{}", w["id"].as_str().unwrap_or_default())
}

/// `--project`: an id, a name, or (without it) the personal one.
pub(super) fn project_of(e: &Enrolled, project: Option<&str>) -> anyhow::Result<String> {
    match project {
        None => Ok("personal".into()),
        Some(p) if p.starts_with("prj_") => Ok(p.to_owned()),
        Some(p) => super::project::project_id(&e.api("GET", "/api/projects", None)?, p),
    }
}

fn all_projects(e: &Enrolled) -> anyhow::Result<Vec<String>> {
    let listed = e.api("GET", "/api/projects", None)?;
    Ok(listed["projects"].as_array().into_iter().flatten().filter_map(|p| p["id"].as_str().map(String::from)).collect())
}

/// A workspace by its id or name: in `--project`, or else in whichever of
/// the account's projects has it.
fn find(e: &Enrolled, project: Option<&str>, workspace: &str) -> anyhow::Result<(String, Value)> {
    let projects = match project {
        Some(p) => vec![project_of(e, Some(p))?],
        None => all_projects(e)?,
    };
    let mut found = vec![];
    for p in projects {
        let listed = e.api("GET", &format!("/api/projects/{p}/workspaces"), None)?;
        for w in listed["workspaces"].as_array().into_iter().flatten() {
            if w["id"] == workspace || w["name"] == workspace {
                found.push((p.clone(), w.clone()));
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => bail!("no hosted workspace called {workspace}"),
        _ => bail!("more than one project has a hosted workspace called {workspace}: say which with --project"),
    }
}

/// Follow a workspace until it's ready or failed.
fn wait_ready(e: &Enrolled, project: &str, w: &Value) -> anyhow::Result<Value> {
    let until = Instant::now() + WAIT;
    let mut w = w.clone();
    let mut said = String::new();
    loop {
        let state = w["state"].as_str().unwrap_or_default().to_owned();
        if matches!(state.as_str(), "ready" | "failed" | "deleted") {
            return Ok(w);
        }
        if state != said {
            eprintln!("arugula: {} is {state}…", w["name"].as_str().unwrap_or_default());
            said = state;
        }
        if Instant::now() > until {
            bail!("{} isn't ready after {} s", w["name"].as_str().unwrap_or_default(), WAIT.as_secs());
        }
        std::thread::sleep(Duration::from_secs(2));
        w = e.api("GET", &path(project, &w), None)?;
    }
}

/// `dev  ready  hw_…  machine hosted-1`, with its error, and "rebuild
/// needed" when the spec changed since its box was made.
fn line(w: &Value) -> String {
    let s = |k: &str| w[k].as_str().unwrap_or_default();
    let mut out = format!("{:<24} {:<10} {}", s("name"), s("state"), s("id"));
    if let Some(m) = w["machine"].as_str() {
        out.push_str(&format!("  machine {m}"));
    }
    if let Some(a) = w["asked"].as_str() {
        out.push_str(&format!("  ({a} asked)"));
    }
    if w["rebuild_needed"] == true {
        out.push_str("  rebuild needed");
    }
    if let Some(e) = w["error"].as_str() {
        out.push_str(&format!("\n  {e}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn commands_parse() {
        for argv in [
            &["arugula", "cloud", "create", "dev", "--repo", "https://github.com/acme/app", "--wait"][..],
            &["arugula", "cloud", "create", "dev", "--setup", "-", "--env", "NPM_TOKEN", "--project", "Acme"],
            &["arugula", "cloud", "ls"],
            &["arugula", "cloud", "list", "--project", "prj_x"],
            &["arugula", "cloud", "open", "dev", "--wait"],
            &["arugula", "cloud", "rebuild", "hw_1"],
            &["arugula", "cloud", "rm", "dev"],
            &["arugula", "cloud", "delete", "dev"],
            &["arugula", "cloud", "key", "set", "anthropic", "--project", "Acme"],
            &["arugula", "cloud", "key", "clear", "openai"],
            &["arugula", "cloud", "key", "ls"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
    }

    #[test]
    fn lines() {
        let w = json!({ "name": "dev", "state": "ready", "id": "hw_1", "machine": "m1", "rebuild_needed": true });
        let l = line(&w);
        assert!(l.starts_with("dev") && l.contains("machine m1") && l.contains("rebuild needed"), "{l}");
        let w = json!({ "name": "dev", "state": "failed", "id": "hw_1", "error": "no delegation" });
        assert!(line(&w).ends_with("\n  no delegation"), "{}", line(&w));
        assert_eq!(
            key_line(&json!({ "provider": "anthropic", "set": true, "set_by": "alice" })),
            "anthropic  set by alice"
        );
        assert_eq!(key_line(&json!({ "provider": "openai", "set": false })), "openai     not set");
        assert!(
            crate::Cli::try_parse_from(["arugula", "cloud", "key", "set", "gemini"]).is_err(),
            "anthropic or openai"
        );
    }
}
