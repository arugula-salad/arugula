//! A layout saved with a remote block (#17, since removed): a place in this
//! layout for a pane on another daemon. Restored by this daemon, the leaf
//! goes, with a log line, and nothing else in the layout is lost: the other
//! panes come back with their scrollback, the tabs and sessions stay, and
//! the layout saved again has no trace of it.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agentd::Daemon;

fn layout(d: &Daemon) -> Value {
    serde_json::from_slice(&std::fs::read(d.state.join("layout.json")).unwrap()).unwrap()
}

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
fn a_saved_remote_block_is_dropped_and_the_rest_of_the_layout_kept() {
    let mut d = Daemon::child();
    let local = d.post("/api/run", json!({ "command": "echo local-$((6*7)); sleep 600" }))["pane"].as_u64().unwrap();
    // Beside it in the same tab: what a daemon before the removal saved as
    // a remote block, {host, pane} and nothing else.
    let remote = d.post("/api/run", json!({ "command": "sleep 600", "split": local }))["pane"].as_u64().unwrap();
    // And a tab of its own, holding only such a block.
    let alone = d.post("/api/run", json!({ "command": "sleep 600", "session": "alpha" }))["pane"].as_u64().unwrap();
    until("the local pane's echo", || screen(&d, local).contains("local-42"));
    let session_of = |d: &Daemon, id: u64| {
        d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()["session"].as_u64().unwrap()
    };
    let (home_session, alpha_session) = (session_of(&d, local), session_of(&d, alone));
    assert_ne!(home_session, alpha_session);
    d.stop();
    let path = d.state.join("layout.json");
    let mut saved = layout(&d);
    let sessions = saved["mux"]["sessions"].as_array().unwrap().len();
    for id in [remote, alone] {
        let meta = &mut saved["panes"][id.to_string()];
        meta["type"] = json!("remote");
        meta["config"] = json!({ "host": "mac", "pane": 7 });
        meta.as_object_mut().unwrap().remove("command");
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();

    d.start();
    let ids =
        || -> Vec<u64> { d.get("/api/panes").as_array().unwrap().iter().filter_map(|p| p["id"].as_u64()).collect() };
    assert!(ids().contains(&local), "the local pane is back: {:?}", ids());
    assert!(!ids().contains(&remote) && !ids().contains(&alone), "the remote blocks are gone: {:?}", ids());
    // The local pane is itself, with its scrollback, and fills its tab.
    until("the local pane's scrollback", || screen(&d, local).contains("local-42"));
    let panes = d.get("/api/panes");
    let mine = panes.as_array().unwrap().iter().find(|p| p["id"] == local).unwrap();
    assert_eq!(mine["type"], "terminal", "{mine}");
    assert_eq!(mine["session"], home_session, "its session is the one it was in");

    // Saved again: no remote block. The session that held nothing but
    // one has nothing left, so it goes too; the other stays.
    d.stop();
    let again = layout(&d);
    assert!(again["panes"].get(remote.to_string()).is_none() && again["panes"].get(alone.to_string()).is_none());
    assert!(!serde_json::to_string(&again).unwrap().contains("\"remote\""), "{again}");
    let ids: Vec<u64> = again["mux"]["sessions"].as_array().unwrap().iter().filter_map(|s| s["id"].as_u64()).collect();
    assert_eq!(ids.len(), sessions - 1, "{again}");
    assert!(ids.contains(&home_session) && !ids.contains(&alpha_session), "{again}");
}
