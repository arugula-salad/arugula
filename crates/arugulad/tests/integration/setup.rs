//! Getting started's "Use Claude Code with Arugula" (`POST
//! /api/setup/agents/claude`): with stand-ins for Claude Code, Node and npm,
//! in a home of the test's own, it adds Arugula's hooks to Claude Code's
//! settings.json and says so, naming the file; the Agents step's check
//! (`GET /api/setup?part=agents`, `claude.hooks`) follows.

use std::{path::Path, process::Command};

use arugula_testkit::arugulad;
use serde_json::{Value, json};

/// A shell script at `path`, executable.
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn use_claude_code_installs_the_hooks_and_says_where() {
    // The daemon runs the `arugula` beside it.
    assert!(Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap().success());
    let state = std::env::temp_dir().join(format!("ilg-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let bin = state.join("bin");
    // A Node new enough, an npm that "installs" the adapter, and a Claude
    // Code that has Arugula's MCP server (`claude mcp get` succeeds).
    script(&bin.join("node"), "echo v22.1.0");
    script(&bin.join("npm"), "echo 'added 1 package'");
    script(&bin.join("claude"), "exit 0");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let d = arugulad!("setup")
        .state_dir(&state)
        .home_in_state()
        .no_wisp()
        .env("PATH", &path)
        .env("ARUGULA_NPM", bin.join("npm"))
        .env("ARUGULA_AGENTS_DIR", state.join("agents"))
        .start();
    let settings = state.join("home/.claude/settings.json");
    let agents = || d.get("/api/setup?part=agents")["claude"].clone();
    assert_eq!(agents()["hooks"], false, "{}", agents());

    let o: Value = d.post("/api/setup/agents/claude", json!({}));
    assert_eq!(o["ok"], true, "{o}");
    let done: Vec<&str> = o["done"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(done.iter().any(|l| l.starts_with("Added Arugula's hooks to ~/.claude/settings.json: ")), "{done:?}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let ask = &v["hooks"]["PreToolUse"][0];
    assert_eq!(
        (ask["matcher"].clone(), ask["hooks"][0]["command"].clone()),
        (json!("AskUserQuestion"), json!("arugula ask"))
    );
    assert_eq!(v["hooks"]["PermissionRequest"][0]["hooks"][0]["command"], "arugula hook");
    assert_eq!(agents()["hooks"], true, "{}", agents());

    // Again: the hooks are there, so no line says they were added.
    let o = d.post("/api/setup/agents/claude", json!({}));
    assert_eq!(o["ok"], true, "{o}");
    assert!(!o["done"].to_string().contains("hooks to"), "{o}");
}
