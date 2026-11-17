//! Arugula #469: Claude Code's command hooks (`arugula hook`, `ask`,
//! `inbox`, `attention`) find their pane in `ARUGULA_PANE`, which an editor
//! or the desktop app started from a pane inherits. Each sends its own pid,
//! and the daemon takes the pane only when that process is in front of it:
//! in its foreground job, or in the window a `tmux` client there shows. A
//! hook from anywhere else goes to no pane: no card, no wait, no attention,
//! no conversation to resume. One with no pid (an older CLI) is taken as
//! before.
//!
//! The real CLI runs each hook: in the pane, as Claude Code at its prompt
//! would, or from this test's process with the pane in its environment, as
//! an app started from the pane would.

// Over the daemon's Unix socket, and the process table of a Unix.
#![cfg(unix)]

use crate::agentd;

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use agentd::*;
use serde_json::{Value, json};

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

fn fixture(name: &str) -> Value {
    let p = format!("{}/tests/fixtures/{name}", concat!(env!("CARGO_MANIFEST_DIR"), "/../daemon"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// AskUserQuestion's `PreToolUse`, as `arugula ask` gets it.
fn question(id: &str) -> Value {
    json!({
        "session_id": "ae46ef2f-cbb5-48a0-acbc-5c980d7c0ed4",
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_use_id": id,
        "tool_input": { "questions": [{
            "question": "Which one?", "header": "Pick", "multiSelect": false,
            "options": [{ "label": "A", "description": "a" }, { "label": "B", "description": "b" }],
        }] },
    })
}

/// A hook input as a file, for a pane's command to read.
fn input(dir: &Path, name: &str, v: &Value) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, v.to_string()).unwrap();
    p
}

/// The environment a Claude Code hook gets, with nothing of this test's
/// own Claude Code or Arugula pane in it.
fn hook_env(c: &mut Command, d: &Daemon) {
    for k in ["ARUGULA_PANE", "ARUGULA_SOCK", "CLAUDECODE", "CLAUDE_PLUGIN_ROOT", "CLAUDE_CODE_ENTRYPOINT"] {
        c.env_remove(k);
    }
    c.env("ARUGULA_SOCK", d.sock()).env("CLAUDE_CODE_SESSION_ATTENDED", "1");
}

/// Runs `arugula ARGS` as Claude Code in an app started from `pane` would:
/// the pane in its environment, its process outside the pane (this test's).
/// Its exit code and output; it must finish within 20 seconds, where one
/// the pane took would wait for an answer.
fn from_an_app(d: &Daemon, pane: u64, args: &[&str], hook: &Value) -> (i32, String) {
    let mut c = Command::new(cli_bin());
    hook_env(&mut c, d);
    c.args(args).env("ARUGULA_PANE", pane.to_string()).stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(hook.to_string().as_bytes()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("arugula {args:?} from an app waited on %{pane}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A pane with nothing of Claude Code's in front: a shell's `sleep`.
fn a_pane(d: &Daemon) -> u64 {
    let pane = d.post("/api/run", json!({ "command": "sleep 600" }))["pane"].as_u64().unwrap();
    d.wait_for("the pane", || info(d, pane)["id"] == pane);
    pane
}

/// `POST /api/panes/N/ask` as an older CLI sends it (no pid), on a thread:
/// it waits for its answer.
fn old_ask(d: &Daemon, pane: u64, id: &str) -> std::thread::JoinHandle<(u16, String)> {
    let sock = d.sock();
    let body = json!({ "questions": question(id)["tool_input"]["questions"], "id": id });
    std::thread::spawn(move || {
        let out = Command::new("curl")
            .args(["-s", "-w", "\n%{http_code}", "--unix-socket"])
            .arg(&sock)
            .args(["-X", "POST", "-H", "content-type: application/json", "-d", &body.to_string()])
            .arg(format!("http://localhost/api/panes/{pane}/ask"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let (body, code) = text.rsplit_once('\n').unwrap_or((&text, "0"));
        (code.trim().parse().unwrap_or(0), body.to_owned())
    })
}

/// An app started from the pane asks, wants a permission, says it needs
/// you and waits for a follow-up: none of it lands on the pane, and each
/// hook ends at once, so Claude Code asks in its own dialog.
#[test]
fn an_app_started_from_a_pane_puts_nothing_on_it() {
    let d = Daemon::child();
    let pane = a_pane(&d);

    // Its question and its permission: Claude Code's own picker and dialog.
    assert_eq!(from_an_app(&d, pane, &["ask"], &question("toolu_app_q")), (0, String::new()));
    assert_eq!(from_an_app(&d, pane, &["hook"], &fixture("s18-hook-permission.json")), (0, String::new()));
    assert!(info(&d, pane)["ask"].is_null(), "{}", info(&d, pane));
    // Its attention.
    let (code, _) =
        from_an_app(&d, pane, &["attention", "needs-input"], &fixture("s18-hook-notification-permission.json"));
    assert_eq!(code, 0);
    assert_ne!(info(&d, pane)["attention"], "needs_input");
    // Its follow-ups: nothing waits for one on the pane.
    assert_eq!(from_an_app(&d, pane, &["inbox"], &fixture("s18-hook-stop.json")).0, 0);

    // An older CLI sends no pid: its question comes to the pane as before.
    let old = old_ask(&d, pane, "toolu_old_q");
    d.wait_for("the older CLI's card", || info(&d, pane)["ask"]["id"] == "toolu_old_q");
    d.post(&format!("/api/panes/{pane}/ask/withdraw"), json!({ "id": "toolu_old_q" }));
    let (status, body) = old.join().unwrap();
    assert_eq!((status, serde_json::from_str::<Value>(&body).unwrap()), (200, json!({ "action": "withdrawn" })));
    // And its attention.
    d.post(&format!("/api/panes/{pane}/attention"), json!({ "state": "needs_input" }));
    assert_eq!(info(&d, pane)["attention"], "needs_input");
}

/// Claude Code at the pane's prompt asks: its card is the pane's. An app
/// started from the pane asking meanwhile neither shows a card nor takes
/// the pane's back, and its hooks don't close the pane's permission card
/// (which an older CLI's, with no pid, still do).
#[test]
fn claude_in_front_of_its_pane_keeps_its_cards() {
    let d = Daemon::child();
    let bin = cli_bin();
    let dir = d.sessions.join("front");
    let q = input(&dir, "q.json", &question("toolu_front_q"));
    let cmd = format!(
        "{bin} ask < {q} > {dir}/out; echo done > {dir}/exit; sleep 600",
        bin = bin.display(),
        q = q.display(),
        dir = dir.display()
    );
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    d.wait_for("the pane's card", || info(&d, pane)["ask"]["id"] == "toolu_front_q");

    assert_eq!(from_an_app(&d, pane, &["ask"], &question("toolu_app_q")), (0, String::new()));
    assert_eq!(info(&d, pane)["ask"]["id"], "toolu_front_q", "the app's question took the pane's card");
    // Claude Code interrupting the app's hook withdraws only what it put up:
    // nothing.
    d.post(&format!("/api/panes/{pane}/ask/withdraw"), json!({ "pid": std::process::id() }));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(info(&d, pane)["ask"]["id"], "toolu_front_q", "the app took the pane's card back");

    // The pane's own Claude Code's hooks answer it.
    d.post(&format!("/api/panes/{pane}/ask/withdraw"), json!({ "id": "toolu_front_q" }));
    d.wait_for("the pane's hook to finish", || dir.join("exit").exists());
    assert_eq!(std::fs::read_to_string(dir.join("out")).unwrap(), "");

    // A permission card from the pane's Claude Code: the app's events (the
    // same session, as a copy of it would be) leave it; an older CLI's
    // close it, as they always did.
    let (pre, perm) = (fixture("s18-hook-pretooluse-bash.json"), fixture("s18-hook-permission.json"));
    let dir = d.sessions.join("perm");
    let (pre, perm) = (input(&dir, "pre.json", &pre), input(&dir, "perm.json", &perm));
    let cmd = format!(
        "{bin} hook < {pre}; {bin} hook < {perm} > {dir}/out; echo done > {dir}/exit; sleep 600",
        bin = bin.display(),
        pre = pre.display(),
        perm = perm.display(),
        dir = dir.display(),
    );
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    d.wait_for("the permission card", || info(&d, pane)["ask"]["kind"] == "permission");
    let stop = fixture("s18-hook-stop.json");
    assert_eq!(from_an_app(&d, pane, &["hook"], &stop), (0, String::new()));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(info(&d, pane)["ask"]["kind"], "permission", "an app's Stop closed the pane's card");
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(stop));
    assert_eq!(status, 200, "{body}");
    d.wait_for("the older CLI's Stop to close it", || dir.join("exit").exists());
    assert_eq!(info(&d, pane)["answered"]["how"], "closed");
}

/// Through `tmux` in the pane: Claude Code in the window its client shows
/// is in front, and its card is the pane's; one in a window behind it
/// isn't.
#[test]
fn through_tmux_only_the_window_it_shows_is_in_front() {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux here: skipping");
        return;
    }
    let d = Daemon::child();
    let bin = cli_bin();
    let dir = d.sessions.join("tmux");
    let q = input(&dir, "q.json", &question("toolu_tmux_q"));
    let server = format!("arugula-469-{}", std::process::id());
    struct Kill(String);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = Command::new("tmux").args(["-L", &self.0, "kill-server"]).output();
        }
    }
    let _kill = Kill(server.clone());
    // The pane's own process is the client, as `tmux` typed at a prompt is
    // the terminal's foreground job. Its server takes the pane's
    // environment, so each window's Claude Code names the pane.
    let window = format!(
        "{bin} ask < {q} > {dir}/out; echo done > {dir}/exit; sleep 600",
        bin = bin.display(),
        q = q.display(),
        dir = dir.display()
    );
    let cmd = format!("exec tmux -L {server} -f /dev/null new-session -s t '{window}'");
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    d.wait_for("the card from the window tmux shows", || info(&d, pane)["ask"]["id"] == "toolu_tmux_q");
    d.post(&format!("/api/panes/{pane}/ask/withdraw"), json!({ "id": "toolu_tmux_q" }));
    d.wait_for("its hook to finish", || dir.join("exit").exists());

    // A window behind it: its question is its own picker's.
    let q = input(&dir, "behind.json", &question("toolu_behind_q"));
    let behind = format!(
        "{bin} ask < {q} > {dir}/behind-out; echo done > {dir}/behind-exit; sleep 600",
        bin = bin.display(),
        q = q.display(),
        dir = dir.display()
    );
    let ok = Command::new("tmux").args(["-L", &server, "new-window", "-d", &behind]).status().unwrap();
    assert!(ok.success());
    d.wait_for("the hook behind it to finish", || dir.join("behind-exit").exists());
    assert_eq!(std::fs::read_to_string(dir.join("behind-out")).unwrap(), "");
    assert!(info(&d, pane)["ask"].is_null(), "{}", info(&d, pane));
}
