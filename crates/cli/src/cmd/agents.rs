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
        Cmd::Card { agent } => {
            let v = request(&sock, "GET", &format!("/api/a2a/agents/{}/card", enc(&agent)), None)?.json()?;
            print_json(&v);
        }
    }
    Ok(0)
}
