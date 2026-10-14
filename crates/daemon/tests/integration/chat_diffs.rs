//! #887: a linked pane's edit waiting as an IDE diff, in its conversation.
//!
//! arugulad is Claude Code's IDE, so a Write asks its permission first (the
//! hook's card), then waits as a diff (`fake_claude.py`'s `openDiff`), and
//! the ask can't be answered any more. The conversation's card for the ask
//! is retired by the diff's card, which shows the diff and is accepted or
//! rejected from the conversation, through the pane's machine, and recorded
//! there; a card whose ask went is refused (409). The same over the relay,
//! and the page, are `web/e2e/chat-events.spec.ts`'s.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::agentd::Daemon;
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

fn fixture(name: &str) -> Value {
    let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

fn capture(d: &Daemon, pane: u64) -> String {
    d.raw("GET", &format!("/api/panes/{pane}/capture?format=text"), None).1
}

fn wait_text(d: &Daemon, pane: u64, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !capture(d, pane).contains(what) {
        assert!(Instant::now() < deadline, "no {what:?} in %{pane}:\n{}", capture(d, pane));
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn say(d: &Daemon, pane: u64, line: &str) {
    d.post(&format!("/api/panes/{pane}/send"), json!({ "text": line, "enter": true }));
}

/// The conversation's events, as its timeline has them.
fn events(d: &Daemon, thread: &str) -> Vec<Value> {
    d.get(thread)["messages"].as_array().unwrap().iter().filter_map(|m| m.get("event").cloned()).collect()
}

/// The newest card in the timeline, waiting until it's `id` (or any, for
/// `None`).
fn card(d: &Daemon, thread: &str, id: Option<&str>) -> Value {
    let mut last = Value::Null;
    d.wait_for("a card", || {
        last = events(d, thread).into_iter().rev().find(|e| e["kind"] == "needs_input").unwrap_or_default();
        !last.is_null() && id.is_none_or(|id| last["ask"]["id"] == id)
    });
    last
}

#[test]
fn an_edit_waiting_as_a_diff_is_a_card_answered_from_the_conversation() {
    let locks = std::env::temp_dir().join(format!("arugula-conv-diff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&locks);
    let d = Daemon::child_labs_env(
        &["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"],
        &[("ARUGULA_CLAUDE_IDE_DIR", locks.to_str().unwrap())],
    );
    let c = d.post("/api/convs", json!({ "title": "calc tests" }));
    let id = c["id"].as_str().unwrap().to_owned();
    let thread = format!("/api/threads/conv-{id}");
    let act = format!("/api/convs/{id}/act");

    // A pane where Claude Code's permission hook waits (in the background,
    // as Claude Code runs it) while Claude Code itself is in front.
    let dir = d.sessions.join("hook");
    std::fs::create_dir_all(&dir).unwrap();
    let (pre, perm) = (fixture("s18-hook-pretooluse-bash.json"), fixture("s18-hook-permission.json"));
    std::fs::write(dir.join("pre.json"), pre.to_string()).unwrap();
    std::fs::write(dir.join("perm.json"), perm.to_string()).unwrap();
    let cmd = format!(
        "{bin} hook < {d}/pre.json; ({bin} hook < {d}/perm.json > {d}/out; echo done > {d}/exit) & python3 {m}/tests/fake_claude.py",
        bin = cli_bin().display(),
        d = dir.display(),
        m = env!("CARGO_MANIFEST_DIR"),
    );
    let pane = d.post("/api/run", json!({ "cwd": d.sessions.display().to_string() }))["pane"].as_u64().unwrap();
    d.wait_for("a prompt", || info(&d, pane)["cwd"].is_string());
    d.post(&format!("/api/panes/{pane}/conv-grants"), json!({ "conv": id, "conv_title": "calc tests" }));
    d.post(&format!("/api/convs/{id}/links"), json!({ "pane": pane, "title": "calc" }));
    say(&d, pane, &cmd);
    wait_text(&d, pane, "connected");
    d.wait_for("Claude Code found in its pane", || info(&d, pane)["claude_ide"] == true);
    d.wait_for("the permission card", || info(&d, pane)["ask"]["kind"] == "permission");

    // Nothing answers the ask: after the hold, it's a card.
    let ask = card(&d, &thread, None);
    let ask_id = ask["ask"]["id"].as_str().unwrap().to_owned();
    assert_eq!(ask["ask"]["tool"], "Bash", "{ask}");

    // Claude Code opens the edit's diff: the pane shows the diff, not the
    // ask, and the conversation's card for it comes at once.
    let file = d.sessions.join("calc.py");
    std::fs::write(&file, "def mean(xs):\n    return 0\n").unwrap();
    say(&d, pane, &format!("edit {} def mean(xs):\\n    return sum(xs) / len(xs)\\n", file.display()));
    d.wait_for("a diff", || info(&d, pane)["diff"].is_object());
    let diff_id = info(&d, pane)["diff"]["id"].as_str().unwrap().to_owned();
    let e = card(&d, &thread, Some(&diff_id));
    let a = &e["ask"];
    assert_eq!((a["what"].as_str(), a["tool"].as_str()), (Some("approve"), Some("Edit")), "{e}");
    assert_eq!(a["input"], "edit calc.py (+1 −1)");
    assert_eq!(a["diff"]["file"], "calc.py");
    assert!(a["diff"]["text"].as_str().unwrap().contains("+    return sum(xs) / len(xs)"), "{e}");
    // The pane's own info keeps the diff once, in `diff`.
    assert!(info(&d, pane)["reason"].get("diff").is_none());

    // The ask's card is stale: its answer is refused, and nothing is
    // recorded.
    let (s, v) = d.raw("POST", &act, Some(json!({ "link": "L1", "id": ask_id, "action": "allow" })));
    assert_eq!(s, 409, "{v}");
    assert!(v.contains("isn't asking anything"), "{v}");
    // Accept and reject are an edit's.
    let (s, v) = d.raw("POST", &act, Some(json!({ "link": "L1", "id": ask_id, "action": "accept" })));
    assert_eq!(s, 400, "{v}");

    // Accepted from the conversation: Claude Code writes it, the pane
    // records who, via the conversation, and so does the conversation.
    d.post(&act, json!({ "link": "L1", "id": diff_id, "action": "accept" }));
    wait_text(&d, pane, "result FILE_SAVED");
    d.wait_for("the edit written", || std::fs::read_to_string(&file).unwrap().contains("sum(xs)"));
    let answered = info(&d, pane)["answered"].clone();
    assert_eq!(answered["how"], "accepted", "{answered}");
    assert!(answered["name"].as_str().unwrap().contains("via conversation \u{201c}calc tests\u{201d}"), "{answered}");
    let msgs = d.get(&thread)["messages"].as_array().unwrap().clone();
    let line = msgs.iter().rev().find(|m| m["event"]["kind"] == "answered").unwrap();
    assert_eq!(line["event"]["action"], "accept");
    assert!(line["text"].as_str().unwrap().ends_with("accepted L1: edit calc.py (+1 −1)"), "{line}");
    // Answered once is answered: the diff is gone.
    let (s, _) = d.raw("POST", &act, Some(json!({ "link": "L1", "id": diff_id, "action": "accept" })));
    assert_eq!(s, 409);

    // The tool ran: the hook's ask closes.
    let mut post = pre.clone();
    post["hook_event_name"] = json!("PostToolUse");
    let (s, v) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(post));
    assert_eq!(s, 200, "{v}");
    d.wait_for("the hook to finish", || dir.join("exit").exists());

    // The next edit, rejected from a page that doesn't know diffs (deny).
    say(&d, pane, &format!("edit {} gone\\n", file.display()));
    d.wait_for("a second diff", || info(&d, pane)["diff"]["id"].as_str().is_some_and(|i| i != diff_id));
    let second = info(&d, pane)["diff"]["id"].as_str().unwrap().to_owned();
    card(&d, &thread, Some(&second));
    d.post(&act, json!({ "link": "L1", "id": second, "action": "deny" }));
    wait_text(&d, pane, "result DIFF_REJECTED");
    assert!(std::fs::read_to_string(&file).unwrap().contains("sum(xs)"), "nothing written");
    let last = events(&d, &thread).into_iter().rev().find(|e| e["kind"] == "answered").unwrap();
    assert_eq!((last["action"].as_str(), last["ask"]["id"].as_str()), (Some("reject"), Some(second.as_str())));

    // One the terminal answers first: its card's answer is refused.
    say(&d, pane, &format!("edit {} again\\n", file.display()));
    d.wait_for("a third diff", || info(&d, pane)["diff"]["id"].as_str().is_some_and(|i| i != second));
    let third = info(&d, pane)["diff"]["id"].as_str().unwrap().to_owned();
    card(&d, &thread, Some(&third));
    say(&d, pane, "close");
    wait_text(&d, pane, "result TAB_CLOSED");
    d.wait_for("the diff to close", || info(&d, pane)["diff"].is_null());
    let (s, v) = d.raw("POST", &act, Some(json!({ "link": "L1", "id": third, "action": "accept" })));
    assert_eq!(s, 409, "{v}");
    assert!(v.contains("no edit waiting"), "{v}");
    let answers = events(&d, &thread).into_iter().filter(|e| e["kind"] == "answered").count();
    assert_eq!(answers, 2, "only what was answered here is recorded");

    say(&d, pane, "quit");
    drop(d);
    let _ = std::fs::remove_dir_all(locks);
}
