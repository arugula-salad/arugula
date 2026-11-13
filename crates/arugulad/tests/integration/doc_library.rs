//! #1493: the Doc library, against the real daemon. A Doc is kept beside no
//! block: it survives its block closing, a restart and a new block with a new
//! id, with its content, its pictures and its link; opening an open Doc is
//! the same block; an agent lists, opens and fills; deleting one unshares it
//! first, and with control out of reach keeps everything; and the start-up
//! migration moves the old layout's Docs, open, closed and shared, once.

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use std::{path::PathBuf, sync::atomic::Ordering};

use serde_json::{Value, json};

use crate::agentd::*;
use crate::doc_share_ask::World;
use crate::mcp::agent_mcp;

const ONLY_OWNER: &str = "only the owner can delete a Doc";

fn rows(d: &Daemon) -> Vec<Value> {
    d.get("/api/docs").as_array().unwrap().clone()
}

fn row_of(d: &Daemon, block: u64) -> Value {
    rows(d).into_iter().find(|r| r["open"] == block).unwrap_or_else(|| panic!("no Doc in %{block}"))
}

fn id_of(d: &Daemon, block: u64) -> String {
    row_of(d, block)["id"].as_str().unwrap().to_owned()
}

fn lib(d: &Daemon, id: &str) -> PathBuf {
    d.state.join("docs").join(id)
}

fn bodies(state: &Value) -> Vec<Option<String>> {
    state["entries"].as_array().unwrap().iter().map(|e| e["body"].as_str().map(str::to_owned)).collect()
}

fn open_doc(d: &Daemon, id: &str) -> u64 {
    d.post("/api/docs/open", json!({ "id": id }))["pane"].as_u64().unwrap()
}

fn close(d: &Daemon, block: u64) {
    d.post(&format!("/api/panes/{block}/close"), json!({}));
    d.wait_for("the block to close", || d.raw("GET", &format!("/api/blocks/{block}"), None).0 == 404);
}

fn call(d: &Daemon, block: u64, method: &str, args: Value) -> Value {
    d.post(&format!("/api/blocks/{block}/call/{method}"), args)
}

