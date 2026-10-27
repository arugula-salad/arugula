//! `telemetry.note` (M103): the web client's view counts, into the day's,
//! read back through `telemetry.get`'s preview. Only while telemetry is on,
//! only from the owner in person, and capped.

use arugula_testkit::arugulad;
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";

/// A daemon telemetry may be on in: none of what keeps it off (the
/// testkit's `ARUGULA_TELEMETRY=off`, nextest, CI), and reports go nowhere.
fn daemon(tag: &str) -> arugula_testkit::Daemon {
    arugulad!(tag)
        .no_wisp()
        .args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"])
        .env_remove("ARUGULA_TELEMETRY")
        .env_remove("NEXTEST_RUN_ID")
        .env_remove("CI")
        .env_remove("DO_NOT_TRACK")
        .env("ARUGULA_TELEMETRY_URL", "http://127.0.0.1:9")
        .start()
}

/// The preview's coming `daily`.
fn daily(d: &arugula_testkit::Daemon) -> Value {
    let info = d.get("/api/telemetry");
    info["preview"]["events"].as_array().unwrap().iter().find(|e| e["event"] == "daily").cloned().unwrap()
}

fn note(d: &arugula_testkit::Daemon, body: Value) -> u16 {
    d.raw("POST", "/api/telemetry/note", Some(body)).0
}

#[test]
fn a_note_counts_only_while_on_and_only_the_owners() {
    let d = daemon("telemetry-note");
    let body = json!({ "views": { "swarm": 2, "chat": 1, "made-up": 5 }, "web_errors": 1 });

    // Not asked: taken, and nothing counted.
    assert_eq!(note(&d, body.clone()), 200);
    assert_eq!(daily(&d)["views"], json!({}));

    // On: counted, an unknown view as `other`.
    d.put("/api/telemetry", json!({ "on": true, "via": "web" }));
    assert_eq!(note(&d, body.clone()), 200);
    let day = daily(&d);
    assert_eq!(day["views"], json!({ "swarm": 2, "chat": 1, "other": 5 }), "{day}");
    assert_eq!(day["web_errors"], 1);

    // Capped: one note can't run a count up.
    assert_eq!(note(&d, json!({ "views": { "swarm": 4_000_000_000u32 }, "web_errors": 4_000_000_000u32 })), 200);
    let day = daily(&d);
    assert_eq!(day["views"]["swarm"], 1_002, "{day}");
    assert_eq!(day["web_errors"], 1_001);

    // An agent on the owner's CLI and a guest are refused, and count nothing.
    let bearer = d.bearer();
    let text = body.to_string();
    let agent = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    assert_eq!(d.tcp("POST", "/api/telemetry/note", &agent, Some(&text)).0, 403);
    let guest = [("tailscale-user-login", "friend@example.com"), ("content-type", "application/json")];
    assert_eq!(d.tcp("POST", "/api/telemetry/note", &guest, Some(&text)).0, 403);
    assert_eq!(daily(&d)["views"]["swarm"], 1_002);

    // Off: taken, counted nowhere (no's counts are gone, and stay gone).
    d.put("/api/telemetry", json!({ "on": false, "via": "web" }));
    assert_eq!(note(&d, body), 200);
    assert_eq!(daily(&d)["views"], json!({}));
}
