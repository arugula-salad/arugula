//! A project's boards from agents (boards, step 3; lanes, aru-125): the board
//! in Arugula control (`brd_…`, an ordered list of rows, each one piece of
//! work, read as four lanes that follow the work), read with
//! `board_read` and changed with `board_write`, behind the `boards` Labs
//! flag (listed only when it is on, and refused when it is off).
//!
//! - **Through the owner's grants.** Each call is a machine-signed route on
//!   the platform (`/api/daemon/boards/...`,
//!   `crates/platform/src/boards/agent.rs`), which asks this machine's
//!   grants every time: `boards.read` to read, `boards.write` to change
//!   (which reads too), over one project each, given by its owner
//!   (`arugula grants add <machine> boards.write --project <name>`), and
//!   never more than their role. What an agent writes lands at once, and a
//!   board or a row it makes says which machine's agent made it.
//! - **What `boards.write` may do:** make a board (with a goal), set its
//!   title and goal, close it with a done note and open it again; add rows
//!   (an issue or a PR is a row's item; a PR that names the issue of a row is
//!   attached to that row), set any row's note, attach work (a PR, an agent
//!   block, a run) and proof (a run, a link) to any row and take off what it
//!   attached; remove or repoint a row its own machine added. Never remove
//!   or repoint a row another hand added: the platform refuses those
//!   (aru-123). **Nobody moves a row:** the lane follows the work, and
//!   moves, zones and statuses are refused (aru-125).
//! - **Revs.** A row's `rev` counts changes to its note and pointer. A
//!   write of a note names the `rev` that `board_read` showed; if the row
//!   moved on, the platform answers 409 and this says to read again.
//! - **Which project** a list means is the issues' rule: `project`, else
//!   the one this clone's GitHub repo is linked to, else the owner's
//!   personal project.
//!
//! The text form of a board (72 columns) is `arugula_control_wire::boards::
//! render`. An agent a guest stands behind may not use these: they act as
//! the owner.

use arugula_control_wire::boards::{
    self as wire, AttachRole, AttachRow, BoardList, BoardView, CloseBoard, EditBoard, NewBoard, Pointer, PutEntry,
    Status, agent_board, agent_board_attachment, agent_board_attachments, agent_board_close, agent_board_entries,
    agent_board_entry, agent_board_reopen, agent_project_boards,
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::tools::{Call, Kind, Out, done};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const READ_TOOL: &str = "board_read";
pub(crate) const WRITE_TOOL: &str = "board_write";

pub(crate) const READ_DESCRIPTION: &str = "Read a project's board in Arugula control (an epic when it has a goal; with this machine's boards.read grant over its project): a board is rows, each one piece of work: an issue or a PR, or a note, with a one-line note, and what is attached to it (work: PRs, agent blocks, runs; proof: runs, links). It is read as four lanes that follow the work, never placed by anyone: Next, Doing (an agent block that isn't closed, or a run running), Review (a PR is open) and Done (the issue is closed, or the PR merged or closed); a Done row is proven by CI (a PR merged with every check green) or by evidence, or not proven yet. Returns the lanes and rows with their ids and revs (what board_write names), and the board as 72-column text. With board (brd_…): that board. Without: the project's boards (project, else the one this clone's GitHub repo is linked to, else your person's personal project), to pick one from.";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to a project's board in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. Their boards.write grant over the project says you may, never more than their role. A board is rows, one piece of work each, and its lanes (Next, Doing, Review, Done) follow the live state of the work: you can't move a row, make or change a zone, or set a status (except needs_input); the platform refuses those, \"lanes follow the work\". You may make a board (an epic, with a goal), set its title and goal, close it with a note saying what a person can now do, and open it again; add a row for an issue or a PR (a PR that names the issue of a row is attached to that row, and an item already on the board isn't added again; the answer says so), set a row's note, attach work (a PR, an agent block or a run, which moves the lane) or proof (a run or a link, which marks a Done row proven) to a row, take off an attachment you made, and remove or repoint a row you added. You may not remove or repoint a row someone else added. A note or a pointer names the rev board_read showed for the row; if it has changed since, this says so: board_read again and retry. Every kind returns the board as it is now.";

/// The one status a row may be given: it asks a person for input, until
/// asks replace it.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StatusArg {
    /// Clear it.
    None,
    NeedsInput,
}