/// A Doc with a picture, shared, then its block closed, the daemon restarted
/// and the Doc opened in a new block: the same content, picture and link,
/// and the link updated and unshared through the new block.
#[test]
fn a_doc_survives_its_block_a_restart_and_a_new_block_with_its_picture_and_link() {
    let mut w = World::new(&[]);
    let (agent, doc) = (w.agent, w.doc);
    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"pretend pixels"].concat();
    use base64::Engine;
    agent_mcp(
        &w.d,
        agent,
        "doc",
        json!({ "kind": "asset", "block": doc, "name": "dot.png", "data": base64::engine::general_purpose::STANDARD.encode(&png) }),
    )
    .unwrap();
    agent_mcp(
        &w.d,
        agent,
        "doc",
        json!({ "kind": "fill", "block": doc, "section": "s2", "markdown": "![dot](assets/dot.png)" }),
    )
    .unwrap();
    let id = id_of(&w.d, doc);
    // Another shared Doc, with a link of its own: the right one is found.
    let sections = json!([{ "heading": "One", "intent": "first" }]);
    let other =
        agent_mcp(&w.d, agent, "doc", json!({ "kind": "new", "title": "Other", "sections": sections })).unwrap();
    let other = other["block"].as_u64().unwrap();
    let other_url = call(&w.d, other, "doc_share", json!({}))["url"].as_str().unwrap().to_owned();
    let shared = call(&w.d, doc, "doc_share", json!({}));
    let url = shared["url"].as_str().unwrap().to_owned();
    let before = w.d.state(doc);
    assert!(before["shared"].is_object());

    // Closing the block leaves the Doc, whole, and the block's folder has
    // none of its files (retiring it can't take the Doc along).
    close(&w.d, doc);
    for dir in std::iter::once(w.d.state.join(format!("blocks/{doc}"))).chain(
        std::fs::read_dir(w.d.state.join("closed"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("{doc}-"))),
    ) {
        assert!(!dir.join("doc").exists() && !dir.join("doc.json").exists(), "{}", dir.display());
    }
    let listed = rows(&w.d);
    let row = listed.iter().find(|r| r["id"] == id.as_str()).expect("a closed Doc is listed");
    assert_eq!(
        (row["title"].as_str(), row["open"].is_null(), row["shared"].is_object()),
        (Some("Q3 plan"), true, true)
    );
    assert!(row["by"].as_str().unwrap().starts_with("mcp:"), "{row}");
    // A Doc that never said reads `doc`; one whose `meta.json` says `tabs`
    // lists as that, and keeps it through the restart (Arugula #827).
    assert_eq!(row["layout"], "doc", "{row}");
    let meta_path = lib(&w.d, &id).join("meta.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    meta["layout"] = json!("tabs");
    std::fs::write(&meta_path, meta.to_string()).unwrap();
    assert_eq!(rows(&w.d).iter().find(|r| r["id"] == id.as_str()).unwrap()["layout"], "tabs");

    // ...and a restart.
    w.d.stop();
    w.d.start();
    let again = open_doc(&w.d, &id);
    assert_ne!(again, doc, "a new block with a new id");
    let after = w.d.state(again);
    assert_eq!(bodies(&after), bodies(&before));
    assert_eq!(after["assets"][0]["name"], "dot.png");
    assert_eq!(after["title"], before["title"]);
    assert!(after["shared"].is_object(), "its share menu is back: {after}");
    assert_eq!(after["layout"], "tabs", "{after}");
    let read = call(&w.d, again, "doc_asset", json!({ "name": "dot.png" }));
    assert!(read["data"].as_str().unwrap().len() > 8);

    // The link follows: the URL again, an update under the same link, and
    // an unshare that takes it down at control and forgets the key.
    assert_ne!(url, other_url);
    assert_eq!(call(&w.d, again, "doc_shares", json!({}))["url"], url.as_str());
    let commits = w.fake.commits.load(Ordering::SeqCst);
    assert_eq!(call(&w.d, again, "doc_share_update", json!({}))["url"], url.as_str());
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), commits + 1);
    call(&w.d, again, "doc_unshare", json!({}));
    assert_eq!(w.fake.deletes.load(Ordering::SeqCst), 1);
    assert!(w.d.state(again)["shared"].is_null());
    assert!(
        !std::fs::read_to_string(w.d.state.join("doc-links.json"))
            .unwrap()
            .contains(&url[url.find("#k=").unwrap() + 3..])
    );
    w.rt = None;
}

/// #608: a closed Doc is read whole, a section past the first, its picture
/// and its prototype too, and the read leaves it closed.
#[test]
fn a_read_gives_the_whole_closed_doc_and_its_picture_and_leaves_it_closed() {
    let w = World::new(&[]);
    let (d, agent, doc) = (&w.d, w.agent, w.doc);
    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"pretend pixels"].concat();
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(&png);
    agent_mcp(d, agent, "doc", json!({ "kind": "asset", "block": doc, "name": "dot.png", "data": data })).unwrap();
    let second = "Past the first.\n\n![dot](assets/dot.png)\n\n```prototype Tune it\n<div>hi</div>\n```\n";
    agent_mcp(d, agent, "doc", json!({ "kind": "fill", "block": doc, "section": "s2", "markdown": second })).unwrap();
    let id = id_of(d, doc);
    close(d, doc);

    let read = d.get(&format!("/api/docs/{id}/content"));
    assert_eq!(read["title"], "Q3 plan");
    assert_eq!(read["doc"], id.as_str());
    assert_eq!(bodies(&read), [Some("Secret words.".to_owned()), Some(second.to_owned())]);
    assert_eq!(read["assets"][0]["name"], "dot.png");
    let asset = d.get(&format!("/api/docs/{id}/assets/dot.png"));
    assert_eq!(asset["mime"], "image/png");
    assert_eq!(asset["data"], data.as_str());
    assert_eq!(d.raw("GET", &format!("/api/docs/{id}/assets/nope.png"), None).0, 404);

    // Neither opened it: no block shows it, and the list still has it closed.
    assert!(rows(d).iter().find(|r| r["id"] == id.as_str()).unwrap()["open"].is_null());
    assert_eq!(d.get("/api/panes").as_array().unwrap().iter().filter(|p| p["type"] == "doc").count(), 0);
}

