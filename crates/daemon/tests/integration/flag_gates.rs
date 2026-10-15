//! Each Labs flag gates its feature, not just its listing (#347): where the
//! machine has a flag off, its feature's routes, its block types and its
//! WebSocket messages are refused with how to turn it on, and the same
//! requests get past the gate once the flag is on, and are refused again
//! when it goes off. No restart: the flags are read on each request. The
//! CLI's hidden commands, run against such a daemon, say the same.
//!
//! "Past the gate" is all the on side asks: this daemon has no studio, no
//! Fountain login and no sandbox provider, so most of them fail next for
//! that, in their own words.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::path::{Path, PathBuf};

use arugula_testkit::arugulad;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const OWNER: &str = "me@example.com";

fn daemon(tag: &str) -> arugula_testkit::Daemon {
    arugulad!(tag).no_wisp().args(["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]).start()
}

/// What a refusal for `flag` says, whatever else it says.
fn hint(flag: &str) -> String {
    format!("`arugulad flags {flag} on`")
}

fn set(state: &Path, flag: &str, on: bool) {
    arugula_proto::flags::set(state, flag, on).unwrap();
}

/// Each flag, and requests that are its feature's: a route, or a block.
fn requests(pane: u64) -> Vec<(&'static str, &'static str, String, Option<Value>)> {
    let block = |kind: &str, config: Value| Some(json!({ "type": kind, "config": config, "local": true }));
    vec![
        ("chat", "GET", format!("/api/threads/pane-{pane}"), None),
        ("chat", "POST", format!("/api/threads/pane-{pane}"), Some(json!({ "text": "hi" }))),
        ("chat", "POST", format!("/api/threads/pane-{pane}/read"), Some(json!({ "upto": 1 }))),
        ("vms", "GET", "/api/machines".into(), None),
        ("vms", "POST", "/api/machines/1/reset".into(), Some(json!({}))),
        ("vms", "GET", "/api/sandboxes".into(), None),
        ("vms", "POST", "/api/run".into(), Some(json!({ "vm": true }))),
        ("vms", "POST", "/api/run".into(), Some(json!({ "sandbox": "someone-elses" }))),
        ("fountain", "GET", "/api/fountain/agents".into(), None),
        // Configs that don't parse: on, they fail past the gate, in the
        // block type's own words, and nothing starts.
        ("fountain", "POST", "/api/blocks".into(), block("fountain", json!({ "profile": 42 }))),
        ("fountain", "POST", "/api/blocks".into(), block("agent", json!({ "agent": "fountain" }))),
        ("studio", "GET", "/api/studio".into(), None),
        ("studio", "GET", "/api/studio/apps".into(), None),
        ("studio", "POST", "/api/blocks".into(), block("app", json!({ "app": "nothing-here" }))),
        ("workspaces", "POST", "/api/blocks".into(), block("workspace", json!({}))),
        ("guest-ssh", "GET", "/api/guests".into(), None),
        ("agents", "GET", "/api/a2a/agents".into(), None),
        (
            "forges",
            "POST",
            "/api/blocks".into(),
            block("forge", json!({ "repo": "o/r", "number": 0, "provider": "forgejo" })),
        ),
    ]
}

#[test]
fn each_flag_gates_its_routes_and_blocks_both_ways() {
    let d = daemon("flag-gates");
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let off = |what: &str| {
        for (flag, method, path, body) in requests(pane) {
            let (status, text) = d.raw(method, &path, body.clone());
            assert!((400..500).contains(&status), "{what}: {method} {path} {body:?}: {status} {text}");
            assert!(text.contains(&hint(flag)), "{what}: {method} {path} {body:?}: {text}");
        }
    };
    // A new install: every flag off.
    off("by default");
    for (flag, method, path, body) in requests(pane) {
        set(&d.state, flag, true);
        let (_, text) = d.raw(method, &path, body.clone());
        assert!(!text.contains("arugulad flags"), "{flag} on: {method} {path} {body:?}: {text}");
        set(&d.state, flag, false);
    }
    // And off again, with no restart.
    off("off again");
    // Nothing was made meanwhile but the first pane.
    let panes = d.get("/api/panes");
    assert_eq!(panes.as_array().unwrap().len(), 1, "{panes}");

    // One flag on turns on its feature and no other.
    set(&d.state, "studio", true);
    let (status, text) = d.raw("GET", "/api/studio", None);
    assert_eq!(status, 200, "{text}");
    let (status, text) = d.raw("GET", "/api/guests", None);
    assert_eq!(status, 404, "{text}");
    assert!(text.contains(&hint("guest-ssh")), "{text}");
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next message, as JSON.
async fn next(ws: &mut Ws, deadline: tokio::time::Instant) -> Value {
    loop {
        let got = tokio::time::timeout_at(deadline, ws.next()).await.expect("no message in time");
        if let Some(Ok(Message::Text(t))) = got {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// A huddle is joined only where the machine has the `huddles` flag on.
#[tokio::test(flavor = "multi_thread")]
async fn a_huddle_is_joined_only_with_its_flag() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let d = daemon("flag-gates-huddle");
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
    req.headers_mut().insert("tailscale-user-login", OWNER.parse().unwrap());
    let (mut ws, _) = connect_async(req).await.unwrap();
    let join = json!({ "type": "call_join", "session": session }).to_string();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while next(&mut ws, deadline).await["type"] != "hello" {}

    // Off: refused, saying how to turn it on.
    ws.send(Message::Text(join.clone().into())).await.unwrap();
    let refusal = loop {
        let v = next(&mut ws, deadline).await;
        if v["type"] == "error" {
            break v;
        }
    };
    assert!(refusal["message"].as_str().unwrap().contains(&hint("huddles")), "{refusal}");

    // On: the same message starts a huddle, which the state then has.
    set(&d.state, "huddles", true);
    ws.send(Message::Text(join.into())).await.unwrap();
    loop {
        let v = next(&mut ws, deadline).await;
        assert_ne!(v["type"], "error", "{v}");
        if v.to_string().contains("\"members\":[{") {
            break;
        }
    }
}

fn cli_bin() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_arugulad")).with_file_name("arugula");
    let status = std::process::Command::new(env!("CARGO")).args(["build", "-q", "-p", "arugula"]).status().unwrap();
    assert!(status.success(), "building the CLI");
    bin
}

/// A command `--help` hides without its flag still runs, and against a
/// daemon that has the flag off it says how to turn it on there.
#[test]
fn a_hidden_command_says_how_to_turn_its_flag_on() {
    let d = daemon("flag-gates-cli");
    let cli = cli_bin();
    let run = |args: &[&str]| arugula_testkit::command(&cli).arg("--socket").arg(d.sock()).args(args).output().unwrap();
    for (flag, args) in [
        ("vms", &["machines"][..]),
        ("vms", &["sandboxes"][..]),
        ("studio", &["studio"][..]),
        ("studio", &["app"][..]),
        ("guest-ssh", &["guests"][..]),
    ] {
        let out = run(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} succeeded with {flag} off");
        assert!(err.contains(&hint(flag)), "{args:?}: {err}");
        set(&d.state, flag, true);
        let out = run(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("arugulad flags"), "{args:?} with {flag} on: {err}");
        set(&d.state, flag, false);
    }
}
