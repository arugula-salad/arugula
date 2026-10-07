//! What each tool answers with (#448): one type per result, serialized to the
//! JSON agents see. `tools.rs` adds the `summary` sentence (`done`).
//!
//! They live here, not in proto, because only agents read them: no Rust or
//! TypeScript client deserializes them, so they derive `Serialize` alone.
//! Where a result is an API type already (`PromptResult`, `ThreadMsg`,
//! `ForgeWant`, `Draft`, `Ask`, `AppInfo`…) it is that type, so the tool and
//! the API can't drift. The JSON of each is what the old `json!` literal
//! wrote: a field is skipped only where the old code left the key out, and an
//! absent value is `null`, as before. Each is tested against that literal.
//!
//! A few fields stay `serde_json::Value`: what a block's untyped state or a
//! device handed back (`DiffFile`, `Member`, `DeviceCalled::result`), which
//! the tool passes through.

use arugula_proto::{
    Attention, BlockType, PaneId, ThreadTarget,
    api::{HistoryKind, Unreached},
    ask::Ask,
    forge::{Draft, ForgeWant},
};
use serde::Serialize;
use serde_json::Value;

use crate::apps::studio::AppInfo;

/// Only the pane: closed, answered, or sent a prompt.
#[derive(Debug, Serialize)]
pub struct PaneOnly {
    pub pane: PaneId,
}

/// A file pasted into a terminal (`attach` pastes only on Unix).
#[cfg(unix)]
#[derive(Debug, Serialize)]
pub struct Pasted {
    pub pane: PaneId,
    pub path: String,
}

/// Keys typed into a terminal: where its output from here starts.
#[derive(Debug, Serialize)]
pub struct Typed {
    pub pane: PaneId,
    pub next_offset: u64,
}

/// A prompt sent to an app block's agent.
#[derive(Debug, Serialize)]
pub struct Prompted {
    pub pane: PaneId,
    pub tab: String,
    /// The app's own answer (`out["chat"]`, `out["position"]`).
    pub chat: Value,
    pub position: Value,
}

/// A command's pane, by what it's doing (`state`).
#[derive(Debug, Serialize)]
#[serde(tag = "state")]
pub enum RunState {
    #[serde(rename = "started")]
    Started { pane: PaneId },
    #[serde(rename = "still running")]
    StillRunning { pane: PaneId, command: Option<String>, next_offset: u64, last_lines: String },
    #[serde(rename = "done")]
    Done {
        pane: PaneId,
        command: Option<String>,
        exit: Option<i32>,
        output_offset: u64,
        next_offset: u64,
        last_lines: String,
    },
    #[serde(rename = "exited")]
    Exited { pane: PaneId, exit: Option<i32>, last_lines: String },
    #[serde(rename = "matched")]
    Matched {
        pane: PaneId,
        #[serde(rename = "match")]
        matched: String,
        offset: u64,
        next_offset: u64,
    },
}

/// `wait` until idle or needs_input: where the agent got (its `state` is the
/// attention itself, so it isn't a [`RunState`]).
#[derive(Debug, Serialize)]
pub struct Attended {
    pub pane: PaneId,
    pub state: Attention,
    pub ask: Option<Box<Ask>>,
}

/// The command `read_output` was asked about (`last_command`).
#[derive(Debug, Serialize)]
pub struct OutputCommand {
    pub text: Option<String>,
    pub exit: Option<i32>,
    pub running: bool,
}

/// A page of a terminal's output.
#[derive(Debug, Serialize)]
pub struct Output {
    pub pane: PaneId,
    pub offset: u64,
    pub next_offset: u64,
    pub more: bool,
    pub command: Option<OutputCommand>,
    pub text: String,
}

/// A page of a block's text (an agent's transcript), paged by character.
#[derive(Debug, Serialize)]
pub struct Page {
    pub pane: PaneId,
    pub offset: usize,
    pub next_offset: usize,
    pub more: bool,
    pub text: String,
}