/// `doc.open` on an open Doc is the same pane, however many ask and however
/// close together; a closed Doc opens one block for them all.
#[test]
fn opening_an_open_doc_is_the_same_pane_and_two_opens_at_once_make_one() {
    let w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    assert_eq!(open_doc(d, &id), doc);
    assert_eq!(open_doc(d, &id), doc);
    // Released together, so they all look before the first has opened; a
    // few rounds, as the window is a mux round trip wide.
    let mut block = doc;
    for round in 0..6 {
        close(d, block);
        let gate = std::sync::Barrier::new(8);
        let panes: Vec<u64> = std::thread::scope(|s| {
            let all: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        gate.wait();
                        open_doc(d, &id)
                    })
                })
                .collect();
            all.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(panes.iter().all(|p| *p == panes[0]), "round {round}: {panes:?}");
        let docs = d.get("/api/panes").as_array().unwrap().iter().filter(|p| p["type"] == "doc").count();
        assert_eq!(docs, 1, "round {round}: one block for the Doc");
        block = panes[0];
    }
    assert_eq!(row_of(d, block)["id"], id.as_str());

    // Not an id, and an id nobody has.
    let (status, text) = d.raw("POST", "/api/docs/open", Some(json!({ "id": "../../etc" })));
    assert_eq!(status, 400, "{text}");
    let (status, _) = d.raw("POST", "/api/docs/open", Some(json!({ "id": "0123456789abcdef" })));
    assert_eq!(status, 404);
    // A Doc made twice is two Docs.
    let sections = json!([{ "heading": "One", "intent": "first" }]);
    for _ in 0..2 {
        agent_mcp(d, w.agent, "doc", json!({ "kind": "new", "title": "Same", "sections": sections })).unwrap();
    }
    assert_eq!(rows(d).iter().filter(|r| r["title"] == "Same").count(), 2);
}

/// `doc.open` takes a link's id (control's Shared Docs lists those) for the
/// Doc id: the shared Doc's block, closed or open; exactly one of the two;
/// and a link that names no Doc or no link at all is a 404.
#[test]
fn opening_by_link_finds_the_shared_doc_and_wants_exactly_one_of_id_and_link() {
    let w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    let url = call(d, doc, "doc_share", json!({}))["url"].as_str().unwrap().to_owned();
    let link = url[..url.find("#k=").unwrap()].rsplit('/').next().unwrap().to_owned();
    let by_link = |body: Value| d.raw("POST", "/api/docs/open", Some(body));

    // Open: the block it is in. Closed: a new block, with its menu back.
    let (status, text) = by_link(json!({ "link": link }));
    assert_eq!(status, 200, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["pane"], doc);
    close(d, doc);
    let (status, text) = by_link(json!({ "link": link }));
    assert_eq!(status, 200, "{text}");
    let block = serde_json::from_str::<Value>(&text).unwrap()["pane"].as_u64().unwrap();
    assert_ne!(block, doc);
    assert_eq!(row_of(d, block)["id"], id.as_str());
    assert!(d.state(block)["shared"].is_object());

    // Both, or neither, is the caller's mistake; no such link is not found.
    for body in [json!({ "id": id, "link": link }), json!({})] {
        let (status, text) = by_link(body);
        assert_eq!(status, 400, "{text}");
        assert!(text.contains("exactly one of id and link"), "{text}");
    }
    assert_eq!(by_link(json!({ "link": "0".repeat(32) })).0, 404);

    // An entry from before the library, which no migration mapped: its Doc
    // isn't in the library.
    let path = d.state.join("doc-links.json");
    let mut links: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    links[&link].as_object_mut().unwrap().remove("doc");
    std::fs::write(&path, serde_json::to_vec(&links).unwrap()).unwrap();
    let (status, text) = by_link(json!({ "link": link }));
    assert_eq!(status, 404, "{text}");
    assert!(text.contains("this link's Doc isn't in the library"), "{text}");
}

