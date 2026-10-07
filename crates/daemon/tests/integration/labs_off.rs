//! A build without Labs has no chat and no huddles: the thread routes
//! answer 501 with "Chat isn't in this build", a huddle message gets an
//! error back, and an invite made from a thread is refused while one
//! without a thread works as it does everywhere.

#![cfg(not(feature = "labs"))]
// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use arugula_testkit::arugulad;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";
const CHAT: &str = "Chat isn't in this build (built without labs)";
const HUDDLES: &str = "A huddle isn't in this build (built without labs)";

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

#[tokio::test(flavor = "multi_thread")]
async fn a_huddle_message_gets_the_refusal() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let d = daemon("labs-off-huddle");
    let (_, session) = first(&d);
    let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
    req.headers_mut().insert("tailscale-user-login", OWNER.parse().unwrap());
    let (mut ws, _) = connect_async(req).await.unwrap();
    let mut hello = false;
    let mut refusals = 0;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while refusals < 4 {
        let Some(Ok(Message::Text(t))) = tokio::time::timeout_at(deadline, ws.next()).await.expect("no refusal") else {
            continue;
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        match v["type"].as_str() {
            Some("hello") if !hello => {
                hello = true;
                // Left out when empty, as in every build.
                for key in ["calls", "threads"] {
                    assert!(v["state"][key].as_array().is_none_or(Vec::is_empty), "{key}: {v}");
                }
                for m in [
                    json!({ "type": "call_join", "session": session }),
                    json!({ "type": "call_leave", "session": session }),
                    json!({ "type": "call_mute", "session": session, "muted": true }),
                    json!({ "type": "call_signal", "session": session, "to": 99, "signal": {} }),
                ] {
                    ws.send(Message::Text(m.to_string().into())).await.unwrap();
                }
            }
            Some("error") => {
                assert_eq!(v["message"], HUDDLES, "{v}");
                refusals += 1;
            }
            _ => {}
        }
    }
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
