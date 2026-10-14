//! #858: a conversation's lead, on one machine. It starts with the
//! conversation (Claude Code's adapter is the fake ACP agent here). A
//! member's `@lead` is its next prompt, with the conversation's links by
//! name; its reply is its `post_thread`, or its turn's last message when it
//! posts nothing; and a reply notifies whoever asked, on their phone too.
//! (Links on another machine, cards, the restart and the reach are
//! `web/e2e/chat-lead.spec.ts`'s, with a control and two machines.)

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::{Duration, Instant};

use crate::agentd::{self, Daemon, Phone};
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const DRIVER: &str = "driver@example.com";

/// An agents directory whose Claude Code adapter is the fake ACP agent.
fn agents_dir(sessions: &std::path::Path) -> std::path::PathBuf {
    let dir = sessions.with_extension("agents");
    let bin = dir.join("claude/node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let adapter = bin.join("claude-agent-acp");
    std::fs::write(&adapter, format!("#!/bin/sh\nexec python3 {} \"$@\"\n", agentd::fake())).unwrap();
    std::fs::set_permissions(&adapter, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    dir
}

fn as_driver(d: &Daemon, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let (s, b) = d.raw_as(DRIVER, method, path, body);
    (s, serde_json::from_str(&b).unwrap_or(Value::String(b)))
}

/// The lead's messages in `thread`, waiting until there are `n`.
fn lead_says(d: &Daemon, thread: &str, n: usize) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let said: Vec<String> = d.get(thread)["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["who"] == "lead")
            .map(|m| m["text"].as_str().unwrap().to_owned())
            .collect();
        if said.len() >= n || Instant::now() > until {
            assert_eq!(said.len(), n, "the lead's messages: {said:?}");
            return said;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn a_lead_answers_an_at_lead_and_tells_whoever_asked() {
    let scratch = std::env::temp_dir().join(format!("arugula-lead-{}", std::process::id()));
    let agents = agents_dir(&scratch);
    let d = Daemon::child_labs_env(
        &["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
        &[("ARUGULA_AGENTS_DIR", agents.to_str().unwrap())],
    );
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{DRIVER}"), "role": "editor" }));
    let phone = Phone::subscribe_as(&d, Some(DRIVER));

    // It starts with the conversation: an agent block here.
    let c = d.post("/api/convs", json!({ "title": "calc tests" }));
    let id = c["id"].as_str().unwrap().to_owned();
    let lead = c["lead"]["block"].as_u64().expect("a lead");
    assert_eq!(d.state(lead)["agent"], "claude", "a Claude Code block");
    let thread = format!("/api/threads/conv-{id}");
    d.post(&format!("/api/convs/{id}/members"), json!({ "who": "@driver" }));
    assert!(phone.next()["title"].as_str().unwrap().ends_with("added you to calc tests"));
    // A second lead while it's there: no.
    let (s, _) = d.raw("POST", &format!("/api/convs/{id}/lead"), Some(json!({})));
    assert_eq!(s, 409);

    // L1, an agent here.
    let l1 = d.open("hello");
    d.wait(l1, "idle");
    d.post(&format!("/api/panes/{l1}/conv-grants"), json!({ "conv": id, "conv_title": "calc tests" }));
    d.post(&format!("/api/convs/{id}/links"), json!({ "pane": l1, "title": "helper" }));

    // A member's @lead is its next prompt; it prompts L1 by name and says
    // so without posting: its last message is posted for it.
    let ask = r#"@lead please ;; mcp prompt_agent {"pane":"L1","text":"hello","timeout":20} ;; say L1 said hello"#;
    let (s, v) = as_driver(&d, "POST", &thread, Some(json!({ "text": ask })));
    assert_eq!(s, 200, "{v}");
    assert!(v["agent"]["delivered"].is_boolean(), "it went to the lead: {v}");
    assert_eq!(v["message"]["landed"], json!(["lead"]));
    let said = lead_says(&d, &thread, 1);
    let lines: Vec<&str> = said[0].lines().collect();
    assert_eq!(lines.len(), 2, "{said:?}");
    assert!(lines[0].contains(r#""result": "done""#) && lines[0].contains(r#""link": "L1""#), "{said:?}");
    assert!(!lines[0].contains(&format!("%{l1}")), "named, not numbered: {said:?}");
    assert_eq!(lines[1], "L1 said hello");
    // Whoever asked is told, opening the conversation.
    let push = phone.next();
    assert_eq!(push["title"], "The lead answered you in calc tests", "{push}");
    assert_eq!(push["thread"], format!("conv-{id}"));
    assert!(push["body"].as_str().unwrap().starts_with("MCP"), "its first 200 characters: {push}");

    // What it was told: who wrote what, and the links by name.
    let (_, told) = d.raw("GET", &format!("/api/panes/{lead}/capture"), None);
    assert!(told.contains("driver"), "{told}");
    assert!(told.contains("- L1: helper on"), "{told}");

    // A reply it posts itself is the reply: nothing more is posted for it.
    let ask = r#"@lead mcp post_thread {"text":"posted myself"}"#;
    let (s, v) = as_driver(&d, "POST", &thread, Some(json!({ "text": ask })));
    assert_eq!(s, 200, "{v}");
    let said = lead_says(&d, &thread, 2);
    assert_eq!(said[1], "posted myself");
    let push = phone.next();
    assert_eq!(
        (push["title"].as_str(), push["body"].as_str()),
        (Some("The lead answered you in calc tests"), Some("posted myself"))
    );
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(lead_says(&d, &thread, 2).len(), 2, "its last message isn't posted too");
    assert!(phone.quiet(500), "one reply, one push");

    drop(d);
    let _ = std::fs::remove_dir_all(agents);
}
