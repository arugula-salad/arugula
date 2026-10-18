//! #963: the Doc block, against the real daemon. An agent builds a document
//! over MCP (`doc new`, then `doc fill`) and every client attached sees it
//! fill in: as patches, and only a page that shows it. An agent block's
//! token drives its own Doc and no one else's; its approvals let `doc`
//! through with no card (the fake ACP agent); the owner writes it and an
//! editor or a viewer can't; the flag hides it and refuses it; a restart
//! brings it back; and an idle Doc costs nothing.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{
    net::TcpStream,
    path::Path,
    time::{Duration, SystemTime},
};

use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{self, Message, WebSocket, client::IntoClientRequest};

use crate::agentd::*;
use crate::mcp::agent_mcp;

const OWNER: &str = "me@example.com";
const FRIEND: &str = "friend@example.com";
const EDITOR: &str = "pal@example.com";

/// A daemon with its owner, FRIEND a viewer and EDITOR an editor of its
/// session, the `docs` flag on, and the first terminal pane.
fn shared() -> (Daemon, u64, u64) {
    let args = ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"];
    let d = Daemon::child_with(&args);
    arugula_proto::flags::set(&d.state, "docs", true).unwrap();
    let p = &d.get("/api/panes")[0];
    let (pane, session) = (p["id"].as_u64().unwrap(), p["session"].as_u64().unwrap());
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "viewer" }));
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{EDITOR}"), "role": "editor" }));
    (d, pane, session)
}

fn outline(n: usize) -> Value {
    let sections: Vec<Value> =
        (1..=n).map(|i| json!({ "heading": format!("Part {i}"), "intent": format!("what part {i} holds") })).collect();
    json!({ "kind": "new", "title": "Report", "sections": sections })
}

fn fill(block: u64, section: &str, markdown: &str) -> Value {
    json!({ "kind": "fill", "block": block, "section": section, "markdown": markdown })
}

/// One WebSocket client, as a page is: it takes patches, and says which
/// tab it shows.
struct Page {
    ws: WebSocket<TcpStream>,
    hello: Value,
}

impl Page {
    /// As the owner (`None`) or a tailnet user, taking patches as a page does.
    fn connect(d: &Daemon, login: Option<&str>) -> Page {
        let mut page = Page::connect_plain(d, login);
        page.send(json!({ "type": "block_patches" }));
        page
    }

    /// ...or one that doesn't (the CLI's fronts).
    fn connect_plain(d: &Daemon, login: Option<&str>) -> Page {
        let stream = TcpStream::connect(("127.0.0.1", d.port)).unwrap();
        let req = match login {
            None => d.ws("/ws"),
            Some(who) => {
                let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
                req.headers_mut().insert("Tailscale-User-Login", who.parse().unwrap());
                req
            }
        };
        let (ws, _) = tungstenite::client(req, stream).expect("the WebSocket");
        let mut page = Page { ws, hello: Value::Null };
        page.hello = page.until("hello", |m| m["type"] == "hello");
        page
    }

    fn send(&mut self, msg: Value) {
        self.ws.send(Message::Text(msg.to_string().into())).unwrap();
    }

