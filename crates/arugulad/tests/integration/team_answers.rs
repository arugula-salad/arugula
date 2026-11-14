//! M29: team answers, for Claude Code in a terminal.
//!
//! `arugula hook` fed the hook inputs S18 recorded from Claude Code
//! 2.1.287: a `PermissionRequest` matched to the `PreToolUse` before it
//! becomes an approval card; allowed (once, or always with Claude's own
//! suggestion) or denied from the card, with who answered in the pane's
//! history and the audit log; closed when the terminal answers first (its
//! `PostToolUse`, the next `PreToolUse`, `Stop`, or the hook's SIGTERM).
//! `arugula inbox` waits for a follow-up and exits 2 with it, which is
//! recorded as its sender's.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;

use std::{
    path::{Path, PathBuf},
    process::Command,
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

/// A hook input as a file, for a pane's command to read.
fn input(dir: &Path, name: &str, v: &Value) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, v.to_string()).unwrap();
    p
}

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A pane running Claude Code's hooks as it would: `PreToolUse` then
/// `PermissionRequest`, whose output goes to `dir/out` (and its pid to
/// `dir/pid`); `dir/exit` when it's done.
fn permission_in_pane(d: &Daemon, dir: &Path, pre: &Value, perm: &Value) -> u64 {
    let _ = std::fs::remove_dir_all(dir);
    let (pre, perm) = (input(dir, "pre.json", pre), input(dir, "perm.json", perm));
    let cmd = format!(
        "{bin} hook < {pre}; sh -c 'echo $$ > {dir}/pid; exec {bin} hook < {perm} > {dir}/out'; echo done > {dir}/exit",
        bin = cli_bin().display(),
        pre = pre.display(),
        perm = perm.display(),
        dir = dir.display(),
    );
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    d.wait_for("the card", || info(d, pane)["ask"]["kind"] == "permission");
    pane
}

/// Runs one more hook event in a pane (as Claude Code would, in its own
/// process).
fn event(d: &Daemon, pane: u64, v: &Value) {
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/hook"), Some(v.clone()));
    assert_eq!(status, 200, "{body}");
}

fn wait_done(d: &Daemon, dir: &Path) -> String {
    d.wait_for("the hook to finish", || dir.join("exit").exists());
    std::fs::read_to_string(dir.join("out")).unwrap_or_default()
}