/// An agent lists, opens and fills; it can't delete (no such kind, and the
/// owner's CLI with the agent header is refused).
#[test]
fn an_agent_lists_opens_and_fills_but_cant_delete() {
    let w = World::new(&[]);
    let (d, agent, doc) = (&w.d, w.agent, w.doc);
    let id = id_of(d, doc);
    close(d, doc);
    let listed = agent_mcp(d, agent, "doc", json!({ "kind": "list" })).unwrap();
    let docs = listed["docs"].as_array().unwrap();
    assert!(docs.iter().any(|r| r["id"] == id.as_str() && r["title"] == "Q3 plan" && r["open"].is_null()), "{listed}");

    let opened = agent_mcp(d, agent, "doc", json!({ "kind": "open", "doc": id })).unwrap();
    let block = opened["block"].as_u64().unwrap();
    assert_eq!(opened["sections"], json!(["s1", "s2"]), "{opened}");
    agent_mcp(
        d,
        agent,
        "doc",
        json!({ "kind": "fill", "block": block, "section": "s2", "markdown": "Written after." }),
    )
    .unwrap();
    let again = agent_mcp(d, agent, "doc", json!({ "kind": "open", "doc": id })).unwrap();
    assert_eq!(again["block"], block, "the same block");
    assert_eq!(bodies(&d.state(block))[1].as_deref(), Some("Written after."));
    assert!(lib(d, &id).join("s2.md").is_file());
    let e = agent_mcp(d, agent, "doc", json!({ "kind": "open", "doc": "nope" })).unwrap_err();
    assert!(e.contains("isn't a Doc id"), "{e}");
    let e = agent_mcp(d, agent, "doc", json!({ "kind": "delete", "doc": id })).unwrap_err();
    assert!(e.contains("delete") || e.contains("kind"), "{e}");

    // The owner's CLI, as an agent runs it: refused, and nothing goes.
    let bearer = d.bearer();
    let agent_h = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    let (status, _, text) = d.tcp("DELETE", &format!("/api/docs/{id}"), &agent_h, None);
    assert_eq!(status, 403, "{text}");
    assert!(text.contains(ONLY_OWNER), "{text}");
    assert!(lib(d, &id).join("meta.json").is_file());
    assert_eq!(open_doc(d, &id), block);
}

/// #979: an agent renames a Doc that is open (one it didn't open), and one
/// no block shows; the block, the list and the folder all say the new title,
/// and a bad one changes nothing.
#[test]
fn an_agent_renames_an_open_doc_and_a_closed_one() {
    let w = World::new(&[]);
    let (d, agent, doc) = (&w.d, w.agent, w.doc);
    let id = id_of(d, doc);
    let said = agent_mcp(d, agent, "doc", json!({ "kind": "rename", "doc": format!("%{doc}"), "title": " Q4 plan " }))
        .unwrap();
    assert_eq!(said["title"], "Q4 plan", "{said}");
    assert_eq!(d.state(doc)["title"], "Q4 plan", "the open block shows it");
    assert_eq!(row_of(d, doc)["title"], "Q4 plan", "the list does");
    assert_eq!(bodies(&d.state(doc))[1], None, "its sections stay");
    for bad in ["", "a\nb", &"x".repeat(201)] {
        let e = agent_mcp(d, agent, "doc", json!({ "kind": "rename", "doc": id, "title": bad })).unwrap_err();
        assert!(e.contains("title"), "{bad:?}: {e}");
    }
    assert_eq!(d.state(doc)["title"], "Q4 plan");
    // Closed: renamed where it's kept, and it comes back with the title.
    close(d, doc);
    agent_mcp(d, agent, "doc", json!({ "kind": "rename", "doc": id, "title": "Q5 plan" })).unwrap();
    let listed = agent_mcp(d, agent, "doc", json!({ "kind": "list" })).unwrap();
    assert!(
        listed["docs"].as_array().unwrap().iter().any(|r| r["id"] == id.as_str() && r["title"] == "Q5 plan"),
        "{listed}"
    );
    let block = open_doc(d, &id);
    assert_eq!(d.state(block)["title"], "Q5 plan");
    let e =
        agent_mcp(d, agent, "doc", json!({ "kind": "rename", "doc": "0123456789abcdef", "title": "X" })).unwrap_err();
    assert!(e.contains("no Doc"), "{e}");
}