    /// The next JSON message, or `None` after `wait`.
    fn recv(&mut self, wait: Duration) -> Option<Value> {
        self.ws.get_ref().set_read_timeout(Some(wait)).unwrap();
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return serde_json::from_str(&t).ok(),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
                {
                    return None;
                }
                Err(e) => panic!("the WebSocket: {e}"),
            }
        }
    }

    fn until(&mut self, what: &str, mut f: impl FnMut(&Value) -> bool) -> Value {
        let end = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < end {
            if let Some(m) = self.recv(Duration::from_millis(500))
                && f(&m)
            {
                return m;
            }
        }
        panic!("no {what}");
    }

    /// Show the tab `pane` is in, as a page does.
    fn show(&mut self, pane: u64) {
        let tabs = self.hello["state"]["tabs"].as_array().unwrap();
        let has = |t: &&Value| t["layout"]["panes"].as_array().unwrap().iter().any(|p| p[0] == pane);
        let tab = tabs.iter().find(has).map(|t| t["id"].clone()).expect("a tab with the pane");
        self.send(json!({ "type": "view", "tab": tab, "cols": 100, "rows": 30, "zoom": null, "claim": false }));
    }

    /// Everything the daemon sent before it read this: a ping answered.
    fn sync(&mut self) {
        self.send(json!({ "type": "ping", "id": 9 }));
        self.until("a pong", |m| m["type"] == "pong" && m["id"] == 9);
    }

    /// Every `block` message for `id` until nothing else arrives for `quiet`.
    fn blocks_for(&mut self, id: u64, quiet: Duration) -> Vec<Value> {
        let mut out = vec![];
        while let Some(m) = self.recv(quiet) {
            if m["type"] == "block" && m["block"] == id {
                out.push(m["state"].clone());
            }
        }
        out
    }

    /// What a page that has just shown the Doc is sent: its states until
    /// they stop, the last of them (the Doc whole, as it is now).
    fn settled(&mut self, id: u64) -> Value {
        let mut state = self.block(id);
        for next in self.blocks_for(id, Duration::from_millis(600)) {
            state = arugula_proto::block_patch::merge(&state, next).expect("each follows the last");
        }
        state
    }

    /// The next `block` message for `id`.
    fn block(&mut self, id: u64) -> Value {
        self.until("a block message", |m| m["type"] == "block" && m["block"] == id)["state"].clone()
    }
}

fn bodies(state: &Value) -> Vec<Option<String>> {
    state["entries"].as_array().unwrap().iter().map(|e| e["body"].as_str().map(str::to_owned)).collect()
}

