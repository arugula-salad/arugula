//! A build without Labs has no chat: the thread routes answer 501 with
//! "Chat isn't in this build", and an invite made from a thread is refused
//! while one without a thread works as it does everywhere.

#![cfg(not(feature = "labs"))]
// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use arugula_testkit::arugulad;
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";
const CHAT: &str = "Chat isn't in this build (built without labs)";

fn daemon(tag: &str) -> arugula_testkit::Daemon {
    arugulad!(tag).no_wisp().args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]).start()
}

fn first(d: &arugula_testkit::Daemon) -> (u64, u64) {
    let p = &d.get("/api/panes")[0];
    (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap())
}

#[test]
fn the_thread_routes_answer_501() {
    let d = daemon("labs-off-threads");
    let (pane, _) = first(&d);
    for (method, path, body) in [
        ("POST", format!("/api/threads/pane-{pane}"), Some(json!({ "text": "hi" }))),
        ("GET", format!("/api/threads/pane-{pane}"), None),
        ("POST", format!("/api/threads/pane-{pane}/read"), Some(json!({ "upto": 1 }))),
    ] {
        let (status, text) = d.raw(method, &path, body);
        assert_eq!(status, 501, "{method} {path}: {text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error"], CHAT, "{method} {path}: {text}");
    }
    assert!(!d.state.join("threads").exists(), "a build without Labs leaves the state dir's threads alone");
}

#[test]
fn an_invite_from_a_thread_is_refused_and_one_without_works() {
    let d = daemon("labs-off-invite");
    let (pane, session) = first(&d);
    let (status, text) = d.raw(
        "POST",
        "/api/invite",
        Some(json!({ "session": session, "who": FRIEND, "thread": format!("pane-{pane}"), "whole_thread": true })),
    );
    assert_eq!(status, 400, "{text}");
    assert!(text.contains(CHAT), "{text}");
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "viewer" }));
    let (status, text) = d.raw("POST", "/api/invite", Some(json!({ "session": session, "who": FRIEND })));
    assert_eq!(status, 200, "{text}");
}

/// Managoat left Labs (#942, F5), but its code is still the `labs` cargo
/// feature's: a build without it has no Managoat, whatever the flags file
/// says of `fountain` and whatever login is here. A Managoat block says it
/// isn't in this build, and the host offers none.
#[test]
fn fountain_is_not_in_a_build_without_labs() {
    let d = arugulad!("labs-off-fountain")
        .no_wisp()
        .args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"])
        .env("FOUNTAIN_API_KEY", "fk_test")
        .start();
    std::fs::write(d.state.join("flags.json"), r#"{"flags":{"fountain":true}}"#).unwrap();
    assert_eq!(d.get("/api/host")["features"]["fountain"], false);
    let (status, text) = d.raw("POST", "/api/blocks", Some(json!({ "type": "fountain", "config": {}, "local": true })));
    assert!((400..600).contains(&status), "{status} {text}");
    assert!(text.contains("A Managoat block isn't in this build"), "{text}");
    assert!(!text.contains("arugulad flags"), "{text}");
}