/// The owner deletes a shared Doc: unshared at control first, its block
/// closed, its folder and its link entry gone.
#[test]
fn deleting_a_shared_doc_unshares_it_first_and_removes_it() {
    let w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    let url = call(d, doc, "doc_share", json!({}))["url"].as_str().unwrap().to_owned();
    assert!(url.contains("#k="));
    assert!(std::fs::read_to_string(d.state.join("doc-links.json")).unwrap().contains(&id));

    let (status, text) = d.raw("DELETE", &format!("/api/docs/{id}"), None);
    assert_eq!(status, 200, "{text}");
    assert_eq!(w.fake.deletes.load(Ordering::SeqCst), 1, "unshared at control");
    assert!(!lib(d, &id).exists());
    assert!(rows(d).is_empty());
    assert!(!std::fs::read_to_string(d.state.join("doc-links.json")).unwrap().contains(&id));
    assert_eq!(d.raw("GET", &format!("/api/blocks/{doc}"), None).0, 404, "its block closed");
    let (status, _) = d.raw("DELETE", &format!("/api/docs/{id}"), None);
    assert_eq!(status, 404);
}

/// With control out of reach a shared Doc is not deleted: 503 with the
/// reason, and the block, the files and the key all stay.
#[test]
fn deleting_a_shared_doc_with_control_down_refuses_and_keeps_everything() {
    let mut w = World::new(&[]);
    let (doc, id) = (w.doc, id_of(&w.d, w.doc));
    call(&w.d, doc, "doc_share", json!({}));
    let links = std::fs::read(w.d.state.join("doc-links.json")).unwrap();
    let files = std::fs::read(lib(&w.d, &id).join("s1.md")).unwrap();
    w.rt = None;

    let (status, text) = w.d.raw("DELETE", &format!("/api/docs/{id}"), None);
    assert_eq!(status, 503, "{text}");
    assert!(text.contains("can't unshare it at control, so the Doc is kept"), "{text}");
    assert_eq!(std::fs::read(lib(&w.d, &id).join("s1.md")).unwrap(), files);
    assert_eq!(std::fs::read(w.d.state.join("doc-links.json")).unwrap(), links, "the key is kept");
    assert!(row_of(&w.d, doc)["shared"].is_object(), "the block is open and still shared");
    // A Doc that was never shared is gone without control.
    let sections = json!([{ "heading": "One", "intent": "first" }]);
    let other =
        agent_mcp(&w.d, w.agent, "doc", json!({ "kind": "new", "title": "Local", "sections": sections })).unwrap();
    let other_id = id_of(&w.d, other["block"].as_u64().unwrap());
    assert_eq!(w.d.raw("DELETE", &format!("/api/docs/{other_id}"), None).0, 200);
    assert!(!lib(&w.d, &other_id).exists());
}