/// Every file under `dir` with its modification time.
fn mtimes(dir: &Path) -> Vec<(String, SystemTime)> {
    let mut out = vec![];
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let meta = e.metadata().unwrap();
            out.push((e.path().display().to_string(), meta.modified().unwrap()));
            if meta.is_dir() {
                todo.push(e.path());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_agent_opens_a_doc_beside_its_pane_and_clients_watch_it_fill() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let made = m.call("doc", outline(3)).unwrap();
    let doc = made["block"].as_u64().unwrap();
    let ids: Vec<&str> = made["sections"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["s1", "s2", "s3"], "{made}");

    // A block of kind doc, in the pane's own tab, all sections pending.
    let panes = d.get("/api/panes");
    let find = |id: u64| panes.as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap();
    assert_eq!(find(doc)["type"], "doc");
    assert_eq!(find(doc)["tab"], find(pane)["tab"], "beside the pane");
    assert_eq!(find(doc)["started_by"]["by"], "mcp:claude-code");
    let state = d.state(doc);
    assert_eq!(state["title"], "Report");
    assert_eq!(bodies(&state), [None, None, None]);
    assert_eq!(state["entries"][1]["intent"], "what part 2 holds");

    // The owner's page and a viewer's both show the tab.
    let mut owner = Page::connect(&d, None);
    let mut viewer = Page::connect(&d, Some(FRIEND));
    for page in [&mut owner, &mut viewer] {
        page.show(pane);
    }
    let mut had = [owner.settled(doc), viewer.settled(doc)];
    for state in &had {
        assert!(state.get("entries_at").is_none(), "whole to start with: {state}");
        assert_eq!(bodies(state), [None, None, None]);
    }

    // Filled in order, each is a patch from the section filled, and the
    // patches add up to what the daemon holds.
    for (i, id) in ["s1", "s2", "s3"].iter().enumerate() {
        m.call("doc", fill(doc, id, &format!("Text of {id}."))).unwrap();
        for (page, had) in [&mut owner, &mut viewer].into_iter().zip(&mut had) {
            let patch = page.block(doc);
            assert_eq!(patch["entries_at"], i, "{patch}");
            assert_eq!(patch["entries"].as_array().unwrap().len(), 3 - i, "{patch}");
            *had = arugula_proto::block_patch::merge(had, patch).expect("each patch follows the last");
        }
    }
    for state in &had {
        assert_eq!(bodies(state), bodies(&d.state(doc)));
        assert_eq!(bodies(state)[2].as_deref(), Some("Text of s3."));
    }

    // An early refill after later ones are filled re-sends the tail: the
    // rule is a suffix from the first entry that changed.
    m.call("doc", fill(doc, "s1", "First, again.")).unwrap();
    let patch = viewer.block(doc);
    assert_eq!((patch["entries_at"].as_u64(), patch["entries"].as_array().unwrap().len()), (Some(0), 3), "{patch}");
    let merged = arugula_proto::block_patch::merge(&had[1], patch).unwrap();
    assert_eq!(bodies(&merged), bodies(&d.state(doc)));

    // The text is the Doc as Markdown.
    let text = d.raw("GET", &format!("/api/panes/{doc}/capture"), None).1;
    assert!(text.contains("# Report") && text.contains("## Part 2") && text.contains("Text of s2."), "{text}");
    drop(m);
}

#[test]
fn a_page_that_doesnt_show_the_doc_is_sent_nothing_of_it() {
    let (d, pane, _) = shared();
    let other = d.post("/api/run", json!({}))["pane"].as_u64().unwrap();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();

    // One page shows the Doc's tab, one another; a client that doesn't
    // take patches (the CLI's fronts) is sent every state, as before.
    let mut shows = Page::connect(&d, None);
    let mut elsewhere = Page::connect(&d, None);
    shows.show(pane);
    elsewhere.show(other);
    elsewhere.sync();
    shows.settled(doc);
    let mut cli = Page::connect_plain(&d, None);

    m.call("doc", fill(doc, "s1", "One.")).unwrap();
    let seen = shows.block(doc);
    assert_eq!(bodies(&seen)[0].as_deref(), Some("One."));
    assert!(!cli.blocks_for(doc, Duration::from_millis(700)).is_empty(), "the CLI's front follows it");
    assert!(
        elsewhere.blocks_for(doc, Duration::from_secs(1)).is_empty(),
        "a page showing another tab is sent nothing of the Doc"
    );

    // It shows it: the Doc whole, as it is now, then patches.
    elsewhere.show(pane);
    let state = elsewhere.block(doc);
    assert!(state.get("entries_at").is_none(), "{state}");
    assert_eq!(bodies(&state), bodies(&d.state(doc)));
    drop(m);
}

#[test]
fn an_idle_doc_sends_and_writes_nothing() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    m.call("doc", fill(doc, "s1", "Written.")).unwrap();
    let mut page = Page::connect(&d, None);
    page.show(pane);
    page.settled(doc);
    let dir = d.state.join(format!("blocks/{doc}"));
    let before = mtimes(&dir);
    assert!(before.iter().any(|(p, _)| p.ends_with("doc/s1.md")), "{before:?}");

    // Nothing polls and nothing ticks: five quiet seconds.
    let frames = page.blocks_for(doc, Duration::from_secs(5));
    assert!(frames.is_empty(), "{} block messages while idle: {frames:?}", frames.len());
    assert_eq!(mtimes(&dir), before, "nothing under blocks/{doc}/ was written");
    drop(m);
}

