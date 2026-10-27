//! Chat blocks: a plain conversation with Claude, run by the agent block's
//! code with every tool off. The fake ACP agent stands in for Claude Code's
//! adapter (through `$ARUGULA_AGENTS_DIR`) and records what its
//! `session/new` got.
//!
//! What's checked: the session has no tools, settings or MCP servers (not
//! Arugula's either) and a chat's system prompt, in the block's own folder;
//! what would give it tools is left out of its config; Codex is refused, as
//! is an agent block that asks to be a chat; a permission request is turned
//! down unseen; it's named by its first message; the model can be switched;
//! and after a restart its conversation is back and goes on.

// Over the daemon's Unix socket.
#![cfg(unix)]

use crate::agentd::*;

use std::os::unix::fs::PermissionsExt;

use serde_json::{Value, json};

/// A daemon whose Claude Code adapter is the fake.
fn daemon(dir: &std::path::Path) -> Daemon {
    let bin = dir.join("agents/claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let adapter = bin.join("claude-agent-acp");
    std::fs::write(&adapter, format!("#!/bin/sh\nexec python3 {} \"$@\"\n", fake())).unwrap();
    std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o755)).unwrap();
    let agents = dir.join("agents").display().to_string();
    Daemon::child_env(&[], &[("ARUGULA_AGENTS_DIR", agents.as_str())])
}

fn session(d: &Daemon, block: u64) -> Value {
    let sid = d.state(block)["session_id"].as_str().unwrap().to_owned();
    serde_json::from_str(&std::fs::read_to_string(d.sessions.join(format!("{sid}.json"))).unwrap()).unwrap()
}

#[test]
fn a_chat_is_claude_with_no_tools_in_a_folder_of_its_own() {
    let dir = Scratch::new("chat-block");
    let mut d = daemon(&dir);
    // What would give it tools or a project is left out.
    let config = json!({
        "prompt": "remember heron",
        "cwd": "/",
        "command": ["sh", "-c", "exit 1"],
        "mcp_servers": ["x=python3 -c pass"],
        "allow": [{ "tool": "Bash" }],
        "permission_mode": "bypassPermissions",
    });
    let id = d.open_with(json!({ "type": "chat", "config": config }));
    assert_eq!(d.wait(id, "idle"), "done", "{}", d.state(id));
    let info = d.get(&format!("/api/blocks/{id}"))["info"].clone();
    assert_eq!(info["type"], "chat");

    let sess = session(&d, id);
    let o = &sess["meta"]["claudeCode"]["options"];
    assert_eq!(o["tools"], json!([]), "{sess}");
    assert_eq!(o["settingSources"], json!([]));
    assert_eq!(o["extraArgs"], json!({ "strict-mcp-config": "" }));
    assert!(sess["meta"]["systemPrompt"].as_str().unwrap().contains("no tools"), "{sess}");
    assert_eq!(sess["mcp"], json!([]), "no MCP servers, Arugula's neither");
    let cwd = sess["cwd"].as_str().unwrap();
    assert!(cwd.ends_with("/chat") && cwd.starts_with(&d.state.display().to_string()), "{cwd}");
    assert!(sess.get("mode").is_none(), "no permission mode: {sess}");

    // Named by its first message until the session names itself.
    let s = d.state(id);
    assert_eq!(s["title"], "remember heron");
    assert_eq!(s["label"], "Claude");

    // Anything that asks to run something is turned down, unseen.
    d.call(id, "send", json!({ "text": "run touch x" }));
    assert_eq!(d.wait(id, "idle"), "done", "{}", d.state(id));
    let s = d.state(id);
    assert!(s["pending"].as_array().unwrap().is_empty(), "{s}");
    assert!(last_tool(&s)["status"] != "completed", "{s}");

    // The model, from the session's options.
    d.call(id, "set_model", json!({ "model": "haiku" }));
    d.wait_for("the models", || d.state(id)["models"].as_array().is_some_and(|m| m.len() == 3));
    d.call(id, "send", json!({ "text": "model" }));
    d.wait(id, "idle");
    assert!(entries(&d.state(id)).iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("haiku"))));
    let (status, _) = d.raw("POST", &format!("/api/blocks/{id}/call/set_model"), Some(json!({ "model": "gpt" })));
    assert_eq!(status, 400, "only one of its models");

    // After a restart its conversation is back, and goes on.
    d.stop();
    d.start();
    d.wait_for("the block", || d.raw("GET", &format!("/api/blocks/{id}"), None).0 == 200);
    assert_eq!(d.get(&format!("/api/blocks/{id}"))["info"]["type"], "chat");
    d.call(id, "send", json!({ "text": "recall" }));
    d.wait(id, "idle");
    assert!(entries(&d.state(id)).iter().any(|e| e["text"] == "You said heron."), "{}", d.state(id));
    let sess = session(&d, id);
    assert_eq!(sess["meta"]["claudeCode"]["options"]["tools"], json!([]), "still no tools: {sess}");
    assert_eq!(sess["model"], "haiku", "its model again");
}

#[test]
fn only_claude_chats_and_an_agent_block_is_never_one() {
    let dir = Scratch::new("chat-refused");
    let d = daemon(&dir);
    let (status, body) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "chat", "config": { "agent": "codex", "prompt": "hi" } })));
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("Codex has no way to turn all of its tools off"), "{body}");
    let (status, body) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "agent", "config": { "chat": true, "prompt": "hi" } })));
    assert_eq!(status, 400, "{body}");
}