/// A pane's screen.
#[derive(Debug, Serialize)]
pub struct Screen {
    pub pane: PaneId,
    pub text: String,
}

/// The last command in a [`PaneEntry`].
#[derive(Debug, Serialize)]
pub struct LastCommand {
    pub command: Option<String>,
    pub exit: Option<i32>,
}

/// A pane or block in `list` (and in the `arugula://block/N` resource).
#[derive(Debug, Serialize)]
pub struct PaneEntry {
    pub pane: PaneId,
    #[serde(rename = "type")]
    pub kind: BlockType,
    pub session: String,
    pub tab: u32,
    pub tab_name: Option<String>,
    pub machine: Option<String>,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub title: Option<String>,
    pub running: bool,
    pub attention: Attention,
    pub why: Option<String>,
    pub current: Option<String>,
    pub last: Option<LastCommand>,
    pub started_by: Option<String>,
    /// Set on the caller's own pane only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub you: Option<bool>,
}

/// `list` kind panes.
#[derive(Debug, Serialize)]
pub struct Panes {
    pub panes: Vec<PaneEntry>,
}

/// The `arugula://block/N` resource: the pane, and the block's state.
#[derive(Debug, Serialize)]
pub struct BlockResource {
    pub pane: PaneEntry,
    pub state: Value,
}

/// `read_thread`.
#[derive(Debug, Serialize)]
pub struct ThreadRead {
    pub thread: ThreadTarget,
    pub messages: Vec<arugula_proto::ThreadMsg>,
    pub last: u64,
}

/// `post_thread`.
#[derive(Debug, Serialize)]
pub struct ThreadPosted {
    pub thread: ThreadTarget,
    pub message: arugula_proto::ThreadMsg,
    pub unreached: Vec<Unreached>,
}

/// One command in `history`.
#[derive(Debug, Serialize)]
pub struct CommandEntry {
    pub pane: PaneId,
    pub open: bool,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub exit: Option<i32>,
    pub started_ms: u64,
    pub started: String,
    pub seconds: Option<u64>,
    pub by: Option<String>,
    pub kind: HistoryKind,
    pub output_offset: u64,
}

/// `history`.
#[derive(Debug, Serialize)]
pub struct Commands {
    pub commands: Vec<CommandEntry>,
    pub total: usize,
}

/// One line in `history` kind output.
#[derive(Debug, Serialize)]
pub struct Hit {
    pub pane: PaneId,
    pub open: bool,
    pub offset: u64,
    pub command: Option<String>,
    pub thread: Option<String>,
    pub line: String,
}

/// `history` kind output.
#[derive(Debug, Serialize)]
pub struct Hits {
    pub hits: Vec<Hit>,
}

/// A browser block on a port.
#[derive(Debug, Serialize)]
pub struct PortOpened {
    pub block: PaneId,
    pub port: u16,
}

/// A studio app opened in a block.
#[derive(Debug, Serialize)]
pub struct AppOpened {
    pub block: PaneId,
    pub app: String,
}

/// The studio's apps.
#[derive(Debug, Serialize)]
pub struct Apps {
    pub apps: Vec<AppInfo>,
}

/// A block opened, with nothing more to say: a file, a started agent.
#[derive(Debug, Serialize)]
pub struct BlockOnly {
    pub block: PaneId,
}

/// A file of a diff block. The fields are the block's own, passed through.
#[derive(Debug, Serialize)]
pub struct DiffFile {
    pub path: Value,
    pub old: Value,
    pub status: Value,
    pub add: Value,
    pub del: Value,
    pub binary: Value,
    pub big: Value,
}

/// A diff block opened.
#[derive(Debug, Serialize)]
pub struct DiffOpened {
    pub block: PaneId,
    pub repo: Value,
    pub files: Vec<DiffFile>,
}

