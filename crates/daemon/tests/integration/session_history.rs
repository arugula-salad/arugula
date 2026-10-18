//! M112a (#1027): `/api/history` and `/api/events` for one session's panes,
//! with an `id` on each entry that's the same on every read, and a guest's
//! read of them kept to the panes they may read.

// Over the daemon's Unix socket.
#![cfg(unix)]

use std::collections::BTreeSet;

use arugula_testkit::{Daemon, arugulad};
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";

fn start() -> Daemon {
    arugulad!("session-history").no_tailscale().args(["--owner", OWNER]).env("PS1", "$ ").wait_secs(10).start()
}

/// Run a command in a session (made by name), wait for it to end, and say
/// its pane and the pane's session.
fn run_in(d: &Daemon, session: &str, command: &str) -> (u64, u64) {
    let pane = d.post("/api/run", json!({"command": command, "session": session}))["pane"].as_u64().unwrap();
    d.get(&format!("/api/panes/{pane}/wait?until=exit&timeout=10"));
    let panes = d.get("/api/panes");
    let p = panes.as_array().unwrap().iter().find(|p| p["id"] == pane).unwrap();
    (pane, p["session"].as_u64().unwrap())
}

fn panes_of(v: &Value) -> BTreeSet<u64> {
    v.as_array().unwrap().iter().map(|e| e["pane"].as_u64().unwrap()).collect()
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_owned()).collect()
}

fn ndjson(text: &str) -> Value {
    Value::Array(text.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect())
}

/// A GET over TCP as the friend: status and body.
fn as_friend(d: &Daemon, path: &str) -> (u16, String) {
    let (status, _, body) = d.tcp("GET", path, &[("tailscale-user-login", FRIEND)], None);
    (status, body)
}

#[test]
fn history_and_events_by_session_with_stable_ids_and_a_guest_sees_only_theirs() {
    let d = start();
    let (a, sa) = run_in(&d, "alpha", "echo in-alpha");
    let (b, sb) = run_in(&d, "beta", "echo in-beta");
    assert_ne!(sa, sb);
    d.wait_for("both commands in history", || {
        let all = panes_of(&d.get("/api/history"));
        all.contains(&a) && all.contains(&b)
    });

    // The session filter: only that session's panes.
    let ha = d.get(&format!("/api/history?session={sa}"));
    assert_eq!(panes_of(&ha), BTreeSet::from([a]), "{ha}");
    assert!(ha.as_array().unwrap().iter().any(|e| e["text"] == "echo in-alpha"), "{ha}");
    assert_eq!(panes_of(&d.get(&format!("/api/history?session={sb}"))), BTreeSet::from([b]));
    assert_eq!(d.raw("GET", "/api/history?session=999", None).0, 404);

    // Ids: the same on a second read, and unique.
    let all = d.get("/api/history");
    let first = ids(&all);
    assert!(first.iter().all(|i| !i.is_empty()), "{all}");
    assert_eq!(first, ids(&d.get("/api/history")));
    assert_eq!(first.iter().collect::<BTreeSet<_>>().len(), first.len(), "{all}");
    // A filtered read gives an entry the id it has unfiltered.
    for e in ha.as_array().unwrap() {
        assert!(first.contains(&e["id"].as_str().unwrap().to_owned()), "{e}");
    }

    // Events: the same filter, and ids that hold across reads.
    let events = |path: &str| ndjson(&d.raw("GET", path, None).1);
    let ea = events(&format!("/api/events?session={sa}"));
    assert!(!ea.as_array().unwrap().is_empty(), "no events for alpha");
    assert_eq!(panes_of(&ea), BTreeSet::from([a]), "{ea}");
    let eids = ids(&ea);
    assert!(eids.iter().all(|i| !i.is_empty()), "{ea}");
    assert_eq!(eids, ids(&events(&format!("/api/events?session={sa}"))));
    assert_eq!(eids.iter().collect::<BTreeSet<_>>().len(), eids.len(), "{ea}");
    assert_eq!(d.raw("GET", "/api/events?session=999", None).0, 404);

    // A guest: nothing before a share, then only the shared session's panes.
    assert_eq!(as_friend(&d, "/api/history").0, 403);
    d.post(
        "/api/acl",
        json!({ "session": sa, "principal": format!("tailnet:{FRIEND}"), "role": "viewer", "history": true }),
    );
    let (s, body) = as_friend(&d, "/api/history");
    assert_eq!(s, 200, "{body}");
    let theirs: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(panes_of(&theirs), BTreeSet::from([a]), "{theirs}");
    assert_eq!(ids(&theirs), ids(&ha), "a guest's ids are the owner's");
    let (s, body) = as_friend(&d, &format!("/api/history?session={sa}"));
    assert_eq!((s, panes_of(&serde_json::from_str(&body).unwrap())), (200, BTreeSet::from([a])));
    // Another session is no more theirs by naming it, nor another host's.
    assert_eq!(as_friend(&d, &format!("/api/history?session={sb}")).0, 404);
    assert_eq!(as_friend(&d, "/api/history?host=*").0, 403);
    let (s, body) = as_friend(&d, "/api/events");
    assert_eq!(s, 200, "{body}");
    assert_eq!(panes_of(&ndjson(&body)), BTreeSet::from([a]), "{body}");
    assert_eq!(as_friend(&d, &format!("/api/events?session={sb}")).0, 404);
}
