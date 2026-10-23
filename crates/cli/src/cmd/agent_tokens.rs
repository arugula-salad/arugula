//! `arugula agent-tokens`: agent grant tokens (M230), for agents with no
//! machine of their own (a hosted service's, redone's) that reach your
//! projects through the platform's MCP endpoint on Arugula control. Each is
//! a bearer token with powers over projects you read: `threads.read`,
//! `threads.post`, `issues.write`, `deploys.plan`, `runs.read`,
//! `runs.start`, `runs.command` and `secrets.write` (set a project's
//! secrets by name, never read them). None approves anything. Its agent
//! acts as yours, labelled "<you>'s agent on <name> (MCP)", never past your
//! role. The token is shown once, when it's made; `ls` lists them, `calls`
//! what each one's agent did, and `revoke` takes one back at once. Through
//! control, as this CLI's account (`arugula login`).

use anyhow::bail;
use arugula_control_wire::agent_mcp::{AGENT_TOKENS, agent_token_calls, agent_token_revoke};
use serde_json::{Value, json};

use crate::control::Enrolled;
use crate::util::print_json;

/// What a token can hold.
const POWERS: [&str; 8] = [
    "threads.read",
    "threads.post",
    "issues.write",
    "deploys.plan",
    "runs.read",
    "runs.start",
    "runs.command",
    "secrets.write",
];

#[derive(clap::Subcommand)]
pub enum AgentTokensCmd {
    /// Your agent tokens, live and revoked, with their powers.
    Ls,
    /// Make a token for an agent with no machine, and print it (once).
    ///
    /// The powers are threads.read, threads.post, issues.write,
    /// deploys.plan (plan deploys and ask for approval, never approve),
    /// runs.read, runs.start (run specs), runs.command (commands of its
    /// own) and secrets.write (set a project's secrets by name, never read
    /// them), over each --project. A token's powers don't change: make
    /// another for more.
    Add {
        /// What its agent is called: "<you>'s agent on <name> (MCP)".
        name: String,
        /// One or more powers.
        #[arg(required = true)]
        powers: Vec<String>,
        /// A project, by its name or id (one or more).
        #[arg(long = "project", required = true)]
        projects: Vec<String>,
    },
    /// Revoke a token, by its name or id: its agent is refused from its
    /// next call.
    Revoke {
        /// The token: its name or id (agt_…).
        token: String,
    },
    /// What a token's agent did, newest first.
    Calls {
        /// The token: its name or id (agt_…).
        token: String,
        /// At most this many (default 50, at most 200).
        #[arg(long)]
        limit: Option<u32>,
    },
}

pub fn run(cmd: Option<AgentTokensCmd>, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    match cmd.unwrap_or(AgentTokensCmd::Ls) {
        AgentTokensCmd::Ls => {
            let listed = e.api("GET", AGENT_TOKENS, None)?;
            if json_out {
                print_json(&listed);
                return Ok(0);
            }
            let tokens = listed["tokens"].as_array().cloned().unwrap_or_default();
            if tokens.is_empty() {
                println!("No agent tokens. `arugula agent-tokens add NAME POWER... --project NAME` makes one.");
            }
            for t in &tokens {
                println!("{}", line(t));
            }
            if let Some(url) = listed["mcp_url"].as_str() {
                println!("Their MCP endpoint: {url}");
            }
        }
        AgentTokensCmd::Add { name, powers, projects } => {
            if let Some(p) = powers.iter().find(|p| !POWERS.contains(&p.as_str())) {
                bail!("a token can't hold {p}: {}", POWERS.join(", "));
            }
            let all = e.api("GET", "/api/projects", None)?;
            let ids =
                projects.iter().map(|p| crate::cmd::grants::project_id(&all, p)).collect::<Result<Vec<_>, _>>()?;
            let made =
                e.api("POST", AGENT_TOKENS, Some(&json!({ "name": name, "projects": ids, "powers": powers })))?;
            if json_out {
                print_json(&made);
                return Ok(0);
            }
            let token = made["token"].as_str().unwrap_or_default();
            let url = made["mcp_url"].as_str().unwrap_or_default();
            println!("{}", line(&made["agent_token"]));
            println!();
            println!("The token, shown this once (it isn't kept, only its hash):");
            println!();
            println!("  {token}");
            println!();
            println!("Give it to the agent as an MCP server over HTTP:");
            println!("  url: {url}");
            println!("  header: Authorization: Bearer {token}");
            println!(
                "For Claude Code: claude mcp add --transport http arugula {url} --header \"Authorization: Bearer {token}\""
            );
        }
        AgentTokensCmd::Revoke { token } => {
            let id = token_id(&e, &token)?;
            let t = e.api("POST", &agent_token_revoke(&id), None)?;
            if json_out { print_json(&t) } else { println!("{}", line(&t)) }
        }
        AgentTokensCmd::Calls { token, limit } => {
            let id = token_id(&e, &token)?;
            let path = match limit {
                Some(n) => format!("{}?limit={n}", agent_token_calls(&id)),
                None => agent_token_calls(&id),
            };
            let calls = e.api("GET", &path, None)?;
            if json_out {
                print_json(&calls);
                return Ok(0);
            }
            let calls = calls["calls"].as_array().cloned().unwrap_or_default();
            if calls.is_empty() {
                println!("Its agent hasn't called anything yet.");
            }
            for c in &calls {
                println!("{}", call_line(c));
            }
        }
    }
    Ok(0)
}

