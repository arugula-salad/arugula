//! Programs that say what they're doing (OSC 7501, #720): a fake program
//! in a pane reports blocked, working, done or error, and the pane's
//! attention and card follow, in the API and in a client (`arugula
//! attention --json`), with no screen rules involved. A program that
//! reports wins over its screen.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use agentd::*;
use serde_json::{Value, json};

/// `Allow rm -rf build?`, `Deploy failed: 3 regions` and `Tests passed`, in
/// base64.
const ALLOW: &str = "QWxsb3cgcm0gLXJmIGJ1aWxkPw==";
const FAILED: &str = "RGVwbG95IGZhaWxlZDogMyByZWdpb25z";
const PASSED: &str = "VGVzdHMgcGFzc2Vk";

fn send(d: &Daemon, pane: u64, text: &str) {
    d.post(&format!("/api/panes/{pane}/send"), json!({"text": text, "enter": true}));
}

fn attention(d: &Daemon, pane: u64) -> String {
    let panes = d.get("/api/panes");
    let p = panes.as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default();
    p["attention"].as_str().unwrap_or_default().to_owned()
}

/// The pane's card in the API's "needs you" list.
fn card(d: &Daemon, pane: u64) -> Value {
    d.get("/api/attention").as_array().unwrap().iter().find(|i| i["pane"] == pane).cloned().unwrap_or_default()
}

fn until(d: &Daemon, pane: u64, want: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while attention(d, pane) != want {
        assert!(
            Instant::now() < deadline,
            "not {want} but {}: {}",
            attention(d, pane),
            d.get(&format!("/api/panes/{pane}/capture"))
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// The pane's card as a client shows it: `arugula attention --json`.
fn client_card(d: &Daemon, cli: &Path, pane: u64) -> Value {
    let out =
        arugula_testkit::command(cli).arg("--socket").arg(d.sock()).args(["attention", "--json"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let all: Value = serde_json::from_slice(&out.stdout).unwrap();
    all.as_array().unwrap().iter().find(|i| i["pane"] == pane).cloned().unwrap_or_default()
}

/// A shell script at `dir/name`.
fn script(dir: &Path, name: &str, body: &str) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    path.display().to_string()
}

/// `printf` of a report.
fn report(body: &str) -> String {
    format!(r"printf '\033]7501;{body}\033\\'")
}

#[test]
fn blocked_shows_its_message_then_working_and_error_fail() {
    let d = Daemon::child();
    let cli = cli_bin();
    let scratch = Scratch::new("status-deploy");
    let deploy = script(
        &scratch,
        "deploy",
        &[
            report(&format!("state=blocked:kind=permission:app=deploy:msg={ALLOW}")),
            "read answer".into(),
            report("state=working:app=deploy:progress=50"),
            "sleep 1".into(),
            report(&format!("state=error:app=deploy:msg={FAILED}")),
        ]
        .join("\n"),
    );
    send(&d, 1, &deploy);

    until(&d, 1, "needs_input");
    let c = card(&d, 1);
    assert_eq!(
        (c["reason"]["kind"].as_str(), c["reason"]["headline"].as_str()),
        (Some("input"), Some("Allow rm -rf build?")),
        "{c}"
    );
    let c = client_card(&d, &cli, 1);
    assert_eq!(c["reason"]["headline"], "Allow rm -rf build?", "{c}");

    // Answered: it works, then fails.
    send(&d, 1, "y");
    until(&d, 1, "done");
    let c = card(&d, 1);
    assert_eq!(
        (c["reason"]["kind"].as_str(), c["reason"]["headline"].as_str()),
        (Some("failed"), Some("Deploy failed: 3 regions")),
        "{c}"
    );
    let c = client_card(&d, &cli, 1);
    assert_eq!(
        (c["reason"]["kind"].as_str(), c["reason"]["headline"].as_str()),
        (Some("failed"), Some("Deploy failed: 3 regions")),
        "{c}"
    );

    // Its command ended and the prompt came back: the error stays until
    // someone sees it.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(attention(&d, 1), "done");
    assert_eq!(card(&d, 1)["reason"]["kind"], "failed");
}

#[test]
fn a_prompt_or_an_exit_drops_blocked_and_keeps_done() {
    let d = Daemon::child();
    let scratch = Scratch::new("status-lifetime");
    // Blocked, then gone without saying how it ended: the prompt drops it.
    let asks = script(&scratch, "asks", &[report(&format!("state=blocked:msg={ALLOW}")), "sleep 2".into()].join("\n"));
    send(&d, 1, &asks);
    until(&d, 1, "needs_input");
    until(&d, 1, "idle");

    // Done survives its prompt.
    let tests = script(&scratch, "tests", &[report(&format!("state=done:app=cargo:msg={PASSED}"))].join("\n"));
    send(&d, 1, &tests);
    until(&d, 1, "done");
    std::thread::sleep(Duration::from_millis(1500));
    let c = card(&d, 1);
    assert_eq!(
        (c["reason"]["kind"].as_str(), c["reason"]["headline"].as_str()),
        (Some("done"), Some("Tests passed")),
        "{c}"
    );

    // The pane's own process exits: blocked goes with it, error stays.
    let blocked = d
        .post("/api/run", json!({ "command": format!("{}; sleep 2", report(&format!("state=blocked:msg={ALLOW}"))) }))
        ["pane"]
        .as_u64()
        .unwrap();
    until(&d, blocked, "needs_input");
    until(&d, blocked, "done");
    assert_ne!(card(&d, blocked)["reason"]["headline"], "Allow rm -rf build?");

    let failed = d.post("/api/run", json!({ "command": report(&format!("state=error:msg={FAILED}")) }))["pane"]
        .as_u64()
        .unwrap();
    d.wait_for("its exit", || d.get(&format!("/api/panes/{failed}/wait?until=exit&timeout=10"))["result"] == "exit");
    until(&d, failed, "done");
    std::thread::sleep(Duration::from_millis(500));
    let c = card(&d, failed);
    assert_eq!(
        (c["reason"]["kind"].as_str(), c["reason"]["headline"].as_str()),
        (Some("failed"), Some("Deploy failed: 3 regions")),
        "{c}"
    );
}

#[test]
fn screen_rules_dont_override_a_program_that_reports() {
    let d = Daemon::child();
    let scratch = Scratch::new("status-screen");
    // Claude Code's permission prompt, as the screen rules know it. Each
    // waits in `read`, a builtin, so the program in front stays `claude`.
    let dialog = r"printf '%s\n' '────────────────────────────────' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' ' Esc to cancel'";
    let plain = script(&scratch.join("plain"), "claude", &[dialog, "read x"].join("\n"));
    let reports = script(
        &scratch.join("reports"),
        "claude",
        &[report("state=working:app=claude").as_str(), dialog, "read x"].join("\n"),
    );

    // Without a report the screen says it's blocked...
    send(&d, 1, &plain);
    until(&d, 1, "needs_input");

    // ...with one, the program says it's working, and that's what counts.
    let pane = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    d.wait_for("a prompt", || {
        d.get("/api/panes").as_array().unwrap().iter().any(|p| p["id"] == pane && p["running"] == true)
    });
    std::thread::sleep(Duration::from_millis(500));
    send(&d, pane, &reports);
    until(&d, pane, "working");
    let quiet = Instant::now();
    while quiet.elapsed() < Duration::from_secs(4) {
        assert_eq!(attention(&d, pane), "working", "the screen won: {}", d.get(&format!("/api/panes/{pane}/capture")));
        std::thread::sleep(Duration::from_millis(100));
    }
}
