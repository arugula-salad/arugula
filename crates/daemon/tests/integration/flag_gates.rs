//! Each Labs flag gates its feature, not just its listing (#347): where the
//! machine has a flag off, its feature's routes, its block types and its
//! WebSocket messages are refused with how to turn it on, and the same
//! requests get past the gate once the flag is on, and are refused again
//! when it goes off. No restart: the flags are read on each request. The
//! CLI's hidden commands, run against such a daemon, say the same.
//!
//! "Past the gate" is all the on side asks: this daemon has no sandbox
//! provider, so most of them fail next for that, in their own words.
//!
//! `fountain` left the flags with #942's F5, and daemon-side Managoat went
//! after it: a `fountain` the flags file still names, on or off, changes
//! nothing. `studio` (the daemon's studio login) is retired the same way
//! (native #206): an app block is anyone's, by its address.

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
fn requests() -> Vec<(&'static str, &'static str, String, Option<Value>)> {
    let block = |kind: &str, config: Value| Some(json!({ "type": kind, "config": config, "local": true }));
    vec![
        ("vms", "GET", "/api/machines".into(), None),
        ("vms", "POST", "/api/machines/1/reset".into(), Some(json!({}))),
        ("vms", "GET", "/api/sandboxes".into(), None),
        ("vms", "POST", "/api/run".into(), Some(json!({ "vm": true }))),
        ("vms", "POST", "/api/run".into(), Some(json!({ "sandbox": "someone-elses" }))),
        // Configs that don't parse: on, they fail past the gate, in the
        // block type's own words, and nothing starts.
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
    let off = |what: &str| {
        for (flag, method, path, body) in requests() {
            let (status, text) = d.raw(method, &path, body.clone());
            assert!((400..500).contains(&status), "{what}: {method} {path} {body:?}: {status} {text}");
            assert!(text.contains(&hint(flag)), "{what}: {method} {path} {body:?}: {text}");
        }
    };
    // A new install: every flag off.
    off("by default");
    for (flag, method, path, body) in requests() {
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
    set(&d.state, "agents", true);
    let (status, text) = d.raw("GET", "/api/a2a/agents", None);
    assert_eq!(status, 200, "{text}");
    let (status, text) = d.raw("GET", "/api/guests", None);
    assert_eq!(status, 404, "{text}");
    assert!(text.contains(&hint("guest-ssh")), "{text}");
}

/// Daemon-side Managoat's requests, removed with it: its routes are gone,
/// and its blocks are refused, saying where cloud agents went (an agent)
/// or that there's no such block (the account view).
fn managoat_requests() -> Vec<(&'static str, &'static str, Option<Value>)> {
    let block = |kind: &str, config: Value| Some(json!({ "type": kind, "config": config, "local": true }));
    vec![
        ("GET", "/api/fountain/agents", None),
        ("GET", "/api/fountain/login", None),
        ("POST", "/api/blocks", block("managoat", json!({}))),
        ("POST", "/api/blocks", block("agent", json!({ "agent": "managoat", "managoat_agent": "arena" }))),
    ]
}

/// Daemon-side Managoat is gone whatever the flags file says of `fountain`
/// (from before F5, either way). `arugulad flags` doesn't list it, and
/// setting it says why.
#[test]
fn managoat_is_gone_and_an_old_flag_changes_nothing() {
    let d = daemon("flag-gates-fountain");
    for file in [None, Some(r#"{"flags":{"fountain":false}}"#), Some(r#"{"flags":{"fountain":true}}"#)] {
        if let Some(text) = file {
            std::fs::write(d.state.join("flags.json"), text).unwrap();
        }
        for (method, path, body) in managoat_requests() {
            let (status, text) = d.raw(method, path, body.clone());
            assert!(!text.contains("arugulad flags"), "{file:?}: {method} {path} {body:?}: {status} {text}");
            assert!((400..500).contains(&status), "{file:?}: {method} {path} {body:?}: {status} {text}");
        }
        assert!(d.get("/api/flags").as_array().unwrap().iter().all(|f| f["name"] != "fountain"));
    }
    // What the file said is kept, for an older daemon reading it.
    set(&d.state, "chat", true);
    let file: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("flags.json")).unwrap()).unwrap();
    assert_eq!(file["flags"], json!({ "fountain": true, "chat": true }));

    let arugulad = |args: &[&str]| {
        arugula_testkit::command(Path::new(env!("CARGO_BIN_EXE_arugulad")))
            .args(args)
            .arg("--state-dir")
            .arg(&d.state)
            .output()
            .unwrap()
    };
    let list = arugulad(&["flags"]);
    assert!(list.status.success());
    let list = String::from_utf8_lossy(&list.stdout);
    assert!(list.contains("chat") && !list.lines().any(|l| l.starts_with("fountain")), "{list}");
    for args in [&["flags", "fountain", "on"][..], &["flags", "fountain"][..]] {
        let out = arugulad(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}");
        assert!(err.contains("isn't a flag any more") && err.contains("Cloud workspaces"), "{args:?}: {err}");
    }
}

/// The studio login is retired (native #206): its routes are gone, an app
/// block is made by its address whatever the flags file says of `studio`,
/// a name alone is refused for want of one, and `arugulad flags` doesn't
/// list it.
#[test]
fn the_studio_login_is_gone_and_an_old_flag_changes_nothing() {
    let d = daemon("flag-gates-studio");
    for file in [None, Some(r#"{"flags":{"studio":false}}"#), Some(r#"{"flags":{"studio":true}}"#)] {
        if let Some(text) = file {
            std::fs::write(d.state.join("flags.json"), text).unwrap();
        }
        for path in ["/api/studio", "/api/studio/apps"] {
            let (status, text) = d.raw("GET", path, None);
            assert_eq!(status, 404, "{file:?}: {path}: {text}");
        }
        let block = |config: Value| Some(json!({ "type": "app", "config": config, "local": true }));
        let (status, text) = d.raw("POST", "/api/blocks", block(json!({ "app": "nothing-here" })));
        assert_eq!(status, 400, "{file:?}: {text}");
        assert!(text.contains("address") && !text.contains("arugulad flags"), "{file:?}: {text}");
        let (status, text) = d.raw("POST", "/api/blocks", block(json!({ "box_url": "https://pinboard.example/x" })));
        assert_eq!(status, 200, "{file:?}: {text}");
        let id = serde_json::from_str::<Value>(&text).unwrap()["block"].as_u64().unwrap();
        assert_eq!(d.get(&format!("/api/blocks/{id}"))["state"]["box_url"], "https://pinboard.example");
        assert!(d.get("/api/flags").as_array().unwrap().iter().all(|f| f["name"] != "studio"));
    }
    let out = arugula_testkit::command(Path::new(env!("CARGO_BIN_EXE_arugulad")))
        .args(["flags", "studio", "on", "--state-dir"])
        .arg(&d.state)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("isn't a flag any more"), "{err}");
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

/// Huddles were removed (2026-10-18). A page from before, or a `flags.json`
/// that still has them on, sends a call message: it's dropped, and the
/// connection carries on as if it hadn't come.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_message_from_an_older_page_is_dropped() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let d = daemon("flag-gates-old-call");
    std::fs::write(d.state.join("flags.json"), r#"{"flags":{"huddles":true}}"#).unwrap();
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
    req.headers_mut().insert("tailscale-user-login", OWNER.parse().unwrap());
    let (mut ws, _) = connect_async(req).await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while next(&mut ws, deadline).await["type"] != "hello" {}
    for m in [
        json!({ "type": "call_join", "session": session }),
        json!({ "type": "call_signal", "session": session, "to": 99, "signal": {} }),
        json!({ "type": "ping", "id": 7 }),
    ] {
        ws.send(Message::Text(m.to_string().into())).await.unwrap();
    }
    loop {
        let v = next(&mut ws, deadline).await;
        assert_ne!(v["type"], "error", "{v}");
        if v["type"] == "pong" {
            assert_eq!(v["id"], 7, "{v}");
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
    for (flag, args) in [("vms", &["machines"][..]), ("vms", &["sandboxes"][..]), ("guest-ssh", &["guests"][..])] {
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