/// A token named on the command line, by its id or its name among the
/// live ones (a revoked one by its id).
fn token_id(e: &Enrolled, want: &str) -> anyhow::Result<String> {
    if want.starts_with("agt_") {
        return Ok(want.to_owned());
    }
    let listed = e.api("GET", AGENT_TOKENS, None)?;
    pick(&listed, want)
}

fn pick(listed: &Value, want: &str) -> anyhow::Result<String> {
    let ids: Vec<String> = listed["tokens"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["live"] == true && t["name"] == want)
        .filter_map(|t| t["id"].as_str().map(String::from))
        .collect();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => bail!("no live agent token is called {want} (`arugula agent-tokens` lists them)"),
        _ => bail!("more than one live agent token is called {want}: name one by its id ({})", ids.join(", ")),
    }
}

/// `redone  live  used 3m ago  issues.write, deploys.plan on Acme  agt_…`.
fn line(t: &Value) -> String {
    let state = match (t["revoked_at"].as_u64(), t["last_used_at"].as_u64()) {
        (Some(at), _) => format!("revoked {}", crate::hosts::ago(at)),
        (None, Some(at)) => format!("live, used {}", crate::hosts::ago(at)),
        (None, None) => "live, never used".into(),
    };
    let mut by: Vec<(String, Vec<String>)> = vec![];
    for g in t["grants"].as_array().into_iter().flatten() {
        let p = g["project"]["name"].as_str().or(g["project"]["id"].as_str()).unwrap_or("?").to_owned();
        let power = g["power"].as_str().unwrap_or("?").to_owned();
        match by.iter_mut().find(|(n, _)| *n == p) {
            Some((_, ps)) => ps.push(power),
            None => by.push((p, vec![power])),
        }
    }
    let reach = by.iter().map(|(p, ps)| format!("{} on {p}", ps.join(", "))).collect::<Vec<_>>().join("; ");
    format!(
        "{:<20} {state:<24} {reach}  {}",
        t["name"].as_str().unwrap_or_default(),
        t["id"].as_str().unwrap_or_default()
    )
}

/// `3m ago  issue_write  ok  filed #4 (iss_…)`.
fn call_line(c: &Value) -> String {
    format!(
        "{:<10} {:<15} {:<8} {}",
        c["at"].as_u64().map(crate::hosts::ago).unwrap_or_default(),
        c["tool"].as_str().unwrap_or_default(),
        c["outcome"].as_str().unwrap_or_default(),
        c["detail"].as_str().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn a_token_is_made_with_powers_over_projects() {
        let parse = |args: &[&str]| crate::Cli::try_parse_from([&["arugula", "agent-tokens"], args].concat());
        assert!(parse(&["add", "redone", "issues.write", "deploys.plan", "--project", "acme"]).is_ok());
        assert!(parse(&["add", "redone", "issues.write", "--project", "acme", "--project", "web"]).is_ok());
        assert!(parse(&["add", "redone", "issues.write"]).is_err(), "a project");
        assert!(parse(&["add", "redone", "--project", "acme"]).is_err(), "a power");
        assert!(parse(&["revoke", "redone"]).is_ok());
        assert!(parse(&["calls", "redone", "--limit", "5"]).is_ok());
        assert!(parse(&[]).is_ok());
    }

    #[test]
    fn a_token_reads_as_a_line_and_is_named_once() {
        let t = json!({ "id": "agt_1", "name": "redone", "live": true, "revoked_at": null, "last_used_at": null,
            "grants": [
                { "power": "issues.write", "project": { "id": "prj_1", "name": "Acme" } },
                { "power": "deploys.plan", "project": { "id": "prj_1", "name": "Acme" } },
                { "power": "runs.read", "project": { "id": "prj_2", "name": null } },
            ] });
        let l = line(&t);
        assert!(
            l.contains("live, never used") && l.contains("issues.write, deploys.plan on Acme; runs.read on prj_2"),
            "{l}"
        );
        let listed = json!({ "tokens": [t, { "id": "agt_2", "name": "redone", "live": false },
            { "id": "agt_3", "name": "two", "live": true }, { "id": "agt_4", "name": "two", "live": true }] });
        assert_eq!(pick(&listed, "redone").unwrap(), "agt_1", "a revoked one isn't named");
        assert!(pick(&listed, "two").unwrap_err().to_string().contains("agt_3, agt_4"));
        assert!(pick(&listed, "none").is_err());
        let c = json!({ "at": 0, "tool": "issue_write", "outcome": "ok", "detail": "filed #4 (iss_1)" });
        assert!(call_line(&c).contains("issue_write") && call_line(&c).ends_with("filed #4 (iss_1)"));
    }
}
