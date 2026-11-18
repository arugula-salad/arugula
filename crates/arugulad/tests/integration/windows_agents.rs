//! #1178: agent blocks on Windows, against the scripted fake agent server
//! (`fake_acp.py`, as `agents.rs` drives it on Unix): it starts, answers,
//! asks before it acts, and closing it ends the agent server and
//! everything it started (its job object).

#![cfg(windows)]

use serde_json::json;

use crate::agentd::{self, Daemon, entries, last_tool};

/// How long a wait here lasts (`windows.rs`'s WAIT: GitHub's runner starts
/// several daemons cold at once).
const WAIT: u64 = 60;

#[test]
fn an_agent_block_starts_answers_asks_and_closes() {
    let d = Daemon::child();
    let id = d.open("hello");
    assert_eq!(d.wait_secs(id, "idle", WAIT), "done", "{}", d.state(id));
    let s = d.state(id);
    assert_eq!((s["status"].as_str(), s["server"]["name"].as_str()), (Some("ready"), Some("fake-acp")), "{s}");
    assert!(entries(&s).iter().any(|e| e["type"] == "agent" && e["text"] == "Hello! I am fake."), "{s}");

    // It asks before it runs something; approving runs it.
    d.call(id, "send", json!({ "text": "run touch x" }));
    assert_eq!(d.wait_secs(id, "needs-input", WAIT), "needs_input");
    let s = d.state(id);
    let p = &s["pending"][0];
    assert_eq!((p["tool"].as_str(), p["command"].as_str()), (Some("Bash"), Some("touch x")), "{s}");
    d.call(id, "approve", json!({ "id": p["id"] }));
    assert_eq!(d.wait_secs(id, "idle", WAIT), "done");
    let tool = last_tool(&d.state(id));
    assert_eq!((tool["status"].as_str(), tool["exit"].as_i64()), (Some("completed"), Some(0)), "{tool}");

    // Closing it ends the agent server.
    let pid = d.state(id)["pid"].as_u64().unwrap();
    assert!(agentd::alive(pid));
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    d.wait_for("the agent server to go", || !agentd::alive(pid));
}

/// As npm installs an adapter: a `.cmd` shim, named without its extension.
/// Through cmd.exe, with something of its own still running after the
/// agent server's stdin closes: closing the block ends all of it.
#[test]
fn closing_an_agent_ends_what_it_started() {
    let d = Daemon::child();
    let shim = d.sessions.join("fake-acp.cmd");
    let script = format!(
        "@echo off\r\nstart \"\" /b ping -n 600 127.0.0.1 >nul\r\n{} \"{}\" %*\r\n",
        agentd::PYTHON,
        agentd::fake()
    );
    std::fs::write(&shim, script).unwrap();
    let command = [d.sessions.join("fake-acp").display().to_string()];
    let config = json!({ "agent": "acp", "command": command, "cwd": d.sessions, "prompt": "hello" });
    let id = d.open_with(json!({ "type": "agent", "config": config }));
    assert_eq!(d.wait_secs(id, "idle", WAIT), "done", "{}", d.state(id));
    let pid = d.state(id)["pid"].as_u64().unwrap();
    // cmd.exe's: Python and ping (and its console's conhost).
    let mut started = vec![];
    d.wait_for("the shim's programs", || {
        started = children(pid);
        let has = |name: &str| started.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
        has("ping.exe") && has("python.exe")
    });
    d.post(&format!("/api/panes/{id}/close"), json!({}));
    d.wait_for("the agent server and what it started to go", || {
        !agentd::alive(pid) && started.iter().all(|(_, p)| !agentd::alive(*p))
    });
}

/// The processes `pid` started, by name (Windows keeps a parent's id
/// after it has gone, so this still names them then).
fn children(pid: u64) -> Vec<(String, u64)> {
    let query = format!(
        "Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}' | ForEach-Object {{ $_.Name + ' ' + $_.ProcessId }}"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &query])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().rsplit_once(' ').and_then(|(n, p)| Some((n.to_owned(), p.parse().ok()?))))
        .collect()
}
