//! `access.get` (Arugula #681): who can reach this machine, in one answer
//! the owner reads over the socket, composed from what the machine holds;
//! someone shared a session with is refused it over the tailnet, whatever
//! their role. What it lists is what `GET /api/acl` and `GET /api/shares`
//! list, with trust live now: nothing more.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd::Daemon;
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";

fn tailnet_daemon() -> Daemon {
    let args = ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"];
    Daemon::child_env(&args, &[("ARUGULA_TRUST_MINUTE_MS", "600000")])
}

fn rows<'a>(v: &'a Value, kind: &str) -> Vec<&'a Value> {
    v["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == kind).collect()
}

#[test]
fn the_owner_reads_who_reaches_the_machine_and_nobody_else_does() {
    let d = tailnet_daemon();
    let p = &d.get("/api/panes")[0];
    let (pane, session) = (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap());

    // Not joined to control: the owner, and nothing else yet.
    let v = d.get("/api/access");
    assert_eq!(v["kind"], "local", "{v}");
    assert_eq!(v["rows"].as_array().map(Vec::len), Some(1), "{v}");
    let owner = &v["rows"][0];
    assert_eq!(
        (owner["kind"].as_str(), owner["who"].as_str(), owner["role"].as_str()),
        (Some("owner"), Some(OWNER), Some("owner"))
    );
    assert_eq!(owner["word"], "this_machine");
    assert!(owner["session"].is_null() && owner["pane"].is_null(), "every session: {owner}");
    assert_eq!(v["sessions"][0]["id"], session);
    assert!(v["machine"].is_null(), "no device id without control: {v}");

    // A session share, a link to a pane, and trust: one row each, saying
    // where it reaches, and all this machine's word.
    d.post(
        "/api/acl",
        json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "editor", "history": false }),
    );
    let link = d.post("/api/shares", json!({ "pane": pane, "ttl_secs": 600 }));
    d.trust(pane, &format!("tailnet:{FRIEND}"), 5);
    let v = d.get("/api/access");
    let share = rows(&v, "share")[0];
    assert_eq!(
        (share["who"].as_str(), share["role"].as_str(), share["session"].as_u64()),
        (Some(FRIEND), Some("editor"), Some(session))
    );
    assert!(share["from_ms"].as_u64().is_some(), "a share from now on has its floor: {share}");
    let l = rows(&v, "link")[0];
    assert_eq!(
        (l["pane"].as_u64(), l["session"].as_u64(), l["role"].as_str()),
        (Some(pane), Some(session), Some("viewer"))
    );
    assert_eq!(l["until_ms"], link["expires_ms"]);
    let t = rows(&v, "trust")[0];
    assert_eq!((t["who"].as_str(), t["pane"].as_u64(), t["role"].as_str()), (Some(FRIEND), Some(pane), Some("editor")));
    assert!(t["until_ms"].as_u64().is_some(), "{t}");
    assert!(v["rows"].as_array().unwrap().iter().all(|r| r["word"] == "this_machine"), "{v}");
    // Nothing the owner can't read elsewhere: the share is `/api/acl`'s,
    // the link `/api/shares`'.
    assert_eq!(d.get("/api/acl")["grants"][0]["principal"], share["principal"]);
    assert_eq!(d.get("/api/shares")[0]["expires_ms"], l["until_ms"]);

    // A read-only link to the session (`POST /api/links`, a key of its
    // own): one row with its end, and neither its key nor the pane link's
    // token anywhere in the answer.
    let key = "ab".repeat(32);
    let made = d.post("/api/links", json!({ "session": session, "key": key, "ttl_secs": 600 }));
    assert!(made["link"].as_str().is_some_and(|l| l.starts_with("link:")), "{made}");
    let (status, text) = d.raw("GET", "/api/access", None);
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&text).unwrap();
    let session_link = rows(&v, "link").into_iter().find(|r| r["pane"].is_null()).expect("the session link's row");
    assert_eq!(session_link["session"].as_u64(), Some(session));
    assert_eq!(session_link["until_ms"], made["expires"]);
    let token = link["token"].as_str().unwrap();
    for secret in [key.as_str(), token, "\"key\"", "\"token\""] {
        assert!(!text.contains(secret), "{secret} is in the answer: {text}");
    }

    // The editor the session was shared with gets none of it: the
    // operation is the owner's on the tailnet as on the socket.
    let (status, body) = d.raw_as(FRIEND, "GET", "/api/access", None);
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("only the owner"), "{body}");

    // Revoked: the row goes with the grant.
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": null }));
    assert!(rows(&d.get("/api/access"), "share").is_empty());
}
