//! `arugula agents`: the recipes this machine offers to other people's
//! agents (M76), each a Claude Code subagent file, and their A2A cards.

use super::Ctx;
use crate::http::{enc, request};
use crate::util::{absolute, print_json};
use serde_json::{Value, json};

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// What this machine offers, as A2A agent cards (the default).
    Ls,
    /// The recipes a project has, and the user's (`.claude/agents/*.md`),
    /// offered or not.
    Recipes {
        /// The project [default: here].
        #[arg(long)]
        dir: Option<String>,
    },
    /// Offer a recipe, working in its project; or stop offering it.
    Offer {
        /// Its `name`, as its subagent file says.
        agent: String,
        /// The project it works in [default: here].
        #[arg(long, conflicts_with = "stop")]
        dir: Option<String>,
        /// Stop offering it.
        #[arg(long)]
        stop: bool,
    },
    /// One offered agent's A2A card.
    Card { agent: String },
    /// Hand a task to an agent a machine offers (this one's too), and wait
    /// for it.
    Send {
        /// The machine (its name, as `agents catalog` shows it; `here` for
        /// this one; `-` for whichever offers the agent).
        machine: String,
        agent: String,
        /// What to do.
        #[arg(required = true)]
        text: Vec<String>,
        /// Seconds to wait for it to finish, or ask something (0: don't).
        #[arg(long, default_value_t = 300)]
        wait: u64,
        /// Have it open a pull request in its own project, not hand back a
        /// patch.
        #[arg(long)]
        pr: bool,
    },
    /// Run an offered agent to talk to, wherever it's offered.
    ///
    /// It starts in its project, on its machine (one of yours), in a
    /// session named after it.
    Run {
        agent: String,
        /// The machine [default: whichever offers it, this one first].
        #[arg(long)]
        on: Option<String>,
        /// What to ask it first.
        prompt: Vec<String>,
    },
    /// A task you sent: its state, or answer it, or stop it.
    Task {
        machine: String,
        agent: String,
        task: String,
        /// Answer its question (or send it a follow-up).
        #[arg(long, conflicts_with = "cancel")]
        answer: Option<String>,
        #[arg(long)]
        cancel: bool,
        /// Seconds to wait for it to finish, or ask something.
        #[arg(long, default_value_t = 0)]
        wait: u64,
    },
    /// A finished task's patch, in a diff block on a copy of your checkout.
    Review {
        task: String,
        /// Your checkout [default: here].
        #[arg(long)]
        dir: Option<String>,
    },
    /// Apply a finished task's patch in your checkout.
    Apply {
        task: String,
        /// Your checkout [default: here].
        #[arg(long)]
        dir: Option<String>,
    },
    /// Tasks from other people waiting for you to allow them here.
    Waiting,
    /// Allow a waiting task (once, or with --hour its caller's tasks for that
    /// agent for an hour).
    Allow {
        task: String,
        #[arg(long)]
        hour: bool,
    },
    /// Turn a waiting task down.
    Deny { task: String },
    /// Standing grants (`allow --hour`), or take one back.
    Grants {
        /// Take back ACCOUNT's grant for AGENT.
        #[arg(long, num_args = 2, value_names = ["ACCOUNT", "AGENT"])]
        revoke: Option<Vec<String>>,
    },
    /// The agents your team offers, on every machine you reach.
    ///
    /// Yours, your teams', and teammates' that offer agents, as this machine
    /// last read them (a minute at most).
    Catalog {
        /// Ask every machine now.
        #[arg(long)]
        fresh: bool,
    },
}

/// A task as the daemon answered: its summary, or the JSON. Exit 2 when it
/// waits on you (a question), 1 when it failed or was turned down.
fn task_out(v: Value, json_out: bool) -> anyhow::Result<i32> {
    if json_out {
        print_json(&v);
    } else {
        print!("{}", v["summary"].as_str().unwrap_or_default());
    }
    Ok(match v["task"]["status"]["state"].as_str().unwrap_or_default() {
        "TASK_STATE_INPUT_REQUIRED" => 2,
        "TASK_STATE_FAILED" | "TASK_STATE_REJECTED" => 1,
        _ => 0,
    })
}

