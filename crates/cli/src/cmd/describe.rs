//! `arugula describe`: a block's type, place and state, how an agent's screen reads, and which agents
//! are configured here.

use super::Ctx;
use crate::http::request;
use crate::util::{Pane, print_json};
use arugula_proto::api::{AgentsInventory, DetectionAnswer};
use serde_json::Value;

/// `describe %N --detection` for people: what fired, then each rule with
/// the text it saw, highest priority first.
fn detection_text(pane: u32, v: &DetectionAnswer) -> String {
    use std::fmt::Write;
    let d = match v {
        DetectionAnswer::Read(d) => d,
        DetectionAnswer::NoAgent(n) => {
            return match n.command.as_deref() {
                Some(c) => format!("%{pane} runs `{c}`: no agent with screen rules\n"),
                None => format!("%{pane} is at its shell: no agent with screen rules\n"),
            };
        }
    };
    let agent = &d.agent;
    let mut out = format!("%{pane} runs {} ({agent})", d.name);
    if d.unread {
        let found = d.configured.iter().flatten().cloned().collect::<Vec<_>>();
        let found = if found.is_empty() { "nothing".to_owned() } else { found.join(", ") };
        let _ = writeln!(
            out,
            ": its screen isn't read here. chant audit --agents found {found} configured on this machine, not {agent} \
             (arugula describe --agents --refresh after you set it up)"
        );
        return out;
    }
    match d.fired.as_deref() {
        Some(rule) => {
            let state = d.rules.iter().find(|r| r.rule == rule);
            let _ = write!(out, ": {} by rule {rule}", state.map(|r| r.state.as_str()).unwrap_or_default());
        }
        None => out.push_str(": no rule matches"),
    }
    let _ = writeln!(out, " (shown: {})", d.shown.as_deref().unwrap_or("nothing yet"));
    let _ = writeln!(out, "title: {}", d.title);
    for r in &d.rules {
        let mark = if r.matched { "matched" } else { "no match" };
        let _ = writeln!(out, "\n{} ({}, {}) {}: {mark}", r.rule, r.state, r.priority, r.region);
        if r.text.iter().all(|l| l.trim().is_empty()) {
            out.push_str("  (empty)\n");
        }
        for l in r.text.iter().filter(|l| !l.trim().is_empty()) {
            let _ = writeln!(out, "  | {}", l.trim_end());
        }
    }
    out
}

/// `describe --agents` for people: what chant found, then whose screen
/// rules run here.
fn inventory_text(inv: &AgentsInventory) -> String {
    use std::fmt::Write;
    let v = &inv.inventory;
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let mut out = String::new();
    match v["state"].as_str() {
        Some("read") => {
            let _ = writeln!(out, "chant {} (audit --agents) found:", s(&v["version"]));
            if v["sites"].as_array().is_none_or(|a| a.is_empty()) {
                out.push_str("  no agent configuration\n");
            }
            for site in v["sites"].as_array().into_iter().flatten() {
                let _ = writeln!(
                    out,
                    "  {:<9} {:<7} {}  {}",
                    s(&site["runtime"]),
                    s(&site["scope"]),
                    s(&site["root"]),
                    s(&site["summary"])
                );
            }
        }
        Some("reading") => out.push_str("chant audit --agents hasn't answered yet\n"),
        Some("off") => out.push_str("not asking chant here (ARUGULA_CHANT is empty)\n"),
        _ => {
            let _ = writeln!(out, "no inventory: {}", v["error"].as_str().unwrap_or("chant didn't say"));
        }
    }
    let (run, off) = (inv.rules.run.join(", "), inv.rules.off.join(", "));
    let _ =
        writeln!(out, "screen rules run for: {}", if off.is_empty() { "every agent with rules".into() } else { run });
    if !off.is_empty() {
        let _ = writeln!(out, "not configured here, so not read: {off}");
    }
    out
}

