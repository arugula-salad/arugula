//! `illogical a2a` (S34 spike): the agents a machine offers, and A2A tasks
//! sent to them. With `--host`, the machine is reached as any other: over
//! the end-to-end channel, through control when there's no direct path.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::Subcommand;
use serde_json::{Value, json};

use crate::http::{self, Target};

#[derive(Subcommand)]
pub enum A2aCmd {
    /// The agents this machine offers, as A2A agent cards.
    Agents,
    /// Offer one of this machine's Claude Code agents, or stop.
    ///
    /// The agent is `.claude/agents/NAME.md` in DIR, or in
    /// `~/.claude/agents/`. The owner's.
    Offer {
        agent: String,
        /// The project (a git checkout) its tasks work in.
        #[arg(long)]
        dir: Option<String>,
        #[arg(long)]
        stop: bool,
    },
    /// Send an agent a task, or the next message on one, and wait.
    ///
    /// Waits until the task ends or waits on someone.
    Send {
        agent: String,
        text: String,
        /// The next message on this task (an answer, a follow-up).
        #[arg(long)]
        task: Option<String>,
        /// Return once it's accepted.
        #[arg(long)]
        no_wait: bool,
        /// Give up waiting after this many seconds.
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
    /// A task's state, reply and changes.
    Get {
        agent: String,
        task: String,
    },
    /// A task's changes as a patch (`| git apply --3way`).
    Patch {
        agent: String,
        task: String,
    },
    Cancel {
        agent: String,
        task: String,
    },
    /// The tasks you sent this agent.
    Tasks {
        agent: String,
    },
    /// Tasks waiting for you (this machine's owner) to allow them.
    Waiting,
    /// Allow a waiting task (once, or this caller and agent for an hour),
    /// or refuse it.
    Answer {
        task: String,
        /// allow, hour or deny.
        #[arg(default_value = "allow")]
        answer: String,
    },
}

fn rpc(target: &Target, agent: &str, method: &str, params: Value) -> anyhow::Result<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
    let path = format!("/api/a2a/agents/{}?a2a-version=1.0", http::enc(agent));
    let v = http::send(
        target,
        "POST",
        &path,
        &[("Content-Type", "application/json"), ("A2A-Version", "1.0")],
        body.as_bytes(),
    )?
    .json()?;
    if let Some(e) = v.get("error") {
        bail!("{} ({})", e["message"].as_str().unwrap_or("error"), e["code"]);
    }
    Ok(v["result"].clone())
}

/// Where this message comes from: a claim the other side shows.
fn whence() -> Value {
    let machine = std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_owned());
    let pane = std::env::var("ILLOGICAL_PANE").ok().and_then(|p| p.trim_start_matches('%').parse::<u64>().ok());
    json!({ "machine": machine, "pane": pane })
}

fn state(t: &Value) -> &str {
    t["status"]["state"].as_str().unwrap_or("?")
}

fn says(t: &Value) -> String {
    t["status"]["message"]["parts"][0]["text"].as_str().unwrap_or_default().to_owned()
}

fn waits(s: &str) -> bool {
    matches!(
        s,
        "TASK_STATE_COMPLETED"
            | "TASK_STATE_FAILED"
            | "TASK_STATE_CANCELED"
            | "TASK_STATE_REJECTED"
            | "TASK_STATE_INPUT_REQUIRED"
            | "TASK_STATE_AUTH_REQUIRED"
    )
}

fn show(t: &Value) {
    println!("{} {}", t["id"].as_str().unwrap_or("?"), state(t).trim_start_matches("TASK_STATE_").to_lowercase());
    let s = says(t);
    if !s.is_empty() {
        println!("  {s}");
    }
    for a in t["artifacts"].as_array().into_iter().flatten() {
        match a["name"].as_str() {
            Some("reply") => println!("\n{}", a["parts"][0]["text"].as_str().unwrap_or_default()),
            Some("patch") => println!(
                "\npatch against {}:\n{}",
                a["metadata"]["illogical"]["base"].as_str().unwrap_or("?"),
                a["metadata"]["illogical"]["stat"].as_str().unwrap_or_default()
            ),
            _ => {}
        }
    }
}

