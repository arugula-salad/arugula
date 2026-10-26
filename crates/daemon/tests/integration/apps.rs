//! M35: app blocks, and questions raised on a page's block.
//!
//! An app's box is opened by its address and framed on its own origin; the
//! box signs the person in itself, through the hub, so the daemon keeps no
//! token and mints nothing (native #206 retired the studio login that
//! listed apps and minted ways in, and the hud follower that came with it).
//! Something following a page's agent (S22's bridge) asks on its block
//! through `/ask`, and guests who only watch can't answer.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;

use agentd::*;
use serde_json::{Value, json};

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";
/// An app's box: nothing has to answer there for the block.
const BOX: &str = "http://127.0.0.1:9";

fn info(d: &Daemon, pane: u64) -> Value {
    d.get("/api/panes").as_array().unwrap().iter().find(|p| p["id"] == pane).cloned().unwrap_or_default()
}

fn wait_ask(d: &Daemon, pane: u64, id: &str) -> Value {
    d.wait_for(&format!("the card for {id}"), || info(d, pane)["ask"]["id"] == id);
    info(d, pane)
}

/// An app block by its address, with no flag and no login: its origin is
/// kept, a reload reaches every client, it comes back after a restart, and
/// it has no way in to mint. One an older daemon saved by its studio name
/// comes back from its box.
#[test]
fn an_app_block_opens_by_its_address_and_comes_back() {
    let mut d = Daemon::child_with(&["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]);
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let (status, body) =
        d.raw("POST", "/api/blocks", Some(json!({ "type": "app", "config": { "app": "pinboard" }, "split": pane })));
    assert_eq!(status, 400, "a name alone was studio's to look up: {body}");
    let b = d.post(
        "/api/blocks",
        json!({ "type": "app", "config": { "box_url": "https://pinboard.example/x?y", "title": "Pinboard" }, "split": pane }),
    )["block"]
        .as_u64()
        .unwrap();
    let s = d.state(b);
    assert_eq!(
        (s["box_url"].as_str(), s["title"].as_str()),
        (Some("https://pinboard.example"), Some("Pinboard")),
        "{s}"
    );
    for gone in ["studio", "follower", "follower_credential", "gates"] {
        assert!(s.get(gone).is_none(), "{gone} in {s}");
    }
    assert_eq!(info(&d, b)["type"], "app");
    d.call(b, "reload", json!({}));
    assert_eq!(d.state(b)["reloads"], 1);
    let (status, body) = d.raw("POST", &format!("/api/blocks/{b}/call/enter"), Some(json!({})));
    assert!((400..500).contains(&status), "nothing mints a way in: {status} {body}");

    d.stop();
    assert!(!d.state.join("studio.json").exists());
    d.start();
    assert_eq!(d.state(b)["box_url"], "https://pinboard.example");
    assert_eq!(info(&d, b)["type"], "app");
}

/// The studio login's token, left by an older daemon, is removed when the
/// daemon starts.
#[test]
fn an_old_studio_token_is_removed() {
    let d = Daemon::child_in(&["--owner", OWNER], &[], |state| {
        std::fs::create_dir_all(state).unwrap();
        std::fs::write(state.join("studio.json"), r#"{"url":"https://s.example","token":"studio_pat_x"}"#).unwrap();
    });
    d.wait_for("the old token to go", || !d.state.join("studio.json").exists());
    let (status, _) = d.raw("GET", "/api/studio", None);
    assert_eq!(status, 404);
}

#[test]
fn asks_on_a_browser_block_and_who_may_answer() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let d = Daemon::child_labs_with(&[
        "--wisp-token-file",
        "/nonexistent",
        "--owner",
        OWNER,
        "--tailscale-socket",
        "/nonexistent/sock",
    ]);
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    let page = d.post("/api/blocks", json!({ "type": "browser", "config": { "url": BOX }, "split": pane }))["block"]
        .as_u64()
        .unwrap();

    // Something following the page's agent (S22's bridge) asks on the block.
    let ask = |id: &str| {
        let (sock, id) = (d.sock(), id.to_owned());
        std::thread::spawn(move || {
            let mut s = std::os::unix::net::UnixStream::connect(sock).unwrap();
            let body = json!({ "id": id, "source": "bridge", "agent": "hud",
                "questions": [{ "question": format!("Ship {id}?"), "header": "hud", "multiSelect": false,
                    "options": [{ "label": "Yes" }, { "label": "No" }] }] })
            .to_string();
            use std::io::{Read, Write};
            write!(
                s,
                "POST /api/panes/{page}/ask HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            let start = out.find("\r\n\r\n").unwrap() + 4;
            serde_json::from_str::<Value>(out[start..].trim()).unwrap_or(Value::String(out))
        })
    };
    let asked = ask("q1");
    let i = wait_ask(&d, page, "q1");
    assert_eq!((i["ask"]["source"].as_str(), i["ask"]["agent"].as_str()), (Some("bridge"), Some("hud")));
    assert_eq!(i["reason"]["ask"]["agent"], "hud");
    assert!(i["reason"]["bundle"].as_str().unwrap().ends_with(":hud"), "{i}");
    // Withdrawing another question leaves this one.
    let (status, body) = d.raw("POST", &format!("/api/panes/{page}/ask/withdraw"), Some(json!({ "id": "nope" })));
    assert_eq!(status, 200, "{body}");
    assert_eq!(info(&d, page)["ask"]["id"], "q1");

    // A guest who watches can't answer it; one who edits can, as themselves.
    let http = reqwest::Client::new();
    let as_friend = |path: String, body: Value| {
        let r = rt.block_on(
            http.post(format!("http://127.0.0.1:{}{path}", d.port))
                .header("tailscale-user-login", FRIEND)
                .json(&body)
                .send(),
        );
        let r = r.unwrap();
        (r.status().as_u16(), rt.block_on(r.text()).unwrap())
    };
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "viewer" }));
    let answer = json!({ "action": "answer", "pane": page, "content": { "question_0": "Yes" } });
    let (status, body) = as_friend("/api/attention/act".into(), answer.clone());
    assert_eq!(status, 403, "{body}");
    let (status, body) =
        as_friend(format!("/api/blocks/{page}/call/answer"), json!({ "content": { "question_0": "Yes" } }));
    assert_eq!(status, 403, "{body}");
    assert_eq!(info(&d, page)["ask"]["id"], "q1");
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "editor" }));
    let (status, body) =
        as_friend(format!("/api/blocks/{page}/call/answer"), json!({ "id": "q1", "content": { "question_0": "Yes" } }));
    assert_eq!(status, 200, "{body}");
    let got = asked.join().unwrap();
    assert_eq!(got["action"], "accept", "{got}");
    assert_eq!(got["content"]["question_0"], "Yes");
    assert_eq!(got["by"]["who"], format!("tailnet:{FRIEND}"), "{got}");
    assert_eq!(info(&d, page)["answered"]["who"], format!("tailnet:{FRIEND}"));

    // Withdrawn by its asker.
    let asked = ask("q2");
    wait_ask(&d, page, "q2");
    d.post(&format!("/api/panes/{page}/ask/withdraw"), json!({ "id": "q2" }));
    assert_eq!(asked.join().unwrap()["action"], "withdrawn");
    assert!(info(&d, page)["ask"].is_null());
}
