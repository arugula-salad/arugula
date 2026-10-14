//! What a conversation's watcher of a link here starts from, and which turns
//! it counts.
//!
//! - #895: a pane linked while it's already asking (a permission card, as
//!   `arugula hook` raises it) is a card in the conversation after the hold,
//!   pushed to its people and answered from there like any other.
//! - #888: a turn is one someone prompted (Claude Code's `UserPromptSubmit`
//!   hook, or a follow-up delivered to it). Once the pane has said so, a
//!   turn nobody prompted that says nothing and asks nothing (a client
//!   opening the pane pokes it working until it's quiet) isn't posted.
//!
//! The same over the relay, and the page, are `web/e2e/chat-events.spec.ts`'s.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::agentd::{Daemon, Phone};
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

fn daemon() -> Daemon {
    Daemon::child_labs_with(&["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"])
}

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// The conversation's events, as its timeline has them.
fn events(d: &Daemon, thread: &str) -> Vec<Value> {
    d.get(thread)["messages"].as_array().unwrap().iter().filter_map(|m| m.get("event").cloned()).collect()
}

fn kinds(d: &Daemon, thread: &str) -> Vec<String> {
    events(d, thread).iter().map(|e| e["kind"].as_str().unwrap_or_default().to_owned()).collect()
}

/// A conversation linking `pane` as L1: its thread.
fn link(d: &Daemon, pane: u64) -> (String, String) {
    let c = d.post("/api/convs", json!({ "title": "calc tests" }));
    let id = c["id"].as_str().unwrap().to_owned();
    d.post(&format!("/api/panes/{pane}/conv-grants"), json!({ "conv": id, "conv_title": "calc tests" }));
    d.post(&format!("/api/convs/{id}/links"), json!({ "pane": pane, "title": "claude" }));
    (id.clone(), format!("/api/threads/conv-{id}"))
}

/// Claude Code's hooks, as `arugula attention` and `arugula hook` send them.
fn state(d: &Daemon, pane: u64, state: &str, why: Option<&str>) {
    let (s, v) = d.raw("POST", &format!("/api/panes/{pane}/attention"), Some(json!({ "state": state, "why": why })));
    assert_eq!(s, 200, "{v}");
}

fn hook(d: &Daemon, pane: u64, event: &str) {
    let h = json!({ "hook_event_name": event, "session_id": "6bca3562-8453-4503-b936-ec6830805177" });
    let (s, v) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(h));
    assert_eq!(s, 200, "{v}");
}

/// Wait as long as a card's hold and a little more: what's posted by then
/// is all there is.
fn settle() {
    std::thread::sleep(Duration::from_millis(3_000));
}

#[test]
fn a_pane_linked_while_it_asks_is_a_card_answered_from_the_conversation() {
    let d = daemon();
    let phone = Phone::subscribe(&d);
    // Claude Code's permission hook in a pane, waiting for its answer (its
    // output in `out`, then `exit`).
    let dir = d.sessions.join("early");
    std::fs::create_dir_all(&dir).unwrap();
    let permit = json!({
        "tool_name": "Bash", "tool_input": { "command": "python3 -m pytest -q" },
        "session_id": "s", "hook_event_name": "PermissionRequest",
    });
    std::fs::write(dir.join("perm.json"), permit.to_string()).unwrap();
    let cmd = format!(
        "{bin} hook < {d}/perm.json > {d}/out; echo done > {d}/exit",
        bin = cli_bin().display(),
        d = dir.display()
    );
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    d.wait_for("the permission card", || info(&d, pane)["ask"]["kind"] == "permission");
    let ask = info(&d, pane)["ask"]["id"].as_str().unwrap().to_owned();

    // Linked now: its ask is a card after the hold, as any ask is.
    let (id, thread) = link(&d, pane);
    let mut card = Value::Null;
    d.wait_for("the card", || {
        card = events(&d, &thread).into_iter().find(|e| e["kind"] == "needs_input").unwrap_or_default();
        !card.is_null()
    });
    assert_eq!((card["link"].as_str(), card["ask"]["id"].as_str()), (Some("L1"), Some(ask.as_str())), "{card}");
    assert_eq!(card["ask"]["input"], "Bash: python3 -m pytest -q");
    // Pushed to the conversation's people, as any card is.
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < until, "no push for the card");
        let p = phone.next();
        if p["title"] == "L1 needs you in calc tests" {
            assert_eq!(p["body"], "Bash: python3 -m pytest -q", "{p}");
            break;
        }
    }

    // Answered from the conversation: the hook hears it.
    d.post(&format!("/api/convs/{id}/act"), json!({ "link": "L1", "id": ask, "action": "allow" }));
    d.wait_for("the hook to finish", || dir.join("exit").exists());
    let out = std::fs::read_to_string(dir.join("out")).unwrap();
    assert!(out.contains(r#""behavior":"allow""#), "{out}");
    settle();
    // One card, once (then the hook's command ends: its turn, and exited).
    let k = kinds(&d, &thread);
    assert_eq!(k[..2], ["needs_input", "answered"], "{k:?}");
    assert_eq!(k.iter().filter(|k| *k == "needs_input").count(), 1, "{k:?}");
}

#[test]
fn a_turn_nobody_prompted_with_nothing_to_say_isnt_posted() {
    let d = daemon();
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let (_, thread) = link(&d, pane);
    // The watcher is up once its link is: give it a moment.
    std::thread::sleep(Duration::from_millis(500));

    // Prompted (the lead's prompt_agent types it; Claude Code's
    // UserPromptSubmit says so), worked, ended with its last message.
    hook(&d, pane, "UserPromptSubmit");
    state(&d, pane, "working", None);
    state(&d, pane, "done", Some("Both tests pass."));
    d.wait_for("the turn", || kinds(&d, &thread) == ["turn_ended"]);

    // A client opens the pane: working, then quiet 2.5 s later, with
    // nobody prompting it (M85's live run, three times).
    state(&d, pane, "working", None);
    std::thread::sleep(Duration::from_millis(2_500));
    state(&d, pane, "idle", None);
    settle();
    assert_eq!(kinds(&d, &thread), ["turn_ended"], "a poke isn't a turn");

    // A follow-up from the conversation's pane thread: queued, then taken
    // by Claude Code's inbox hook. Its turn is one, even saying nothing.
    d.post(&format!("/api/panes/{pane}/followup"), json!({ "text": "say hi" }));
    let inbox = json!({ "hook_event_name": "Stop", "session_id": "s" });
    let (s, v) = d.raw("POST", &format!("/api/panes/{pane}/inbox"), Some(inbox));
    assert_eq!(s, 200, "{v}");
    assert!(v.contains("say hi"), "{v}");
    state(&d, pane, "working", None);
    std::thread::sleep(Duration::from_millis(2_500));
    state(&d, pane, "idle", None);
    d.wait_for("the follow-up's turn", || kinds(&d, &thread).len() == 2);
    let last = events(&d, &thread).pop().unwrap();
    assert!(last["worked_ms"].as_u64().unwrap() >= 2_000, "{last}");
}
