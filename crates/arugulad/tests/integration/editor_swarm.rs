//! M28: editors in the swarm. An editor (a stand-in for Arugula's VS Code
//! extension or nvim plugin, speaking its protocol on the daemon's socket)
//! joins as an entry of its own: kind editor, its project, its file, its
//! diagnostics and debugger, but no tab. Following it streams its cursor and
//! file to the follower alone; a debugger stopping is a card that Continue
//! answers; errors after a save and a merge conflict are cards too; leaving
//! removes it at once. `ide.rs` covers Claude Code's diffs; Playwright runs
//! the real extension in code-server (`web/e2e/editor-swarm.spec.ts`).

// Over the daemon's Unix socket; Windows gets its named pipe in M56 (#219).
#![cfg(unix)]

use crate::agentd;

use std::time::{Duration, Instant};

use agentd::*;
use arugula_proto::{ClientMsg, PaneInfo, ReasonKind, ServerMsg, State};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// An editor connected to the daemon, as the extension connects.
struct Editor {
    r: BufReader<tokio::net::unix::OwnedReadHalf>,
    w: tokio::net::unix::OwnedWriteHalf,
    id: u32,
}

impl Editor {
    async fn join(d: &Daemon, hello: Value) -> Self {
        Self::join_at(&d.sock(), hello).await
    }

    async fn join_at(sock: &std::path::Path, hello: Value) -> Self {
        Self::join_as(sock, "arugula-editor", hello).await
    }

    /// Joining with the upgrade asking for `protocol`.
    async fn join_as(sock: &std::path::Path, protocol: &str, hello: Value) -> Self {
        let mut s = UnixStream::connect(sock).await.unwrap();
        s.write_all(
            format!(
                "GET /api/editors/connect HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: {protocol}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        // The 101 and its headers, a byte at a time (nothing after them).
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0u8; 1];
            assert_eq!(s.read(&mut b).await.unwrap(), 1, "hung up: {}", String::from_utf8_lossy(&head));
            head.push(b[0]);
        }
        let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
        assert!(head.starts_with("http/1.1 101"), "{head}");
        let first = protocol.split(',').next().unwrap().trim();
        assert!(head.contains(&format!("upgrade: {first}\r\n")), "answered in the name asked for: {head}");
        let (r, w) = s.into_split();
        let mut e = Self { r: BufReader::new(r), w, id: 0 };
        let mut hello = hello;
        hello["t"] = "hello".into();
        e.send(hello).await;
        e.id = e.expect("welcome").await["id"].as_u64().unwrap() as u32;
        e
    }

    async fn send(&mut self, v: Value) {
        self.w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }

    /// The next message of type `t` from the daemon.
    async fn expect(&mut self, t: &str) -> Value {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let mut line = String::new();
            match tokio::time::timeout(left, self.r.read_line(&mut line)).await {
                Ok(Ok(n)) if n > 0 => {
                    let v: Value = serde_json::from_str(&line).unwrap();
                    if v["t"] == t {
                        return v;
                    }
                }
                _ => panic!("no {t} from the daemon"),
            }
        }
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A web client: its view of every pane, and what it's sent
/// about editors it follows.
struct Client {
    ws: Ws,
    state: Option<State>,
    follows: Vec<(u32, Value)>,
}

impl Client {
    async fn connect(d: &Daemon) -> Self {
        let (ws, _) = connect_async(d.ws("/ws")).await.unwrap();
        Self { ws, state: None, follows: vec![] }
    }

    async fn send(&mut self, m: ClientMsg) {
        self.ws.send(Message::Text(serde_json::to_string(&m).unwrap().into())).await.unwrap();
    }

    async fn pump(&mut self, for_: Duration) {
        let until = Instant::now() + for_;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(left, self.ws.next()).await else { return };
            match serde_json::from_str::<ServerMsg>(&t).unwrap() {
                ServerMsg::Hello { state, .. } | ServerMsg::State { state } => self.state = Some(state),
                ServerMsg::Delta { delta } => {
                    if let Some(s) = &mut self.state {
                        s.apply(&delta);
                    }
                }
                ServerMsg::Follow { pane, msg } => self.follows.push((pane, msg)),
                _ => {}
            }
        }
    }

    fn pane(&self, id: u32) -> Option<PaneInfo> {
        self.state.as_ref()?.panes.iter().find(|p| p.id == id).cloned()
    }

    /// Wait until `f` holds for pane `id` (`None`: it's gone).
    async fn until(&mut self, what: &str, id: u32, f: impl Fn(Option<&PaneInfo>) -> bool) -> Option<PaneInfo> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if f(self.pane(id).as_ref()) {
                return self.pane(id);
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {:?}", self.pane(id));
            self.pump(Duration::from_millis(100)).await;
        }
    }

    async fn followed(&mut self, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some((_, v)) = self.follows.iter().find(|(_, v)| f(v)) {
                return v.clone();
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {:?}", self.follows);
            self.pump(Duration::from_millis(100)).await;
        }
    }
}

