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