/// A member of a workspace block. The fields are the block's own.
#[derive(Debug, Serialize)]
pub struct Member {
    pub name: Value,
    pub dir: Value,
    pub kind: Value,
    pub errors: Value,
    pub gates: Value,
}

/// A workspace block opened.
#[derive(Debug, Serialize)]
pub struct WorkspaceOpened {
    pub block: PaneId,
    pub root: Value,
    pub members: Vec<Member>,
    pub gates: Value,
}

/// A card in `list` kind fountain_agents.
#[derive(Debug, Serialize)]
pub struct AgentRow {
    pub name: String,
    pub id: String,
    pub runtime: String,
    pub model: String,
    pub source: Option<crate::fountain::catalog::Source>,
    pub app: Option<String>,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
    pub description: String,
}

/// `list` kind fountain_agents.
#[derive(Debug, Serialize)]
pub struct FountainAgents {
    pub total: usize,
    pub agents: Vec<AgentRow>,
    pub unreadable: usize,
}

/// A Fountain block's text.
#[derive(Debug, Serialize)]
pub struct BlockText {
    pub block: PaneId,
    pub text: String,
}

/// A new issue drafted in a forge block, waiting for the user.
#[derive(Debug, Serialize)]
pub struct ForgeDrafted {
    pub block: PaneId,
    pub status: &'static str,
    pub text: String,
}

/// A PR or issue block opened.
#[derive(Debug, Serialize)]
pub struct ForgeOpened {
    pub block: PaneId,
    pub text: String,
    pub wants: Vec<ForgeWant>,
}

/// `read_forge`.
#[derive(Debug, Serialize)]
pub struct ForgeRead {
    pub block: PaneId,
    pub text: String,
    pub wants: Vec<ForgeWant>,
    pub drafts: Vec<Draft>,
    pub error: Option<String>,
}

/// An invite drafted on the user's card.
#[derive(Debug, Serialize)]
pub struct InviteDrafted {
    pub draft: String,
    pub status: &'static str,
    pub block: PaneId,
    pub who: String,
    pub name: String,
    pub role: arugula_core::Role,
    pub session: arugula_core::SessionId,
    pub pane: PaneId,
}

/// `read_invite`: the draft as its card has it (the block's state while it's
/// open, the kept card once it was closed), with its `draft` id and `block`
/// set over it.
#[derive(Debug, Serialize)]
pub struct InviteRead<D: Serialize> {
    #[serde(flatten)]
    pub card: D,
    pub draft: String,
    pub block: PaneId,
}

/// `prompt_agent`: what prompting came to, with the pane.
#[derive(Debug, Serialize)]
pub struct PromptedAgent {
    #[serde(flatten)]
    pub result: arugula_proto::api::PromptResult,
    pub pane: PaneId,
}

/// `read_file`.
#[derive(Debug, Serialize)]
pub struct FileRead {
    pub path: String,
    pub size: u64,
    pub offset: u64,
    pub next_offset: u64,
    pub more: bool,
    pub text: String,
}

/// `list` kind devices. Each is `hand.rs`'s own object, passed through.
#[derive(Debug, Serialize)]
pub struct Devices {
    pub devices: Vec<Value>,
}

/// `device_call`. `result` is whatever the device answered.
#[derive(Debug, Serialize)]
pub struct DeviceCalled {
    pub device: String,
    pub name: String,
    pub result: Value,
    pub woke_ms: Option<u64>,
    pub answer_ms: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn same(got: impl Serialize, want: Value) {
        assert_eq!(serde_json::to_value(got).unwrap(), want);
    }

