//! #963: the Doc block, against the real daemon. An agent builds a document
//! over MCP (`doc new`, then `doc fill`) and every client attached sees it
//! fill in: as patches, and only a page that shows it. An agent block's
//! token drives its own Doc and no one else's; its approvals let `doc`
//! through with no card (the fake ACP agent); the owner writes it and an
//! editor or a viewer can't; the flag hides it and refuses it; a restart
//! brings it back; and an idle Doc costs nothing. Its pictures (#965): put by
//! path or data, read by a viewer, written by the owner alone, and never in
//! the state a client is sent.

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

/// Where the Doc a block shows keeps its files: its folder in the library.
fn library_dir(d: &Daemon, block: u64) -> std::path::PathBuf {
    let rows = d.get("/api/docs");
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["open"] == block)
        .unwrap_or_else(|| panic!("no Doc in %{block}: {rows}"));
    d.state.join("docs").join(row["id"].as_str().unwrap())
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
    let dir = library_dir(&d, doc);
    let before = mtimes(&dir);
    assert!(before.iter().any(|(p, _)| p.ends_with("/s1.md")), "{before:?}");

    // Nothing polls and nothing ticks: five quiet seconds.
    let frames = page.blocks_for(doc, Duration::from_secs(5));
    assert!(frames.is_empty(), "{} block messages while idle: {frames:?}", frames.len());
    assert_eq!(mtimes(&dir), before, "nothing in the Doc's folder was written");
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
    assert!(library_dir(&d, doc).join("doc.json").is_file());
    assert!(!d.state.join(format!("blocks/{doc}/doc")).exists(), "no Doc file in the block's folder");
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

/// A file from `tests/fixtures/docs/`, the same ones the web client's
/// `doc-grammar.spec.ts` reads.
fn example(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docs").join(name)).unwrap()
}

#[test]
fn a_chart_and_a_drawing_fill_and_a_bad_one_leaves_the_section_as_it_was() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(3)).unwrap()["block"].as_u64().unwrap();
    let mut viewer = Page::connect(&d, Some(FRIEND));
    viewer.show(pane);
    viewer.settled(doc);

    // A chart and a drawing, in sections, as patches a second client merges.
    let (chart, drawing) = (example("bar-chart.md"), example("script-drawing.md"));
    m.call("doc", fill(doc, "s1", &chart)).unwrap();
    let patch = viewer.block(doc);
    assert_eq!((patch["entries_at"].as_u64(), patch["entries"][0]["body"].as_str()), (Some(0), Some(chart.as_str())));
    // An SVG with a script in it is stored: it is only ever shown by <img>.
    m.call("doc", fill(doc, "s2", &drawing)).unwrap();
    let patch = viewer.block(doc);
    assert_eq!((patch["entries_at"].as_u64(), patch["entries"][0]["body"].as_str()), (Some(1), Some(drawing.as_str())));
    let file = |s: &str| library_dir(&d, doc).join(format!("{s}.md"));
    assert_eq!(std::fs::read_to_string(file("s2")).unwrap(), drawing);

    // A bad chart over a filled section, and over a pending one: refused with
    // a line saying what to fix, and nothing changes on disk, in the state or
    // on a page.
    let before = (bodies(&d.state(doc)), mtimes(&library_dir(&d, doc)));
    for (section, bad, says) in [
        ("s1", "bad/unordered-line.md", "x must be numbers, dates"),
        ("s3", "bad/bad-json.md", "chart: not JSON"),
        ("s2", "bad/too-many-series.md", "1 to 4 keys"),
        ("s3", "bad/svg-no-root.md", "start with <svg"),
    ] {
        let e = m.call("doc", fill(doc, section, &example(bad))).unwrap_err();
        assert!(e.contains(says) && e.contains("section is unchanged") && !e.contains('\n'), "{bad}: {e}");
    }
    assert_eq!((bodies(&d.state(doc)), mtimes(&library_dir(&d, doc))), before);
    assert!(!file("s3").exists());
    assert!(viewer.blocks_for(doc, Duration::from_millis(700)).is_empty(), "a refusal sends nothing");

    // The text has a line for each.
    let text = d.raw("GET", &format!("/api/panes/{doc}/capture"), None).1;
    assert!(text.contains("[chart: Sales by month (bar, 3 rows)]") && text.contains("[drawing]"), "{text}");
    drop(m);
}

