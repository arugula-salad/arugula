//! `arugula cloud`: a project's hosted workspaces on Arugula control
//! (M179). Each is `arugulad` in a box at the platform's compute provider,
//! in a project: it enrolls into the project and shows in its machines.
//! Through control, as this CLI's account (`arugula login`).
//!
//! On its first boot a box sets itself up from its spec in a pane anyone on
//! the project can watch (M183): `ls` and `open` say how that goes, and
//! "rebuild needed" once the spec changed (`edit`). `rebuild` asks the
//! box first, through the relay, for work it would lose.

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
        /// Wait until it's set up (or failed).
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
        /// Wait until it's set up (or failed).
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// Change what a hosted workspace is built from.
    ///
    /// Each flag given replaces that part of its spec; the rest stays. Its
    /// box isn't touched: it shows "rebuild needed" until it's rebuilt.
    Edit {
        workspace: String,
        /// An HTTPS clone URL. Repeatable; replaces the repos.
        #[arg(long = "repo")]
        repos: Vec<String>,
        /// No repos.
        #[arg(long, conflicts_with = "repos")]
        no_repos: bool,
        /// A package to install. Repeatable; replaces the packages.
        #[arg(long = "package")]
        packages: Vec<String>,
        /// No packages.
        #[arg(long, conflicts_with = "packages")]
        no_packages: bool,
        /// The setup script: a file, or `-` for stdin (an empty file for
        /// none).
        #[arg(long)]
        setup: Option<String>,
        /// The name of an env var it takes from its secrets. Repeatable;
        /// replaces the names.
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// A hosted workspace's secrets, which its box reads on its next setup.
    ///
    /// `GIT_TOKEN` (and `GIT_USERNAME`) clone repos the GitHub App doesn't;
    /// the rest are the env vars its spec's `--env` names.
    Secret {
        #[command(subcommand)]
        cmd: SecretCmd,
    },
    /// Make a hosted workspace's box again from its spec.
    ///
    /// It asks the box first: a repo with changes or commits that aren't
    /// pushed stops it, unless `--download` saves them first or `--force`.
    Rebuild {
        workspace: String,
        /// Save a tarball of the repos with unpushed work here first.
        #[arg(long)]
        download: Option<std::path::PathBuf>,
        /// Rebuild even if the box has unpushed work, or can't be asked.
        #[arg(long)]
        force: bool,
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

#[derive(clap::Subcommand)]
pub enum SecretCmd {
    /// Set one: its value from stdin.
    Set {
        workspace: String,
        /// Capital letters, digits and `_`.
        name: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Its secrets' names (never their values).
    #[command(visible_alias = "list")]
    Ls {
        workspace: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Remove one.
    #[command(visible_alias = "remove")]
    Rm {
        workspace: String,
        name: String,
        #[arg(long)]
        project: Option<String>,
    },
}

/// How long `--wait` waits: a setup that installs a lot takes a while.
const WAIT: Duration = Duration::from_secs(30 * 60);

pub fn run(cmd: CloudCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let show = |w: &Value| if json_out { print_json(w) } else { println!("{}", line(w)) };
    match cmd {
        CloudCmd::Create { name, repos, packages, setup, env, wait, project } => {
            let setup = match setup.as_deref() {
                None => String::new(),
                Some(f) => read_setup(f)?,
            };
            let project = project_of(&e, project.as_deref())?;
            let body =
                json!({ "name": name, "spec": { "repos": repos, "packages": packages, "setup": setup, "env": env } });
            let w = e.api("POST", &format!("/api/projects/{project}/workspaces"), Some(&body))?;
            let w = if wait { with_tail(wait_ready(&e, &project, &w)?) } else { w };
            show(&w);
            return Ok(failed(&w));
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
            let w = if wait { with_tail(wait_ready(&e, &project, &w)?) } else { w };
            show(&w);
            return Ok(failed(&w));
        }
        CloudCmd::Edit { workspace, repos, no_repos, packages, no_packages, setup, env, project } => {
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            let mut spec = w["spec"].clone();
            if no_repos || !repos.is_empty() {
                spec["repos"] = json!(repos);
            }
            if no_packages || !packages.is_empty() {
                spec["packages"] = json!(packages);
            }
            if let Some(f) = setup.as_deref() {
                spec["setup"] = json!(read_setup(f)?);
            }
            if !env.is_empty() {
                spec["env"] = json!(env);
            }
            show(&e.api("PUT", &format!("{}/spec", path(&project, &w)), Some(&spec))?);
        }
        CloudCmd::Secret { cmd } => return secret(&e, cmd, json_out),
        CloudCmd::Rebuild { workspace, download, force, project } => {
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            if !before_rebuild(&w, download.as_deref(), force)? {
                return Ok(1);
            }
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

/// 1 for a workspace or a setup that failed.
fn failed(w: &Value) -> i32 {
    i32::from(w["state"] == "failed" || w["setup"]["status"] == "failed")
}

/// A setup script: a file, or `-` for stdin.
fn read_setup(f: &str) -> anyhow::Result<String> {
    if f == "-" {
        return std::io::read_to_string(std::io::stdin()).context("reading the setup script");
    }
    std::fs::read_to_string(f).with_context(|| format!("reading {f}"))
}

fn secret(e: &Enrolled, cmd: SecretCmd, json_out: bool) -> anyhow::Result<i32> {
    match cmd {
        SecretCmd::Set { workspace, name, project } => {
            let (project, w) = find(e, project.as_deref(), &workspace)?;
            let value = std::io::read_to_string(std::io::stdin()).context("reading the secret from stdin")?;
            // A trailing newline from `echo` or a file isn't part of it.
            let value = value.strip_suffix('\n').map(|v| v.strip_suffix('\r').unwrap_or(v)).unwrap_or(&value);
            e.api("PUT", &format!("{}/secrets/{name}", path(&project, &w)), Some(&json!({ "value": value })))?;
            if !json_out {
                println!(
                    "Set {name} on {}: its box reads it on its next setup.",
                    w["name"].as_str().unwrap_or_default()
                );
            }
        }
        SecretCmd::Ls { workspace, project } => {
            let (project, w) = find(e, project.as_deref(), &workspace)?;
            let listed = e.api("GET", &format!("{}/secrets", path(&project, &w)), None)?;
            if json_out {
                print_json(&listed);
            } else {
                let names = listed["secrets"].as_array().cloned().unwrap_or_default();
                if names.is_empty() {
                    println!("No secrets.");
                }
                for s in names {
                    println!("{}", s["name"].as_str().unwrap_or_default());
                }
            }
        }
        SecretCmd::Rm { workspace, name, project } => {
            let (project, w) = find(e, project.as_deref(), &workspace)?;
            e.api("DELETE", &format!("{}/secrets/{name}", path(&project, &w)), None)?;
        }
    }
    Ok(0)
}

/// Before a rebuild: ask the box, through the relay, for repos with work
/// that isn't pushed, and save it to `download` if asked. Whether to go on:
/// not when there is some (or the box can't be asked) and nothing saved
/// it, unless `force`.
fn before_rebuild(w: &Value, download: Option<&std::path::Path>, force: bool) -> anyhow::Result<bool> {
    use arugula_proto::{api::Empty, op::ops::HostedSetupGet};
    let name = w["name"].as_str().unwrap_or_default();
    let Some(machine) = w["machine"].as_str() else { return Ok(true) };
    let asked = crate::control::target(machine).and_then(|t| {
        let t = t.with_context(|| format!("control doesn't list its machine {machine}"))?;
        let setup = crate::http::call::<HostedSetupGet>(&t, &(), &Empty {})?;
        Ok((t, setup))
    });
    let (target, setup) = match asked {
        Ok(got) => got,
        Err(err) if force => {
            eprintln!("arugula: couldn't ask {name} for unpushed work ({err:#}); rebuilding anyway (--force)");
            return Ok(true);
        }
        Err(err) => {
            eprintln!("arugula: couldn't ask {name} for unpushed work: {err:#}");
            eprintln!("arugula: open it first (`arugula cloud open {name}`), or rebuild anyway with --force");
            return Ok(false);
        }
    };
    let dirty: Vec<_> = setup.repos.iter().filter(|r| r.dirty()).collect();
    if dirty.is_empty() {
        return Ok(true);
    }
    for r in &dirty {
        eprintln!("arugula: {}: {} changed file(s), {} commit(s) on no remote branch", r.dir, r.changed, r.unpushed);
    }
    if let Some(to) = download {
        let mut res = crate::http::request(&target, "GET", "/api/hosted/setup/archive", None)?;
        if res.status != 200 {
            bail!("{name} didn't hand over its unpushed work: {}", String::from_utf8_lossy(&res.bytes()?));
        }
        let mut f = std::fs::File::create(to).with_context(|| format!("writing {}", to.display()))?;
        let n = std::io::copy(&mut res, &mut f).with_context(|| format!("writing {}", to.display()))?;
        eprintln!("arugula: saved {} ({n} bytes)", to.display());
        return Ok(true);
    }
    if force {
        eprintln!("arugula: rebuilding anyway (--force): that work is lost");
        return Ok(true);
    }
    eprintln!(
        "arugula: a rebuild loses that work: push it, save it with --download FILE, or rebuild anyway with --force"
    );
    Ok(false)
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

/// Follow a workspace until it's set up (ready, its setup done) or failed.
fn wait_ready(e: &Enrolled, project: &str, w: &Value) -> anyhow::Result<Value> {
    let until = Instant::now() + WAIT;
    let mut w = w.clone();
    let mut said = String::new();
    loop {
        let mut state = w["state"].as_str().unwrap_or_default().to_owned();
        let setup = w["setup"]["status"].as_str().unwrap_or_default();
        if matches!(state.as_str(), "failed" | "deleted") || (state == "ready" && matches!(setup, "ready" | "failed")) {
            return Ok(w);
        }
        if state == "ready" {
            state = match w["setup"]["step"].as_str() {
                Some(step) => format!("setting up: {step}"),
                None => "starting its setup".into(),
            };
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

/// `dev  ready  hw_…  machine hosted-1  setting up: clone app`, with its
/// error, "rebuild needed" when the spec changed since its box was made,
/// and a failed setup's last lines when they were read from its box.
fn line(w: &Value) -> String {
    let s = |k: &str| w[k].as_str().unwrap_or_default();
    let mut out = format!("{:<24} {:<10} {}", s("name"), s("state"), s("id"));
    if let Some(m) = w["machine"].as_str() {
        out.push_str(&format!("  machine {m}"));
    }
    let setup = &w["setup"];
    let step = setup["step"].as_str().unwrap_or_default();
    match setup["status"].as_str() {
        Some("setting_up") => out.push_str(&format!("  setting up: {step}")),
        Some("ready") => out.push_str("  set up"),
        Some("failed") => {
            let pane = setup["pane"].as_u64().map(|p| format!(" (%{p} on its machine)")).unwrap_or_default();
            out.push_str(&format!("  setup failed at {step}: open the setup pane{pane}"));
        }
        _ => {}
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
    // Read from the box, end to end (`with_tail`): control never has it.
    if let Some(t) = setup["tail"].as_str().filter(|t| !t.is_empty()) {
        for l in t.lines() {
            out.push_str(&format!("\n  | {l}"));
        }
    }
    out
}

/// A failed setup's last lines, read from its box through the relay as
/// anyone on the project reads its pane (control never holds them), put in
/// `setup.tail`. Left as it is when the box can't be asked, saying why.
fn with_tail(mut w: Value) -> Value {
    use arugula_proto::{api::Empty, op::ops::HostedSetupGet};
    if w["setup"]["status"] != "failed" {
        return w;
    }
    let Some(machine) = w["machine"].as_str().map(str::to_owned) else { return w };
    let got = crate::control::target(&machine).and_then(|t| {
        let t = t.with_context(|| format!("control doesn't list its machine {machine}"))?;
        crate::http::call::<HostedSetupGet>(&t, &(), &Empty {})
    });
    match got {
        Ok(s) if !s.tail.is_empty() => w["setup"]["tail"] = json!(s.tail),
        Ok(_) => {}
        Err(e) => eprintln!("arugula: couldn't read the setup's pane from its box: {e:#}"),
    }
    w
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
            &["arugula", "cloud", "rebuild", "dev", "--download", "dev.tgz"],
            &["arugula", "cloud", "rebuild", "dev", "--force"],
            &["arugula", "cloud", "edit", "dev", "--repo", "https://github.com/acme/app", "--setup", "setup.sh"],
            &["arugula", "cloud", "edit", "dev", "--no-packages", "--env", "NPM_TOKEN"],
            &["arugula", "cloud", "secret", "set", "dev", "GIT_TOKEN"],
            &["arugula", "cloud", "secret", "ls", "dev"],
            &["arugula", "cloud", "secret", "rm", "dev", "GIT_TOKEN", "--project", "Acme"],
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
        let w = json!({ "name": "dev", "state": "ready", "id": "hw_1", "setup": { "status": "setting_up", "step": "clone app" } });
        assert!(line(&w).ends_with("setting up: clone app"), "{}", line(&w));
        let w = json!({ "name": "dev", "state": "ready", "id": "hw_1",
            "setup": { "status": "failed", "step": "setup script", "pane": 2 } });
        let l = line(&w);
        assert!(l.ends_with("setup failed at setup script: open the setup pane (%2 on its machine)"), "{l}");
        let mut w = w;
        w["setup"]["tail"] = json!("make: no\nError 2");
        assert!(line(&w).ends_with("\n  | make: no\n  | Error 2"), "{}", line(&w));
        assert!(crate::Cli::try_parse_from(["arugula", "cloud", "edit", "dev", "--repo", "x", "--no-repos"]).is_err());
    }
}