    #[test]
    fn a_pane_alone_and_the_small_ones() {
        same(PaneOnly { pane: 7 }, json!({ "pane": 7 }));
        #[cfg(unix)]
        same(Pasted { pane: 7, path: "/tmp/a.png".into() }, json!({ "pane": 7, "path": "/tmp/a.png" }));
        same(Typed { pane: 7, next_offset: 120 }, json!({ "pane": 7, "next_offset": 120 }));
        same(
            Prompted { pane: 7, tab: "its tab".into(), chat: json!("c1"), position: Value::Null },
            json!({ "pane": 7, "tab": "its tab", "chat": "c1", "position": null }),
        );
        same(BlockOnly { block: 9 }, json!({ "block": 9 }));
        same(PortOpened { block: 9, port: 3000 }, json!({ "block": 9, "port": 3000 }));
        same(AppOpened { block: 9, app: "notes".into() }, json!({ "block": 9, "app": "notes" }));
        same(BlockText { block: 9, text: "t".into() }, json!({ "block": 9, "text": "t" }));
        same(Screen { pane: 7, text: "hi".into() }, json!({ "pane": 7, "text": "hi" }));
        same(
            FileRead { path: "/a".into(), size: 10, offset: 0, next_offset: 10, more: false, text: "x".into() },
            json!({ "path": "/a", "size": 10, "offset": 0, "next_offset": 10, "more": false, "text": "x" }),
        );
        same(
            Page { pane: 7, offset: 0, next_offset: 5, more: true, text: "x".into() },
            json!({ "pane": 7, "offset": 0, "next_offset": 5, "more": true, "text": "x" }),
        );
    }

    #[test]
    fn the_states_of_a_command() {
        same(RunState::Started { pane: 7 }, json!({ "pane": 7, "state": "started" }));
        same(
            RunState::StillRunning { pane: 7, command: None, next_offset: 9, last_lines: String::new() },
            json!({ "pane": 7, "state": "still running", "command": null, "next_offset": 9, "last_lines": "" }),
        );
        same(
            RunState::StillRunning { pane: 7, command: Some("make".into()), next_offset: 9, last_lines: "x".into() },
            json!({ "pane": 7, "state": "still running", "command": "make", "next_offset": 9, "last_lines": "x" }),
        );
        same(
            RunState::Done {
                pane: 7,
                command: None,
                exit: None,
                output_offset: 1,
                next_offset: 2,
                last_lines: "l".into(),
            },
            json!({ "pane": 7, "state": "done", "command": null, "exit": null, "output_offset": 1,
                "next_offset": 2, "last_lines": "l" }),
        );
        same(
            RunState::Done {
                pane: 7,
                command: Some("ls".into()),
                exit: Some(2),
                output_offset: 1,
                next_offset: 2,
                last_lines: "l".into(),
            },
            json!({ "pane": 7, "state": "done", "command": "ls", "exit": 2, "output_offset": 1,
                "next_offset": 2, "last_lines": "l" }),
        );
        same(
            RunState::Exited { pane: 7, exit: None, last_lines: "bye".into() },
            json!({ "pane": 7, "state": "exited", "exit": null, "last_lines": "bye" }),
        );
        same(
            RunState::Matched { pane: 7, matched: "ready".into(), offset: 3, next_offset: 8 },
            json!({ "pane": 7, "state": "matched", "match": "ready", "offset": 3, "next_offset": 8 }),
        );
        same(
            Attended { pane: 7, state: Attention::Done, ask: None },
            json!({ "pane": 7, "state": Attention::Done, "ask": null }),
        );
    }

    #[test]
    fn output_with_and_without_its_command() {
        same(
            Output { pane: 7, offset: 0, next_offset: 4, more: false, command: None, text: "abc\n".into() },
            json!({ "pane": 7, "offset": 0, "next_offset": 4, "more": false, "command": null, "text": "abc\n" }),
        );
        same(
            Output {
                pane: 7,
                offset: 0,
                next_offset: 4,
                more: true,
                command: Some(OutputCommand { text: Some("ls".into()), exit: None, running: true }),
                text: String::new(),
            },
            json!({ "pane": 7, "offset": 0, "next_offset": 4, "more": true,
                "command": { "text": "ls", "exit": null, "running": true }, "text": "" }),
        );
    }