/// A PNG's first bytes and then `extra`: the daemon tells a file by them.
fn png(extra: &str) -> Vec<u8> {
    [b"\x89PNG\r\n\x1a\n".as_slice(), extra.as_bytes()].concat()
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn asset_data(block: u64, name: &str, bytes: &[u8]) -> Value {
    json!({ "kind": "asset", "block": block, "name": name, "data": b64(bytes) })
}

fn asset_path(block: u64, name: &str, path: &Path) -> Value {
    json!({ "kind": "asset", "block": block, "name": name, "path": path })
}

/// The pictures a Doc's state lists, by name.
fn asset_names(state: &Value) -> Vec<String> {
    state["assets"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn an_agent_puts_a_picture_from_a_file_or_from_data_and_fills_with_it() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    let mut viewer = Page::connect(&d, Some(FRIEND));
    viewer.show(pane);
    viewer.settled(doc);

    // From a file the agent saved, by its absolute path.
    let file = d.sessions.join("flow.png");
    std::fs::write(&file, png("from a file")).unwrap();
    let put = m.call("doc", asset_path(doc, "flow.png", &file)).unwrap();
    assert_eq!(put["path"], "assets/flow.png", "{put}");
    assert_eq!(put["mime"], "image/png");
    // The viewer's page is told it exists, by name, and nothing of its bytes.
    let patch = viewer.block(doc);
    assert_eq!(patch["assets"][0]["name"], "flow.png", "{patch}");
    assert!(!patch.to_string().contains(&b64(&png("from a file"))[8..]), "{patch}");
    // ...and from base64.
    m.call("doc", asset_data(doc, "shot.png", &png("from data"))).unwrap();
    assert_eq!(asset_names(&d.state(doc)), ["flow.png", "shot.png"]);
    assert_eq!(std::fs::read(library_dir(&d, doc).join("assets/flow.png")).unwrap(), png("from a file"));

    // Now a section can show them, and its text says what they are.
    m.call("doc", fill(doc, "s1", "The flow:\n\n![The flow, in boxes](assets/flow.png)\n")).unwrap();
    let text = d.raw("GET", &format!("/api/panes/{doc}/capture"), None).1;
    assert!(text.contains("[image: The flow, in boxes]"), "{text}");

    // A name used again is replaced: the same Doc, one picture, a new rev.
    let rev = |d: &Daemon| d.state(doc)["assets"][0]["rev"].as_u64().unwrap();
    let before = rev(&d);
    m.call("doc", asset_data(doc, "flow.png", &png("replaced"))).unwrap();
    assert_eq!(asset_names(&d.state(doc)), ["flow.png", "shot.png"]);
    assert!(rev(&d) > before);

    // A picture the Doc doesn't have is refused, and the section stays.
    let was = bodies(&d.state(doc));
    let e = m.call("doc", fill(doc, "s2", "![Gone](assets/gone.png)")).unwrap_err();
    assert!(e.contains("image assets/gone.png: no such asset") && e.contains("section is unchanged"), "{e}");
    let e = m.call("doc", fill(doc, "s2", "![Far](http://example.com/a.png)")).unwrap_err();
    assert!(e.contains("must be assets/<file>"), "{e}");
    assert_eq!(bodies(&d.state(doc)), was);
    drop(m);
}

#[test]
fn a_picture_that_isnt_one_is_refused_with_a_line_saying_what_to_fix() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(1)).unwrap()["block"].as_u64().unwrap();
    let save = |name: &str, bytes: &[u8]| {
        let p = d.sessions.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    };
    let svg = save("drawing.png", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    let html = save("page.png", b"<html><body>hi</body></html>");
    let real = save("real.png", &png("x"));
    let huge = save("huge.png", &png(&"x".repeat(2 * 1024 * 1024)));
    let refuse = |args: Value, says: &str| {
        let e = m.call("doc", args.clone()).unwrap_err();
        assert!(e.contains(says) && !e.trim().contains('\n'), "{args}: {e}");
    };
    // The bytes decide: an SVG file (named .png or .svg), an HTML file named
    // .png and a PNG named .svg are all refused.
    refuse(asset_path(doc, "drawing.png", &svg), "```svg fence");
    refuse(asset_path(doc, "drawing.svg", &svg), "must end in .png");
    refuse(asset_path(doc, "page.png", &html), "isn't a PNG, JPEG, GIF or WebP");
    refuse(asset_path(doc, "real.svg", &real), "must end in .png");
    // A name is a plain file name that isn't a scratch file.
    for name in ["../real.png", "a/b.png", "x.tmp", ".hidden.png"] {
        refuse(asset_path(doc, name, &real), if name.ends_with("tmp") { ".tmp" } else { "1 to 100" });
    }
    // A path: absolute (the daemon's working directory isn't the agent's), a
    // file that's there, and at most 2 MB (its size is looked at first).
    refuse(asset_path(doc, "a.png", Path::new("real.png")), "give an absolute path, or send data");
    refuse(asset_path(doc, "a.png", &d.sessions.join("missing.png")), "missing.png");
    refuse(asset_path(doc, "a.png", &huge), "is 2097160 bytes; a picture is 2097152 (2 MB) at most");
    refuse(json!({ "kind": "asset", "block": doc, "name": "a.png" }), "give path or data");
    refuse(
        json!({ "kind": "asset", "block": doc, "name": "a.png", "path": real, "data": b64(&png("x")) }),
        "give path or data",
    );
    // Over the cap as data; the cap itself is fine.
    refuse(asset_data(doc, "a.png", &png(&"x".repeat(2 * 1024 * 1024))), "the cap is 2097152");
    m.call("doc", asset_data(doc, "ok.png", &png(&"x".repeat(2 * 1024 * 1024 - 8)))).unwrap();
    // At most 20 pictures.
    for i in 1..20 {
        m.call("doc", asset_data(doc, &format!("p{i}.png"), &png(&i.to_string()))).unwrap();
    }
    refuse(asset_data(doc, "p20.png", &png("x")), "at most 20 assets");
    m.call("doc", asset_data(doc, "p1.png", &png("replaced"))).unwrap();
    assert_eq!(asset_names(&d.state(doc)).len(), 20);
    assert!(!library_dir(&d, doc).join("assets/p20.png").exists());
    // Only a Doc takes one.
    let file = d.sessions.join("notes.txt");
    std::fs::write(&file, "x").unwrap();
    let shown = m.call("show", json!({ "kind": "file", "path": file })).unwrap()["block"].as_u64().unwrap();
    refuse(asset_data(shown, "a.png", &png("x")), "isn't a Doc");
    drop(m);
}

#[test]
fn an_agent_block_puts_a_picture_in_its_own_doc_and_no_one_elses() {
    let (d, _, _) = shared();
    let a = d.open("hello");
    d.wait(a, "idle");
    let doc = agent_mcp(&d, a, "doc", outline(1)).unwrap()["block"].as_u64().unwrap();
    // The agent block's token: asset (by data and by a file), then fill with them.
    agent_mcp(&d, a, "doc", asset_data(doc, "one.png", &png("one"))).unwrap();
    let file = d.sessions.join("two.png");
    std::fs::write(&file, png("two")).unwrap();
    agent_mcp(&d, a, "doc", asset_path(doc, "two.png", &file)).unwrap();
    agent_mcp(&d, a, "doc", fill(doc, "s1", "![One](assets/one.png) and ![Two](assets/two.png)")).unwrap();
    assert_eq!(asset_names(&d.state(doc)), ["one.png", "two.png"]);
    assert_eq!(bodies(&d.state(doc))[0].as_deref(), Some("![One](assets/one.png) and ![Two](assets/two.png)"));

    // Another agent in its tab may not put one in a Doc it didn't start.
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
    let e = agent_mcp(&d, b, "doc", asset_data(doc, "three.png", &png("three"))).unwrap_err();
    assert!(e.contains("wasn't started by this agent"), "{e}");
    assert_eq!(asset_names(&d.state(doc)), ["one.png", "two.png"]);
}

#[test]
fn a_viewer_reads_a_picture_and_neither_a_viewer_nor_an_editor_can_put_one() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(1)).unwrap()["block"].as_u64().unwrap();
    let bytes = png("the picture");
    m.call("doc", asset_data(doc, "pic.png", &bytes)).unwrap();
    m.call("doc", fill(doc, "s1", "![Pic](assets/pic.png)")).unwrap();

    // The read is a call, and a viewer's: they get the bytes, with the type
    // the daemon's sniff found.
    let read = format!("/api/blocks/{doc}/call/doc_asset");
    for who in [FRIEND, EDITOR] {
        let (status, text) = d.raw_as(who, "POST", &read, Some(json!({ "name": "pic.png" })));
        assert_eq!(status, 200, "{who}: {text}");
        let got: Value = serde_json::from_str(&text).unwrap();
        assert_eq!((got["mime"].as_str(), got["data"].as_str()), (Some("image/png"), Some(b64(&bytes).as_str())));
    }
    let (status, text) = d.raw("POST", &read, Some(json!({ "name": "pic.png" })));
    assert_eq!(status, 200, "the owner too: {text}");
    // A name it doesn't have is an error, whoever asks.
    let (status, _) = d.raw_as(FRIEND, "POST", &read, Some(json!({ "name": "../doc.json" })));
    assert_ne!(status, 200);

    // Writing is the owner's: a viewer's and an editor's put and fill are 403,
    // and nothing changes.
    let put = format!("/api/blocks/{doc}/call/doc_asset_put");
    let fill_path = format!("/api/blocks/{doc}/call/doc_fill");
    let before = (d.state(doc), mtimes(&library_dir(&d, doc)));
    for who in [FRIEND, EDITOR] {
        let (status, text) =
            d.raw_as(who, "POST", &put, Some(json!({ "name": "evil.png", "data": b64(&png("evil")) })));
        assert_eq!(status, 403, "{who}: {text}");
        let (status, text) = d.raw_as(who, "POST", &put, Some(json!({ "name": "pic.png", "data": b64(&png("evil")) })));
        assert_eq!(status, 403, "{who} replacing: {text}");
        let body = json!({ "section": "s1", "markdown": "![Pic](assets/pic.png) rewritten" });
        let (status, text) = d.raw_as(who, "POST", &fill_path, Some(body));
        assert_eq!(status, 403, "{who}: {text}");
    }
    assert_eq!((d.state(doc), mtimes(&library_dir(&d, doc))), before);
    let (status, text) = d.raw("POST", &put, Some(json!({ "name": "owner.png", "data": b64(&png("mine")) })));
    assert_eq!(status, 200, "the owner puts one: {text}");
    // Another block's `asset` call is not opened by the Doc's row: a viewer
    // can't call it (the Editor default), here on the Doc, which has no such method.
    let (status, text) = d.raw_as(FRIEND, "POST", &format!("/api/blocks/{doc}/call/asset"), Some(json!({})));
    assert_eq!(status, 403, "{text}");
    drop(m);
}