#[test]
fn a_permission_prompt_is_a_card_answered_by_whoever_may() {
    let d = Daemon::child();
    let (pre, perm) = (fixture("s18-hook-pretooluse-bash.json"), fixture("s18-hook-permission.json"));
    let dir = d.sessions.join("allow");
    let pane = permission_in_pane(&d, &dir, &pre, &perm);
    let i = info(&d, pane);
    let ask = &i["ask"];
    // Matched to its tool call by the PreToolUse just before it.
    assert_eq!(ask["id"], "toolu_017BorY8g2uLK1UvfQ3yBjc1", "{ask}");
    assert_eq!((ask["tool"].as_str(), ask["message"].as_str()), (Some("Bash"), Some("Bash: touch a.txt")));
    assert_eq!(ask["input"]["command"], "touch a.txt");
    assert_eq!(ask["suggestions"][1]["type"], "setMode");
    assert_eq!(i["attention"], "needs_input");
    assert_eq!(i["reason"]["kind"], "ask");
    assert_eq!(i["reason"]["ask"]["what"], "approve");
    assert_eq!(i["reason"]["actions"], json!(["allow", "deny", "dismiss"]));
    // A question's answer doesn't fit it.
    let (status, _) = d.raw("POST", &format!("/api/blocks/{pane}/call/answer"), Some(json!({ "content": {} })));
    assert_eq!(status, 400);

    let r = d.post("/api/attention/act", json!({ "action": "allow", "pane": pane }));
    assert_eq!(r["results"][0]["ok"], true, "{r}");
    let out: Value = serde_json::from_str(&wait_done(&d, &dir)).unwrap();
    assert_eq!(
        out,
        json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } })
    );
    let i = info(&d, pane);
    assert!(i["ask"].is_null());
    assert_eq!((i["answered"]["how"].as_str(), i["answered"]["who"].as_str()), (Some("allowed"), Some("owner")));
    assert_eq!(i["answered"]["headline"], "Bash: touch a.txt");
    // First answer wins.
    let (status, _) = d.raw("POST", "/api/attention/act", Some(json!({ "action": "deny", "pane": pane })));
    assert_eq!(status, 409);
    // History and the audit log say who.
    let hist = d.get(&format!("/api/history?pane={pane}"));
    assert!(
        hist.as_array().unwrap().iter().any(|h| h["text"] == "allowed: Bash: touch a.txt" && h["by"].is_string()),
        "{hist}"
    );
    // It's an answer: not a command, so not a failure, and never the
    // pane's last command.
    let answers = d.get(&format!("/api/history?pane={pane}&kind=answer"));
    assert!(answers.as_array().unwrap().iter().any(|h| h["text"] == "allowed: Bash: touch a.txt"), "{answers}");
    let commands = d.get(&format!("/api/history?pane={pane}&kind=command"));
    assert!(commands.as_array().unwrap().iter().all(|h| h["kind"] == "command"), "{commands}");
    assert!(!commands.as_array().unwrap().iter().any(|h| h["text"] == "allowed: Bash: touch a.txt"), "{commands}");
    let (status, _) = d.raw("GET", "/api/history?kind=nope", None);
    assert_eq!(status, 400);
    let acl = d.get("/api/acl");
    assert!(acl["audit"].as_array().unwrap().iter().any(|a| a["action"] == "answer" && a["how"] == "allowed"), "{acl}");

    // Allow always: Claude's own suggestion, kept as a rule.
    let perm = fixture("s18-hook-permission-accept-edits.json");
    let mut pre2 = perm.clone();
    pre2["hook_event_name"] = json!("PreToolUse");
    pre2["tool_use_id"] = json!("toolu_always");
    let dir = d.sessions.join("always");
    let pane = permission_in_pane(&d, &dir, &pre2, &perm);
    d.post("/api/attention/act", json!({ "action": "allow", "pane": pane, "option": "always" }));
    let out: Value = serde_json::from_str(&wait_done(&d, &dir)).unwrap();
    assert_eq!(out["hookSpecificOutput"]["decision"]["updatedPermissions"], json!([perm["permission_suggestions"][0]]));
    assert_eq!(info(&d, pane)["answered"]["how"], "allowed always");

    // Deny with a message the agent reads.
    let dir = d.sessions.join("deny");
    let pane = permission_in_pane(&d, &dir, &pre, &fixture("s18-hook-permission.json"));
    d.post("/api/attention/act", json!({ "action": "deny", "pane": pane, "message": "do a dry run first" }));
    let out: Value = serde_json::from_str(&wait_done(&d, &dir)).unwrap();
    // (The owner's name is their tailnet login, where there is one.)
    let decision = &out["hookSpecificOutput"]["decision"];
    assert_eq!(decision["behavior"], "deny");
    assert!(
        decision["message"].as_str().unwrap().ends_with(" said no (through Arugula): do a dry run first"),
        "{decision}"
    );
}

#[test]
fn the_terminal_answering_first_closes_the_card() {
    let d = Daemon::child();
    let (pre, perm) = (fixture("s18-hook-pretooluse-bash.json"), fixture("s18-hook-permission.json"));

    // "Yes" in the terminal: the tool runs, and its PostToolUse closes it.
    let dir = d.sessions.join("yes");
    let pane = permission_in_pane(&d, &dir, &pre, &perm);
    let mut post = pre.clone();
    post["hook_event_name"] = json!("PostToolUse");
    event(&d, pane, &post);
    assert_eq!(wait_done(&d, &dir), "", "no decision: the terminal's stands");
    let i = info(&d, pane);
    assert!(i["ask"].is_null());
    assert_eq!(
        (i["answered"]["how"].as_str(), i["answered"]["who"].as_str()),
        (Some("allowed in the terminal"), Some("terminal"))
    );
    assert_ne!(i["attention"], "needs_input");

    // Another session's events leave it alone; this session's next tool
    // call, or its Stop, close it.
    for (tag, next) in [("next", "PreToolUse"), ("stop", "Stop"), ("prompt", "UserPromptSubmit")] {
        let dir = d.sessions.join(tag);
        let pane = permission_in_pane(&d, &dir, &pre, &perm);
        let mut other = pre.clone();
        other["session_id"] = json!("someone-else");
        other["hook_event_name"] = json!(next);
        other["tool_use_id"] = json!("toolu_other");
        event(&d, pane, &other);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(info(&d, pane)["ask"]["kind"], "permission", "{tag}: another session's event");
        let mut ev = fixture("s18-hook-stop.json");
        if next != "Stop" {
            ev = pre.clone();
            ev["hook_event_name"] = json!(next);
            ev["tool_use_id"] = json!("toolu_next");
        }
        event(&d, pane, &ev);
        assert_eq!(wait_done(&d, &dir), "", "{tag}");
        assert_eq!(info(&d, pane)["answered"]["how"], "closed", "{tag}");
    }

    // "No" or Esc: Claude Code stops the hook, which withdraws its card.
    let dir = d.sessions.join("no");
    let pane = permission_in_pane(&d, &dir, &pre, &perm);
    let pid: i32 = std::fs::read_to_string(dir.join("pid")).unwrap().trim().parse().unwrap();
    Command::new("kill").args(["-TERM", &pid.to_string()]).status().unwrap();
    wait_done(&d, &dir);
    d.wait_for("the card to close", || info(&d, pane)["ask"].is_null());
    assert_eq!(info(&d, pane)["answered"]["how"], "denied in the terminal");

    // AskUserQuestion stays with `arugula ask`.
    let dir = d.sessions.join("question");
    let mut q = fixture("s18-hook-permission-ask.json");
    q["hook_event_name"] = json!("PermissionRequest");
    let f = input(&dir, "q.json", &q);
    let cmd = format!(
        "{} hook < {} > {}/out; echo done > {}/exit",
        cli_bin().display(),
        f.display(),
        dir.display(),
        dir.display()
    );
    let pane = d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap();
    assert_eq!(wait_done(&d, &dir), "");
    assert!(info(&d, pane)["ask"].is_null());
}

