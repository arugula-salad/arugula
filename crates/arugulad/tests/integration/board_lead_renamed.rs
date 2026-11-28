//! Arugula #1563: a board's lead is told of its board's posts by its Claude
//! Code conversation, never by its name. A session takes the lead with
//! `lead_board`, which sends its conversation's id, and is renamed. The posts
//! control tells for it then (by that id, and the name it led with) still
//! reach it: a thread reply, a top-level post with a file and another
//! top-level post, in quick succession while it's mid-turn, each once.
//! Against a fake control (the board's lead route, and the relay socket that
//! sends `agents.mention` and takes the daemon's answers and turn reports)
//! and a real daemon whose agent block is the fake ACP agent.
#![cfg(unix)]

use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp, PYTHON, fake};
use super::mention_agent_block::{Fake, answer, enroll, serve, states};

/// Post `seq` in board `brd_1`'s channel, as control tells it to the lead:
/// by the name it led with and its conversation.
fn post(id: &str, seq: i64, parent: Option<i64>, conversation: &str, files: Value) -> String {
    json!({ "t": "op", "id": id, "op": { "op": "agents.mention", "room": "ob_1", "target": "brd_1",
        "room_name": "board Launch", "seq": seq, "parent": parent, "author": "Aaron", "text": format!("post {seq}"),
        "session": "Lead", "from_owner": true, "files": files, "conversation": conversation } })
    .to_string()
}

/// Waits up to 30s for `f`, or fails naming `what`.
fn until(what: &str, f: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !f() {
        assert!(std::time::Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_renamed_lead_gets_every_post_on_its_board_once_mid_turn() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let f = Fake::default();
    let url = serve(&rt, f.clone());
    let d = Daemon::child_in(&[], &[], |state| enroll(state, &url));
    until("the daemon on the relay", || f.said.lock().unwrap().iter().any(|v| v["t"] == "ops"));
    let config = json!({ "agent": "acp", "command": [PYTHON, fake()], "title": "Lead", "prompt": "hello" });
    let block = d.open_with(json!({ "type": "agent", "config": config }));
    d.wait(block, "idle");
    let conversation = d.state(block)["session_id"].as_str().unwrap().to_owned();

    // It takes the lead: control is told its conversation with its name.
    let mcp = Mcp::bridge(&d, Some(block));
    mcp.call("board_write", json!({ "kind": "lead_board", "board": "brd_1" })).unwrap();
    let led = f.leads.lock().unwrap()[0].clone();
    assert_eq!((&led["session"], &led["conversation"]), (&json!("Lead"), &json!(conversation)), "{led}");

    // Renamed, and mid-turn when three posts come in a row.
    d.call(block, "set_title", json!({ "title": "Design Lift" }));
    d.call(block, "send", json!({ "text": "stream 20 150" }));
    assert_eq!(d.state(block)["status"], "working");
    let shot = json!([{ "id": "cf_1", "name": "shot.png", "mime": "image/png", "kind": "image", "size": 10,
                        "url": "/api/daemon/chat/rooms/ob_1/files/cf_1" }]);
    f.outbox.lock().unwrap().extend([
        post("p147", 147, Some(145), &conversation, json!([])),
        post("p148", 148, None, &conversation, shot),
        post("p151", 151, None, &conversation, json!([])),
    ]);
    until("the daemon's answers", || ["p147", "p148", "p151"].iter().all(|id| answer(&f, id).is_some()));
    for id in ["p147", "p148", "p151"] {
        let a = answer(&f, id).unwrap();
        let to = (&a["answer"], &a["routed"]["step"], &a["routed"]["session"]);
        assert_eq!(to, (&json!("done"), &json!("named"), &json!("Design Lift")), "{id}: {a}");
    }

    // Each is a turn of its own once the one running ends, working then
    // read, once.
    until("each read", || [147, 148, 151].iter().all(|n| states(&f).contains(&(*n, "read".to_owned()))));
    d.wait(block, "idle");
    let all = states(&f);
    for n in [147, 148, 151] {
        let of: Vec<&str> = all.iter().filter(|(s, _)| *s == n).map(|(_, w)| w.as_str()).collect();
        assert_eq!(of, ["working", "read"], "post {n}: {all:?}");
    }
    let said: Vec<Value> = d.state(block)["entries"].as_array().unwrap().clone();
    let prompts: Vec<&str> = said.iter().filter(|e| e["type"] == "user").filter_map(|e| e["text"].as_str()).collect();
    for n in [147, 148, 151] {
        assert_eq!(prompts.iter().filter(|p| p.contains(&format!("post {n}"))).count(), 1, "{prompts:#?}");
    }
    assert!(prompts.iter().any(|p| p.contains("post 148") && p.contains("shot.png")), "{prompts:#?}");
}