impl From<StatusArg> for Status {
    fn from(s: StatusArg) -> Self {
        match s {
            StatusArg::None => Status::None,
            StatusArg::NeedsInput => Status::NeedsInput,
        }
    }
}

/// What a row's item is, or what is attached to it.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointerArg {
    /// A note: no item.
    None,
    /// A mirrored GitHub or Plane PR or issue (fi_…): a row's item, or a PR
    /// attached as work.
    ForgeItem { id: String },
    /// A native issue (iss_…): a row's item.
    Issue { id: String },
    /// A run (run_…): attached as work or proof.
    Run { id: String },
    /// An agent block on a machine: attached as work.
    Block { machine: String, pane: String },
    /// An https:// URL as evidence (a CI page, a report): no user name or
    /// password in it, at most 2048 bytes. Attached as proof.
    Link { url: String },
}

impl From<PointerArg> for Pointer {
    fn from(p: PointerArg) -> Self {
        match p {
            PointerArg::None => Pointer::None,
            PointerArg::ForgeItem { id } => Pointer::ForgeItem { id },
            PointerArg::Issue { id } => Pointer::Issue { id },
            PointerArg::Run { id } => Pointer::Run { id },
            PointerArg::Block { machine, pane } => Pointer::Block { machine, pane },
            PointerArg::Link { url } => Pointer::Link { url },
        }
    }
}