    fn entry() -> PaneEntry {
        PaneEntry {
            pane: 3,
            kind: BlockType::Terminal,
            session: "main".into(),
            tab: 1,
            tab_name: None,
            machine: None,
            cwd: None,
            command: None,
            title: None,
            running: false,
            attention: Attention::Done,
            why: None,
            current: None,
            last: None,
            started_by: None,
            you: None,
        }
    }

    #[test]
    fn a_pane_in_list_leaves_out_you_unless_it_is_the_callers() {
        // Every key is there, null when empty, but `you`.
        same(
            Panes { panes: vec![entry()] },
            json!({ "panes": [{
                "pane": 3, "type": BlockType::Terminal, "session": "main", "tab": 1, "tab_name": null,
                "machine": null, "cwd": null, "command": null, "title": null, "running": false,
                "attention": Attention::Done, "why": null, "current": null, "last": null, "started_by": null,
            }] }),
        );
        let mine = PaneEntry {
            tab_name: Some("work".into()),
            machine: Some("m2".into()),
            last: Some(LastCommand { command: Some("ls".into()), exit: Some(0) }),
            you: Some(true),
            ..entry()
        };
        let v = serde_json::to_value(&mine).unwrap();
        assert_eq!(v["you"], true);
        assert_eq!(v["machine"], "m2");
        assert_eq!(v["last"], json!({ "command": "ls", "exit": 0 }));
        same(BlockResource { pane: entry(), state: Value::Null }, {
            let mut want = serde_json::to_value(entry()).unwrap();
            want = json!({ "pane": want, "state": null });
            want
        });
    }

    #[test]
    fn history_and_search_rows() {
        same(
            Commands {
                commands: vec![CommandEntry {
                    pane: 2,
                    open: true,
                    command: None,
                    cwd: None,
                    exit: None,
                    started_ms: 5,
                    started: "1m ago".into(),
                    seconds: None,
                    by: None,
                    kind: HistoryKind::Command,
                    output_offset: 0,
                }],
                total: 1,
            },
            json!({ "commands": [{ "pane": 2, "open": true, "command": null, "cwd": null, "exit": null,
                "started_ms": 5, "started": "1m ago", "seconds": null, "by": null, "kind": "command",
                "output_offset": 0 }], "total": 1 }),
        );
        same(
            Hits {
                hits: vec![Hit {
                    pane: 2,
                    open: false,
                    offset: 4,
                    command: Some("ls".into()),
                    thread: None,
                    line: "l".into(),
                }],
            },
            json!({ "hits": [{ "pane": 2, "open": false, "offset": 4, "command": "ls", "thread": null,
                "line": "l" }] }),
        );
    }

    #[test]
    fn blocks_that_were_opened() {
        same(
            DiffOpened {
                block: 4,
                repo: json!("/r"),
                files: vec![DiffFile {
                    path: json!("a"),
                    old: Value::Null,
                    status: json!("M"),
                    add: json!(1),
                    del: json!(2),
                    binary: json!(false),
                    big: json!(false),
                }],
            },
            json!({ "block": 4, "repo": "/r", "files": [{ "path": "a", "old": null, "status": "M", "add": 1,
                "del": 2, "binary": false, "big": false }] }),
        );
        same(
            WorkspaceOpened {
                block: 4,
                root: json!("/w"),
                members: vec![Member {
                    name: json!("m"),
                    dir: json!("/w/m"),
                    kind: json!("rust"),
                    errors: json!(0),
                    gates: json!([]),
                }],
                gates: json!([]),
            },
            json!({ "block": 4, "root": "/w", "members": [{ "name": "m", "dir": "/w/m", "kind": "rust",
                "errors": 0, "gates": [] }], "gates": [] }),
        );
        same(
            FountainAgents {
                total: 3,
                agents: vec![AgentRow {
                    name: "n".into(),
                    id: "i".into(),
                    runtime: "claude".into(),
                    model: "m".into(),
                    source: None,
                    app: None,
                    skills: vec![],
                    mcp: vec!["x".into()],
                    description: String::new(),
                }],
                unreadable: 0,
            },
            json!({ "total": 3, "agents": [{ "name": "n", "id": "i", "runtime": "claude", "model": "m",
                "source": null, "app": null, "skills": [], "mcp": ["x"], "description": "" }],
                "unreadable": 0 }),
        );
        same(Apps { apps: vec![] }, json!({ "apps": [] }));
    }