/// `arugula inbox` in a pane as Claude Code's background Stop hook:
/// its exit code and what it wrote to stderr go in `dir`.
fn inbox_in_pane(d: &Daemon, dir: &Path) -> u64 {
    let _ = std::fs::remove_dir_all(dir);
    let stop = input(dir, "stop.json", &fixture("s18-hook-stop.json"));
    let cmd = format!(
        "{bin} inbox < {stop} 2> {dir}/err; echo $? > {dir}/exit",
        bin = cli_bin().display(),
        stop = stop.display(),
        dir = dir.display()
    );
    d.post("/api/run", json!({ "command": cmd }))["pane"].as_u64().unwrap()
}

/// The code in `dir/exit`: the pane's shell has that file open before it
/// writes the code, so wait for the code, not the file.
fn wait_exit(d: &Daemon, what: &str, dir: &Path) -> String {
    let code = || std::fs::read_to_string(dir.join("exit")).unwrap_or_default().trim().to_owned();
    d.wait_for(what, || !code().is_empty());
    code()
}

#[test]
fn a_follow_up_wakes_the_agent_through_its_inbox() {
    let d = Daemon::child();
    let dir = d.sessions.join("inbox");
    let pane = inbox_in_pane(&d, &dir);
    d.wait_for("the waiter", || info(&d, pane)["inbox"] == true);
    let r = d.post(&format!("/api/panes/{pane}/followup"), json!({ "text": "now run the tests" }));
    assert_eq!(r["delivered"], true, "{r}");
    assert_eq!(wait_exit(&d, "the hook to wake it", &dir), "2");
    let err = std::fs::read_to_string(dir.join("err")).unwrap();
    assert!(
        err.starts_with("A follow-up from ") && err.trim().ends_with(" (sent through Arugula): now run the tests"),
        "{err}"
    );
    // Recorded as the sender's, by the pane's own task: it can come after
    // the hook has woken.
    d.wait_for("the follow-up in the pane's history", || {
        let hist = d.get(&format!("/api/history?pane={pane}"));
        hist.as_array().unwrap().iter().any(|h| h["text"] == "follow-up: now run the tests")
    });
    d.wait_for("the sender's turn", || d.get(&format!("/api/panes/{pane}/drivers")).as_array().unwrap().len() == 1);

    // Sent to Claude Code before anything listens: it waits for the next
    // waiter.
    let dir = d.sessions.join("queued");
    let pane = claude_in_pane(&d, &dir, 1);
    // Held while no hook has spoken there (#630).
    let r = d.post(&format!("/api/panes/{pane}/followup"), json!({ "text": "then open a PR" }));
    assert_eq!((&r["delivered"], &r["held"]), (&json!(false), &json!(true)), "{r}");
    std::fs::write(dir.join("go1"), "").unwrap();
    d.wait_for("the queued follow-up", || dir.join("exit1").exists());
    assert!(std::fs::read_to_string(dir.join("err1")).unwrap().contains("then open a PR"));

    // A newer waiter replaces the older, which leaves quietly.
    let a = d.sessions.join("a");
    let pane_a = inbox_in_pane(&d, &a);
    d.wait_for("the first waiter", || info(&d, pane_a)["inbox"] == true);
    // A second waiter for the same pane (Claude Code's next turn).
    let sock = d.sock();
    let second = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut s = std::os::unix::net::UnixStream::connect(sock).unwrap();
        let body = fixture("s18-hook-stop.json").to_string();
        let req = format!(
            "POST /api/panes/{pane_a}/inbox HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    });
    assert_eq!(wait_exit(&d, "the first to step aside", &a), "0");
    d.post(&format!("/api/panes/{pane_a}/followup"), json!({ "text": "to the second" }));
    assert!(second.join().unwrap().contains("to the second"));
}

/// #630: a follow-up queued before a restart reaches the agent after it,
/// once: the hook acks it, and the next wait finds nothing.
#[test]
fn a_queued_follow_up_survives_a_restart_and_comes_once() {
    let mut d = Daemon::child();
    let dir = d.sessions.join("durable");
    let pane = claude_in_pane(&d, &dir, 2);
    let r = d.post(&format!("/api/panes/{pane}/followup"), json!({ "text": "after the restart" }));
    assert_eq!(r["delivered"], false, "{r}");
    d.stop();
    d.start();
    d.wait_for("the pane", || !info(&d, pane).is_null());
    // A pane comes back by its policy: a shell, where Claude Code starts
    // again (typed into its own input, it changes nothing).
    d.wait_for("the shell", || info(&d, pane)["running"] == true);
    d.post(
        &format!("/api/panes/{pane}/send"),
        json!({ "text": dir.join("claude").display().to_string(), "enter": true }),
    );
    std::fs::write(dir.join("go1"), "").unwrap();
    let code = || std::fs::read_to_string(dir.join("exit1")).unwrap_or_default().trim().to_owned();
    d.wait_for("the kept follow-up", || !code().is_empty());
    assert_eq!(code(), "2");
    assert!(std::fs::read_to_string(dir.join("err1")).unwrap().contains("after the restart"));
    // Acked: the next wait has nothing to take, and waits.
    std::fs::write(dir.join("go2"), "").unwrap();
    d.wait_for("the next waiter", || info(&d, pane)["inbox"] == true);
    assert!(!dir.join("exit2").exists());
}

/// #630: a follow-up to a pane where nothing could take one (a shell) is
/// refused at once, saying why.
#[test]
fn a_shell_refuses_a_follow_up_at_once() {
    let d = Daemon::child();
    let pane = d.post("/api/run", json!({ "command": "exec bash --norc" }))["pane"].as_u64().unwrap();
    let (st, body) = d.raw("POST", &format!("/api/panes/{pane}/followup"), Some(json!({ "text": "hello" })));
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("can't take follow-ups"), "{body}");
}