fn repo(d: &Daemon) -> String {
    let root = d.sessions.join("proj");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::canonicalize(root).unwrap().display().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_editor_joins_reports_and_leaves_at_once() {
    let d = Daemon::child();
    let root = repo(&d);
    let mut web = Client::connect(&d).await;
    let mut ed = Editor::join(
        &d,
        json!({ "editor": "vscode", "remote": "ssh-remote", "authority": "ssh-remote+geek", "hostname": "geek", "workspace": root }),
    )
    .await;
    let id = ed.id;
    // An entry of its own: an editor, its project, no tab.
    let p = web.until("the editor to show", id, |p| p.is_some()).await.unwrap();
    assert_eq!(p.kind, arugula_proto::BlockType::Editor);
    assert_eq!(p.work, Some(arugula_proto::WorkKind::Editor));
    assert_eq!(p.project.as_ref().map(|p| p.name.as_str()), Some("proj"));
    assert_eq!(p.cwd.as_deref(), Some(root.as_str()));
    let info = p.editor.clone().unwrap();
    assert_eq!(
        (info.app.as_str(), info.remote.as_deref(), info.authority.as_deref()),
        ("vscode", Some("ssh-remote"), Some("ssh-remote+geek"))
    );
    let st = web.state.as_ref().unwrap();
    assert!(st.tabs.iter().all(|t| !t.root.panes().contains(&id)), "it's in no tab");

    // What it reports: the file (relative to the workspace), its counts.
    ed.send(json!({ "t": "summary", "file": format!("{root}/src/main.rs"), "diag": { "e": 0, "w": 2, "i": 1 }, "dirty": 1 }))
        .await;
    let p =
        web.until("its summary", id, |p| p.is_some_and(|p| p.file.as_deref() == Some("src/main.rs"))).await.unwrap();
    let info = p.editor.unwrap();
    assert_eq!((info.diag.w, info.dirty), (2, 1));
    assert_eq!(p.title.as_deref(), Some("main.rs — proj (VS Code)"));
    // The lines around its cursor: what a preview and `capture` show.
    ed.send(json!({ "t": "peek", "file": format!("{root}/src/main.rs"), "line": 3, "col": 5, "top": 1, "lines": ["fn main() {", "    let x = 1;", "    go(x);"], "dirty": 1 }))
        .await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, text) = d.raw("GET", &format!("/api/panes/{id}/capture?format=text"), None);
        if text.starts_with("src/main.rs:3\nfn main() {") {
            break;
        }
        assert!(Instant::now() < deadline, "capture: {text}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // ...and `arugula editors` lists it.
    let list = d.get("/api/editors");
    assert_eq!(list[0]["pane"], id);
    assert_eq!(list[0]["editor"]["app"], "vscode");

    // Turning the workspace off (the connection closes): gone at once.
    drop(ed);
    web.until("it to go", id, |p| p.is_none()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn following_streams_to_the_follower_alone() {
    let d = Daemon::child();
    let root = repo(&d);
    let mut ed = Editor::join(&d, json!({ "editor": "nvim", "workspace": root })).await;
    let id = ed.id;
    let mut alice = Client::connect(&d).await;
    let mut bob = Client::connect(&d).await;
    alice.until("the editor", id, |p| p.is_some()).await;
    // Nobody follows: it doesn't stream. The first follower starts it.
    alice.send(ClientMsg::Follow { pane: id, on: true }).await;
    assert_eq!(ed.expect("followers").await["n"], 1);
    alice
        .until("followers in the summary", id, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.followers == 1))
        .await;
    let file = format!("{root}/src/lib.rs");
    ed.send(json!({ "t": "open", "file": file, "version": 1, "text": "pub fn a() {}\n" })).await;
    ed.send(json!({ "t": "follow", "file": file, "line": 1, "col": 8, "sel": [1, 4, 1, 9], "view": [1, 40] })).await;
    ed.send(json!({ "t": "edit", "file": file, "version": 2, "changes": [{ "range": [1, 13, 1, 13], "text": "x" }] }))
        .await;
    ed.send(json!({ "t": "diagnostics", "file": file, "items": [{ "range": [1, 7, 1, 8], "severity": "error", "message": "nope" }] }))
        .await;
    let open = alice.followed("the file", |v| v.get("open").is_some()).await;
    assert_eq!(open["open"]["text"], "pub fn a() {}\n");
    let cursor = alice.followed("the cursor", |v| v.get("line").is_some()).await;
    assert_eq!((cursor["line"].as_u64(), cursor["sel"][1].as_u64()), (Some(1), Some(4)));
    alice.followed("the edit", |v| v["edit"]["version"] == 2).await;
    alice.followed("diagnostics", |v| v["diagnostics"]["items"][0]["message"] == "nope").await;
    // Content: only to the follower.
    bob.pump(Duration::from_millis(500)).await;
    assert!(bob.follows.is_empty(), "{:?}", bob.follows);

    // A second follower gets what it shows now, at once.
    bob.send(ClientMsg::Follow { pane: id, on: true }).await;
    assert_eq!(ed.expect("followers").await["n"], 2);
    let open = bob.followed("the file, from the daemon", |v| v.get("open").is_some()).await;
    assert_eq!(open["open"]["text"], "pub fn a() {}\n");
    bob.followed("the edit since", |v| v["edit"]["version"] == 2).await;
    bob.followed("the cursor", |v| v["line"] == 1).await;

    // Unfollowing, and a follower leaving, count down.
    bob.send(ClientMsg::Follow { pane: id, on: false }).await;
    assert_eq!(ed.expect("followers").await["n"], 1);
    drop(alice);
    assert_eq!(ed.expect("followers").await["n"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_debugger_is_a_card_continue_answers() {
    let d = Daemon::child();
    let root = repo(&d);
    let mut ed = Editor::join(&d, json!({ "editor": "vscode", "workspace": root })).await;
    let id = ed.id;
    let mut web = Client::connect(&d).await;
    web.until("the editor", id, |p| p.is_some()).await;
    ed.send(json!({ "t": "summary", "debug": { "state": "paused", "reason": "breakpoint", "file": format!("{root}/src/app.ts"), "line": 12 } }))
        .await;
    let p = web.until("a card", id, |p| p.is_some_and(|p| p.reason.is_some())).await.unwrap();
    let r = p.reason.unwrap();
    assert_eq!(p.attention, arugula_proto::Attention::NeedsInput);
    assert_eq!(r.kind, arugula_proto::ReasonKind::Paused);
    assert_eq!(r.headline, "Paused at app.ts:12 (breakpoint)");
    assert_eq!(r.actions, vec![arugula_proto::Action::Continue, arugula_proto::Action::Dismiss]);
    // On the rail's list too.
    let list = d.get("/api/attention");
    assert!(list.as_array().unwrap().iter().any(|i| i["pane"] == id && i["reason"]["kind"] == "paused"), "{list}");

    // Continue reaches the editor; it runs on, and the card goes.
    d.post("/api/attention/act", json!({ "action": "continue", "pane": id }));
    ed.expect("continue").await;
    ed.send(json!({ "t": "summary", "debug": { "state": "running" } })).await;
    web.until("the card to go", id, |p| p.is_some_and(|p| p.reason.is_none())).await;
    // Not paused: nothing to continue.
    let (status, body) = d.raw("POST", "/api/attention/act", Some(json!({ "action": "continue", "pane": id })));
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("isn't paused"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_after_a_save_and_conflicts_are_cards() {
    let d = Daemon::child();
    let root = repo(&d);
    let mut ed = Editor::join(&d, json!({ "editor": "vscode", "workspace": root })).await;
    let id = ed.id;
    let mut web = Client::connect(&d).await;
    web.until("the editor", id, |p| p.is_some()).await;
    // Errors while typing aren't news; after a save they are.
    let file = format!("{root}/src/a.rs");
    ed.send(json!({ "t": "summary", "file": file, "diag": { "e": 2 } })).await;
    web.pump(Duration::from_millis(1500)).await;
    assert!(web.pane(id).unwrap().reason.is_none());
    ed.send(json!({ "t": "summary", "diag": { "e": 0 } })).await;
    ed.send(json!({ "t": "summary", "diag": { "e": 3 }, "saved": true })).await;
    let p = web.until("an errors card", id, |p| p.is_some_and(|p| p.reason.is_some())).await.unwrap();
    assert_eq!(p.reason.as_ref().unwrap().headline, "3 errors after saving a.rs");
    // Fixed: the card goes by itself.
    ed.send(json!({ "t": "summary", "diag": { "e": 0 }, "saved": true })).await;
    web.until("the card to go", id, |p| p.is_some_and(|p| p.reason.is_none())).await;
    // A merge conflict opened.
    ed.send(json!({ "t": "summary", "conflict": format!("{root}/src/lib.rs") })).await;
    let p = web.until("a conflict card", id, |p| p.is_some_and(|p| p.reason.is_some())).await.unwrap();
    assert_eq!(p.reason.as_ref().unwrap().kind, arugula_proto::ReasonKind::Conflict);
    assert_eq!(p.reason.as_ref().unwrap().headline, "Merge conflict in lib.rs");
    // Dismissing it clears the card; the editor stays.
    d.post("/api/attention/act", json!({ "action": "dismiss", "pane": id }));
    web.until("dismissed", id, |p| p.is_some_and(|p| p.reason.is_none())).await;
}

/// arugula.nvim, in a real headless nvim driven over its RPC socket.
#[tokio::test(flavor = "multi_thread")]
async fn nvim_joins_follows_and_leaves() {
    if std::process::Command::new("nvim").arg("--version").output().is_err() {
        eprintln!("no nvim here; skipping");
        return;
    }
    let d = Daemon::child();
    let root = repo(&d);
    let file = format!("{root}/src/a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
    let data = d.sessions.join("nvim-data");
    let rpc = d.sessions.join("nvim.sock");
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim");
    let mut nvim = std::process::Command::new("nvim")
        .args(["--headless", "--clean", "--listen"])
        .arg(&rpc)
        .arg("--cmd")
        .arg(format!("set rtp^={}", plugin.display()))
        .args(["-c", "runtime plugin/arugula.lua", "-c", "ArugulaJoin", &file])
        .current_dir(&root)
        .env("ARUGULA_SOCK", d.sock())
        .env_remove("ARUGULA_PANE")
        .env("XDG_DATA_HOME", &data)
        .env("XDG_STATE_HOME", d.sessions.join("nvim-state"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let keys = |k: &str| {
        let ok = std::process::Command::new("nvim")
            .arg("--server")
            .arg(&rpc)
            .args(["--remote-send", k])
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "sending {k}");
    };
    let mut web = Client::connect(&d).await;
    // It joins as nvim, in its project, with its file.
    let deadline = Instant::now() + Duration::from_secs(15);
    let id = loop {
        web.pump(Duration::from_millis(100)).await;
        let found = web
            .state
            .as_ref()
            .and_then(|s| s.panes.iter().find(|p| p.editor.as_ref().is_some_and(|e| e.app == "nvim")).map(|p| p.id));
        if let Some(id) = found {
            break id;
        }
        assert!(Instant::now() < deadline, "nvim didn't join");
    };
    let p = web.until("its file", id, |p| p.is_some_and(|p| p.file.as_deref() == Some("src/a.txt"))).await.unwrap();
    assert_eq!(p.project.as_ref().map(|p| p.name.as_str()), Some("proj"));
    // Following: the file, then the cursor and each change.
    web.send(ClientMsg::Follow { pane: id, on: true }).await;
    let open = web.followed("the file", |v| v.get("open").is_some()).await;
    assert_eq!(open["open"]["text"], "one\ntwo\nthree\nfour");
    keys("jj");
    web.followed("the cursor on line 3", |v| v["line"] == 3).await;
    keys("x");
    let edit = web.followed("an edit", |v| v.get("edit").is_some()).await;
    assert_eq!(edit["edit"]["changes"][0], json!({ "range": [3, 0, 4, 0], "text": "hree\n" }));
    // Deleting the last line: the newline before it goes too.
    keys("Gdd");
    let edit = web
        .followed("the last line deleted", |v| {
            v["edit"]["changes"][0]["text"] == "" && v["edit"]["changes"][0]["range"][0] == 3
        })
        .await;
    assert_eq!(edit["edit"]["changes"][0]["range"], json!([3, 4, 5, 0]));
    // A save that leaves it unsaved-free, and the summary says so.
    keys(":w<CR>");
    web.until("no unsaved buffers", id, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.dirty == 0)).await;
    // Leaving: gone at once, and the folder's forgotten.
    keys(":ArugulaLeave<CR>");
    web.until("it to go", id, |p| p.is_none()).await;
    let remembered = std::fs::read_to_string(data.join("nvim/arugula/folders.json")).unwrap();
    assert_eq!(remembered.trim(), "[]");
    let _ = nvim.kill();
    let _ = nvim.wait();
}

fn have_nvim() -> bool {
    std::process::Command::new("nvim").arg("--version").output().is_ok()
}

/// A headless nvim with arugula.nvim on its runtimepath, in `root`.
fn nvim_with(root: &str, data: &std::path::Path, rpc: &std::path::Path, args: &[&str]) -> std::process::Command {
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim");
    let mut c = std::process::Command::new("nvim");
    c.args(["--headless", "--clean", "--listen"])
        .arg(rpc)
        .arg("--cmd")
        .arg(format!("set rtp^={}", plugin.display()))
        .args(args)
        .current_dir(root)
        .env_remove("ARUGULA_SOCK")
        .env_remove("ARUGULA_PANE")
        .env("XDG_DATA_HOME", data)
        .env("XDG_STATE_HOME", data.join("state"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c
}

/// arugula.nvim: a folder it remembered rejoins by itself, `$ARUGULA_SOCK`
/// finds the daemon, and `id()` is its pane.
#[tokio::test(flavor = "multi_thread")]
async fn nvim_rejoins_what_it_remembered() {
    if !have_nvim() {
        eprintln!("no nvim here; skipping");
        return;
    }
    let d = Daemon::child();
    let root = repo(&d);
    let data = d.sessions.join("nvim-data");
    std::fs::create_dir_all(data.join("nvim/arugula")).unwrap();
    std::fs::write(data.join("nvim/arugula/folders.json"), serde_json::to_string(&[&root]).unwrap()).unwrap();
    let rpc = d.sessions.join("nvim.sock");
    let mut nvim = nvim_with(&root, &data, &rpc, &["-c", "runtime plugin/arugula.lua"])
        .env("ARUGULA_SOCK", d.sock())
        .spawn()
        .unwrap();
    let mut web = Client::connect(&d).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    let id = loop {
        web.pump(Duration::from_millis(100)).await;
        let found = web
            .state
            .as_ref()
            .and_then(|s| s.panes.iter().find(|p| p.editor.as_ref().is_some_and(|e| e.app == "nvim")).map(|p| p.id));
        if let Some(id) = found {
            break id;
        }
        assert!(Instant::now() < deadline, "nvim didn't rejoin");
    };
    let expr = |e: &str| {
        let out =
            std::process::Command::new("nvim").arg("--server").arg(&rpc).args(["--remote-expr", e]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    assert_eq!(expr(r#"luaeval('require("arugula").id()')"#), id.to_string());
    assert_eq!(expr("exists(':ArugulaLeave')"), "2");
    let _ = nvim.kill();
    let _ = nvim.wait();
}

/// nvim's version, if it's here: `NVIM v0.11.6`.
fn nvim_version() -> Option<(u32, u32)> {
    let out = std::process::Command::new("nvim").arg("--version").output().ok()?;
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.to_owned();
    let v = first.split_whitespace().nth(1)?.trim_start_matches('v').to_owned();
    let mut parts = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??))
}

/// Whether nvim is here and new enough for the daemon to read it in a pane
/// (#1730): it says why not, loudly, when it isn't.
fn nvim_for_panes() -> bool {
    match nvim_version() {
        None => {
            eprintln!("SKIPPED: no nvim here (CI installs it; 0.10 or later)");
            false
        }
        Some(v) if v < (0, 10) => {
            eprintln!("SKIPPED: nvim {}.{} is older than 0.10, which panes need (CI must install 0.10+)", v.0, v.1);
            false
        }
        Some(_) => true,
    }
}

/// A shell pane in `cwd`, at its prompt.
fn shell_pane(d: &Daemon, cwd: &str) -> u32 {
    let pane = d.post("/api/run", json!({ "cwd": cwd }))["pane"].as_u64().unwrap() as u32;
    d.wait_for("a prompt", || d.raw("GET", &format!("/api/panes/{pane}/capture"), None).1.contains('$'));
    pane
}

/// Type into a pane, as someone at its keyboard.
fn type_in(d: &Daemon, pane: u32, text: &str) {
    d.post(&format!("/api/panes/{pane}/send"), json!({ "text": text }));
}

/// The pid of the nvim (its TUI, not its `--embed` server) whose command
/// line has `marker`.
fn tui_pid(marker: &str) -> Option<u32> {
    let out = std::process::Command::new("ps").args(["-A", "-o", "pid=,args="]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let (pid, args) = l.trim().split_once(' ')?;
        (args.contains(marker) && args.contains("nvim") && !args.contains("--embed")).then(|| pid.parse().ok())?
    })
}

/// Every entry that's an editor.
fn editors_in(web: &Client) -> Vec<u32> {
    web.state
        .as_ref()
        .map(|s| s.panes.iter().filter(|p| p.editor.is_some()).map(|p| p.id).collect())
        .unwrap_or_default()
}

/// #1730: nvim in a pane, with no plugin and no config, is an editor on
/// that pane: the daemon finds it by its pid and reads it over its own RPC
/// socket. Its file and line show on the pane within 2 s of opening a file,
/// its diagnostic counts and unsaved buffers too, a merge conflict is a
/// reason on the pane, and quitting or `kill -9` takes it all off.
#[tokio::test(flavor = "multi_thread")]
async fn nvim_in_a_pane_is_an_editor_without_the_plugin() {
    if !nvim_for_panes() {
        return;
    }
    let d = Daemon::child();
    let root = repo(&d);
    let file = format!("{root}/src/a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(format!("{root}/src/b.txt"), "b1\nb2\nb3\nb4\nb5\n").unwrap();
    std::fs::write(format!("{root}/src/c.txt"), "<<<<<<< ours\nmine\n=======\ntheirs\n>>>>>>> theirs\n").unwrap();
    let pane = shell_pane(&d, &root);
    let mut web = Client::connect(&d).await;
    type_in(&d, pane, &format!("nvim --clean {file}\r"));
    // It's the pane: its file and line on it, and nothing beside it.
    let p = web
        .until("nvim's file on the pane", pane, |p| {
            p.and_then(|p| p.editor.as_ref().map(|e| (e, p.file.as_deref())))
                .is_some_and(|(e, f)| e.app == "nvim" && f == Some("src/a.txt") && e.line == Some(1))
        })
        .await
        .unwrap();
    assert!(p.editor.as_ref().unwrap().no_follow, "without the plugin there's no follow stream");
    assert_eq!(p.title.as_deref(), Some("a.txt:1 (nvim)"));
    assert_eq!(editors_in(&web), vec![pane], "one entry: the pane");
    // Opening another file, and moving in it: on the pane within 2 s.
    type_in(&d, pane, ":e src/b.txt\r");
    let opened = Instant::now();
    web.until("the new file", pane, |p| p.is_some_and(|p| p.file.as_deref() == Some("src/b.txt"))).await;
    assert!(opened.elapsed() < Duration::from_secs(2), "took {:?}", opened.elapsed());
    type_in(&d, pane, "3G");
    let p = web
        .until("the cursor on line 3", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.line == Some(3)))
        .await
        .unwrap();
    assert_eq!(p.title.as_deref(), Some("b.txt:3 (nvim)"));
    // What a language server reports, it counts.
    type_in(
        &d,
        pane,
        ":lua vim.diagnostic.set(vim.api.nvim_create_namespace('t'), 0, {{ lnum = 0, col = 0, message = 'x', severity = 1 }, { lnum = 1, col = 0, message = 'y', severity = 2 }})\r",
    );
    web.until("diagnostic counts", pane, |p| {
        p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.diag.e == 1 && e.diag.w == 1)
    })
    .await;
    // An unsaved change.
    type_in(&d, pane, "ox");
    tokio::time::sleep(Duration::from_millis(300)).await;
    type_in(&d, pane, "\x1b");
    web.until("an unsaved buffer", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.dirty == 1)).await;
    type_in(&d, pane, ":w\r");
    web.until("saved", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.dirty == 0)).await;
    // A merge conflict opening: a reason on the pane.
    type_in(&d, pane, ":e src/c.txt\r");
    let p = web
        .until("the conflict", pane, |p| {
            p.and_then(|p| p.reason.as_ref()).is_some_and(|r| r.kind == ReasonKind::Conflict)
        })
        .await
        .unwrap();
    assert_eq!(p.reason.unwrap().headline, "Merge conflict in c.txt");
    // Quitting: the editor's gone from the pane, and its reason with it
    // (the shell's own "finished" may follow).
    type_in(&d, pane, ":qa!\r");
    let p = web
        .until("nvim gone from the pane", pane, |p| {
            p.is_some_and(|p| p.editor.is_none() && p.reason.as_ref().is_none_or(|r| r.kind != ReasonKind::Conflict))
        })
        .await
        .unwrap();
    assert_eq!(p.file, None);
    // Again, and `kill -9`: gone within 5 s.
    type_in(&d, pane, &format!("nvim --clean {file}\r"));
    web.until("nvim again", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some()).await;
    let pid = tui_pid(&file).expect("nvim's pid");
    assert!(std::process::Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap().success());
    let killed = Instant::now();
    web.until("nvim gone after kill -9", pane, |p| p.is_some_and(|p| p.editor.is_none())).await;
    assert!(killed.elapsed() < Duration::from_secs(5), "took {:?}", killed.elapsed());
    assert!(editors_in(&web).is_empty(), "nothing left behind: {:?}", editors_in(&web));
}

/// #1730: with arugula.nvim, the same nvim in a pane is still one entry,
/// the pane: the plugin says which pane it's in, the daemon attaches it
/// there instead of making a presence, and following the pane follows its
/// buffer. Leaving, the pane is read over nvim's socket again; here that
/// socket is where the pane's own environment put it.
#[tokio::test(flavor = "multi_thread")]
async fn nvim_with_the_plugin_in_a_pane_is_that_pane() {
    if !nvim_for_panes() {
        return;
    }
    let d = Daemon::child();
    let root = repo(&d);
    let file = format!("{root}/src/a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim");
    let plugin = std::fs::canonicalize(plugin).unwrap();
    // A runtime directory only the pane knows of, short as sockets need.
    let run = std::path::PathBuf::from(format!("/tmp/aru-nv-{}", std::process::id()));
    std::fs::create_dir_all(&run).unwrap();
    let data = d.sessions.join("nvim-data");
    let pane = shell_pane(&d, &root);
    let mut web = Client::connect(&d).await;
    type_in(
        &d,
        pane,
        &format!(
            "XDG_RUNTIME_DIR={} XDG_DATA_HOME={} nvim --clean --cmd 'set rtp^={}' -c 'runtime plugin/arugula.lua' {file}\r",
            run.display(),
            data.display(),
            plugin.display(),
        ),
    );
    // The plugin's link on the pane: it can be followed.
    web.until("the plugin on the pane", pane, |p| {
        p.and_then(|p| p.editor.as_ref().map(|e| (e, p.file.as_deref())))
            .is_some_and(|(e, f)| e.app == "nvim" && !e.no_follow && f == Some("src/a.txt"))
    })
    .await;
    web.pump(Duration::from_millis(500)).await;
    assert_eq!(editors_in(&web), vec![pane], "one entry: the pane, no presence beside it");
    // Following the pane: the buffer, then the cursor, live.
    web.send(ClientMsg::Follow { pane, on: true }).await;
    let open = web.followed("the file", |v| v.get("open").is_some()).await;
    assert_eq!(open["open"]["text"], "one\ntwo\nthree\nfour");
    type_in(&d, pane, "jj");
    web.followed("the cursor on line 3", |v| v["line"] == 3).await;
    type_in(&d, pane, "x");
    let edit = web.followed("an edit", |v| v.get("edit").is_some()).await;
    assert_eq!(edit["edit"]["changes"][0], json!({ "range": [3, 0, 4, 0], "text": "hree\n" }));
    // Leaving: the pane is read over nvim's own socket again.
    type_in(&d, pane, ":ArugulaLeave\r");
    web.until("read without the plugin", pane, |p| {
        p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.no_follow && e.line == Some(3))
    })
    .await;
    assert_eq!(editors_in(&web), vec![pane]);
    type_in(&d, pane, ":qa!\r");
    web.until("nvim gone", pane, |p| p.is_some_and(|p| p.editor.is_none())).await;
    let _ = std::fs::remove_dir_all(&run);
}

/// #1733: nvim in a pane, with an empty data directory and no plugin. The
/// pane says Arugula can add arugula.nvim; the install operation (what the
/// tip's "Add arugula.nvim" calls, #1735) writes it into nvim's own package
/// directory and nowhere else (no `init.lua`), loads it into the running
/// nvim, and it joins as the pane without a restart: following works at
/// once. `:checkhealth arugula` then names the socket, the daemon and the
/// pane. The next nvim loads it by itself; remove deletes it, and the
/// offer comes back.
#[tokio::test(flavor = "multi_thread")]
async fn arugula_nvim_installs_into_a_running_nvim_and_joins() {
    if !nvim_for_panes() {
        return;
    }
    let d = Daemon::child();
    let root = repo(&d);
    let file = format!("{root}/src/a.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let home = d.sessions.join("nvim-1733");
    let (data, config, state) = (home.join("data"), home.join("config"), home.join("state"));
    for dir in [&data, &config, &state] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let nvim = format!(
        "XDG_DATA_HOME={} XDG_CONFIG_HOME={} XDG_STATE_HOME={} nvim {file}\r",
        data.display(),
        config.display(),
        state.display()
    );
    let pane = shell_pane(&d, &root);
    let mut web = Client::connect(&d).await;
    type_in(&d, pane, &nvim);
    web.until("the offer", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.no_follow && e.offer_plugin))
        .await;
    assert_eq!(d.get("/api/nvim-plugin")["copies"], json!([]));
    // One click.
    let (status, body) = d.raw("POST", &format!("/api/panes/{pane}/nvim-plugin"), None);
    assert_eq!(status, 200, "{body}");
    let done: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(done["joined"], true, "{done}");
    let dir = std::path::PathBuf::from(done["dir"].as_str().unwrap());
    assert_eq!(
        std::fs::canonicalize(&dir).unwrap(),
        std::fs::canonicalize(data.join("nvim/site/pack/arugula/start/arugula.nvim")).unwrap()
    );
    assert!(dir.join("ARUGULA_INSTALLED").is_file() && dir.join("plugin/arugula.lua").is_file());
    assert_eq!(std::fs::read_dir(&config).unwrap().count(), 0, "nothing in nvim's config: no init.lua");
    // It's the plugin on the pane now: followable, no offer, one entry.
    web.until("the plugin joined", pane, |p| {
        p.and_then(|p| p.editor.as_ref()).is_some_and(|e| !e.no_follow && !e.offer_plugin)
    })
    .await;
    assert_eq!(editors_in(&web), vec![pane]);
    web.send(ClientMsg::Follow { pane, on: true }).await;
    let open = web.followed("the file", |v| v.get("open").is_some()).await;
    assert_eq!(open["open"]["text"], "one\ntwo\nthree");
    web.send(ClientMsg::Follow { pane, on: false }).await;
    // :checkhealth arugula, written out.
    let report = home.join("health.txt");
    type_in(&d, pane, ":checkhealth arugula\r");
    tokio::time::sleep(Duration::from_millis(500)).await;
    type_in(&d, pane, &format!(":w! {}\r", report.display()));
    d.wait_for("the health report", || report.is_file());
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains(&format!("Socket: {}", d.sock().display())), "{text}");
    assert!(text.contains("Daemon: arugulad "), "{text}");
    assert!(text.contains(&format!("Tied to pane %{pane}")), "{text}");
    assert!(text.contains("Installed by Arugula"), "{text}");
    assert!(!text.contains("ERROR"), "{text}");
    // The next nvim has it from the start.
    type_in(&d, pane, ":qa!\r");
    web.until("nvim gone", pane, |p| p.is_some_and(|p| p.editor.is_none())).await;
    type_in(&d, pane, &nvim);
    web.until("the plugin, by itself", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| !e.no_follow)).await;
    type_in(&d, pane, ":qa!\r");
    web.until("nvim gone again", pane, |p| p.is_some_and(|p| p.editor.is_none())).await;
    // Settings' list, and Remove.
    let copies = d.get("/api/nvim-plugin");
    assert_eq!(copies["copies"].as_array().map(Vec::len), Some(1), "{copies}");
    assert_eq!(copies["copies"][0]["dir"], done["dir"]);
    let (status, body) = d.raw("DELETE", "/api/nvim-plugin", None);
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["removed"], json!([done["dir"]]));
    assert!(!dir.exists());
    type_in(&d, pane, &nvim);
    web.until("the offer again", pane, |p| p.and_then(|p| p.editor.as_ref()).is_some_and(|e| e.offer_plugin)).await;
    type_in(&d, pane, ":qa!\r");
    web.until("nvim gone at last", pane, |p| p.is_some_and(|p| p.editor.is_none())).await;
}

/// An editor isn't in any session: someone a session was shared with
/// doesn't see it, its preview, or its stream (the owner's, and a team's
/// members' on a team daemon).
#[tokio::test(flavor = "multi_thread")]
async fn guests_of_a_session_dont_see_editors() {
    const FRIEND: &str = "friend@example.com";
    let d = Daemon::child_with(&[
        "--wisp-token-file",
        "/nonexistent",
        "--owner",
        "me@example.com",
        "--tailscale-socket",
        "/nonexistent/sock",
    ]);
    let root = repo(&d);
    let session = d.get("/api/panes")[0]["session"].as_u64().unwrap();
    d.post("/api/acl", json!({ "session": session, "principal": format!("tailnet:{FRIEND}"), "role": "editor" }));
    let ed = Editor::join(&d, json!({ "editor": "vscode", "workspace": root })).await;
    let id = ed.id;
    let http = reqwest::Client::new();
    let get =
        |p: String| http.get(format!("http://127.0.0.1:{}{p}", d.port)).header("tailscale-user-login", FRIEND).send();
    assert_eq!(get("/api/editors".into()).await.unwrap().json::<Value>().await.unwrap(), json!([]));
    assert!([403, 404].contains(&get(format!("/api/panes/{id}/capture")).await.unwrap().status().as_u16()));
    // Their page: the session's panes, not the editor; following it is refused.
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = format!("ws://127.0.0.1:{}/ws", d.port).into_client_request().unwrap();
    req.headers_mut().insert("tailscale-user-login", FRIEND.parse().unwrap());
    let (ws, _) = connect_async(req).await.unwrap();
    let mut guest = Client { ws, state: None, follows: vec![] };
    guest.pump(Duration::from_millis(500)).await;
    let st = guest.state.as_ref().unwrap();
    assert!(!st.panes.is_empty());
    assert!(st.panes.iter().all(|p| p.id != id && p.editor.is_none()));
    guest.send(ClientMsg::Follow { pane: id, on: true }).await;
    guest.pump(Duration::from_millis(500)).await;
    assert!(guest.follows.is_empty());
    // The owner's does.
    let mut me = Client::connect(&d).await;
    me.until("the editor", id, |p| p.is_some()).await;
}

/// A dev container gets the editors' socket alone (its directory mounted):
/// joining as an editor works there, and nothing else does.
#[tokio::test(flavor = "multi_thread")]
async fn the_editors_socket_only_joins_editors() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::child();
    let root = repo(&d);
    let dir = d.state.join("editors");
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the socket, and nothing else");
    let sock = dir.join("sock");
    let ed = Editor::join_at(&sock, json!({ "editor": "vscode", "remote": "dev-container", "workspace": root })).await;
    let list = d.get("/api/editors");
    assert_eq!(
        (list[0]["pane"].as_u64(), list[0]["editor"]["remote"].as_str()),
        (Some(ed.id as u64), Some("dev-container"))
    );
    // The daemon's API isn't there.
    let mut s = UnixStream::connect(&sock).await.unwrap();
    s.write_all(b"GET /api/panes HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 404"), "{out}");
}

/// Only Arugula's protocol name joins: illogical's, from before the
/// rename, is refused like anything else.
#[tokio::test(flavor = "multi_thread")]
async fn only_our_protocol_joins() {
    let d = Daemon::child();
    for proto in ["illogical-editor", "websocket"] {
        let mut s = UnixStream::connect(d.sock()).await.unwrap();
        s.write_all(
            format!("GET /api/editors/connect HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: {proto}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
        let mut head = [0u8; 12];
        s.read_exact(&mut head).await.unwrap();
        assert_eq!(&head, b"HTTP/1.1 400", "{proto}");
    }
}
