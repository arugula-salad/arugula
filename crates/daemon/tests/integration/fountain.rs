//! Compatibility for saved blocks that wore remote definitions locally.
#![cfg(unix)]
use crate::agentd::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path};

fn layout_config(d: &Daemon, block: u64) -> Value {
    let l: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("layout.json")).unwrap_or_default())
        .unwrap_or_default();
    l["panes"][block.to_string()]["config"].clone()
}

fn agent_mcp(d: &Daemon, agent: u64, tool: &str, args: Value) -> Result<Value, String> {
    let answers = || -> Vec<String> {
        entries(&d.state(agent))
            .into_iter()
            .filter_map(|e| e["text"].as_str().and_then(|t| t.strip_prefix("MCP ")).map(str::to_owned))
            .collect()
    };
    let before = answers().len();
    d.call(agent, "send", json!({ "text": format!("mcp {tool} {args}") }));
    d.wait_for(&format!("{tool}'s answer"), || answers().len() > before);
    let r: Value = serde_json::from_str(answers().last().unwrap()).unwrap();
    if r["isError"] == true || r.get("error").is_some() {
        return Err(r.to_string());
    }
    Ok(r["structuredContent"].clone())
}

fn fake_claude(dir: &Path) -> String {
    let bin = dir.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join("claude-agent-acp");
    std::fs::write(&path, format!("#!/bin/sh\nexec python3 {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.join("agents").display().to_string()
}

/// Wearing a Managoat agent here (M44) was retired on 2026-10-15 (#942).
/// A new block, `arugula agent --as-fountain`'s config and MCP's
/// `start_agent {as_fountain}` are refused with the reason. A block saved
/// wearing one by an older daemon comes back with its transcript, says why
/// it won't start, and never starts as a plain Claude Code, then or after
/// another restart.
#[test]
fn a_block_saved_wearing_a_fountain_agent_says_why() {
    let dir = Scratch::new("fountain-wear-gone");
    let agents = fake_claude(&dir);
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut d = Daemon::child_env(&[], &[("ARUGULA_AGENTS_DIR", &agents)]);

    // New: refused, with the reason, and nothing opened.
    let panes = d.get("/api/panes").as_array().unwrap().len();
    let (status, body) = d.raw(
        "POST",
        "/api/blocks",
        Some(json!({ "type": "agent", "local": true, "config": { "agent": "claude", "as_fountain": "games" } })),
    );
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("was retired") && body.contains("recipe"), "{body}");
    assert_eq!(d.get("/api/panes").as_array().unwrap().len(), panes, "nothing opened");

    // start_agent: refused, not quietly a plain Claude Code.
    let plain = d.open("hello");
    assert_eq!(d.wait(plain, "idle"), "done");
    let err = agent_mcp(&d, plain, "start_agent", json!({ "prompt": "hi", "as_fountain": "games" })).unwrap_err();
    assert!(err.contains("was retired"), "{err}");

    // Saved by an older daemon: a Claude Code block that wore games.
    let config = json!({ "agent": "claude", "cwd": work, "prompt": "remember kestrel" });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done", "{}", d.state(id));
    let said = entries(&d.state(id)).len();
    let sessions = |d: &Daemon| std::fs::read_dir(&d.sessions).map(|r| r.count()).unwrap_or(0);
    d.wait_for("saved", || layout_config(&d, id)["agent"] == "claude");
    d.stop();
    let path = d.state.join("layout.json");
    let mut l: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    l["panes"][id.to_string()]["config"]["as_fountain"] = json!("games");
    l["panes"][id.to_string()]["config"]["specs"] = json!("~/agent-specs");
    std::fs::write(&path, serde_json::to_vec(&l).unwrap()).unwrap();
    let before = sessions(&d);
    for round in 0..2 {
        d.start();
        d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
        d.wait_for("the reason", || d.state(id)["status"] == "exited");
        let st = d.state(id);
        assert!(st["error"].as_str().unwrap_or_default().contains("was retired"), "{round}: {st}");
        assert!(st["pid"].is_null(), "{round}: it never started: {st}");
        assert_eq!(st["as_fountain"], "games");
        assert!(st["worn"].is_null(), "{st}");
        // Its transcript is all there.
        let e = entries(&st);
        assert!(e.len() >= said, "{round}: {st}");
        assert!(e.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("remember kestrel"))), "{st}");
        // Kept, so it keeps saying so rather than starting plain.
        assert_eq!(layout_config(&d, id)["as_fountain"], "games");
        let _ = d.raw("POST", &format!("/api/blocks/{id}/call/send"), Some(json!({ "text": "recall" })));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let st = d.state(id);
        assert!(st["pid"].is_null() && st["status"] == "exited", "{round}: a prompt doesn't start it: {st}");
        assert_eq!(sessions(&d), before, "{round}: no new session");
        d.stop();
    }
}
