//! #579: a mention is never typed into a Claude Code session whose hooks
//! aren't listening. It is held as a card on the pane: "deliver" (the
//! owner's alone) types it and presses Enter once, "dismiss" drops it, and
//! a silenced session isn't reached at all. Against a fake control (the
//! relay socket sends `agents.mention` and takes the daemon's answer and its
//! turn reports) and a real daemon with a fake `claude` in a terminal that
//! keeps printing, as a session mid-turn does.

#![cfg(unix)]

use std::{path::Path, time::Duration};

use arugula_testkit::arugulad;
use serde_json::{Value, json};

use super::mention_agent_block::{Fake, answer, enroll, serve, states};

/// `seq` of room `ch_x`, from Ada, to the session called Worker.
fn mention(id: &str, seq: i64) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "ch_x", "room_name": "general",
        "seq": seq, "author": "Ada", "text": "please look at the build\nand tell me", "session": "worker" } })
    .to_string()
}

fn info(d: &arugula_testkit::Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A `claude` that holds the session `sess` (as Claude Code does, in its
/// config dir's `sessions/`), prints a dot every 100ms (a turn in progress),
/// and appends each line typed into it to `typed`.
pub(super) fn fake_claude(root: &Path, cfg: &Path) -> std::path::PathBuf {
    let script = root.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\n\
             printf '{{\"pid\":%d,\"sessionId\":\"sess-1\",\"kind\":\"interactive\",\"entrypoint\":\"cli\",\"status\":\"busy\"}}' $$ > {cfg}/sessions/$$.json\n\
             (while true; do printf .; sleep 0.1; done) &\n\
             while IFS= read -r l; do echo \"$l\" >> {typed}; done\n",
            cfg = cfg.display(),
            typed = root.join("typed").display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    script
}

fn typed(root: &Path) -> String {
    std::fs::read_to_string(root.join("typed")).unwrap_or_default()
}

#[test]
fn a_mention_for_a_session_without_hooks_is_held_and_only_the_owners_deliver_types_it() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let root = std::env::temp_dir().join(format!("ilg-held-{}", std::process::id()));
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
    let mut d = arugulad!("mentionheld")
        .env("CLAUDE_CONFIG_DIR", &cfg)
        .args(["--owner", "owner@example.com", "--tailscale-socket", "/nonexistent/sock"])
        .args(["--wisp-token-file", "/nonexistent"])
        .wait_secs(30)
        .start();
    d.stop();
    enroll(&d.state, &url);
    d.start();

    let claude = fake_claude(&root, &cfg);
    // Run from a shell, so it is a foreground process the daemon can name.
    let pane = d.post("/api/run", json!({ "command": "exec bash --norc" }))["pane"].as_u64().unwrap();
    d.wait_for("the shell", || info(&d, pane)["running"] == true);
    std::thread::sleep(Duration::from_millis(500));
    d.post(&format!("/api/panes/{pane}/send"), json!({ "text": claude.display().to_string(), "enter": true }));
    d.wait_for("claude without hooks", || info(&d, pane)["unhooked"] == true);
    d.wait_for("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let reason = |d: &arugula_testkit::Daemon| info(d, pane)["reason"].clone();

    // The mention names it: queued (its hooks aren't listening), a card
    // on the pane, and nothing typed while the session prints.
    f.outbox.lock().unwrap().push(mention("m1", 7));
    d.wait_for("the daemon's answer", || answer(&f, "m1").is_some());
    let a = answer(&f, "m1").unwrap();
    assert_eq!((a["routed"]["step"].as_str(), a["routed"]["queued"].as_bool()), (Some("named"), Some(true)), "{a}");
    assert_eq!(a["routed"]["session"], "Worker", "{a}");
    d.wait_for("the card", || reason(&d)["kind"] == "input");
    let card = reason(&d);
    assert_eq!(card["headline"], "Ada's agent mentioned this session in #general: please look at the build", "{card}");
    assert_eq!(card["actions"], json!(["deliver", "dismiss"]), "{card}");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(typed(&root), "", "a held mention is never typed");
    assert!(states(&f).is_empty(), "queued is not working: {:?}", states(&f));

    // Not the owner's: an agent on the owner's CLI (MCP's `agent_respond`
    // can't say deliver or dismiss at all).
    let bearer = d.bearer();
    let agent = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    for action in ["deliver", "dismiss"] {
        let body = json!({ "action": action, "pane": pane }).to_string();
        let (status, _, text) = d.tcp("POST", "/api/attention/act", &agent, Some(&body));
        assert_eq!(status, 403, "{action}: {text}");
        assert!(text.contains("only this machine's owner"), "{action}: {text}");
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(typed(&root), "", "nothing an agent did typed it");
    assert_eq!(reason(&d)["kind"], "input", "the card stays");

    // The owner's deliver types the framed text and Enter, once: working.
    let r = d.post("/api/attention/act", json!({ "action": "deliver", "pane": pane }));
    assert_eq!(r["results"][0]["ok"], true, "{r}");
    d.wait_for("the mention typed", || typed(&root).contains("Message from Ada in general"));
    assert_eq!(typed(&root).matches("Message from Ada").count(), 1, "{}", typed(&root));
    d.wait_for("working", || states(&f) == [(7, "working".into())]);
    d.wait_for("the card gone", || info(&d, pane)["reason"].is_null());
    let (status, again) = d.raw("POST", "/api/attention/act", Some(json!({ "action": "deliver", "pane": pane })));
    assert_eq!(status, 409, "nothing left to deliver: {again}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(typed(&root).matches("Message from Ada").count(), 1, "delivered once");

    // Dismiss drops it: nothing is typed, and there's nothing to deliver.
    f.outbox.lock().unwrap().push(mention("m2", 8));
    d.wait_for("the second card", || reason(&d)["kind"] == "input");
    let r = d.post("/api/attention/act", json!({ "action": "dismiss", "pane": pane }));
    assert_eq!(r["results"][0]["ok"], true, "{r}");
    d.wait_for("the card gone", || info(&d, pane)["reason"].is_null());
    let (status, r) = d.raw("POST", "/api/attention/act", Some(json!({ "action": "deliver", "pane": pane })));
    assert_eq!(status, 409, "a dismissed mention can't be delivered: {r}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!typed(&root).contains("message 8"), "{}", typed(&root));

    // Silenced: not reached, no card, and the sender is told which.
    // An agent on the owner's CLI can neither silence nor allow.
    for on in [false, true] {
        let body = json!({ "on": on }).to_string();
        let (status, _, text) = d.tcp("POST", &format!("/api/panes/{pane}/mentions"), &agent, Some(&body));
        assert_eq!(status, 403, "on={on}: {text}");
        assert_eq!(info(&d, pane)["mentions_off"], Value::Null, "{text}");
    }
    d.post(&format!("/api/panes/{pane}/mentions"), json!({ "on": false }));
    let body = json!({ "on": true }).to_string();
    let (status, _, text) = d.tcp("POST", &format!("/api/panes/{pane}/mentions"), &agent, Some(&body));
    assert_eq!(status, 403, "{text}");
    assert_eq!(info(&d, pane)["mentions_off"], true, "an agent can't un-silence its pane");
    assert_eq!(info(&d, pane)["mentions_off"], true);
    f.outbox.lock().unwrap().push(mention("m3", 9));
    d.wait_for("the daemon's answer", || answer(&f, "m3").is_some());
    let a = answer(&f, "m3").unwrap();
    assert_eq!((a["routed"]["step"].as_str(), a["routed"]["silenced"].as_bool()), (Some("inbox"), Some(true)), "{a}");
    assert!(info(&d, pane)["reason"].is_null());
    d.post(&format!("/api/panes/{pane}/mentions"), json!({ "on": true }));

    // Hooks start listening before the owner taps: the inbox hook takes it
    // as for any session, the card goes, and nothing is typed.
    f.outbox.lock().unwrap().push(mention("m4", 10));
    d.wait_for("the card", || reason(&d)["kind"] == "input");
    let hook =
        std::fs::read_to_string(format!("{}/../daemon/tests/fixtures/s18-hook-stop.json", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let hook: Value = serde_json::from_str(&hook).unwrap();
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/inbox"), Some(hook));
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("message 10"), "the hook was handed it: {body}");
    d.wait_for("the card gone", || info(&d, pane)["reason"].is_null());
    assert!(!typed(&root).contains("message 10"), "{}", typed(&root));

    drop(d);
    let _ = std::fs::remove_dir_all(&root);
}