/// The old layout's Docs move at start: an open one, a closed one and a
/// shared open one, each with its content, the open blocks' configs and the
/// links following; a second start changes nothing.
#[test]
fn the_start_up_migration_moves_open_closed_and_shared_docs_once() {
    let mut d = Daemon::child_with(&["--owner", "me@example.com", "--tailscale-socket", "/nonexistent/sock"]);
    let pane = d.get("/api/panes")[0]["id"].as_u64().unwrap();
    let make = |d: &Daemon, title: &str| -> (u64, String) {
        let block = d
            .post("/api/blocks", json!({ "type": "doc", "config": { "title": title }, "split": pane, "local": true }))
            ["block"]
            .as_u64()
            .unwrap();
        let sections = json!([{ "heading": "One", "intent": "first" }]);
        call(d, block, "doc_outline", json!({ "title": title, "sections": sections }));
        call(d, block, "doc_fill", json!({ "section": "s1", "markdown": format!("Words of {title}.") }));
        (block, id_of(d, block))
    };
    let ((shared, shared_id), (plain, plain_id), (closed, closed_id)) =
        (make(&d, "Shared open"), make(&d, "Plain open"), make(&d, "Closed"));
    close(&d, closed);
    d.stop();

    // Put the state back as the last release left it: each Doc's files in its
    // block's folder, the open blocks' configs without a Doc, and the links
    // naming blocks.
    let state = d.state.clone();
    let to_block = |id: &str, dir: PathBuf| {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::rename(state.join("docs").join(id), dir.join("doc")).unwrap();
    };
    to_block(&shared_id, state.join(format!("blocks/{shared}")));
    to_block(&plain_id, state.join(format!("blocks/{plain}")));
    let gone = std::fs::read_dir(state.join("closed"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with(&format!("{closed}-")))
        .unwrap()
        .path();
    to_block(&closed_id, gone);
    std::fs::remove_dir_all(state.join("docs")).unwrap();
    let mut layout: Value = serde_json::from_slice(&std::fs::read(state.join("layout.json")).unwrap()).unwrap();
    for b in [shared, plain] {
        layout["panes"][b.to_string()]["config"].as_object_mut().unwrap().remove("doc").expect("a block names its Doc");
    }
    std::fs::write(state.join("layout.json"), layout.to_string()).unwrap();
    let link = |block: u64| json!({ "key": "a2V5", "ver": 1, "block": block, "title": "t", "made": 5, "updated": 6, "size": 7, "viewer": "https://docs.test" });
    let links =
        json!({ "aaaaaaaabbbbbbbbccccccccdddddddd": link(shared), "11111111222222223333333344444444": link(closed) });
    std::fs::write(state.join("doc-links.json"), links.to_string()).unwrap();

    d.start();
    let listed = rows(&d);
    assert_eq!(listed.len(), 3, "{listed:?}");
    let by = |t: &str| listed.iter().find(|r| r["title"] == t).unwrap_or_else(|| panic!("no {t}")).clone();
    assert!(by("Shared open")["shared"].is_object() && by("Closed")["shared"].is_object());
    assert!(by("Plain open")["shared"].is_null());
    assert_eq!(by("Shared open")["open"], shared);
    assert_eq!(by("Plain open")["open"], plain);
    assert!(by("Closed")["open"].is_null());
    assert_eq!(by("Plain open")["by"], "migrated");
    assert_eq!(bodies(&d.state(shared)), [Some("Words of Shared open.".to_owned())]);
    assert!(d.state(shared)["shared"].is_object(), "its menu is where it was");
    // The closed one opens with its content and its link.
    let reopened = open_doc(&d, by("Closed")["id"].as_str().unwrap());
    assert_eq!(bodies(&d.state(reopened)), [Some("Words of Closed.".to_owned())]);
    assert!(d.state(reopened)["shared"].is_object());
    for dir in [format!("blocks/{shared}"), format!("blocks/{plain}")] {
        assert!(!d.state.join(dir).join("doc").exists(), "moved");
    }

    // A second start changes nothing.
    let tree = |d: &Daemon| {
        let mut out = vec![];
        let mut todo = vec![d.state.join("docs")];
        while let Some(dir) = todo.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                if e.path().is_dir() {
                    todo.push(e.path());
                } else {
                    out.push((e.path(), std::fs::read(e.path()).unwrap()));
                }
            }
        }
        out.sort();
        out
    };
    let (before, ids) =
        (tree(&d), rows(&d).into_iter().map(|r| r["id"].to_string()).collect::<std::collections::BTreeSet<_>>());
    d.stop();
    d.start();
    assert_eq!(tree(&d), before);
    assert_eq!(rows(&d).into_iter().map(|r| r["id"].to_string()).collect::<std::collections::BTreeSet<_>>(), ids);
}

const FRIEND: &str = "friend@example.com";
const EDITOR: &str = "pal@example.com";

fn links_held(d: &Daemon) -> usize {
    let held: Value = serde_json::from_slice(&std::fs::read(d.state.join("doc-links.json")).unwrap()).unwrap();
    held.as_object().unwrap().len()
}