/// A fake `claude` in a pane, its hooks not installed: after each `dir/goN`
/// appears (N from 1 to `waits`) it runs `arugula inbox` as Claude Code's
/// Stop hook would, its stderr in `dir/errN` and its code in `dir/exitN`,
/// then stays in front. The pane, once it's seen as Claude Code with no
/// hooks.
fn claude_in_pane(d: &Daemon, dir: &Path, waits: u32) -> u64 {
    let _ = std::fs::remove_dir_all(dir);
    let stop = input(dir, "stop.json", &fixture("s18-hook-stop.json"));
    // It waits with bash's own `read`, so that it is what runs in front.
    let mut body = format!("#!/bin/bash\nmkfifo {d}/tick; exec 9<> {d}/tick\n", d = dir.display());
    for n in 1..=waits {
        body += &format!(
            "while [ ! -e {dir}/go{n} ]; do read -t 0.1 -u 9; done\n{bin} inbox < {stop} 2> {dir}/err{n}; echo $? > {dir}/exit{n}\n",
            dir = dir.display(),
            bin = cli_bin().display(),
            stop = stop.display(),
        );
    }
    body += "while true; do read -t 1 -u 9; done\n";
    let script = dir.join("claude");
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    // Typed into a shell, as `claude` is: what runs in front of it is seen.
    let pane = d.post("/api/run", json!({ "command": "exec bash --norc" }))["pane"].as_u64().unwrap();
    d.wait_for("the shell", || info(d, pane)["running"] == true);
    d.post(&format!("/api/panes/{pane}/send"), json!({ "text": script.display().to_string(), "enter": true }));
    d.wait_for("Claude Code with no hooks", || info(d, pane)["unhooked"] == true);
    pane
}