#[derive(clap::Args)]
pub struct Args {
    #[arg(required_unless_present = "agents")]
    block: Option<Pane>,
    #[arg(long, requires = "block")]
    detection: bool,
    #[arg(long, conflicts_with_all = ["block", "detection"])]
    agents: bool,
    /// With `--agents`: ask chant again first.
    #[arg(long, requires = "agents")]
    refresh: bool,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { agents: true, refresh, .. } => {
            let (inv, v) = match refresh {
                true => {
                    request(&sock, "POST", "/api/hosts/self/agents/refresh", None)?.parse_raw::<AgentsInventory>()?
                }
                false => request(&sock, "GET", "/api/hosts/self/agents", None)?.parse_raw::<AgentsInventory>()?,
            };
            if json_out {
                print_json(&v);
            } else {
                print!("{}", inventory_text(&inv));
            }
        }
        Args { block: Some(block), detection: true, .. } => {
            let v: DetectionAnswer =
                request(&sock, "GET", &format!("/api/panes/{}/detection", block.0), None)?.parse()?;
            print!("{}", detection_text(block.0, &v));
        }
        Args { block: Some(block), .. } => {
            print_json(&request(&sock, "GET", &format!("/api/blocks/{}", block.0), None)?.json()?);
        }
        Args { block: None, .. } => unreachable!("clap requires a block"),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn detection_for_people() {
        let v = serde_json::json!({
            "agent": "claude", "name": "Claude Code", "shown": "blocked", "fired": "permission_prompt",
            "title": "✳ Create a file",
            "rules": [
                {"rule": "permission_prompt", "state": "blocked", "priority": 1000, "region": "after the last rule",
                 "text": [" Do you want to proceed?", "", " ❯ 1. Yes"], "matched": true},
                {"rule": "title_idle", "state": "idle", "priority": 250, "region": "title", "text": [""], "matched": false},
            ],
        });
        let text = super::detection_text(4, &serde_json::from_value(v).unwrap());
        assert!(
            text.starts_with("%4 runs Claude Code (claude): blocked by rule permission_prompt (shown: blocked)\n"),
            "{text}"
        );
        assert!(text.contains("\npermission_prompt (blocked, 1000) after the last rule: matched\n  |  Do you want to proceed?\n  |  ❯ 1. Yes\n"), "{text}");
        assert!(text.contains("\ntitle_idle (idle, 250) title: no match\n  (empty)\n"), "{text}");
        let none = super::detection_text(
            2,
            &serde_json::from_value(serde_json::json!({"agent": null, "command": "vim notes"})).unwrap(),
        );
        assert_eq!(none, "%2 runs `vim notes`: no agent with screen rules\n");
        let unread = super::detection_text(
            3,
            &serde_json::from_value(
                serde_json::json!({"agent": "codex", "name": "Codex", "unread": true, "rules": [], "configured": ["claude"]}),
            )
            .unwrap(),
        );
        assert!(
            unread.starts_with(
                "%3 runs Codex (codex): its screen isn't read here. chant audit --agents found claude configured"
            ),
            "{unread}"
        );
    }

    #[test]
    fn inventory_for_people() {
        let v = serde_json::json!({
            "state": "read", "version": "0.95.0", "notes": [],
            "sites": [{"id": "user-claude", "scope": "user", "runtime": "claude", "root": "/home/ada",
                       "summary": "1 instruction file · model sonnet"}],
            "rules": {"run": ["claude"], "off": ["codex"]},
        });
        assert_eq!(
            super::inventory_text(&serde_json::from_value(v).unwrap()),
            "chant 0.95.0 (audit --agents) found:\n  claude    user    /home/ada  1 instruction file · model sonnet\n\
             screen rules run for: claude\nnot configured here, so not read: codex\n"
        );
        let none = serde_json::json!({"state": "no_chant", "error": "no chant on PATH", "sites": [],
                                      "rules": {"run": ["claude", "codex"], "off": []}});
        assert_eq!(
            super::inventory_text(&serde_json::from_value(none).unwrap()),
            "no inventory: no chant on PATH\nscreen rules run for: every agent with rules\n"
        );
    }
}
