//! Each Labs flag gates its feature, not just its listing (#347): where the
//! machine has a flag off, its feature's routes, its block types and its
//! WebSocket messages are refused with how to turn it on, and the same
//! requests get past the gate once the flag is on, and are refused again
//! when it goes off. No restart: the flags are read on each request. The
//! CLI's hidden commands, run against such a daemon, say the same.
//!
//! "Past the gate" is all the on side asks: this daemon has no studio and
//! no sandbox provider, so most of them fail next for that, in their own
//! words.
//!
//! `fountain` left the flags with #942's F5, and daemon-side Managoat went
//! after it: a `fountain` the flags file still names, on or off, changes
//! nothing. The renamed flags' old names (aru-117) still work.

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
        ("sandbox-providers", "GET", "/api/machines".into(), None),
        ("sandbox-providers", "POST", "/api/machines/1/reset".into(), Some(json!({}))),
        ("sandbox-providers", "GET", "/api/sandboxes".into(), None),
        ("sandbox-providers", "POST", "/api/run".into(), Some(json!({ "vm": true }))),
        ("sandbox-providers", "POST", "/api/run".into(), Some(json!({ "sandbox": "someone-elses" }))),
        // Configs that don't parse: on, they fail past the gate, in the
        // block type's own words, and nothing starts.
        ("studio", "GET", "/api/studio".into(), None),
        ("studio", "GET", "/api/studio/apps".into(), None),
        ("studio", "POST", "/api/blocks".into(), block("app", json!({ "app": "nothing-here" }))),
        ("chant-workspaces", "POST", "/api/blocks".into(), block("workspace", json!({}))),
        ("guest-ssh", "GET", "/api/guests".into(), None),
        ("agent-delegation", "GET", "/api/a2a/agents".into(), None),
        (
            "additional-forges",
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
    set(&d.state, "studio", true);
    let (status, text) = d.raw("GET", "/api/studio", None);
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

/// The renamed flags' old names (aru-117) still turn their features on:
/// in `flags.json`, through `PUT /api/flags/<old>` and `arugulad flags <old>
/// on`. Only the new names are listed.
#[test]
fn an_old_flag_name_still_turns_its_feature_on() {
    let d = daemon("flag-gates-aliases");
    std::fs::write(d.state.join("flags.json"), r#"{"flags":{"vms":true}}"#).unwrap();
    let (_, text) = d.raw("GET", "/api/sandboxes", None);
    assert!(!text.contains("arugulad flags"), "vms in the file: {text}");
    let listed = d.get("/api/flags");
    let names: Vec<_> = listed.as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.iter().all(|n| !["vms", "workspaces", "forges", "agents"].contains(&n.as_str())), "{names:?}");
    assert!(listed.as_array().unwrap().iter().any(|f| f["name"] == "sandbox-providers" && f["on"] == true));

    let (status, text) = d.raw("PUT", "/api/flags/agents", Some(json!({ "on": true })));
    assert_eq!(status, 200, "{text}");
    let row: Value = serde_json::from_str(&text).unwrap();
    assert_eq!((&row["name"], &row["on"]), (&json!("agent-delegation"), &json!(true)), "{row}");
    let (_, text) = d.raw("GET", "/api/a2a/agents", None);
    assert!(!text.contains("arugulad flags"), "agents set by its old name: {text}");

    let out = arugula_testkit::command(Path::new(env!("CARGO_BIN_EXE_arugulad")))
        .args(["flags", "forges", "on", "--state-dir"])
        .arg(&d.state)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("additional-forges is on"));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(d.state.join("flags.json")).unwrap()).unwrap();
    assert_eq!(file["flags"], json!({ "vms": true, "agent-delegation": true, "additional-forges": true }));
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
    for (flag, args) in [
        ("sandbox-providers", &["machines"][..]),
        ("sandbox-providers", &["sandboxes"][..]),
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