#[test]
fn no_client_is_sent_a_pictures_bytes_and_a_restart_keeps_it() {
    let (mut d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    let mut owner = Page::connect(&d, None);
    let mut viewer = Page::connect(&d, Some(FRIEND));
    for page in [&mut owner, &mut viewer] {
        page.show(pane);
        page.settled(doc);
    }
    // Up to 2 MB, which every client would have been sent on every fill.
    let marker = "UNIQUE-PICTURE-BYTES ";
    let bytes = png(&marker.repeat(20_000));
    m.call("doc", asset_data(doc, "big.png", &bytes)).unwrap();
    m.call("doc", fill(doc, "s1", "![Big](assets/big.png)")).unwrap();
    m.call("doc", fill(doc, "s2", "Text after it.")).unwrap();
    for page in [&mut owner, &mut viewer] {
        let frames = page.blocks_for(doc, Duration::from_millis(900));
        assert!(frames.len() >= 2, "{} frames", frames.len());
        for frame in &frames {
            let text = frame.to_string();
            assert!(!text.contains("UNIQUE-PICTURE") && text.len() < 2000, "a {} byte frame: {text}", text.len());
        }
        assert_eq!(frames.last().unwrap()["assets"][0]["name"], "big.png");
    }
    // Nor does the block's state over HTTP, its text, or a later join.
    let whole = d.get(&format!("/api/blocks/{doc}"));
    assert!(!whole.to_string().contains("UNIQUE-PICTURE"), "{}", whole.to_string().len());
    let text = d.raw("GET", &format!("/api/panes/{doc}/capture"), None).1;
    assert!(!text.contains("UNIQUE-PICTURE") && text.contains("[image: Big]"), "{text}");
    let mut late = Page::connect(&d, Some(FRIEND));
    late.show(pane);
    let state = late.settled(doc);
    assert!(!state.to_string().contains("UNIQUE-PICTURE") && asset_names(&state) == ["big.png"], "{state}");
    drop(m);

    // A restart: the picture is still there, still read by name, and a
    // scratch file left by a crash is not one.
    std::fs::write(library_dir(&d, doc).join("assets/big.tmp"), "half").unwrap();
    d.stop();
    d.start();
    let state = d.state(doc);
    assert_eq!(asset_names(&state), ["big.png"], "{state}");
    assert_eq!(state["assets"][0]["size"], bytes.len());
    let (status, text) =
        d.raw_as(FRIEND, "POST", &format!("/api/blocks/{doc}/call/doc_asset"), Some(json!({ "name": "big.png" })));
    assert_eq!(status, 200, "{}", &text[..text.len().min(200)]);
    assert!(text.contains(&b64(&bytes)[..40]));
}

/// #1066: a Doc's link is the owner's. An editor's or a viewer's call is
/// refused by the access rows; an agent on the owner's CLI (the header it
/// sends) is refused by the handler, and so is the account list's way in.
#[test]
fn only_the_owner_shares_a_doc_and_never_an_agent() {
    let (d, pane, _) = shared();
    let m = Mcp::bridge(&d, Some(pane));
    let doc = m.call("doc", outline(2)).unwrap()["block"].as_u64().unwrap();
    let bearer = d.bearer();
    let agent = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    let id = "0123456789abcdef0123456789abcdef";
    let asks = "ask with the doc tool's share";

    for method in ["doc_share", "doc_share_update", "doc_unshare", "doc_shares"] {
        let path = format!("/api/blocks/{doc}/call/{method}");
        for who in [FRIEND, EDITOR] {
            let (status, text) = d.raw_as(who, "POST", &path, Some(json!({})));
            assert_eq!(status, 403, "{who} {method}: {text}");
        }
        let (status, _, text) = d.tcp("POST", &path, &agent, Some("{}"));
        assert_eq!(status, 403, "{method}: {text}");
        assert!(text.contains(asks), "{method}: {text}");
    }
    for (method, path) in [("GET", format!("/api/doc-links/{id}")), ("DELETE", format!("/api/doc-links/{id}"))] {
        for who in [FRIEND, EDITOR] {
            let (status, text) = d.raw_as(who, method, &path, None);
            assert_eq!(status, 403, "{who} {method}: {text}");
        }
        let (status, _, text) = d.tcp(method, &path, &agent, None);
        assert_eq!(status, 403, "{method}: {text}");
        assert!(text.contains(asks), "{method}: {text}");
        // The owner is past all of it: this machine just never shared that.
        let (status, text) = d.raw(method, &path, None);
        assert_eq!(status, 404, "{method}: {text}");
    }

    // The owner gets as far as control, which this daemon hasn't joined.
    let path = |m: &str| format!("/api/blocks/{doc}/call/{m}");
    let (status, text) = d.raw("POST", &path("doc_share"), Some(json!({})));
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("hasn't joined control: sharing needs control"), "{text}");
    let (status, text) = d.raw("POST", &path("doc_shares"), Some(json!({})));
    assert_eq!(status, 200, "{text}");
    let said: Value = serde_json::from_str(&text).unwrap();
    assert_eq!((said["shared"].clone(), said["can"].clone()), (Value::Null, json!(false)), "{text}");
    assert!(d.state(doc)["shared"].is_null());

    // Off, it is refused like the rest of Docs (and says how to turn it on).
    // A build without Labs has no flags to turn off.
    #[cfg(feature = "labs")]
    {
        arugula_proto::flags::set(&d.state, "docs", false).unwrap();
        let (status, text) = d.raw("POST", &path("doc_share"), Some(json!({})));
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("Labs"), "{text}");
        let (status, text) = d.raw("GET", &format!("/api/doc-links/{id}"), None);
        assert_eq!(status, 404, "{text}");
        assert!(text.contains("Labs"), "{text}");
    }
    drop(m);
}
