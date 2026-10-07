//! A layout saved with a VM pane (#454), restored by a daemon that has no
//! provider: a build without Labs, or a default build with no wisp token.
//! The daemon starts, every other pane and the tabs come back, and the VM
//! pane stays in the layout (with its machine), exited, with a note saying
//! why, so a daemon that has the provider can carry it on.

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
fn a_saved_vm_pane_stays_in_the_layout_when_there_is_no_provider() {
    let mut d = Daemon::child();
    let local = d.post("/api/run", json!({ "command": "echo local-$((6*7)); sleep 600" }))["pane"].as_u64().unwrap();
    let vm = d.post("/api/run", json!({ "command": "sleep 600" }))["pane"].as_u64().unwrap();
    // A daemon with a provider saved it so: the pane on a machine its own.
    d.stop();
    let path = d.state.join("layout.json");
    let mut saved = layout(&d);
    let tabs = saved["mux"]["sessions"][0]["tabs"].as_array().unwrap().len();
    saved["panes"][vm.to_string()]["host"] = json!(1);
    saved["machines"] = json!({
        "1": { "id": 1, "provider": "wisp", "sprite": "illogical-eph-test-1", "owner": { "pane": vm }, "state": "running" }
    });
    saved["next_machine"] = json!(2);
    std::fs::write(&path, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();

    d.start();
    let ids =
        || -> Vec<u64> { d.get("/api/panes").as_array().unwrap().iter().filter_map(|p| p["id"].as_u64()).collect() };
    assert!(ids().contains(&local) && ids().contains(&vm), "{:?}", ids());
    until("the note", || screen(&d, vm).contains("aren't"));
    let note = screen(&d, vm);
    let why = if cfg!(feature = "labs") {
        "VM panes aren't set up here"
    } else {
        "VMs aren't in this build (built without labs)"
    };
    assert!(note.contains(why), "{note}");
    // The other pane is itself: restored, with its scrollback.
    until("the local pane's scrollback", || screen(&d, local).contains("local-42"));

    // Saved again, it still has the pane's machine and where it runs.
    d.stop();
    let again = layout(&d);
    assert_eq!(again["panes"][vm.to_string()]["host"], 1, "{again}");
    assert_eq!(again["machines"]["1"]["sprite"], "illogical-eph-test-1", "{again}");
    assert_eq!(again["mux"]["sessions"][0]["tabs"].as_array().unwrap().len(), tabs);
}