pub fn run(target: &Target, cmd: A2aCmd, json_out: bool) -> anyhow::Result<i32> {
    match cmd {
        A2aCmd::Agents => {
            let v = http::request(target, "GET", "/api/a2a/agents", None)?.json()?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(0);
            }
            let m = v["machine"].as_str().unwrap_or("?");
            for c in v["agents"].as_array().into_iter().flatten() {
                println!("{m}/{:<20} {}", c["name"].as_str().unwrap_or("?"), c["description"].as_str().unwrap_or(""));
            }
            for b in v["broken"].as_array().into_iter().flatten() {
                println!(
                    "{m}/{:<20} (can't be read: {})",
                    b["agent"].as_str().unwrap_or("?"),
                    b["why"].as_str().unwrap_or("")
                );
            }
        }
        A2aCmd::Offer { agent, dir, stop } => {
            let dir = match (dir, stop) {
                (Some(d), _) => Some(std::fs::canonicalize(&d).map(|p| p.display().to_string()).unwrap_or(d)),
                (None, false) => Some(std::env::current_dir()?.display().to_string()),
                (None, true) => None,
            };
            let v = http::request(
                target,
                "POST",
                "/api/a2a/offers",
                Some(&json!({ "agent": agent, "dir": dir, "offer": !stop })),
            )?
            .json()?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        A2aCmd::Send { agent, text, task, no_wait, timeout } => {
            let mut message = json!({
                "messageId": format!("m{}", std::process::id()),
                "role": "ROLE_USER",
                "parts": [{ "text": text }],
                "metadata": { "illogical": whence() },
            });
            if let Some(t) = &task {
                message["taskId"] = json!(t);
            }
            let r = rpc(
                target,
                &agent,
                "SendMessage",
                json!({ "message": message, "configuration": { "returnImmediately": true } }),
            )?;
            let mut t = r["task"].clone();
            let id = t["id"].as_str().context("no task in the answer")?.to_owned();
            eprintln!("task {id}: {}", state(&t));
            let until = Instant::now() + Duration::from_secs(timeout);
            let mut last = (state(&t).to_owned(), says(&t));
            while !no_wait && !waits(state(&t)) && Instant::now() < until {
                std::thread::sleep(Duration::from_secs(2));
                t = rpc(target, &agent, "GetTask", json!({ "id": id, "historyLength": 0 }))?;
                let now = (state(&t).to_owned(), says(&t));
                if now != last {
                    eprintln!(
                        "task {id}: {}{}",
                        now.0,
                        if now.1.is_empty() { String::new() } else { format!(": {}", now.1) }
                    );
                    last = now;
                }
            }
            if json_out {
                println!("{}", serde_json::to_string_pretty(&t)?);
            } else {
                show(&t);
            }
            return Ok(if matches!(state(&t), "TASK_STATE_COMPLETED" | "TASK_STATE_INPUT_REQUIRED") { 0 } else { 1 });
        }
        A2aCmd::Get { agent, task } => {
            let t = rpc(target, &agent, "GetTask", json!({ "id": task }))?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&t)?);
            } else {
                show(&t);
            }
        }
        A2aCmd::Patch { agent, task } => {
            let t = rpc(target, &agent, "GetTask", json!({ "id": task, "historyLength": 0 }))?;
            let p = t["artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["name"] == "patch")
                .and_then(|a| a["parts"][0]["text"].as_str())
                .context("no changes in that task")?;
            print!("{p}");
        }
        A2aCmd::Cancel { agent, task } => {
            let t = rpc(target, &agent, "CancelTask", json!({ "id": task }))?;
            show(&t);
        }
        A2aCmd::Waiting => {
            let v = http::request(target, "GET", "/api/a2a/waiting", None)?.json()?;
            for t in v["waiting"].as_array().into_iter().flatten() {
                let i = &t["metadata"]["illogical"];
                let text = t["history"][0]["parts"][0]["text"].as_str().unwrap_or_default();
                println!(
                    "{} {} from {} on {} (says {}): {text}",
                    t["id"].as_str().unwrap_or("?"),
                    i["agent"].as_str().unwrap_or("?"),
                    i["caller"]["name"].as_str().unwrap_or("?"),
                    i["caller"]["device"].as_str().unwrap_or("?"),
                    i["caller"]["claims"],
                );
            }
        }
        A2aCmd::Answer { task, answer } => {
            let v = http::request(
                target,
                "POST",
                &format!("/api/a2a/tasks/{}/consent", http::enc(&task)),
                Some(&json!({ "answer": answer })),
            )?
            .json()?;
            println!("{v}");
        }
        A2aCmd::Tasks { agent } => {
            let v = rpc(target, &agent, "ListTasks", json!({}))?;
            for t in v["tasks"].as_array().into_iter().flatten() {
                println!("{} {}", t["id"].as_str().unwrap_or("?"), state(t));
            }
        }
    }
    Ok(0)
}
