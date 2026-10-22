//! `arugula runs`: what starts a project's runs on Arugula control (M214).
//! A run executes on one of the project's hosted workspaces; `start` asks
//! for one by hand, and run specs (with their schedules and event rules)
//! start them on their own. Through control, as this CLI's account
//! (`arugula login`). Not `arugula run`: that's a command in a pane here.

use anyhow::bail;
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum RunsCmd {
    /// Start a run by hand: a spec, or a command after `--`.
    ///
    /// It runs on the project's hosted workspace (`--workspace` when it
    /// has more than one). Needs owner or editor on the project.
    Start {
        /// A run spec's name or id.
        #[arg(long, conflicts_with_all = ["command", "instruction"])]
        spec: Option<String>,
        /// The hosted workspace's name or id.
        #[arg(long)]
        workspace: Option<String>,
        /// Where it runs (an absolute path); the workspace's home without.
        #[arg(long)]
        dir: Option<String>,
        /// An instruction to an agent, instead of a command.
        #[arg(long, conflicts_with = "command")]
        instruction: Option<String>,
        /// The agent's recipe, with `--instruction`.
        #[arg(long, requires = "instruction")]
        agent: Option<String>,
        #[arg(long)]
        project: Option<String>,
        /// The command and its arguments.
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// A project's run specs: named runs, with their schedules and rules.
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// A run spec's cron schedules.
    Schedule {
        #[command(subcommand)]
        cmd: ScheduleCmd,
    },
    /// A run spec's event rules: a linked repo's PR opened, check failed,
    /// or push.
    Rule {
        #[command(subcommand)]
        cmd: RuleCmd,
    },
    /// The project's standing grant for runs on its hosted workspaces.
    ///
    /// `show` (the default) says whether there is one and who signed it,
    /// `sign` signs one with this CLI's device (an owner, once the project
    /// has a delegation for hosted machines), and `revoke` takes it back.
    Grant {
        #[command(subcommand)]
        cmd: Option<super::runs_grant::GrantCmd>,
        #[command(flatten)]
        which: super::runs_grant::Which,
    },
}

#[derive(clap::Subcommand)]
pub enum SpecCmd {
    /// Make a run spec: a command after `--`, or `--instruction`.
    Add {
        /// Its name in the project: letters, digits, `-`, `_` and `.`.
        name: String,
        /// The hosted workspace it runs on (name or id).
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        dir: Option<String>,
        #[arg(long, conflicts_with = "command")]
        instruction: Option<String>,
        #[arg(long, requires = "instruction")]
        agent: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// The project's run specs, with their schedules and rules.
    #[command(visible_alias = "list")]
    Ls {
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a run spec, with its schedules and rules.
    #[command(visible_alias = "delete")]
    Rm {
        spec: String,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(clap::Subcommand)]
pub enum ScheduleCmd {
    /// Run a spec on a cron schedule: five fields, at most every 5 minutes.
    Add {
        spec: String,
        /// Such as `0 9 * * 1-5` (9:00 on weekdays) or `@daily`.
        #[arg(long)]
        cron: String,
        /// IANA's name for the time zone; UTC without.
        #[arg(long)]
        tz: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Turn a schedule on.
    On {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Turn a schedule off.
    Off {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a schedule.
    #[command(visible_alias = "delete")]
    Rm {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum On {
    PrOpened,
    CheckFailed,
    Push,
}

impl On {
    fn wire(self) -> &'static str {
        match self {
            On::PrOpened => "pr_opened",
            On::CheckFailed => "check_failed",
            On::Push => "push",
        }
    }
}

#[derive(clap::Subcommand)]
pub enum RuleCmd {
    /// Run a spec on a linked repo's event.
    Add {
        spec: String,
        #[arg(long, value_enum)]
        on: On,
        /// `OWNER/REPO`, linked to the project; any of its repos without.
        #[arg(long)]
        repo: Option<String>,
        /// A branch (`*` for any run of characters): a PR's base, a
        /// check's or a push's branch. Any without.
        #[arg(long)]
        branch: Option<String>,
        /// Also take events from forks and from people who aren't the
        /// repo's owners, members or collaborators (an owner's call).
        #[arg(long)]
        outsiders: bool,
        /// The most runs it starts in an hour (20 without).
        #[arg(long)]
        max_per_hour: Option<u32>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Turn a rule on.
    On {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Turn a rule off.
    Off {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a rule.
    #[command(visible_alias = "delete")]
    Rm {
        spec: String,
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
}

/// The operation fields a spec or a start sends.
fn op(command: Vec<String>, instruction: Option<String>, agent: Option<String>, dir: Option<String>) -> Value {
    let mut o = json!({});
    if !command.is_empty() {
        o["argv"] = json!(command);
    }
    if let Some(i) = instruction {
        o["instruction"] = json!(i);
    }
    if let Some(a) = agent {
        o["agent"] = json!(a);
    }
    if let Some(d) = dir {
        o["directory"] = json!(d);
    }
    o
}

pub fn run(cmd: RunsCmd, json_out: bool) -> anyhow::Result<i32> {
    let cmd = match cmd {
        RunsCmd::Grant { cmd, which } => return super::runs_grant::run(cmd, which, json_out),
        other => other,
    };
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    let project = |p: Option<String>| super::cloud::project_of(&e, p.as_deref());
    let specs = |p: &str| format!("/api/projects/{p}/run-specs");
    let out = match cmd {
        RunsCmd::Grant { .. } => unreachable!("answered above"),
        RunsCmd::Start { spec, workspace, dir, instruction, agent, project: p, command } => {
            let mut body = match spec {
                Some(s) => json!({ "spec": s }),
                None if command.is_empty() && instruction.is_none() => {
                    bail!("say what to run: `--spec NAME`, `--instruction TEXT`, or a command after `--`")
                }
                None => op(command, instruction, agent, dir),
            };
            if let Some(w) = workspace {
                body["workspace"] = json!(w);
            }
            let run = e.api("POST", &format!("/api/projects/{}/runs", project(p)?), Some(&body))?;
            if !json_out {
                println!(
                    "{}  {}  on {}",
                    run["id"].as_str().unwrap_or_default(),
                    run["state"].as_str().unwrap_or_default(),
                    run["workspace"].as_str().unwrap_or_default()
                );
                return Ok(0);
            }
            run
        }
        RunsCmd::Spec { cmd: SpecCmd::Add { name, workspace, dir, instruction, agent, project: p, command } } => {
            if command.is_empty() && instruction.is_none() {
                bail!("say what it runs: a command after `--`, or `--instruction TEXT`");
            }
            let mut body = op(command, instruction, agent, dir);
            body["name"] = json!(name);
            body["workspace"] = json!(workspace);
            e.api("POST", &specs(&project(p)?), Some(&body))?
        }
        RunsCmd::Spec { cmd: SpecCmd::Ls { project: p } } => {
            let listed = e.api("GET", &specs(&project(p)?), None)?;
            if !json_out {
                let all = listed["specs"].as_array().cloned().unwrap_or_default();
                if all.is_empty() {
                    println!("No run specs.");
                }
                for s in &all {
                    println!("{}", spec_lines(s));
                }
                return Ok(0);
            }
            listed
        }
        RunsCmd::Spec { cmd: SpecCmd::Rm { spec, project: p } } => {
            e.api("DELETE", &format!("{}/{spec}", specs(&project(p)?)), None)?;
            json!({ "deleted": spec })
        }
        RunsCmd::Schedule { cmd } => {
            let (spec, p, method, tail, body) = match cmd {
                ScheduleCmd::Add { spec, cron, tz, project: p } => {
                    (spec, p, "POST", String::new(), Some(json!({ "cron": cron, "timezone": tz })))
                }
                ScheduleCmd::On { spec, id, project: p } => (spec, p, "PATCH", id, Some(json!({ "enabled": true }))),
                ScheduleCmd::Off { spec, id, project: p } => (spec, p, "PATCH", id, Some(json!({ "enabled": false }))),
                ScheduleCmd::Rm { spec, id, project: p } => (spec, p, "DELETE", id, None),
            };
            let path = format!("{}/{spec}/schedules{}", specs(&project(p)?), slash(&tail));
            e.api(method, &path, body.as_ref())?
        }
        RunsCmd::Rule { cmd } => {
            let (spec, p, method, tail, body) = match cmd {
                RuleCmd::Add { spec, on, repo, branch, outsiders, max_per_hour, project: p } => {
                    let body = json!({ "event": on.wire(), "repo": repo, "branch": branch,
                        "outsiders": outsiders, "max_per_hour": max_per_hour });
                    (spec, p, "POST", String::new(), Some(body))
                }
                RuleCmd::On { spec, id, project: p } => (spec, p, "PATCH", id, Some(json!({ "enabled": true }))),
                RuleCmd::Off { spec, id, project: p } => (spec, p, "PATCH", id, Some(json!({ "enabled": false }))),
                RuleCmd::Rm { spec, id, project: p } => (spec, p, "DELETE", id, None),
            };
            let path = format!("{}/{spec}/rules{}", specs(&project(p)?), slash(&tail));
            e.api(method, &path, body.as_ref())?
        }
    };
    // A delete answers nothing.
    print_json(&if out == json!("") { json!({ "ok": true }) } else { out });
    Ok(0)
}

fn slash(id: &str) -> String {
    if id.is_empty() { String::new() } else { format!("/{id}") }
}

/// A spec, then a line for each schedule and rule.
fn spec_lines(s: &Value) -> String {
    let t = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    let what = match s["instruction"].as_str() {
        Some(i) => format!("agent: {i}"),
        None => s["argv"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
    };
    let mut out = format!("{:<20} {}  on {}  {what}", t(s, "name"), t(s, "id"), t(s, "workspace"));
    for c in s["schedules"].as_array().into_iter().flatten() {
        let on = if c["enabled"] == true { "" } else { "  (off)" };
        out.push_str(&format!("\n  schedule {}  {} {}{on}", t(c, "id"), t(c, "cron"), t(c, "timezone")));
        if let Some(e) = c["last_error"].as_str() {
            out.push_str(&format!("  last tick: {e}"));
        }
    }
    for r in s["rules"].as_array().into_iter().flatten() {
        let on = if r["enabled"] == true { "" } else { "  (off)" };
        let repo = r["repo"].as_str().unwrap_or("any repo");
        let branch = r["branch"].as_str().unwrap_or("any branch");
        let outside = if r["outsiders"] == true { ", outsiders too" } else { "" };
        let cap = r["max_per_hour"].as_u64().unwrap_or_default();
        out.push_str(&format!("\n  rule {}  {} on {repo}, {branch}{outside}, {cap}/h{on}", t(r, "id"), t(r, "event")));
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
            &["arugula", "runs", "start", "--project", "Acme", "--", "just", "test"][..],
            &["arugula", "runs", "start", "--spec", "nightly"],
            &[
                "arugula",
                "runs",
                "start",
                "--instruction",
                "triage the failure",
                "--agent",
                "fixer",
                "--workspace",
                "dev",
            ],
            &["arugula", "runs", "spec", "add", "nightly", "--workspace", "dev", "--dir", "/work", "--", "just", "e2e"],
            &["arugula", "runs", "spec", "ls", "--project", "prj_x"],
            &["arugula", "runs", "spec", "rm", "nightly"],
            &["arugula", "runs", "schedule", "add", "nightly", "--cron", "0 3 * * *", "--tz", "America/Chicago"],
            &["arugula", "runs", "schedule", "off", "nightly", "rsc_1"],
            &["arugula", "runs", "rule", "add", "nightly", "--on", "check-failed", "--branch", "main"],
            &["arugula", "runs", "rule", "add", "nightly", "--on", "pr-opened", "--outsiders", "--max-per-hour", "5"],
            &["arugula", "runs", "rule", "rm", "nightly", "rru_1"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
        for argv in [
            &["arugula", "runs", "start", "--spec", "x", "--", "ls"][..],
            &["arugula", "runs", "rule", "add", "x", "--on", "deployed"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_err(), "{argv:?}");
        }
    }

    #[test]
    fn spec_lines_say_what_it_runs() {
        let s = json!({ "name": "nightly", "id": "rsp_1", "workspace": "hw_1", "argv": ["just", "e2e"],
            "schedules": [{ "id": "rsc_1", "cron": "0 3 * * *", "timezone": "UTC", "enabled": false }],
            "rules": [{ "id": "rru_1", "event": "push", "branch": "main", "enabled": true, "max_per_hour": 20 }] });
        let l = spec_lines(&s);
        assert!(l.starts_with("nightly") && l.contains("just e2e"), "{l}");
        assert!(l.contains("schedule rsc_1  0 3 * * * UTC  (off)"), "{l}");
        assert!(l.contains("rule rru_1  push on any repo, main, 20/h"), "{l}");
    }
}
