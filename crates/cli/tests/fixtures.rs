//! The CLI against client fixtures (#200): `arugula --host <url>` talking
//! to the fixture server, which plays a recorded daemon and fails on any
//! request the recording doesn't have. What the SSH clients use: `run`,
//! `capture`, and `events -f`.

use std::{
    process::{Command, Stdio},
    time::Duration,
};

use arugula_proto::fixture::{self, Fixture, server::Server};

fn serve(name: &str) -> Server {
    let f = Fixture::load(&fixture::dir().join(format!("{name}.jsonl"))).unwrap();
    Server::start(&f, "127.0.0.1:0").unwrap()
}

/// `arugula --host <server>`, with no token of this machine's and no
/// pane of its own.
fn arugula(s: &Server) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arugula"));
    c.arg("--host").arg(s.url()).env("ARUGULA_LOCAL_TOKEN_FILE", "/nonexistent").env_remove("ARUGULA_PANE");
    c
}

fn stdout(c: &mut Command) -> String {
    let out = c.output().unwrap();
    assert!(out.status.success(), "{c:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn run_then_capture() {
    let s = serve("run-capture");
    assert_eq!(stdout(arugula(&s).args(["run", "--wait", "echo fixture-run"])), "%2\n");
    let screen = stdout(arugula(&s).args(["capture", "%2"]));
    assert!(screen.contains("fixture-run"), "{screen}");
    let r = s.wait(Duration::from_secs(10));
    assert!(r.ok() && r.left.is_empty(), "{r:#?}");
}

#[test]
fn events_follow_a_command() {
    let s = serve("events");
    let events = arugula(&s).args(["events", "-f"]).stdout(Stdio::piped()).spawn().unwrap();
    assert_eq!(stdout(arugula(&s).args(["run", "echo fixture-events"])), "%2\n");
    // The fixture ends the stream once it has played: `events -f` ends too.
    let out = events.wait_with_output().unwrap();
    assert!(out.status.success());
    let lines: Vec<serde_json::Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    for want in ["command_start", "command_end", "exit"] {
        assert!(lines.iter().any(|l| l["type"] == want && l["pane"] == 2), "no {want} in {lines:#?}");
    }
    let r = s.wait(Duration::from_secs(10));
    assert!(r.ok() && r.left.is_empty(), "{r:#?}");
}