/// The project's whole path [default: here], `.` and `..` resolved.
fn project(dir: Option<&str>) -> anyhow::Result<String> {
    let d = absolute(dir.unwrap_or("."))?;
    Ok(std::fs::canonicalize(&d).map(|p| p.display().to_string()).unwrap_or(d))
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args.cmd.unwrap_or(Cmd::Ls) {
        Cmd::Ls => {
            let v = request(&sock, "GET", "/api/a2a/agents", None)?.json()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let cards = v["agents"].as_array().cloned().unwrap_or_default();
            if cards.is_empty() {
                println!(
                    "nothing offered on {} (arugula agents offer NAME)",
                    v["machine"].as_str().unwrap_or("this machine")
                );
            }
            for c in &cards {
                println!("{}\t{}", c["name"].as_str().unwrap_or("?"), c["description"].as_str().unwrap_or(""));
            }
            for b in v["broken"].as_array().into_iter().flatten() {
                eprintln!(
                    "{} (from {}): {}",
                    b["agent"].as_str().unwrap_or("?"),
                    b["dir"].as_str().unwrap_or("?"),
                    b["why"].as_str().unwrap_or("")
                );
            }
        }
        Cmd::Recipes { dir } => {
            let dir = project(dir.as_deref())?;
            let v = request(&sock, "GET", &format!("/api/a2a/recipes?dir={}", enc(&dir)), None)?.json()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("no recipes in {dir}/.claude/agents or ~/.claude/agents");
            }
            for r in &list {
                let offered = r["offered_from"].as_str().map(|d| format!("\toffered from {d}")).unwrap_or_default();
                println!(
                    "{}\t{}\t{}{offered}",
                    r["name"].as_str().unwrap_or("?"),
                    r["path"].as_str().unwrap_or(""),
                    r["description"].as_str().unwrap_or("")
                );
            }
        }
        Cmd::Offer { agent, dir, stop } => {
            let dir: Value = if stop { Value::Null } else { json!(project(dir.as_deref())?) };
            let v = request(&sock, "POST", "/api/a2a/offers", Some(&json!({ "agent": agent, "dir": dir })))?.json()?;
            if json_out {
                print_json(&v);
            } else if stop {
                println!("{agent}: no longer offered");
            } else {
                println!("{agent}: offered from {}", dir.as_str().unwrap_or_default());
            }
        }
        Cmd::Catalog { fresh } => {
            let path = if fresh { "/api/a2a/catalog?fresh=1" } else { "/api/a2a/catalog" };
            let v = request(&sock, "GET", path, None)?.json()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            for m in v["machines"].as_array().into_iter().flatten() {
                let owner =
                    m["owner"].as_str().filter(|o| !o.is_empty()).map(|o| format!(" ({o}'s)")).unwrap_or_default();
                let stale = if m["online"].as_bool().unwrap_or(false) { "" } else { " [offline: as last seen]" };
                for c in m["agents"].as_array().into_iter().flatten() {
                    println!(
                        "{}\t{}{owner}{stale}\t{}",
                        c["name"].as_str().unwrap_or("?"),
                        m["name"].as_str().unwrap_or("?"),
                        c["description"].as_str().unwrap_or("")
                    );
                }
                if let Some(n) = m["note"].as_str() {
                    eprintln!("{}: {n}", m["name"].as_str().unwrap_or("?"));
                }
            }
            if let Some(n) = v["note"].as_str() {
                eprintln!("{n}");
            }
        }
        Cmd::Send { machine, agent, text, wait, pr } => {
            let machine = if machine == "-" { String::new() } else { machine };
            let body = json!({ "kind": "send", "machine": machine, "agent": agent, "text": text.join(" "), "wait": wait, "pr": pr });
            return task_out(request(&sock, "POST", "/api/a2a/delegate", Some(&body))?.json()?, json_out);
        }
        Cmd::Run { agent, on, prompt } => {
            let prompt = Some(prompt.join(" ")).filter(|p| !p.trim().is_empty());
            let body = json!({ "machine": on, "agent": agent, "prompt": prompt });
            let v = request(&sock, "POST", "/api/a2a/run", Some(&body))?.json()?;
            if json_out {
                print_json(&v);
            } else {
                println!(
                    "{agent} runs on {} in %{} (session {agent})",
                    v["machine"].as_str().unwrap_or("?"),
                    v["block"]
                );
            }
        }
        Cmd::Task { machine, agent, task, answer, cancel, wait } => {
            let kind = if cancel {
                "cancel"
            } else if answer.is_some() {
                "answer"
            } else {
                "get"
            };
            let body =
                json!({ "kind": kind, "machine": machine, "agent": agent, "task": task, "text": answer, "wait": wait });
            return task_out(request(&sock, "POST", "/api/a2a/delegate", Some(&body))?.json()?, json_out);
        }
        Cmd::Review { task, dir } => {
            let beside = std::env::var("ARUGULA_PANE").ok().and_then(|p| p.parse::<u32>().ok());
            let body = json!({ "kind": "review", "machine": "", "agent": "", "task": task, "dir": project(dir.as_deref())?, "beside": beside });
            let v = request(&sock, "POST", "/api/a2a/delegate", Some(&body))?.json()?;
            if json_out {
                print_json(&v);
            } else {
                print!("{}", v["summary"].as_str().unwrap_or_default());
            }
            return Ok(if v["applied"]["clean"] == true { 0 } else { 1 });
        }
        Cmd::Apply { task, dir } => {
            let body =
                json!({ "kind": "apply", "machine": "", "agent": "", "task": task, "dir": project(dir.as_deref())? });
            let v = request(&sock, "POST", "/api/a2a/delegate", Some(&body))?.json()?;
            if json_out {
                print_json(&v);
            } else {
                print!("{}", v["summary"].as_str().unwrap_or_default());
            }
            return Ok(if v["applied"]["clean"] == true { 0 } else { 1 });
        }
        Cmd::Waiting => {
            let v = request(&sock, "GET", "/api/a2a/waiting", None)?.json()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let list = v["waiting"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("nothing waits for you");
            }
            for t in &list {
                let m = &t["metadata"]["arugula"];
                let text = t["history"][0]["parts"][0]["text"].as_str().unwrap_or("");
                println!(
                    "{}\t{} for {}\t{text}",
                    t["id"].as_str().unwrap_or("?"),
                    m["caller"].as_str().unwrap_or("?"),
                    m["agent"].as_str().unwrap_or("?")
                );
            }
        }
        Cmd::Allow { task, hour } => {
            let answer = if hour { "hour" } else { "once" };
            let v = request(
                &sock,
                "POST",
                &format!("/api/a2a/tasks/{}/consent", enc(&task)),
                Some(&json!({ "answer": answer })),
            )?
            .json()?;
            if json_out {
                print_json(&v);
            } else {
                println!(
                    "{task}: allowed{}",
                    if hour { ", and its caller's tasks for that agent for an hour" } else { "" }
                );
            }
        }
        Cmd::Deny { task } => {
            let v = request(
                &sock,
                "POST",
                &format!("/api/a2a/tasks/{}/consent", enc(&task)),
                Some(&json!({ "answer": "deny" })),
            )?
            .json()?;
            if json_out {
                print_json(&v);
            } else {
                println!("{task}: denied");
            }
        }
        Cmd::Grants { revoke } => {
            if let Some(r) = revoke {
                let v = request(&sock, "POST", "/api/a2a/grants", Some(&json!({ "account": r[0], "agent": r[1] })))?
                    .json()?;
                if json_out {
                    print_json(&v);
                } else if v["revoked"] == true {
                    println!("taken back: {}'s grant for {}", r[0], r[1]);
                } else {
                    println!("no such grant");
                }
                return Ok(0);
            }
            let v = request(&sock, "GET", "/api/a2a/grants", None)?.json()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("no grants");
            }
            for g in &list {
                println!(
                    "{} ({})\t{}\tuntil {}",
                    g["name"].as_str().unwrap_or("?"),
                    g["account"].as_str().unwrap_or("?"),
                    g["agent"].as_str().unwrap_or("?"),
                    g["until_ms"]
                );
            }
        }
        Cmd::Card { agent } => {
            let v = request(&sock, "GET", &format!("/api/a2a/agents/{}/card", enc(&agent)), None)?.json()?;
            print_json(&v);
        }
    }
    Ok(0)
}
