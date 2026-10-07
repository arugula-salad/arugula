//! `record-fixtures [--daemon PATH] --record DIR [NAME...]`: record the
//! client fixtures (#200) from a throwaway daemon, each session as a client
//! would run it, into DIR (`just record-fixtures` writes
//! `crates/proto/fixtures`). Without names, all of them. The daemon is
//! `target/debug/arugulad` unless `--daemon` says otherwise; each fixture
//! gets a fresh one, set up as `arugula_testkit::fixture::daemon` sets
//! it up, so a replay starts where the recording did.

use std::{path::PathBuf, time::Duration};

use arugula_testkit::fixture::{self, Session, WsIn};
use serde_json::{Value, json};

/// A fixture: its name, what it covers, and the session.
type Recording = (&'static str, &'static str, fn(&Session));

const ALL: [Recording; 3] = [
    (
        "attach",
        "Attach to the first pane as the TUI does (view at 80x24, attach), type a command and see its output, \
         resize to 100x30, detach.",
        attach,
    ),
    (
        "run-capture",
        "`arugula run --wait 'echo fixture-run'`, then `arugula capture %N`: run a command in a new tab, \
         wait for it to exit, and read the screen.",
        run_capture,
    ),
    (
        "events",
        "`arugula events -f`: follow the events stream while a command runs in a new tab, until it exits.",
        events,
    ),
];

fn attach(s: &Session) {
    let mut ws = s.ws();
    let hello = ws.until("hello", |m| m["type"] == "hello");
    let state = &hello["state"];
    let tab = state["tabs"][0]["id"].as_u64().expect("a tab");
    let pane = state["panes"][0]["id"].as_u64().expect("a pane") as u32;
    ws.send(json!({"type": "view", "tab": tab, "cols": 80, "rows": 24, "zoom": null, "claim": true}));
    ws.send(json!({"type": "attach", "panes": [{"pane": pane, "offset": null}]}));
    ws.ping(1);
    ws.wait_output(pane, "$", 1);
    ws.input(pane, b"echo fixture-attach\r");
    // Typed, then printed.
    ws.wait_output(pane, "fixture-attach", 2);
    ws.ping(2);
    ws.send(json!({"type": "view", "tab": tab, "cols": 100, "rows": 30, "zoom": null, "claim": true}));
    ws.until("its new size", |m| m["type"] == "size" && m["pane"] == pane && m["cols"] == 100);
    ws.ping(3);
    ws.send(json!({"type": "detach", "panes": [pane]}));
    ws.ping(4);
    // Nothing more for it after the detach.
    while let Some(m) = ws.recv(Duration::from_millis(300)) {
        if let WsIn::Frame(f) = m {
            assert_ne!(f.pane, pane, "output after the detach");
        }
    }
    ws.close();
}

fn run_capture(s: &Session) {
    // As the CLI sends it to another daemon (`--host`): no cwd.
    let v = s.post("/api/run", json!({"command": "echo fixture-run"}));
    let pane = v["pane"].as_u64().expect("a pane");
    let w = s.get(&format!("/api/panes/{pane}/wait?until=exit"));
    assert_eq!(w["code"], 0, "{w}");
    let text = s.get(&format!("/api/panes/{pane}/capture?format=text&scope=screen"));
    assert!(text.as_str().is_some_and(|t| t.contains("fixture-run")), "{text}");
}

fn events(s: &Session) {
    let lines = s.stream("/api/events?follow=1");
    let v = s.post("/api/run", json!({"command": "echo fixture-events"}));
    let pane = v["pane"].clone();
    let is = |l: &Value, t: &str| l["type"] == t && l["pane"] == pane;
    lines.until("command_start", |l| is(l, "command_start"));
    lines.until("command_end", |l| is(l, "command_end"));
    lines.until("exit", |l| is(l, "exit"));
    // The attention change that follows the exit, if it comes.
    let _ = lines.next(Duration::from_millis(300));
    lines.close();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut bin, mut dir, mut names) = (PathBuf::from("target/debug/arugulad"), None, vec![]);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--daemon" => bin = args.next().map(PathBuf::from).expect("--daemon PATH"),
            "--record" => dir = args.next().map(PathBuf::from),
            _ => names.push(a),
        }
    }
    let Some(dir) = dir else {
        eprintln!("usage: record-fixtures [--daemon PATH] --record DIR [NAME...]");
        std::process::exit(64);
    };
    if let Some(n) = names.iter().find(|n| !ALL.iter().any(|(name, ..)| name == n)) {
        eprintln!("no fixture {n}: {}", ALL.map(|(n, ..)| n).join(", "));
        std::process::exit(64);
    }
    for (name, about, run) in ALL {
        if !names.is_empty() && !names.iter().any(|n| n == name) {
            continue;
        }
        let d = fixture::daemon(&bin, "rec").record(&dir).start();
        fixture::ready(&d);
        let s = d.fixture(name, about);
        run(&s);
        let path = s.finish().expect("recording");
        println!("{}", path.display());
    }
}
