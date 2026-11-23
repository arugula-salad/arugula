//! #1486: `arugula hook` on `SessionStart` adds the issues context in a
//! pane whose daemon answers, with the Docs context first where it has
//! `docs` on, and nothing otherwise; and never keeps a session waiting for
//! a daemon that doesn't answer. Arugula #1403: the issues context only
//! once the machine has joined Arugula cloud. Against a fake daemon on a
//! Unix socket that plays `flags.list` and `host.get`.

#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use arugula_proto::flags::{DOCS_STEERING, ISSUES_STEERING, MENTIONS_STEERING};
use serde_json::{Value, json};

/// How the fake daemon treats a request.
#[derive(Clone, Copy)]
enum Daemon {
    /// Answers `flags.list`: with no `docs` row (a daemon since Docs left
    /// Labs, native #204), or an older one's with it on or off; and
    /// `host.get`, with its control state (`joined`, `not_joined`,
    /// `dropped`).
    Up { docs: Option<bool>, control: &'static str },
    /// Accepts the connection and never answers.
    Silent,
    /// Nothing listens on the socket.
    Gone,
}

fn flags(docs: Option<bool>) -> Value {
    let mut all = vec![
        json!({ "name": "sandbox-providers", "title": "External sandbox providers", "about": "", "on": false, "default": false }),
    ];
    if let Some(on) = docs {
        all.push(json!({ "name": "docs", "title": "Docs", "about": "", "on": on, "default": false }));
    }
    Value::Array(all)
}

/// What `arugula hook` printed and how long it took, for `event` (as Claude
/// Code sends it), in a pane or not, against `daemon`.
fn hook(name: &str, daemon: Daemon, pane: Option<&str>, event: Value) -> (String, Duration, i32) {
    let dir: PathBuf = std::env::temp_dir().join(format!("arugula-cli-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("sock");
    if !matches!(daemon, Daemon::Gone) {
        let listener = UnixListener::bind(&sock).unwrap();
        thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { break };
                thread::spawn(move || {
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut first = String::new();
                    let _ = r.read_line(&mut first);
                    loop {
                        let mut l = String::new();
                        if r.read_line(&mut l).unwrap_or(0) == 0 || l.trim().is_empty() {
                            break;
                        }
                    }
                    match daemon {
                        Daemon::Up { docs, control } => {
                            let out = match first.contains(" /api/host ") {
                                true => json!({ "name": "h", "version": "1", "control_state": { "state": control } }),
                                false => flags(docs),
                            }
                            .to_string();
                            let _ = write!(
                                s,
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                                out.len()
                            );
                        }
                        _ => thread::sleep(Duration::from_secs(30)),
                    }
                });
            }
        });
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_arugula"));
    cmd.arg("--socket").arg(&sock).arg("hook").env("ARUGULA_STATE_DIR", dir.join("state"));
    for k in [
        "ARUGULA_PANE",
        "ARUGULA_SOCK",
        "CLAUDECODE",
        "AI_AGENT",
        "CLAUDE_PLUGIN_ROOT",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CODE_ENTRYPOINT",
    ] {
        cmd.env_remove(k);
    }
    if let Some(p) = pane {
        cmd.env("ARUGULA_PANE", p);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let began = Instant::now();
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(event.to_string().as_bytes()).unwrap();
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    let status = child.wait().unwrap().code().unwrap_or(-1);
    let took = began.elapsed();
    let _ = std::fs::remove_dir_all(&dir);
    (out, took, status)
}

fn start(source: &str) -> Value {
    json!({ "hook_event_name": "SessionStart", "source": source, "session_id": "s" })
}

#[test]
fn session_start_adds_the_context_for_every_source() {
    for (source, docs) in [("startup", None), ("resume", None), ("clear", Some(true)), ("compact", None)] {
        let (out, _, code) =
            hook(&format!("on-{source}"), Daemon::Up { docs, control: "joined" }, Some("3"), start(source));
        assert_eq!(code, 0);
        let said: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{source}: {e}: {out:?}"));
        assert_eq!(
            said,
            json!({ "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                // A session in a pane is told how a teammate's message arrives (#644).
                "additionalContext": format!("{DOCS_STEERING} {ISSUES_STEERING} {MENTIONS_STEERING}"),
            } }),
            "{source}"
        );
    }
}

/// An older daemon, from before Docs left Labs, with its `docs` flag off.
#[test]
fn session_start_with_an_older_daemons_docs_off_adds_only_the_issues_context() {
    let (out, _, code) = hook("off", Daemon::Up { docs: Some(false), control: "joined" }, Some("3"), start("startup"));
    assert_eq!(code, 0);
    let said: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    assert_eq!(
        said,
        json!({ "hookSpecificOutput": { "hookEventName": "SessionStart",
            "additionalContext": format!("{ISSUES_STEERING} {MENTIONS_STEERING}") } })
    );
}

/// Before the machine joins Arugula cloud, or once control dropped it,
/// `issue_write` reaches nowhere: no issues sentence (Arugula #1403).
#[test]
fn session_start_on_an_unjoined_machine_leaves_out_the_issues_context() {
    for control in ["not_joined", "dropped"] {
        let (out, _, code) = hook(control, Daemon::Up { docs: None, control }, Some("3"), start("startup"));
        assert_eq!(code, 0);
        let said: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{control}: {e}: {out:?}"));
        assert_eq!(
            said,
            json!({ "hookSpecificOutput": { "hookEventName": "SessionStart",
                "additionalContext": format!("{DOCS_STEERING} {MENTIONS_STEERING}") } }),
            "{control}"
        );
    }
}

/// A `gh issue create` gets the issues context on a joined machine only
/// (Arugula #1403), with the daemon's control state asked then; a daemon
/// that doesn't say counts as not joined.
#[test]
fn gh_issue_create_gets_the_issues_context_only_once_joined() {
    let create = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "session_id": "s",
        "tool_input": { "command": "gh issue create -t x" } });
    let (out, _, code) = hook("gh-joined", Daemon::Up { docs: None, control: "joined" }, Some("3"), create.clone());
    assert_eq!(code, 0);
    let said: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    assert_eq!(
        said,
        json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "additionalContext": ISSUES_STEERING } })
    );
    for control in ["not_joined", "dropped"] {
        let (out, _, code) =
            hook(&format!("gh-{control}"), Daemon::Up { docs: None, control }, Some("3"), create.clone());
        assert_eq!((out.as_str(), code), ("", 0), "{control}");
    }
    // Not knowing is not joined.
    let (out, _, code) = hook("gh-gone", Daemon::Gone, Some("3"), create);
    assert_eq!((out.as_str(), code), ("", 0), "gone");
}

#[test]
fn session_start_prints_nothing_outside_a_pane() {
    let (out, _, code) = hook("nopane", Daemon::Up { docs: None, control: "joined" }, None, start("startup"));
    assert_eq!((out.as_str(), code), ("", 0));
}

#[test]
fn session_start_does_not_wait_for_a_daemon_that_does_not_answer() {
    for (name, daemon) in [("silent", Daemon::Silent), ("gone", Daemon::Gone)] {
        let (out, took, code) = hook(name, daemon, Some("3"), start("startup"));
        assert_eq!((out.as_str(), code), ("", 0), "{name}");
        assert!(took < Duration::from_millis(2500), "{name}: took {took:?}");
    }
}
