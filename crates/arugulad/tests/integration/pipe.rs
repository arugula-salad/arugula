//! #682: `arugulad` with its output pipe already closed (`| head -c1`) ends
//! quietly: no "failed printing to stdout" panic.

#![cfg(unix)]

use std::{os::unix::process::ExitStatusExt, process::Stdio};

#[test]
fn a_closed_pipe_ends_it_without_a_panic() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = arugula_testkit::command(env!("CARGO_BIN_EXE_arugulad"))
        .arg("flags")
        .arg("--state-dir")
        .arg(std::env::temp_dir().join(format!("ilg-pipe-{}", std::process::id())))
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "{err}");
    assert!(out.status.success() || out.status.signal() == Some(13), "{:?}: {err}", out.status);
}

/// SIGPIPE in `/proc/<pid>/status`: whether it's caught (a handler) and
/// whether it's ignored.
#[cfg(target_os = "linux")]
fn sigpipe(pid: u32) -> Option<(bool, bool)> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let mask =
        |k: &str| status.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| u64::from_str_radix(v.trim(), 16).ok());
    let bit = 1u64 << (13 - 1);
    Some((mask("SigCgt:")? & bit != 0, mask("SigIgn:")? & bit != 0))
}

/// The server, unlike a subcommand, doesn't take SIGPIPE's default: a
/// write to a client that has gone fails instead of ending the daemon. It
/// did end it: in ssh.rs an `events -f` whose ssh bridge died took the
/// box's daemon and its panes down ("no pane %5", exit 141). Its panes'
/// programs still start with the default, not ignoring it.
#[cfg(target_os = "linux")]
#[test]
fn the_server_outlives_sigpipe_and_its_panes_start_with_the_default() {
    let d = arugula_testkit::arugulad!("pipe").env("PS1", "$ ").start();
    let pid = d.pid().expect("a child daemon");
    let (caught, ignored) = sigpipe(pid).expect("the daemon's /proc status");
    assert!(caught || ignored, "the serving daemon takes SIGPIPE's default: a gone client's write ends it");
    // A pane's program says what it started with (panes may run in
    // systemd scopes, not under the daemon, so it reads its own).
    let pane =
        d.post("/api/run", serde_json::json!({"command": "grep ^SigIgn: /proc/self/status"}))["pane"].as_u64().unwrap();
    let w = d.get(&format!("/api/panes/{pane}/wait?until=exit&timeout=20"));
    assert_eq!(w["code"], 0, "{w}");
    let out = d.raw("GET", &format!("/api/panes/{pane}/capture?scope=last-command"), None).1;
    let ign = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("SigIgn:"))
        .and_then(|v| u64::from_str_radix(v.trim(), 16).ok())
        .unwrap_or_else(|| panic!("no SigIgn in {out:?}"));
    assert_eq!(ign & (1 << (13 - 1)), 0, "a pane's program starts with SIGPIPE ignored: {out}");
}
