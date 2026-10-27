//! The long waits (#576) give up what they put up when their client goes
//! away: `arugula ask` or `arugula hook` killed (Claude Code cancelling a
//! hook) leaves no question card or permission card, and `arugula inbox`
//! killed leaves no waiter for a follow-up to be delivered to.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    io::Write,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::agentd::*;

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

/// A POST that is left waiting: the request is written and the connection
/// kept, for the test to drop.
fn post_and_wait(d: &Daemon, path: &str, body: &Value) -> UnixStream {
    let mut s = UnixStream::connect(d.sock()).unwrap();
    let body = body.to_string();
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    s
}

/// The daemon notices a client that went away when it next reads the
/// connection, so a closed one is shut both ways.
fn hang_up(s: UnixStream) {
    s.shutdown(std::net::Shutdown::Both).unwrap();
    drop(s);
}

#[test]
fn a_question_goes_with_the_client_that_asked_it() {
    let d = Daemon::child();
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let question = json!({ "questions": [{ "question": "Which colour?", "header": "Colour",
        "options": [{ "label": "Blue" }, { "label": "Red" }], "multiSelect": false }] });
    let client = post_and_wait(&d, &format!("/api/panes/{pane}/ask"), &question);
    d.wait_for("its question", || info(&d, pane)["ask"]["kind"] == "questions");
    hang_up(client);
    d.wait_for("the question to go with its client", || info(&d, pane)["ask"].is_null());
}

#[test]
fn a_permission_card_goes_with_the_client_that_asked_it() {
    let d = Daemon::child();
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let permit = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "session_id": "s" });
    let client = post_and_wait(&d, &format!("/api/panes/{pane}/permit"), &permit);
    d.wait_for("its permission card", || info(&d, pane)["ask"]["kind"] == "permission");
    hang_up(client);
    d.wait_for("the card to go with its client", || info(&d, pane)["ask"].is_null());
}

#[test]
fn an_inbox_waiter_goes_with_the_client_that_waited() {
    let d = Daemon::child();
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let hook = json!({ "hook_event_name": "Stop", "session_id": "s" });
    let client = post_and_wait(&d, &format!("/api/panes/{pane}/inbox"), &hook);
    d.wait_for("the waiter", || info(&d, pane)["inbox"] == true);
    hang_up(client);
    let deadline = Instant::now() + Duration::from_secs(30);
    while info(&d, pane)["inbox"] == true {
        assert!(Instant::now() < deadline, "the waiter stayed after its client went");
        std::thread::sleep(Duration::from_millis(50));
    }
}
