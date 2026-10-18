//! M105 (#987): `arugula`'s one question about telemetry, on a real
//! terminal (a pty for stdin and stderr), and `arugula telemetry`. Against
//! no daemon (the consent file decides and takes the answer) and against a
//! fake daemon on a Unix socket that plays `telemetry.get` and
//! `telemetry.set`.

// A pty and a Unix socket.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::{fd::OwnedFd, unix::net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const ASKED: &str = "Help improve Arugula?";

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("arugula-cli-telemetry-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("state")).unwrap();
    d
}

/// `arugula` on `dir`'s socket and state, with nothing in the environment
/// that keeps telemetry off or says an agent runs it.
fn arugula(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arugula"));
    c.arg("--socket").arg(dir.join("sock")).env("ARUGULA_STATE_DIR", dir.join("state"));
    for k in [
        "CI",
        "DO_NOT_TRACK",
        "ARUGULA_TELEMETRY",
        "NEXTEST_RUN_ID",
        "CLAUDECODE",
        "AI_AGENT",
        "ARUGULA_SOCK",
        "ARUGULA_PANE",
    ] {
        c.env_remove(k);
    }
    c
}

/// What a run on a terminal printed, and whether it asked.
struct Ran {
    asked: bool,
    /// The terminal's output (stderr, and the echo of what was typed).
    tty: String,
}

/// Run `c` with stdin and stderr on a pty, typing `answer` if it asks.
fn on_tty(mut c: Command, answer: &str) -> Ran {
    let pty = nix::pty::openpty(None, None).unwrap();
    let slave = |fd: &OwnedFd| Stdio::from(fd.try_clone().unwrap());
    c.stdin(slave(&pty.slave)).stderr(slave(&pty.slave)).stdout(Stdio::null());
    let mut child = c.spawn().unwrap();
    // The command holds its copies of the slave: the reader sees the end
    // only once every one is closed.
    drop(c);
    drop(pty.slave);
    let mut master = std::fs::File::from(pty.master);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let (mut m, seen) = (master.try_clone().unwrap(), seen.clone());
        thread::spawn(move || {
            let mut b = [0u8; 4096];
            // EIO once the child's side is closed.
            while let Ok(n) = m.read(&mut b) {
                if n == 0 {
                    break;
                }
                seen.lock().unwrap().extend_from_slice(&b[..n]);
            }
        })
    };
    let text = |s: &Mutex<Vec<u8>>| String::from_utf8_lossy(&s.lock().unwrap()).into_owned();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut asked = false;
    loop {
        if !asked && text(&seen).contains("[y/N]") {
            asked = true;
            master.write_all(answer.as_bytes()).unwrap();
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "still running: {}", text(&seen));
        thread::sleep(Duration::from_millis(20));
    }
    reader.join().unwrap();
    let tty = text(&seen);
    Ran { asked: asked || tty.contains(ASKED), tty }
}

fn consent(dir: &Path) -> Option<Value> {
    let t = std::fs::read_to_string(dir.join("state/telemetry.json")).ok()?;
    Some(serde_json::from_str(&t).unwrap())
}

/// A daemon that answers only `/api/telemetry`, from `answer` (`None`: not
/// asked), and notes each `PUT`'s body.
struct Fake {
    puts: Arc<Mutex<Vec<Value>>>,
}

fn info(answer: Option<bool>) -> Value {
    let why = match answer {
        None => "not_asked",
        Some(true) => "yes",
        Some(false) => "no",
    };
    let mut v = json!({
        "on": answer == Some(true),
        "why": why,
        "url": "https://control.arugula.io",
        "signed": false,
        "preview": { "install": "<made when you say yes>", "version": "0.0.0", "preview_only": true, "events": [] },
    });
    if answer == Some(true) {
        v["install"] = json!("0123456789abcdef");
        v["preview"]["install"] = json!("0123456789abcdef");
        v["preview"].as_object_mut().unwrap().remove("preview_only");
    }
    v
}

fn fake(dir: &Path) -> Fake {
    let listener = UnixListener::bind(dir.join("sock")).unwrap();
    let (answer, puts) = (Arc::new(Mutex::new(None)), Arc::new(Mutex::new(Vec::new())));
    let f = Fake { puts: puts.clone() };
    thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { break };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            r.read_line(&mut head).unwrap();
            let mut len = 0;
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = l.split_once(':')
                    && k.eq_ignore_ascii_case("content-length")
                {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let (status, out) = match head.split_whitespace().take(2).collect::<Vec<_>>()[..] {
                ["GET", "/api/telemetry"] => (200, info(*answer.lock().unwrap())),
                ["PUT", "/api/telemetry"] => {
                    let req: Value = serde_json::from_slice(&body).unwrap();
                    *answer.lock().unwrap() = req["on"].as_bool();
                    puts.lock().unwrap().push(req);
                    (200, info(*answer.lock().unwrap()))
                }
                _ => (404, json!({ "error": "not in this fake" })),
            };
            let out = out.to_string();
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                out.len()
            );
        }
    });
    f
}

