//! #644: a mention waiting for a Claude Code session goes in at its next tool
//! call, rather than when its turn ends: `arugula hook` asks on `PostToolUse`,
//! and the daemon answers with the mention. Against a fake control (the relay
//! socket sends `agents.mention` and takes the turn reports), with the hooks
//! posted as `arugula hook` posts them. A real session's in `agents_real`.

#![cfg(unix)]

use arugula_proto::api::{HOOK_MENTIONS_TOLD, HOOK_TAKES_MENTION};
use arugula_testkit::arugulad;
use serde_json::{Value, json};

use super::{
    mention_agent_block::{Fake, answer, enroll, serve, states},
    mention_held::fake_claude,
};

fn info(d: &arugula_testkit::Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A `PostToolUse` of the session, asking for a mention or not, and what the
/// daemon answers.
fn tool_ended(d: &arugula_testkit::Daemon, pane: u64, more: Value) -> Value {
    let mut hook = json!({ "hook_event_name": "PostToolUse", "session_id": "sess-1", "tool_name": "Bash",
        "tool_use_id": "toolu_1", "tool_input": { "command": "ls" }, "tool_response": {} });
    for (k, v) in more.as_object().unwrap() {
        hook[k] = v.clone();
    }
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(hook));
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[test]
fn a_tool_call_takes_a_waiting_mention_into_the_running_turn() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let root = std::env::temp_dir().join(format!("ilg-tool-call-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let cfg = root.join("cfg");
    std::fs::create_dir_all(cfg.join("sessions")).unwrap();
    // The conversation's title is the session's name (Worker).
    let project = cfg.join("projects/-work");
    std::fs::create_dir_all(&project).unwrap();
    let line = |v: Value| format!("{v}\n");
    std::fs::write(
        project.join("sess-1.jsonl"),
        line(json!({ "type": "user", "uuid": "u1", "parentUuid": null, "sessionId": "sess-1", "cwd": "/work",
                     "timestamp": "2026-10-02T20:00:00.000Z", "isSidechain": false,
                     "message": { "role": "user", "content": "hello" } }))
            + &line(json!({ "type": "ai-title", "aiTitle": "Worker", "sessionId": "sess-1" })),
    )
    .unwrap();
    let mut d = arugulad!("mentiontool")
        .env("CLAUDE_CONFIG_DIR", &cfg)
        .args(["--owner", "owner@example.com", "--tailscale-socket", "/nonexistent/sock"])
        .args(["--wisp-token-file", "/nonexistent"])
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();
    let claude = fake_claude(&root, &cfg);
    let pane = d.post("/api/run", json!({ "command": "exec bash --norc" }))["pane"].as_u64().unwrap();
    d.wait_for("the shell", || info(&d, pane)["running"] == true);
    std::thread::sleep(std::time::Duration::from_millis(500));
    d.post(&format!("/api/panes/{pane}/send"), json!({ "text": claude.display().to_string(), "enter": true }));
    d.wait_for("claude without hooks", || info(&d, pane)["unhooked"] == true);
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let takes = json!({ HOOK_TAKES_MENTION: true });

    // Nothing waits: the answer is empty, as it always was. These hooks are
    // also what says the session's hooks are live.
    assert_eq!(tool_ended(&d, pane, json!({})), json!({}));
    assert_eq!(tool_ended(&d, pane, takes.clone()), json!({}));
    d.wait_for("hooks live", || info(&d, pane)["unhooked"] != true);

    // The mention names the session, and it's mid-turn: it waits, and isn't
    // working yet.
    let mention =
        json!({ "t": "op", "id": "m1", "op": { "op": "agents.mention", "room": "ch_x", "room_name": "general",
        "seq": 7, "author": "Ada", "text": "which token?", "session": "worker" } })
        .to_string();
    f.outbox.lock().unwrap().push(mention);
    d.wait_for("the daemon's answer", || answer(&f, "m1").is_some());
    let a = answer(&f, "m1").unwrap();
    assert_eq!(a["routed"]["session"], "Worker", "{a}");
    assert!(states(&f).is_empty(), "not taken yet: {:?}", states(&f));

    // A session that never heard how these arrive (it started before this
    // version) doesn't take it: it could read it as injected text.
    assert_eq!(tool_ended(&d, pane, takes.clone()), json!({}));
    assert!(states(&f).is_empty(), "{:?}", states(&f));
    let start = json!({ "hook_event_name": "SessionStart", "session_id": "sess-1", HOOK_MENTIONS_TOLD: true });
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(start));
    assert_eq!(status, 200, "{body}");

    // A hook that doesn't ask (an older CLI, a session nobody drives) and a
    // subagent's tool call don't take it: it would end with the subagent.
    assert_eq!(tool_ended(&d, pane, json!({})), json!({}));
    let sub = json!({ HOOK_TAKES_MENTION: true, "agent_id": "a1" });
    assert_eq!(tool_ended(&d, pane, sub), json!({}));
    assert!(states(&f).is_empty(), "{:?}", states(&f));

    // The session's own next tool call does: working, at once.
    let a = tool_ended(&d, pane, takes.clone());
    let text = a["follow_up"]["text"].as_str().unwrap_or_else(|| panic!("{a}"));
    assert!(text.contains("Message from Ada in general") && text.contains("which token?"), "{text}");
    assert!(text.contains("not an approval"), "framed as a teammate's: {text}");
    d.wait_for("working", || states(&f) == [(7, "working".into())]);

    // Once only, and the turn's `Stop` says it was read.
    assert_eq!(tool_ended(&d, pane, takes), json!({}));
    let stop = json!({ "hook_event_name": "Stop", "session_id": "sess-1" });
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(stop));
    assert_eq!(status, 200, "{body}");
    d.wait_for("read", || states(&f).contains(&(7, "read".into())));

    drop(d);
    let _ = std::fs::remove_dir_all(&root);
}
