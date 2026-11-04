//! #580, #675: replies of one thread waiting on a terminal pane's Claude
//! Code go in as one follow-up, through the mux's queue and the daemon's
//! `TurnReports::joined`, which is the only reader of the tags the mux
//! carries. Against a fake control (the relay socket sends `agents.mention`
//! and takes the turn reports), with the hooks posted as `arugula hook`
//! posts them. The agent block's counterpart is in `mention_agent_block`.

#![cfg(unix)]

use arugula_proto::api::{HOOK_MENTIONS_TOLD, HOOK_TAKES_MENTION};
use arugula_testkit::arugulad;
use serde_json::{Value, json};

use super::{
    mention_agent_block::{Fake, answer, enroll, reply, serve, states},
    mention_held::fake_claude,
};

fn info(d: &arugula_testkit::Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A `PostToolUse` of the session that asks for a mention, and what the
/// daemon answers.
fn tool_ended(d: &arugula_testkit::Daemon, pane: u64) -> Value {
    let hook = json!({ "hook_event_name": "PostToolUse", "session_id": "sess-1", "tool_name": "Bash",
        "tool_use_id": "toolu_1", "tool_input": { "command": "ls" }, "tool_response": {},
        HOOK_TAKES_MENTION: true });
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(hook));
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

/// Three replies wait on the session mid-turn, two of one thread around
/// one of another: its next tool call takes the thread's two as one text,
/// oldest first, both working at once; the next takes the other alone,
/// with its own framing; the turn's `Stop` reads all three.
#[test]
fn replies_waiting_on_a_terminal_pane_are_one_follow_up_per_thread() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let root = std::env::temp_dir().join(format!("ilg-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let cfg = root.join("cfg");
    std::fs::create_dir_all(cfg.join("sessions")).unwrap();
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
    let mut d = arugulad!("mentionmerge")
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
    // Its hooks are live and were told how mentions arrive.
    assert_eq!(tool_ended(&d, pane), json!({}));
    let start = json!({ "hook_event_name": "SessionStart", "session_id": "sess-1", HOOK_MENTIONS_TOLD: true });
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(start));
    assert_eq!(status, 200, "{body}");

    for (id, seq, parent, text) in
        [("m1", 11, 10, "first reply"), ("m2", 21, 20, "elsewhere"), ("m3", 12, 10, "second reply")]
    {
        f.outbox.lock().unwrap().push(reply(id, seq, parent, text));
        d.wait_for("handed over", || answer(&f, id).is_some());
        let a = answer(&f, id).unwrap();
        assert_eq!(a["routed"]["session"], "Worker", "{a}");
    }
    assert!(states(&f).is_empty(), "queued is not working: {:?}", states(&f));

    let a = tool_ended(&d, pane);
    let text = a["follow_up"]["text"].as_str().unwrap_or_else(|| panic!("{a}"));
    assert!(
        text.contains("in the thread \"root 10\":\nAda: first reply\nAda: second reply\n"),
        "the thread's replies as one, oldest first: {text}"
    );
    assert!(!text.contains("elsewhere"), "the other thread's waits: {text}");
    d.wait_for("both working", || states(&f) == [(11, "working".into()), (12, "working".into())]);

    let a = tool_ended(&d, pane);
    let text = a["follow_up"]["text"].as_str().unwrap_or_else(|| panic!("{a}"));
    assert!(text.contains("elsewhere") && !text.contains("first reply"), "{text}");
    assert!(text.starts_with("Message from Ada"), "a lone reply keeps its own framing: {text}");
    d.wait_for("the third working", || states(&f).len() == 3);
    assert_eq!(tool_ended(&d, pane), json!({}), "nothing left");

    let stop = json!({ "hook_event_name": "Stop", "session_id": "sess-1" });
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(stop));
    assert_eq!(status, 200, "{body}");
    d.wait_for("all read", || states(&f).iter().filter(|(_, s)| s == "read").count() == 3);
    let read: Vec<i64> = states(&f).iter().filter(|(_, s)| s == "read").map(|(n, _)| *n).collect();
    assert_eq!(read, [11, 12, 21]);

    drop(d);
    let _ = std::fs::remove_dir_all(&root);
}