/// `board_read`.
#[derive(Deserialize, JsonSchema)]
pub struct ReadBoardArgs {
    /// The board (brd_…). Without it: the project's boards.
    #[serde(default)]
    pub board: Option<String>,
    /// The project's id (prj_…) or personal, to list its boards. Default:
    /// the current project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateBoardArgs {
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// One line, at most 120 characters.
    pub title: String,
    /// What the board is for, in a paragraph (at most 2000 characters): a
    /// board with a goal is an epic. (A board has four lanes, made by no one.)
    #[serde(default)]
    pub goal: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditBoardArgs {
    /// The board (brd_…).
    pub board: String,
    #[serde(default)]
    pub title: Option<String>,
    /// An empty goal clears it.
    #[serde(default)]
    pub goal: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloseBoardArgs {
    /// The board (brd_…).
    pub board: String,
    /// What a person can now do, not empty.
    pub done_note: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct BoardArgs {
    /// The board (brd_…).
    pub board: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// One line, at most 280 characters.
    #[serde(default)]
    pub note: Option<String>,
    /// The row's item: an issue (iss_…, or fi_… for a mirrored issue) or a PR
    /// (fi_…); without one the row is a note. A PR that names the issue of a
    /// row on the board is attached to that row, and one already on the board
    /// isn't added again.
    #[serde(default)]
    pub pointer: Option<PointerArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row (row_…, from board_read).
    pub row: String,
    /// The new note, one line, at most 280 characters. Needs rev.
    #[serde(default)]
    pub note: Option<String>,
    /// The row's item now, for a row you added. Needs rev.
    #[serde(default)]
    pub pointer: Option<PointerArg>,
    /// The row's rev from board_read: needed with note or pointer.
    #[serde(default)]
    pub rev: Option<u64>,
    /// needs_input, or none to clear it. Lanes follow the work: no other
    /// status is taken.
    #[serde(default)]
    pub status: Option<StatusArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RemoveRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// A row you added (row_…).
    pub row: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct AttachArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row (row_…, from board_read).
    pub row: String,
    /// work: a PR (forge_item), an agent block or a run; it moves the row's
    /// lane. proof: a run or a link; it marks a Done row proven.
    pub role: AttachRoleArg,
    pub pointer: PointerArg,
}

/// What an attachment is for.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AttachRoleArg {
    Work,
    Proof,
}

#[derive(Deserialize, JsonSchema)]
pub struct DetachArgs {
    /// The board (brd_…).
    pub board: String,
    /// The attachment (att_…, from board_read): one you made.
    pub attachment: String,
}

pub(crate) const WRITE: &[Kind] = &[
    Kind {
        name: "add_row",
        description: "Add a row last: an issue or a PR as its item (or a note), with a one-line note. A PR that names the issue of a row is attached to that row; an item already on the board isn't added again.",
        schema: schema_for_type::<AddRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "set",
        description: "Set a row's note or pointer (with its rev), or its needs_input status.",
        schema: schema_for_type::<SetRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "attach",
        description: "Attach work (a PR, an agent block or a run) or proof (a run or a link) to a row.",
        schema: schema_for_type::<AttachArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "detach",
        description: "Take off an attachment you made.",
        schema: schema_for_type::<DetachArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "remove_row",
        description: "Remove a row you added.",
        schema: schema_for_type::<RemoveRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "create_board",
        description: "Make a board in a project, with a goal (an epic); returns it.",
        schema: schema_for_type::<CreateBoardArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "edit_board",
        description: "Set a board's title and/or goal.",
        schema: schema_for_type::<EditBoardArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "close_board",
        description: "Close a board with a note saying what a person can now do.",
        schema: schema_for_type::<CloseBoardArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "reopen_board",
        description: "Open a closed board again, keeping its note.",
        schema: schema_for_type::<BoardArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
];

fn args<T: serde::de::DeserializeOwned>(a: Value) -> Result<T, String> {
    serde_json::from_value(a).map_err(|e| format!("bad arguments: {e}"))
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where boards live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents use boards yet".into()
        }
        CloudRefused::Said { status: 409, said, .. } => format!(
            "that row has changed since you read it ({said}): board_read again, then retry with the rev it shows now"
        ),
        e => e.to_string(),
    }
}

/// A board as text: its title, then [`wire::render`].
fn text(v: &BoardView) -> String {
    format!("{}\n{}", v.board.title, wire::render(v))
}

/// `Title (brd_…): 5 row(s): 2 next, 1 doing, 1 review, 1 done`.
fn line(v: &BoardView) -> String {
    let n = v.zones.iter().map(|z| z.rows.len()).sum::<usize>();
    let by: Vec<String> = v.zones.iter().map(|z| format!("{} {}", z.rows.len(), z.title.to_lowercase())).collect();
    format!("{} ({}): {n} row(s): {}", v.board.title, v.board.id, by.join(", "))
}

/// A board's body for a PATCH: only what is given.
fn given(v: impl serde::Serialize) -> Result<Value, String> {
    let mut body = serde_json::to_value(v).map_err(|e| e.to_string())?;
    if let Some(o) = body.as_object_mut() {
        o.retain(|_, v| !v.is_null());
    }
    Ok(body)
}

/// They act as this machine's owner: only the owner's own agents may.
async fn owners_boards(c: &Call<'_>, what: &str) -> Result<(), String> {
    crate::lab_flags::flag(c.app().control.state_dir(), arugula_proto::flags::BOARDS)?;
    if let Some(me) = c.me()
        && c.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err(format!("{what} acts as this machine's owner: only their own agents may"));
    }
    Ok(())
}

async fn boards_call(c: &Call<'_>, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    c.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
}

/// The board the platform answers a write with, as the tool's result.
fn board_done(what: &str, v: Value) -> Out {
    let view: BoardView =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent a board I can't read: {e}"))?;
    let said = view.said.as_deref().map(|s| format!(" Note: {s}.")).unwrap_or_default();
    let s = format!("{what}, now, as your person's agent on this machine. {}{said}", line(&view));
    let t = text(&view);
    let mut out = serde_json::to_value(&view).map_err(|e| e.to_string())?;
    out["text"] = Value::String(t);
    done(s, out)
}