#[test]
fn the_owner_writes_a_doc_and_an_editor_and_a_viewer_cant() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    let path = format!("/api/blocks/{doc}/call/doc_fill");
    let body = || Some(json!({ "section": "s1", "markdown": "Rewritten by someone else." }));
    for who in [FRIEND, EDITOR] {
        let (status, text) = d.raw_as(who, "POST", &path, body());
        assert_eq!(status, 403, "{who}: {text}");
        let (status, text) = d.raw_as(who, "POST", &format!("/api/blocks/{doc}/call/doc_outline"), body());
        assert_eq!(status, 403, "{who}: {text}");
    }
    assert_eq!(bodies(&d.state(doc))[0], None, "nothing was written");
    let (status, text) = d.raw("POST", &path, body());
    assert_eq!(status, 200, "{text}");
    assert_eq!(bodies(&d.state(doc))[0].as_deref(), Some("Rewritten by someone else."));
    // Reading is for everyone the session is shared with.
    for who in [FRIEND, EDITOR] {
        let (status, text) = d.raw_as(who, "GET", &format!("/api/blocks/{doc}"), None);
        assert_eq!(status, 200, "{who}: {text}");
        assert!(text.contains("Rewritten by someone else."), "{text}");
    }
    drop(m);
}

#[test]
fn an_agent_block_drives_its_own_doc_and_no_one_elses() {
    let (d, _, _) = shared();
    let a = d.open("hello");
    d.wait(a, "idle");
    // Through the agent's own token (`Scope::Block`): it opens the Doc
    // with `doc new`, which records that it started it...
    let made = agent_mcp(&d, a, "doc", outline(2)).unwrap();
    let doc = made["block"].as_u64().unwrap();
    assert_eq!(d.get(&format!("/api/blocks/{doc}"))["info"]["started_by"]["block"], a);
    // ...so it can fill it,
    agent_mcp(&d, a, "doc", fill(doc, "s1", "Mine.")).unwrap();
    agent_mcp(&d, a, "doc", fill(doc, "s1", "Mine, again.")).unwrap();
    assert_eq!(bodies(&d.state(doc))[0].as_deref(), Some("Mine, again."));
    let e = agent_mcp(&d, a, "doc", fill(doc, "s9", "x")).unwrap_err();
    assert!(e.contains("no section") && e.contains("s1, s2"), "{e}");

    // ...but not a Doc another agent in its tab started,
    let b = agent_mcp(
        &d,
        a,
        "start_agent",
        json!({ "agent": "acp", "command": format!("python3 {}", fake()), "prompt": "hello" }),
    )
    .unwrap()["block"]
        .as_u64()
        .unwrap();
    d.wait(b, "idle");
    let e = agent_mcp(&d, b, "doc", fill(doc, "s2", "Not yours.")).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    assert_eq!(bodies(&d.state(doc))[1], None);
    // ...and a Doc is the only thing `doc fill` writes to.
    let file = d.sessions.join("notes.txt");
    std::fs::write(&file, "x").unwrap();
    let shown = agent_mcp(&d, a, "show", json!({ "kind": "file", "path": file })).unwrap()["block"].as_u64().unwrap();
    let e = agent_mcp(&d, a, "doc", fill(shown, "s1", "x")).unwrap_err();
    assert!(e.contains("isn't a Doc"), "{e}");
}