    #[test]
    fn forge_and_invite_results() {
        same(
            ForgeDrafted { block: 5, status: "waiting", text: "t".into() },
            json!({ "block": 5, "status": "waiting", "text": "t" }),
        );
        same(
            ForgeOpened { block: 5, text: "t".into(), wants: vec![] },
            json!({ "block": 5, "text": "t", "wants": [] }),
        );
        same(
            ForgeRead { block: 5, text: "t".into(), wants: vec![], drafts: vec![], error: None },
            json!({ "block": 5, "text": "t", "wants": [], "drafts": [], "error": null }),
        );
        same(
            ForgeRead { block: 5, text: "t".into(), wants: vec![], drafts: vec![], error: Some("no".into()) },
            json!({ "block": 5, "text": "t", "wants": [], "drafts": [], "error": "no" }),
        );
        same(
            InviteDrafted {
                draft: "d1".into(),
                status: "waiting",
                block: 6,
                who: "person".into(),
                name: "Pat".into(),
                role: arugula_core::Role::Viewer,
                session: 1,
                pane: 2,
            },
            json!({ "draft": "d1", "status": "waiting", "block": 6, "who": "person", "name": "Pat",
                "role": arugula_core::Role::Viewer, "session": 1, "pane": 2 }),
        );
        // The card's own `draft` and `block` keys, if it had any, lose.
        same(
            InviteRead {
                card: json!({ "id": "d1", "status": "sent", "block": 1, "draft": "x" }),
                draft: "d1".into(),
                block: 6,
            },
            json!({ "id": "d1", "status": "sent", "block": 6, "draft": "d1" }),
        );
    }

    #[test]
    fn prompting_threads_and_devices() {
        use arugula_proto::api::PromptResult;
        same(PromptedAgent { result: PromptResult::Done, pane: 7 }, json!({ "result": "done", "pane": 7 }));
        same(
            PromptedAgent { result: PromptResult::Stalled { why: "w".into(), screen: "s".into() }, pane: 7 },
            json!({ "result": "stalled", "why": "w", "screen": "s", "pane": 7 }),
        );
        same(
            PromptedAgent { result: PromptResult::NeedsInput { question: None, ask: None }, pane: 7 },
            json!({ "result": "needs_input", "pane": 7 }),
        );
        same(
            ThreadRead { thread: ThreadTarget::Pane(3), messages: vec![], last: 0 },
            json!({ "thread": ThreadTarget::Pane(3), "messages": [], "last": 0 }),
        );
        same(
            ThreadPosted {
                thread: ThreadTarget::Session(1),
                message: serde_json::from_value(json!({ "id": 1, "at": 2, "who": "mcp:x", "name": "x", "text": "hi" }))
                    .unwrap(),
                unreached: vec![],
            },
            json!({ "thread": ThreadTarget::Session(1),
                "message": { "id": 1, "at": 2, "who": "mcp:x", "name": "x", "text": "hi" }, "unreached": [] }),
        );
        same(Devices { devices: vec![] }, json!({ "devices": [] }));
        same(
            DeviceCalled {
                device: "d".into(),
                name: "Phone".into(),
                result: json!({ "ok": true }),
                woke_ms: None,
                answer_ms: 40,
            },
            json!({ "device": "d", "name": "Phone", "result": { "ok": true }, "woke_ms": null, "answer_ms": 40 }),
        );
    }
}