/// Arugula #819: the owner shares a Doc by its id with no block showing it:
/// no pane opens, the row says shared, and a block opened after shows it
/// shared with the one link, which a second share refuses.
#[test]
fn sharing_a_closed_doc_opens_no_pane_and_a_block_opened_later_shows_it_shared() {
    let mut w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    close(d, doc);
    let panes = d.get("/api/panes").as_array().unwrap().len();

    let url = d.post(&format!("/api/docs/{id}/share"), json!({}))["url"].as_str().unwrap().to_owned();
    assert!(url.contains("#k="), "{url}");
    assert_eq!(d.get("/api/panes").as_array().unwrap().len(), panes, "no pane opened");
    let listed = rows(d);
    let row = listed.iter().find(|r| r["id"] == id.as_str()).unwrap();
    assert!(row["open"].is_null() && row["shared"].is_object(), "{row}");
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 1);
    assert_eq!(links_held(d), 1);

    let (status, text) = d.raw("POST", &format!("/api/docs/{id}/share"), Some(json!({})));
    assert_ne!(status, 200, "{text}");
    assert!(text.contains("already shared"), "{text}");

    let block = open_doc(d, &id);
    assert!(d.state(block)["shared"].is_object(), "the block opens shared");
    assert_eq!(call(d, block, "doc_shares", json!({}))["url"], url.as_str(), "the same link");
    let (status, text) = d.raw("POST", &format!("/api/blocks/{block}/call/doc_share"), Some(json!({})));
    assert_ne!(status, 200, "{text}");
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 1, "still one link made");
    assert_eq!(links_held(d), 1);
    w.rt = None;
}

/// ...and the other order: a Doc a block shows is shared through the op, and
/// the block says so at once, with no second link.
#[test]
fn sharing_an_open_doc_by_its_id_shows_shared_in_its_block_at_once() {
    let mut w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    assert!(d.state(doc)["shared"].is_null());

    let url = d.post(&format!("/api/docs/{id}/share"), json!({}))["url"].as_str().unwrap().to_owned();
    assert!(d.state(doc)["shared"].is_object(), "the open block says shared, with no call to it");
    assert_eq!(call(d, doc, "doc_shares", json!({}))["url"], url.as_str());
    assert_eq!(rows(d).iter().filter(|r| r["id"] == id.as_str()).count(), 1);
    assert_eq!(row_of(d, doc)["open"], doc, "still the one block");

    let (status, text) = d.raw("POST", &format!("/api/docs/{id}/share"), Some(json!({})));
    assert_ne!(status, 200, "{text}");
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 1);
    assert_eq!(links_held(d), 1);
    w.rt = None;
}

/// Update works the same way, closed or open: the link stays and shows the
/// Doc as it is.
#[test]
fn updating_a_doc_by_its_id_keeps_its_link_closed_or_open() {
    let mut w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    let path = format!("/api/docs/{id}/share");
    // Nothing to update before there's a link.
    let (status, text) = d.raw("POST", &path, Some(json!({ "update": true })));
    assert_ne!(status, 200, "{text}");
    assert!(text.contains("isn't shared"), "{text}");

    let url = d.post(&path, json!({}))["url"].as_str().unwrap().to_owned();
    call(d, doc, "doc_fill", json!({ "section": "s2", "markdown": "Written after." }));
    assert_eq!(d.state(doc)["shared_older"], true);
    // Open: its block hears of the update.
    assert_eq!(d.post(&path, json!({ "update": true }))["url"], url.as_str());
    assert_eq!(d.state(doc)["shared_older"], false);
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 2);

    // Closed: the link's entry is what moves.
    close(d, doc);
    let before = rows(d)[0]["shared"]["updated"].as_u64().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(d.post(&path, json!({ "update": true }))["url"], url.as_str());
    assert!(rows(d)[0]["shared"]["updated"].as_u64().unwrap() > before);
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 3);
    assert_eq!(links_held(d), 1);
    w.rt = None;
}

/// Only the owner's tap shares a Doc by its id: an agent is told to ask, a
/// viewer or editor is refused, and nothing is made.
#[test]
fn an_agent_or_a_guest_cant_share_a_doc_by_its_id() {
    let w = World::new(&[]);
    let (d, doc) = (&w.d, w.doc);
    let id = id_of(d, doc);
    close(d, doc);
    let path = format!("/api/docs/{id}/share");
    for who in [FRIEND, EDITOR] {
        let (status, text) = d.raw_as(who, "POST", &path, Some(json!({})));
        assert_eq!(status, 403, "{who}: {text}");
    }
    let bearer = d.bearer();
    let agent = [("authorization", bearer.as_str()), ("x-arugula-agent", "1"), ("content-type", "application/json")];
    let (status, _, text) = d.tcp("POST", &path, &agent, Some("{}"));
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("ask with the doc tool's share"), "{text}");
    assert_eq!(w.fake.commits.load(Ordering::SeqCst), 0, "nothing was made");
    assert!(!d.state.join("doc-links.json").exists());
}