/// `board_read`.
pub(crate) async fn board_read(c: &Call<'_>, a: ReadBoardArgs) -> Out {
    owners_boards(c, READ_TOOL).await?;
    if let Some(id) = a.board.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        let v = boards_call(c, Method::GET, &agent_board(id), None).await?;
        let view: BoardView =
            serde_json::from_value(v).map_err(|e| format!("Arugula control sent a board I can't read: {e}"))?;
        let s = format!("{}; board_write changes it at once", line(&view));
        let t = text(&view);
        let mut out = serde_json::to_value(&view).map_err(|e| e.to_string())?;
        out["text"] = Value::String(t);
        return done(s, out);
    }
    let project = c.current_project(a.project.as_deref()).await?;
    let v = boards_call(c, Method::GET, &agent_project_boards(&project), None).await?;
    let list: BoardList =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent boards I can't read: {e}"))?;
    let s = match list.boards.len() {
        0 => format!("Project {} has no boards. board_write create_board makes one.", list.project),
        n => format!(
            "{n} board(s) in project {}: {}. board_read with board reads one.",
            list.project,
            list.boards.iter().map(|b| format!("{} ({})", b.title, b.id)).collect::<Vec<_>>().join("; ")
        ),
    };
    done(s, list)
}

/// `board_write`.
pub(crate) async fn board_write(c: &Call<'_>, kind: &str, a: Value) -> Out {
    owners_boards(c, WRITE_TOOL).await?;
    match kind {
        "create_board" => {
            let a: CreateBoardArgs = args(a)?;
            let project = c.current_project(a.project.as_deref()).await?;
            let new = NewBoard { title: a.title, goal: a.goal };
            let v = boards_call(c, Method::POST, &agent_project_boards(&project), Some(given(new)?)).await?;
            board_done("Made a board", v)
        }
        "edit_board" => {
            let a: EditBoardArgs = args(a)?;
            if a.title.is_none() && a.goal.is_none() {
                return Err("edit_board needs a title or a goal".into());
            }
            let v = boards_call(
                c,
                Method::PATCH,
                &agent_board(&a.board),
                Some(given(EditBoard { title: a.title, goal: a.goal })?),
            )
            .await?;
            board_done("Changed a board", v)
        }
        "close_board" => {
            let a: CloseBoardArgs = args(a)?;
            let body = given(CloseBoard { done_note: a.done_note })?;
            let v = boards_call(c, Method::POST, &agent_board_close(&a.board), Some(body)).await?;
            board_done("Closed a board", v)
        }
        "reopen_board" => {
            let a: BoardArgs = args(a)?;
            let v = boards_call(c, Method::POST, &agent_board_reopen(&a.board), None).await?;
            board_done("Opened a board again", v)
        }
        "add_row" => {
            let a: AddRowArgs = args(a)?;
            let put = PutEntry {
                entry: Some(wire::EntryKind::Row),
                note: a.note,
                pointer: a.pointer.map(Pointer::from),
                ..Default::default()
            };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Added a row", v)
        }
        "set" => {
            let a: SetRowArgs = args(a)?;
            if a.note.is_none() && a.pointer.is_none() && a.status.is_none() {
                return Err("set needs note or pointer (with rev) or status (needs_input or none)".into());
            }
            let text = a.note.is_some() || a.pointer.is_some();
            if text && a.rev.is_none() {
                return Err("a note or pointer needs the row's rev, as board_read shows it".into());
            }
            let put = PutEntry {
                id: Some(a.row),
                // A status alone needs no rev, and leaves it be.
                rev: a.rev.filter(|_| text),
                note: a.note,
                pointer: a.pointer.map(Pointer::from),
                status: a.status.map(Status::from),
                ..Default::default()
            };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Changed a row", v)
        }
        "attach" => {
            let a: AttachArgs = args(a)?;
            let role = match a.role {
                AttachRoleArg::Work => AttachRole::Work,
                AttachRoleArg::Proof => AttachRole::Proof,
            };
            let body = serde_json::to_value(AttachRow { row: a.row, pointer: a.pointer.into(), role })
                .map_err(|e| e.to_string())?;
            let v = boards_call(c, Method::POST, &agent_board_attachments(&a.board), Some(body)).await?;
            board_done("Attached", v)
        }
        "detach" => {
            let a: DetachArgs = args(a)?;
            let v = boards_call(c, Method::DELETE, &agent_board_attachment(&a.board, &a.attachment), None).await?;
            board_done("Took off an attachment", v)
        }
        "remove_row" => {
            let a: RemoveRowArgs = args(a)?;
            let v = boards_call(c, Method::DELETE, &agent_board_entry(&a.board, &a.row), None).await?;
            board_done("Removed a row", v)
        }
        _ => Err(format!("{WRITE_TOOL} kind {kind} isn't wired up")),
    }
}

