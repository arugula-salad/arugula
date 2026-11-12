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

/// A cloud workspace's sizes (M266): the platform's `Size`.
const SIZES: [&str; 2] = ["small", "large"];

#[derive(clap::Subcommand)]
pub enum CloudCmd {
    /// Make a cloud workspace.
    ///
    /// Its machine is made from the current release's template of its size
    /// and enrolls into the project. Needs owner or editor on the project.
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
        /// Its machine's size: small (2 vCPU, 2 GiB) or large (8 vCPU, 8 GiB).
        #[arg(long, value_parser = SIZES, default_value = "small")]
        size: String,
        /// Wait until it's set up (or failed).
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// The project's cloud workspaces (every project's, without
    /// `--project`).
    #[command(visible_alias = "list")]
    Ls {
        #[arg(long)]
        project: Option<String>,
    },
    /// Open a cloud workspace: wakes it if it's paused, and says its
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
    /// Change what a cloud workspace is built from.
    ///
    /// Each flag given replaces that part of its spec; the rest stays. Its
    /// machine isn't touched: it shows "rebuild needed" until it's rebuilt,
    /// except for the finish step, which its next turn runs.
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
        /// A command (`just check`) agent sessions run after each turn,
        /// before the push.
        ///
        /// "" for none. It needs no rebuild.
        #[arg(long)]
        finish: Option<String>,
        /// How long the finish step may run, in seconds (default: 15 min).
        #[arg(long, value_name = "SECONDS")]
        finish_timeout: Option<u64>,
        /// Its machine's size: small (2 vCPU, 2 GiB) or large (8 vCPU, 8 GiB).
        ///
        /// It takes a rebuild.
        #[arg(long, value_parser = SIZES)]
        size: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// A cloud workspace's secrets, which its machine reads on its next setup.
    ///
    /// `GIT_TOKEN` (and `GIT_USERNAME`) clone repos the GitHub App doesn't;
    /// the rest are the env vars its spec's `--env` names.
    Secret {
        #[command(subcommand)]
        cmd: SecretCmd,
    },
    /// Make a cloud workspace's machine again from its spec.
    ///
    /// It asks the machine first: work in any git repo in its homes that
    /// a new machine wouldn't have (changed or untracked files, commits on
    /// a branch no remote has, stashes) stops it, unless `--download` saves
    /// it first or `--force`.
    Rebuild {
        workspace: String,
        /// Save a tarball of the repos with unpushed work here first, at
        /// their paths on the machine.
        #[arg(long)]
        download: Option<std::path::PathBuf>,
        /// Rebuild even if the machine has unpushed work, or can't be asked.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a cloud workspace and its machine.
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
    /// it sealed and puts it in each hosted machine's egress rules, so the compute
    /// provider's proxy sets it on the machine's requests to the provider: it's
    /// never on the machine, and never shown again. `clear` removes it; `ls`
    /// says which are set.
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
    /// A cloud workspace's ports, and their public previews.
    ///
    /// `ls` lists the ports and which are public. `public PORT` makes one
    /// public at its own address, which anyone can open signed out, for 7
    /// days; `renew` gives it 7 more; `off` takes it down (within a
    /// minute). Making a port public needs owner or editor on the project,
    /// and shows who did it.
    Preview {
        #[command(subcommand)]
        cmd: PreviewCmd,
    },
}

#[derive(clap::Subcommand)]
pub enum PreviewCmd {
    /// The workspace's ports, and which are public.
    #[command(visible_alias = "list")]
    Ls {
        workspace: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Make a port public for 7 days: anyone with its address can open it.
    Public {
        workspace: String,
        port: u16,
        #[arg(long)]
        project: Option<String>,
    },
    /// Keep a public port public for 7 more days.
    Renew {
        workspace: String,
        port: u16,
        #[arg(long)]
        project: Option<String>,
    },
    /// Take a port's public preview down.
    Off {
        workspace: String,
        port: u16,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(clap::Subcommand)]
pub enum KeyCmd {
    /// Set the project's key for a provider, from stdin.
    ///
    /// Anthropic's may be a Claude subscription's token from `claude
    /// setup-token`.
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
        CloudCmd::Create { name, repos, packages, setup, env, size, wait, project } => {
            let setup = match setup.as_deref() {
                None => String::new(),
                Some(f) => read_setup(f)?,
            };
            let project = project_of(&e, project.as_deref())?;
            let body = json!({ "name": name, "spec": {
                "repos": repos, "packages": packages, "setup": setup, "env": env, "size": size,
            } });
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
                println!("No cloud workspaces.");
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
        CloudCmd::Edit {
            workspace,
            repos,
            no_repos,
            packages,
            no_packages,
            setup,
            env,
            finish,
            finish_timeout,
            size,
            project,
        } => {
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
            if let Some(f) = finish {
                spec["finish"] = json!(f.trim());
            }
            if let Some(t) = finish_timeout {
                spec["finish_timeout_s"] = json!(t);
            }
            if let Some(s) = size {
                spec["size"] = json!(s);
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
        CloudCmd::Preview { cmd } => {
            let (workspace, project, port, how) = match cmd {
                PreviewCmd::Ls { workspace, project } => (workspace, project, None, ""),
                PreviewCmd::Public { workspace, port, project } => (workspace, project, Some(port), "public"),
                PreviewCmd::Renew { workspace, port, project } => (workspace, project, Some(port), "renew"),
                PreviewCmd::Off { workspace, port, project } => (workspace, project, Some(port), "off"),
            };
            let (project, w) = find(&e, project.as_deref(), &workspace)?;
            let ports = format!("{}/ports", path(&project, &w));
            let got = match (port, how) {
                (Some(p), "public") => e.api("PUT", &format!("{ports}/{p}/public"), Some(&json!({})))?,
                (Some(p), "renew") => e.api("PUT", &format!("{ports}/{p}/public"), Some(&json!({ "renew": true })))?,
                (Some(p), _) => e.api("DELETE", &format!("{ports}/{p}/public"), None)?,
                (None, _) => e.api("GET", &ports, None)?,
            };
            if json_out {
                print_json(&got);
            } else if port.is_some() {
                println!("{}", port_line(&got));
            } else {
                let listed = got["ports"].as_array().cloned().unwrap_or_default();
                if listed.is_empty() {
                    println!("No ports noted yet: `arugula cloud preview public WORKSPACE PORT` makes one public.");
                }
                for p in &listed {
                    println!("{}", port_line(p));
                }
                if got["previews"] == false {
                    println!("Public previews aren't set up on this control.");
                }
            }
        }
        CloudCmd::Key { cmd } => {
            let keys = match cmd {
                KeyCmd::Set { provider, project } => {
                    let project = project_of(&e, project.as_deref())?;
                    let ask = match provider.as_str() {
                        "anthropic" => "Anthropic API key, or a token from `claude setup-token`: ".to_owned(),
                        p => format!("{p} API key: "),
                    };
                    let value = super::studio::secret_input(&ask)?;
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
                    "Set {name} on {}: its machine reads it on its next setup.",
                    w["name"].as_str().unwrap_or_default()
                );
                if name == "GIT_TOKEN" {
                    println!(
                        "It's used only on the hosts the workspace's repos are on now: set it again after adding a \
                         repo on another host."
                    );
                }
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
                    println!("{}", secret_line(&s));
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

/// A secret as `cloud secret ls` lists it: its name, and for the clone
/// secret, the hosts it's used on, or that it's used on none until it's set
/// again (one set before it was bound to hosts, or with no repos in the
/// spec).
fn secret_line(s: &Value) -> String {
    let name = s["name"].as_str().unwrap_or_default();
    match s["hosts"].as_array() {
        Some(h) if h.is_empty() => format!("{name} (used on no host: set it again to clone with it)"),
        Some(h) => {
            let hosts: Vec<&str> = h.iter().filter_map(Value::as_str).collect();
            format!("{name} (used on {})", hosts.join(", "))
        }
        None => name.to_owned(),
    }
}

/// Before a rebuild: ask the box, through the relay, for work a rebuild
/// would lose (in every repo in its homes, on any branch, in stashes,
/// changed or untracked: Arugula #932), and save it to `download` if
/// asked. Whether to go on: not when there is some (or the box can't be
/// asked, or couldn't look somewhere) and nothing saved it, unless `force`.
fn before_rebuild(w: &Value, download: Option<&std::path::Path>, force: bool) -> anyhow::Result<bool> {
    use arugula_proto::{api::Empty, op::ops::HostedSetupGet};
    let name = w["name"].as_str().unwrap_or_default();
    let Some(machine) = w["machine"].as_str() else { return Ok(true) };
    let asked = crate::control::target(machine).and_then(|t| {
        let t = t.with_context(|| format!("control doesn't list its machine {machine}"))?;
        let setup = crate::http::call::<HostedSetupGet>(&t, &(), &Empty {})?;
        // A box from before Arugula #932 looks only at its cloned repos'
        // checked-out branches: look with the same script, in a pane.
        let looked = match &setup.check {
            Some(_) => None,
            None => Some(look_in_a_pane(&t).context("looking through its repos in a pane")?),
        };
        Ok((t, setup, looked))
    });
    let (target, setup, looked) = match asked {
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
    let (lost, missed) = match &looked {
        Some(c) => (c.found.iter().map(|f| &f.repo).filter(|r| r.dirty()).collect::<Vec<_>>(), &c.check.missed),
        None => (setup.repos.iter().filter(|r| r.dirty()).collect(), &setup.check.as_ref().expect("asked").missed),
    };
    if lost.is_empty() && missed.is_empty() {
        return Ok(true);
    }
    eprintln!("arugula: {name} has work a rebuild would lose:");
    for l in lost_lines(&lost, missed) {
        eprintln!("  {l}");
    }
    if let Some(to) = download {
        // An older box's tarball has only the repos it calls dirty, so not
        // a stash or another branch; nothing saves what it couldn't look at.
        let unsaved: Vec<&str> = lost
            .iter()
            .filter(|r| looked.is_some() && !setup.repos.iter().any(|s| s.dir == r.dir && s.dirty()))
            .map(|r| r.dir.as_str())
            .chain(missed.iter().map(|m| m.dir.as_str()))
            .collect();
        if !unsaved.is_empty() && !force {
            eprintln!(
                "arugula: --download can't save what's in {}: push it, or rebuild anyway with --force",
                unsaved.join(", ")
            );
            return Ok(false);
        }
        let mut res = crate::http::request(&target, "GET", "/api/hosted/setup/archive", None)?;
        if res.status != 200 {
            bail!("{name} didn't hand over its unpushed work: {}", String::from_utf8_lossy(&res.bytes()?));
        }
        let mut f = std::fs::File::create(to).with_context(|| format!("writing {}", to.display()))?;
        let n = std::io::copy(&mut res, &mut f).with_context(|| format!("writing {}", to.display()))?;
        eprintln!("arugula: saved {} ({n} bytes)", to.display());
        if !unsaved.is_empty() {
            eprintln!("arugula: rebuilding anyway (--force): what's in {} is lost", unsaved.join(", "));
        }
        return Ok(true);
    }
    if force {
        eprintln!("arugula: rebuilding anyway (--force): that work is lost");
        return Ok(true);
    }
    eprintln!("arugula: {SAVE_IT}");
    Ok(false)
}

/// How to keep what a rebuild would lose.
const SAVE_IT: &str = "push it first (a branch: `git push origin BRANCH`; a stash: `git stash branch NAME`, then push \
     that), save it to a file with --download FILE, or rebuild anyway with --force";

/// What a rebuild would lose, a line a repo: `/home/user/app: 2 changed or
/// untracked files; branch feature: 3 commits on no remote; 1 stash`, and
/// each place the box couldn't look.
fn lost_lines(lost: &[&arugula_proto::api::HostedRepo], missed: &[arugula_proto::api::HostedMissed]) -> Vec<String> {
    let n = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut out = Vec::new();
    for r in lost {
        let mut parts = Vec::new();
        if r.changed > 0 {
            parts.push(n(r.changed, "changed or untracked file", "changed or untracked files"));
        }
        for b in &r.branches {
            parts.push(if b.name == "HEAD" {
                format!("detached HEAD: {} on no branch", n(b.commits, "commit", "commits"))
            } else {
                format!("branch {}: {} on no remote", b.name, n(b.commits, "commit", "commits"))
            });
        }
        // An older box counts them without saying where.
        if r.branches.is_empty() && r.unpushed > 0 {
            parts.push(format!("{} on no remote branch", n(r.unpushed, "commit", "commits")));
        }
        if r.stashes > 0 {
            parts.push(n(r.stashes, "stash", "stashes"));
        }
        out.push(format!("{}: {}", r.dir, parts.join("; ")));
    }
    for m in missed {
        out.push(format!("{}: couldn't look ({})", m.dir, m.why));
    }
    out
}

/// Look for work a rebuild would lose on a box whose daemon doesn't (one
/// from before Arugula #932): the daemon's script, in a pane of its own,
/// whose output comes back base64 between markers (a terminal turns tabs
/// to spaces and wraps long lines). The pane is closed after.
fn look_in_a_pane(target: &crate::http::Target) -> anyhow::Result<arugula_proto::hosted_check::Checked> {
    use arugula_proto::{
        api::{Empty, RunRequest, RunResponse, WaitResult},
        hosted_check::SCRIPT,
        op::ops::ClosePane,
    };
    use base64::Engine as _;
    let script = base64::engine::general_purpose::STANDARD.encode(SCRIPT);
    // The markers as the shell prints them: the command line itself never
    // shows them whole.
    let command = format!(
        "printf %s {script} | base64 -d | bash -s | {{ echo ARUGULA-CHECK-''START; base64 -w0; echo; echo ARUGULA-CHECK-''END; }}"
    );
    let body = RunRequest { command: Some(command), ..RunRequest::default() };
    let RunResponse { pane } = crate::http::request_as(target, "POST", "/api/run", &body)?.parse()?;
    let read = (|| {
        let path = format!("/api/panes/{pane}/wait?until=exit&timeout=120");
        let w: WaitResult = crate::http::request(target, "GET", &path, None)?.parse()?;
        if matches!(w, WaitResult::Timeout) {
            bail!("it took over two minutes");
        }
        let path = format!("/api/panes/{pane}/capture?format=text&scope=scrollback");
        let text = crate::http::request(target, "GET", &path, None)?.ok()?.text()?;
        from_pane(&text)
    })();
    let _ = crate::http::call::<ClosePane>(target, &pane, &Empty {});
    read
}

/// What [`look_in_a_pane`]'s pane shows, read back.
fn from_pane(text: &str) -> anyhow::Result<arugula_proto::hosted_check::Checked> {
    use base64::Engine as _;
    let (start, end) = ("ARUGULA-CHECK-START", "ARUGULA-CHECK-END");
    let after = text.find(start).map(|i| &text[i + start.len()..]).context("the pane printed nothing")?;
    let inside = &after[..after.find(end).context("the pane's output stopped short")?];
    let b64: String = inside.chars().filter(|c| !c.is_whitespace()).collect();
    let out = base64::engine::general_purpose::STANDARD.decode(b64).context("the pane's output")?;
    let checked = arugula_proto::hosted_check::parse(&String::from_utf8_lossy(&out));
    anyhow::ensure!(!checked.check.homes.is_empty() || !checked.check.missed.is_empty(), "the pane looked nowhere");
    Ok(checked)
}

/// `5173  public  https://….arugula.site  by alice until 2026-10-29 14:00 UTC`,
/// `5173  private`, or `5173  off` after turning it off.
fn port_line(p: &Value) -> String {
    let port = p["port"].as_u64().unwrap_or_default();
    if p["public"] != true {
        return format!("{port:<6} {}", if p.get("was_public").is_some() { "off" } else { "private" });
    }
    let mut out = format!("{port:<6} public  {}", p["url"].as_str().unwrap_or_default());
    if let Some(by) = p["public_by"].as_str() {
        out.push_str(&format!("  by {by}"));
    }
    if let Some(until) = p["public_until"].as_u64() {
        out.push_str(&format!(" until {}", utc(until)));
    }
    out
}

/// Ms since the epoch as `2026-10-29 14:00 UTC`.
fn utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rest / 3600, rest % 3600 / 60)
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
        0 => bail!("no cloud workspace called {workspace}"),
        _ => bail!("more than one project has a cloud workspace called {workspace}: say which with --project"),
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
    // Small unless it says (a small spec leaves it out).
    if let Some(size) = w["spec"]["size"].as_str().filter(|z| *z != "small") {
        out.push_str(&format!("  {size}"));
    }
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

    /// Arugula #932: the refusal names each repo and what in it a rebuild
    /// would lose, and where the box couldn't look.
    #[test]
    fn it_names_what_a_rebuild_would_lose() {
        use arugula_proto::api::{HostedBranch, HostedMissed, HostedRepo};
        let branch = |name: &str, commits| HostedBranch { name: name.into(), commits };
        let app = HostedRepo {
            dir: "/home/user/app".into(),
            changed: 2,
            unpushed: 4,
            branches: vec![branch("main", 1), branch("feature", 3)],
            stashes: 1,
            ..HostedRepo::default()
        };
        let detached = HostedRepo {
            dir: "/home/ses1/work/app".into(),
            unpushed: 1,
            branches: vec![branch("HEAD", 1)],
            ..HostedRepo::default()
        };
        // From an older box: a count, no branches.
        let old = HostedRepo { dir: "/home/user/old".into(), unpushed: 2, ..HostedRepo::default() };
        let stash = HostedRepo { dir: "/home/user/s".into(), stashes: 2, ..HostedRepo::default() };
        let missed = [HostedMissed { dir: "/home/ses2".into(), why: "can't read it as ses2".into() }];
        assert_eq!(
            lost_lines(&[&app, &detached, &old, &stash], &missed),
            [
                "/home/user/app: 2 changed or untracked files; branch main: 1 commit on no remote; branch feature: 3 \
             commits on no remote; 1 stash",
                "/home/ses1/work/app: detached HEAD: 1 commit on no branch",
                "/home/user/old: 2 commits on no remote branch",
                "/home/user/s: 2 stashes",
                "/home/ses2: couldn't look (can't read it as ses2)",
            ]
        );
    }

    /// An older box's pane: the script's lines come back base64 between
    /// markers, however the terminal wrapped them.
    #[test]
    fn a_pane_says_what_it_found() {
        use base64::Engine as _;
        let said = "arugula-check\thome\t/home/user\n\
                    arugula-check\trepo\t/home/user/app\t\t/home/user/app/.git\tmain\t0\t0\t1\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(said);
        let wrapped: Vec<String> = b64.as_bytes().chunks(20).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        let text = format!(
            "Welcome!\nARUGULA-CHECK-START\n{}\nARUGULA-CHECK-END\n\n[exited with code 0]\n",
            wrapped.join("\n")
        );
        let got = from_pane(&text).unwrap();
        assert_eq!(got.check.homes, ["/home/user"]);
        assert_eq!((got.found[0].repo.dir.as_str(), got.found[0].repo.stashes), ("/home/user/app", 1));
        assert!(from_pane("bash: base64: command not found").is_err());
        assert!(from_pane("ARUGULA-CHECK-START\nAAAA").is_err(), "cut short");
    }

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
            &["arugula", "cloud", "edit", "dev", "--finish", "just check", "--finish-timeout", "1200"],
            &["arugula", "cloud", "create", "dev", "--size", "large"],
            &["arugula", "cloud", "edit", "dev", "--size", "small"],
            &["arugula", "cloud", "secret", "set", "dev", "GIT_TOKEN"],
            &["arugula", "cloud", "secret", "ls", "dev"],
            &["arugula", "cloud", "secret", "rm", "dev", "GIT_TOKEN", "--project", "Acme"],
            &["arugula", "cloud", "rm", "dev"],
            &["arugula", "cloud", "delete", "dev"],
            &["arugula", "cloud", "key", "set", "anthropic", "--project", "Acme"],
            &["arugula", "cloud", "key", "clear", "openai"],
            &["arugula", "cloud", "key", "ls"],
            &["arugula", "cloud", "preview", "ls", "dev"],
            &["arugula", "cloud", "preview", "public", "dev", "5173", "--project", "Acme"],
            &["arugula", "cloud", "preview", "renew", "dev", "5173"],
            &["arugula", "cloud", "preview", "off", "dev", "5173"],
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
        let p = json!({ "port": 5173, "public": true, "url": "https://ab.arugula.site", "public_by": "alice",
                        "public_until": 1_793_282_400_000u64 });
        assert_eq!(port_line(&p), "5173   public  https://ab.arugula.site  by alice until 2026-10-29 14:00 UTC");
        assert_eq!(port_line(&json!({ "port": 80, "public": false })), "80     private");
        assert_eq!(port_line(&json!({ "port": 80, "public": false, "was_public": true })), "80     off");
        assert!(crate::Cli::try_parse_from(["arugula", "cloud", "preview", "public", "dev", "0x"]).is_err());
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
        assert!(crate::Cli::try_parse_from(["arugula", "cloud", "create", "dev", "--size", "huge"]).is_err());
        let w = json!({ "name": "dev", "state": "ready", "id": "hw_1", "spec": { "size": "large" } });
        assert!(line(&w).ends_with("hw_1  large"), "{}", line(&w));
        let w = json!({ "name": "dev", "state": "ready", "id": "hw_1", "spec": { "size": "small" } });
        assert!(line(&w).ends_with("hw_1"), "{}", line(&w));
    }

    /// The clone secret says which hosts it's used on, or that it must be
    /// set again; the rest are their names.
    #[test]
    fn secret_lines() {
        assert_eq!(secret_line(&json!({ "name": "NPM_TOKEN", "store": "sealed" })), "NPM_TOKEN");
        let used = json!({ "name": "GIT_TOKEN", "store": "sealed", "hosts": ["git.corp.example", "git.example:8443"] });
        assert_eq!(secret_line(&used), "GIT_TOKEN (used on git.corp.example, git.example:8443)");
        let none = json!({ "name": "GIT_TOKEN", "store": "sealed", "hosts": [] });
        assert_eq!(secret_line(&none), "GIT_TOKEN (used on no host: set it again to clone with it)");
    }
}
