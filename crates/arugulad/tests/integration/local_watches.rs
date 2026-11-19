//! Local watches (https://app.arugula.io/issues/iss_ad072f84ac1c0fdffb66fbdb982902e4):
//! on a machine never joined to Arugula cloud, an agent block watches
//! another go idle, ends its turn, and is woken by the watch's message.
//! Against a real daemon with fake-ACP agent blocks, through `arugula mcp`
//! from the watching block's pane.

use serde_json::{Value, json};

use super::agentd::{Daemon, Mcp};

fn summary(v: &Value) -> &str {
    v["summary"].as_str().unwrap_or_default()
}

#[test]
fn an_agent_with_no_account_is_woken_when_another_goes_idle() {
    let d = Daemon::child();
    let watcher = d.open("hello");
    d.wait(watcher, "idle");
    let mcp = Mcp::bridge(&d, Some(watcher));
    // A turn of about ten seconds, which a cancel ends sooner.
    let worker = d.open("slow");
    d.wait_for("its turn", || d.state(worker)["status"] == "working");

    let types = mcp.call("watch", json!({ "kind": "types" })).unwrap();
    assert!(
        summary(&types).contains("agent.idle: An agent on this machine finished its turn and waits for a prompt. [watchable now, by this machine]"),
        "{types}"
    );
    // The cloud's types still need an account.
    let cloud = mcp.call("watch", json!({ "kind": "create", "event": "issue.closed" })).unwrap_err();
    assert!(cloud.contains("hasn't joined Arugula cloud"), "{cloud}");
    // What has already happened makes no watch: the watcher itself is idle.
    let met = mcp
        .call("watch", json!({ "kind": "create", "event": "agent.idle", "match": { "pane": watcher.to_string() } }))
        .unwrap();
    assert!(summary(&met).starts_with("It has already happened"), "{met}");

    let made = mcp
        .call(
            "watch",
            json!({ "kind": "create", "event": "agent.idle", "match": { "pane": format!("%{worker}") },
                    "note": "review its work" }),
        )
        .unwrap();
    let id = made["id"].as_str().unwrap_or_else(|| panic!("{made}")).to_owned();
    assert!(summary(&made).contains("End your turn now"), "{made}");
    let list = mcp.call("watch", json!({ "kind": "list" })).unwrap();
    assert_eq!(list["watches"][0]["id"], id, "{list}");
    assert_eq!(list["watches"][0]["targets"][0], format!("agent.idle on pane %{worker} (on this machine)"), "{list}");

    // The worker's turn ends: the watcher's next prompt is the watch's
    // message, once.
    d.call(worker, "cancel", json!({}));
    d.wait(worker, "idle");
    d.wait_for("the watcher woken", || d.state(watcher).to_string().contains(&format!("Your watch {id}")));
    let woke = d.state(watcher).to_string();
    assert!(woke.contains(&format!("agent.idle: pane %{worker}")), "{woke}");
    assert!(woke.contains("its agent finished its turn and waits for a prompt"), "{woke}");
    assert!(woke.contains("Your note: review its work"), "{woke}");
    let told = d.state(watcher)["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "user" && e["text"].as_str().unwrap_or_default().starts_with("Your watch"))
        .count();
    assert_eq!(told, 1, "once: {woke}");
    let list = mcp.call("watch", json!({ "kind": "list" })).unwrap();
    assert_eq!(list["watches"], json!([]), "a fired watch isn't live: {list}");

    // Cancel takes this machine's own, with no account.
    d.call(worker, "send", json!({ "text": "slow" }));
    d.wait_for("its turn", || d.state(worker)["status"] == "working");
    let made = mcp
        .call("watch", json!({ "kind": "create", "event": "agent.needs_input", "match": { "pane": worker } }))
        .unwrap();
    let id = made["id"].as_str().unwrap().to_owned();
    let gone = mcp.call("watch", json!({ "kind": "cancel", "all": true })).unwrap();
    assert_eq!(gone["cancelled"], json!([id]), "{gone}");
    assert_eq!(mcp.call("watch", json!({ "kind": "list" })).unwrap()["watches"], json!([]));
}

/// `wait` with one of this machine's events
/// (https://app.arugula.io/issues/iss_1545620a176cab44b21084c27493241e): a
/// short-lived watch, answered "still running" at its time with the watch
/// gone, with the watch's message when it happens, and at once when it
/// already has.
#[test]
fn wait_with_an_event_blocks_until_it_happens() {
    let d = Daemon::child();
    let mcp = Mcp::bridge(&d, None);
    let worker = d.open("slow");
    d.wait_for("its turn", || d.state(worker)["status"] == "working");
    let idle = json!({ "event": "agent.idle", "match": { "pane": format!("%{worker}") } });
    let live = || mcp.call("watch", json!({ "kind": "list", "all": true })).unwrap()["watches"].clone();

    let mut early = idle.clone();
    early["timeout"] = json!(1);
    let early = mcp.call("wait", early).unwrap();
    assert_eq!(early["state"], "still running", "{early}");
    assert!(summary(&early).starts_with("No agent.idle yet after 1s: still running"), "{early}");
    assert_eq!(live(), json!([]), "its watch is gone");

    // Another client waits while the worker's turn ends.
    let waiter = Mcp::bridge(&d, None);
    let mut asked = idle.clone();
    asked["timeout"] = json!(60);
    let waited = std::thread::spawn(move || waiter.call("wait", asked));
    d.wait_for("the wait's watch", || live().as_array().is_some_and(|w| w.len() == 1));
    d.call(worker, "cancel", json!({}));
    let fired = waited.join().unwrap().unwrap();
    assert_eq!(fired["state"], "fired", "{fired}");
    assert!(
        summary(&fired).starts_with(&format!("agent.idle: pane %{worker}"))
            && summary(&fired).ends_with("its agent finished its turn and waits for a prompt."),
        "the watch's own message: {fired}"
    );
    assert_eq!(live(), json!([]), "a fired wait's watch is gone");

    // It has already happened: answered at once, with no watch.
    let met = mcp.call("wait", idle).unwrap();
    assert!(summary(&met).starts_with("It has already happened"), "{met}");
    assert_eq!(met["state"], "already_met", "{met}");
    // The cloud's types need an account, as watch's do.
    let cloud = mcp.call("wait", json!({ "event": "issue.closed", "match": { "issue": "#3" } })).unwrap_err();
    assert!(cloud.contains("hasn't joined Arugula cloud"), "{cloud}");
    let both = mcp.call("wait", json!({ "event": "agent.idle", "until": "idle", "pane": worker })).unwrap_err();
    assert!(both.contains("one of them"), "{both}");
}