#[test]
fn an_agent_blocks_approval_lets_doc_through_without_a_card() {
    let d = Daemon::child();
    // The fake agent asks permission for the tool: `doc`, answered by
    // Arugula itself and noted so...
    let a = d.open("perm mcp__arugula__doc");
    assert_eq!(d.wait(a, "idle"), "done", "never asked");
    let s = d.state(a);
    assert!(s["pending"].as_array().unwrap().is_empty(), "{s}");
    assert!(
        entries(&s).iter().any(|e| e["text"] == "Allowed mcp__arugula__doc (allowed by Arugula)"),
        "{}",
        s["entries"]
    );
    assert!(entries(&s).iter().any(|e| e["text"] == "Allowed."), "the agent was told yes");
    let log: String = std::fs::read_dir(d.state.join(format!("blocks/{a}")))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("seg-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(log.contains(r#""how":"builtin""#), "noted as built in: {log}");
    let panes = d.get("/api/panes");
    assert!(panes.as_array().unwrap().iter().all(|p| p["id"] != a || p["attention"] != "needs_input"), "{panes}");

    // ...while another tool of ours still raises a card.
    let other = d.open("perm mcp__arugula__run");
    d.wait(other, "needs-input");
    let s = d.state(other);
    assert_eq!(s["pending"][0]["tool"], "mcp__arugula__run", "{s}");
    // ...and so does one that only has doc in its name.
    let near = d.open("perm mcp__arugula__docs");
    d.wait(near, "needs-input");
}

#[cfg(feature = "labs")]
#[test]
fn the_flag_hides_the_doc_and_refuses_it_and_a_restart_keeps_it() {
    let args = ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"];
    let mut d = Daemon::child_with(&args);
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let m = Mcp::bridge(&d, Some(pane));

    // Off: unlisted, not offered, and refused with how to turn it on.
    let caps = m.call("capabilities", json!({})).unwrap();
    assert!(caps["blocks"].as_array().unwrap().iter().all(|b| b["kind"] != "doc"), "{caps}");
    let e = m.call("doc", outline(1)).unwrap_err();
    assert!(e.contains("arugulad flags docs on"), "{e}");
    let (status, text) = d.raw("POST", "/api/blocks", Some(json!({ "type": "doc", "config": { "title": "x" } })));
    assert!((400..500).contains(&status) && text.contains("arugulad flags docs on"), "{status} {text}");
    assert!(d.get("/api/panes").as_array().unwrap().iter().all(|p| p["type"] != "doc"));
    drop(m);

    // On: offered, and made.
    arugula_proto::flags::set(&d.state, "docs", true).unwrap();
    let m = Mcp::bridge(&d, Some(pane));
    let caps = m.call("capabilities", json!({})).unwrap();
    let offer = caps["blocks"].as_array().unwrap().iter().find(|b| b["kind"] == "doc").cloned();
    assert_eq!(offer.expect("capabilities offers a doc")["open"], "doc {kind: \"new\"}");
    let doc = m.call("doc", outline(3)).unwrap()["block"].as_u64().unwrap();
    m.call("doc", fill(doc, "s2", "Second.")).unwrap();
    m.call("doc", fill(doc, "s1", "First.")).unwrap();
    drop(m);

    // A restart with the flag off still brings it back, filled: turning a
    // flag off hides and refuses, it doesn't throw away.
    arugula_proto::flags::set(&d.state, "docs", false).unwrap();
    d.stop();
    d.start();
    let state = d.state(doc);
    assert_eq!(state["title"], "Report");
    assert_eq!(bodies(&state), [Some("First.".into()), Some("Second.".into()), None]);
    assert_eq!(d.get(&format!("/api/blocks/{doc}"))["info"]["type"], "doc");
    let m = Mcp::bridge(&d, Some(pane));
    let e = m.call("doc", fill(doc, "s3", "Third.")).unwrap_err();
    assert!(e.contains("arugulad flags docs on"), "refused while the flag is off: {e}");

    // Back on, the same Doc takes the rest, and the next restart has it all.
    arugula_proto::flags::set(&d.state, "docs", true).unwrap();
    m.call("doc", fill(doc, "s3", "Third.")).unwrap();
    drop(m);
    d.stop();
    d.start();
    let state = d.state(doc);
    assert_eq!(bodies(&state), [Some("First.".into()), Some("Second.".into()), Some("Third.".into())]);
    assert!(d.state.join(format!("blocks/{doc}/doc/doc.json")).is_file());
}

#[test]
fn a_closed_doc_is_kept_for_history_and_search() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    m.call("doc", fill(doc, "s1", "The needle is in section one.")).unwrap();
    m.call("close", json!({ "pane": doc })).unwrap();
    d.wait_for("the block to close", || d.raw("GET", &format!("/api/blocks/{doc}"), None).0 == 404);
    let found = d.get("/api/search?re=needle");
    assert!(found.to_string().contains("The needle is in section one."), "{found}");
    drop(m);
}
