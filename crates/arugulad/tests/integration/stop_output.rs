//! A daemon stopped with panes that end with it (no systemd, no
//! `--keep-panes`): what a pane's program printed before the stop is in
//! its scrollback when the daemon starts again, though the reader hadn't
//! taken it off the terminal yet (#287). A stop leaves unread output in a
//! terminal that's kept for the next daemon (#1515); this one isn't.
//!
//! Only a busy machine leaves output unread at the stop: on an idle one
//! the reader has it long before. Before the fix it failed 1 run in 30
//! (`nextest --stress-count` beside busy loops, at a load of about 90),
//! and none in 100 at a load of 50.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::{Duration, Instant};

use serde_json::json;

use crate::agentd::Daemon;

fn screen(d: &Daemon, id: u64) -> String {
    d.get(&format!("/api/panes/{id}/capture")).as_str().unwrap_or_default().replace('\n', "")
}

fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn what_a_pane_printed_before_a_stop_is_in_its_scrollback_after() {
    let mut d = Daemon::child();
    // Written once the echo is in the terminal; the stop comes straight
    // after, with nothing asking the pane for its screen in between.
    let printed = d.state.join("printed");
    let command = format!("echo stopped-$((6*7)); touch '{}'; sleep 600", printed.display());
    let pane = d.post("/api/run", json!({ "command": command }))["pane"].as_u64().unwrap();
    until("the echo", || printed.exists());
    d.stop();

    d.start();
    until("the echo in the restored scrollback", || screen(&d, pane).contains("stopped-42"));
}