/// No daemon: asked once on a terminal, `y` is written to the file as a
/// yes from the CLI, and the next run doesn't ask.
#[test]
fn asked_once_and_y_turns_it_on() {
    let d = dir("yes");
    let mut c = arugula(&d);
    c.arg("ls");
    let first = on_tty(c, "y\n");
    assert!(first.asked, "{}", first.tty);
    assert!(first.tty.contains("never terminal contents, file names, commands or prompts"), "{}", first.tty);
    assert!(first.tty.contains("Usage reports are on"), "{}", first.tty);
    let c = consent(&d).expect("the answer is written");
    assert_eq!((c["telemetry"].as_bool(), c["via"].as_str()), (Some(true), Some("cli")), "{c}");
    let mut again = arugula(&d);
    again.arg("ls");
    let second = on_tty(again, "y\n");
    assert!(!second.asked, "asked twice: {}", second.tty);
    let _ = std::fs::remove_dir_all(&d);
}

/// A daemon: Enter is no, sent as `telemetry.set {on: false, via: cli}`,
/// and the daemon's answer stops the next run asking.
#[test]
fn enter_turns_it_off_through_the_daemon() {
    let d = dir("enter");
    let f = fake(&d);
    let mut c = arugula(&d);
    c.arg("ls");
    let first = on_tty(c, "\n");
    assert!(first.asked, "{}", first.tty);
    assert!(first.tty.contains("Usage reports stay off"), "{}", first.tty);
    assert_eq!(*f.puts.lock().unwrap(), vec![json!({ "on": false, "via": "cli" })]);
    assert!(consent(&d).is_none(), "the daemon writes it, not the CLI");
    let mut again = arugula(&d);
    again.arg("ls");
    assert!(!on_tty(again, "y\n").asked);
    assert_eq!(f.puts.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// Anything but y or yes is no.
#[test]
fn anything_but_yes_is_no() {
    let d = dir("nope");
    let mut c = arugula(&d);
    c.arg("ls");
    assert!(on_tty(c, "sure\n").asked);
    assert_eq!(consent(&d).unwrap()["telemetry"], json!(false));
    let _ = std::fs::remove_dir_all(&d);
}

/// Not on a pipe, with CI or DO_NOT_TRACK or ARUGULA_TELEMETRY=off, with
/// `--json`, for an agent, over `--host`, or for `arugula telemetry` itself.
#[test]
fn not_asked_on_a_pipe_in_ci_with_json_or_for_an_agent() {
    let d = dir("never");
    // A pipe.
    let out = arugula(&d).arg("ls").stdin(Stdio::null()).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains(ASKED));
    // A terminal, but something else says no.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)]);
    let cases: [Case; 7] = [
        (&["ls"], &[("CI", "1")]),
        (&["ls"], &[("DO_NOT_TRACK", "1")]),
        (&["ls"], &[("ARUGULA_TELEMETRY", "off")]),
        (&["--json", "ls"], &[]),
        (&["ls"], &[("CLAUDECODE", "1")]),
        (&["--host", "http://127.0.0.1:9", "ls"], &[]),
        (&["telemetry", "status"], &[]),
    ];
    for (args, env) in cases {
        let mut c = arugula(&d);
        c.args(args).envs(env.iter().copied());
        let r = on_tty(c, "y\n");
        assert!(!r.asked, "{args:?} {env:?}: {}", r.tty);
    }
    assert!(consent(&d).is_none(), "nothing was answered");
    let _ = std::fs::remove_dir_all(&d);
}

/// `arugula telemetry` through the daemon: `status --json` is its answer,
/// preview and all; `on` and `off` set it as the CLI.
#[test]
fn the_commands_talk_to_the_daemon() {
    let d = dir("cmds");
    let f = fake(&d);
    let run = |args: &[&str]| {
        let out = arugula(&d).args(args).stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let status: Value = serde_json::from_str(&run(&["telemetry", "status", "--json"])).unwrap();
    assert_eq!(status["why"], "not_asked");
    assert_eq!(status["preview"]["preview_only"], true, "{status}");
    let text = run(&["telemetry", "status"]);
    assert!(text.contains("off: not asked yet") && text.contains("\"preview_only\": true"), "{text}");
    assert!(run(&["telemetry", "on"]).contains("on: the owner said yes"));
    assert!(run(&["telemetry", "status"]).contains("install  0123456789abcdef"));
    assert!(run(&["telemetry", "off"]).contains("off: the owner said no"));
    assert_eq!(
        *f.puts.lock().unwrap(),
        vec![json!({ "on": true, "via": "cli" }), json!({ "on": false, "via": "cli" })]
    );
    // An agent is refused before anything is sent.
    let out = arugula(&d).args(["telemetry", "on"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(!out.status.success());
    assert_eq!(f.puts.lock().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&d);
}

/// With no daemon running, the commands work on the file.
#[test]
fn the_commands_use_the_file_without_a_daemon() {
    let d = dir("file");
    let run = |args: &[&str]| {
        let out = arugula(&d).args(args).stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    assert!(run(&["telemetry", "status"]).contains("off: not asked yet"));
    assert!(run(&["telemetry", "on"]).contains("on: the owner said yes"));
    let c = consent(&d).unwrap();
    assert_eq!((c["telemetry"].as_bool(), c["via"].as_str()), (Some(true), Some("cli")));
    let status: Value = serde_json::from_str(&run(&["telemetry", "status", "--json"])).unwrap();
    assert_eq!((status["on"].as_bool(), status["install"].as_str()), (Some(true), c["install"].as_str()));
    run(&["telemetry", "off"]);
    assert_eq!(consent(&d).unwrap()["install"], Value::Null);
    let _ = std::fs::remove_dir_all(&d);
}
