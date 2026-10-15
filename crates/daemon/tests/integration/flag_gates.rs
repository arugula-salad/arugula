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
//!
//! Fountain left Labs (#942, F5): its routes and blocks have no gate, and a
//! `fountain` the flags file still names, on or off, changes nothing.

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
        // Configs that don't parse: on, they fail past the gate, in the
        // block type's own words, and nothing starts.
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

/// Fountain's requests, which no flag gates any more (#942, F5): with no
/// login here they fail for that, or in the block's own words.
fn fountain_requests() -> Vec<(&'static str, &'static str, Option<Value>)> {
    let block = |kind: &str, config: Value| Some(json!({ "type": kind, "config": config, "local": true }));
    vec![
        ("GET", "/api/fountain/agents", None),
        ("GET", "/api/fountain/login", None),
        ("POST", "/api/blocks", block("fountain", json!({ "profile": 42 }))),
        ("POST", "/api/blocks", block("agent", json!({ "agent": "fountain" }))),
    ]
}

/// A new install reaches Fountain's routes and blocks with no flag; so does
/// one whose flags file still says `fountain`, either way, from before F5.
/// `arugulad flags` doesn't list it, and setting it says why.
#[test]
fn fountain_needs_no_flag_and_an_old_entry_changes_nothing() {
    let d = daemon("flag-gates-fountain");
    for file in [None, Some(r#"{"flags":{"fountain":false}}"#), Some(r#"{"flags":{"fountain":true}}"#)] {
        if let Some(text) = file {
            std::fs::write(d.state.join("flags.json"), text).unwrap();
        }
        for (method, path, body) in fountain_requests() {
            let (status, text) = d.raw(method, path, body.clone());
            assert!(!text.contains("arugulad flags"), "{file:?}: {method} {path} {body:?}: {status} {text}");
            assert_ne!(status, 404, "{file:?}: {method} {path} {body:?}: {text}");
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
        assert!(err.contains("isn't a flag any more") && err.contains("arugula fountain login"), "{args:?}: {err}");
    }
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