async fn put_entry(c: &Call<'_>, board: &str, put: &PutEntry) -> Result<Value, String> {
    boards_call(c, Method::PUT, &agent_board_entries(board), Some(given(put)?)).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A stale write comes back as a plain "read again" and a refusal keeps
    /// control's words.
    #[test]
    fn a_409_says_to_read_again() {
        let stale = said(CloudRefused::Said {
            status: 409,
            said: "this entry is at rev 3 now: read the board again".into(),
            reason: Some("stale_rev".into()),
        });
        assert!(stale.contains("board_read again") && stale.contains("rev 3"), "{stale}");
        let no = said(CloudRefused::Said {
            status: 403,
            said: "an agent repoints only the rows its own machine added: this one is someone else's".into(),
            reason: None,
        });
        assert!(
            no.contains("repoints only the rows its own machine added") && !no.contains("board_read again"),
            "{no}"
        );
    }

    /// A board's args take only a title, and leave out what isn't given.
    #[test]
    fn a_board_is_made_from_a_title_alone() {
        let a: CreateBoardArgs = serde_json::from_value(json!({ "title": "Epic" })).unwrap();
        assert!(a.goal.is_none() && a.project.is_none());
        assert_eq!(given(NewBoard { title: a.title, goal: a.goal }).unwrap(), json!({ "title": "Epic" }));
    }

    /// What lanes refuse isn't offered: no zone, no move, and the one status.
    #[test]
    fn the_tool_offers_nothing_lanes_refuse() {
        let names: Vec<&str> = WRITE.iter().map(|k| k.name).collect();
        assert_eq!(
            names,
            [
                "add_row",
                "set",
                "attach",
                "detach",
                "remove_row",
                "create_board",
                "edit_board",
                "close_board",
                "reopen_board"
            ]
        );
        assert!(serde_json::from_value::<SetRowArgs>(json!({ "board": "b", "row": "r", "status": "done" })).is_err());
        assert!(
            serde_json::from_value::<SetRowArgs>(json!({ "board": "b", "row": "r", "status": "needs_input" })).is_ok()
        );
        assert!(serde_json::from_value::<PointerArg>(json!({ "kind": "workspace", "id": "hw_1" })).is_err());
    }

    #[test]
    fn a_pointer_arg_is_a_wire_pointer() {
        let p: PointerArg = serde_json::from_value(json!({ "kind": "block", "machine": "m1", "pane": "7" })).unwrap();
        assert_eq!(Pointer::from(p), Pointer::Block { machine: "m1".into(), pane: "7".into() });
        let l: PointerArg =
            serde_json::from_value(json!({ "kind": "link", "url": "https://ci.example.com/1" })).unwrap();
        assert_eq!(Pointer::from(l), Pointer::Link { url: "https://ci.example.com/1".into() });
        assert!(serde_json::from_value::<PointerArg>(json!({ "kind": "zone" })).is_err());
    }
}
