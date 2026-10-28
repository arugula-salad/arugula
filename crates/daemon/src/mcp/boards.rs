//! A project's live status board from agents (boards, step 3): the board in
//! Arugula control (`brd_…`, an ordered list of zones of rows), read with
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
//!   row it adds says which machine's agent added it.
//! - **What `boards.write` may do:** add rows to the zones there are, set
//!   any row's note and status, and move rows; remove a row its own machine
//!   added. Never make a board, or make, rename, move or delete a zone, or
//!   remove a row another hand added: the platform refuses those.
//! - **Revs.** A row's `rev` counts changes to its note and pointer. A
//!   write of a note names the `rev` that `board_read` showed; if the row
//!   moved on, the platform answers 409 and this says to read again. A
//!   status change and a move need none.
//! - **Which project** a list means is the issues' rule: `project`, else
//!   the one this clone's GitHub repo is linked to, else the owner's
//!   personal project.
//!
//! The text form of a board (72 columns) is `arugula_control_wire::boards::
//! render`. An agent a guest stands behind may not use these: they act as
//! the owner.

use arugula_control_wire::boards::{
    self as wire, BoardList, BoardView, MoveEntry, Pointer, PutEntry, Status, agent_board, agent_board_entries,
    agent_board_entry, agent_board_entry_move, agent_project_boards,
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::tools::{Call, Kind, Out, done};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const READ_TOOL: &str = "board_read";
pub(crate) const WRITE_TOOL: &str = "board_write";

pub(crate) const READ_DESCRIPTION: &str = "Read a project's live status board in Arugula control (with this machine's boards.read grant over its project): a board is zones (a title and a progress bar of how many rows are done) of rows (a pointer to an issue, PR, run, workspace or agent block, or to nothing; a one-line note; a status). Returns the zones and rows with their ids and revs (what board_write names), and the board as 72-column text. With board (brd_…): that board. Without: the project's boards (project, else the one this clone's GitHub repo is linked to, else your person's personal project), to pick one from.";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to a project's live status board in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. Their boards.write grant over the project says you may, never more than their role. You may add rows to the zones there are, set any row's note and status (a PROOFS row's too), move rows, and remove a row you added; you may not make boards or zones, rename, move or delete a zone, repoint a row, or remove a row someone else added. A note names the rev board_read showed for its row; if it has changed since, this says so: board_read again and retry. A status or a move needs no rev. Every kind returns the board as it is now.";

/// Where a row stands.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StatusArg {
    None,
    Open,
    Working,
    NeedsInput,
    Done,
    Failed,
}

impl From<StatusArg> for Status {
    fn from(s: StatusArg) -> Self {
        match s {
            StatusArg::None => Status::None,
            StatusArg::Open => Status::Open,
            StatusArg::Working => Status::Working,
            StatusArg::NeedsInput => Status::NeedsInput,
            StatusArg::Done => Status::Done,
            StatusArg::Failed => Status::Failed,
        }
    }
}

/// What a row points at: nothing, or one thing.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointerArg {
    /// Free text: no pointer.
    None,
    /// A mirrored GitHub PR or issue (fi_…).
    ForgeItem { id: String },
    /// A native issue (iss_…).
    Issue { id: String },
    /// A run (run_…).
    Run { id: String },
    /// A Cloud workspace (hw_…).
    Workspace { id: String },
    /// An agent block on a machine.
    Block { machine: String, pane: String },
    /// An https:// URL as evidence (a CI page, a report): no user name or
    /// password in it, at most 2048 bytes. On a PROOFS row it counts as proven.
    Link { url: String },
}

impl From<PointerArg> for Pointer {
    fn from(p: PointerArg) -> Self {
        match p {
            PointerArg::None => Pointer::None,
            PointerArg::ForgeItem { id } => Pointer::ForgeItem { id },
            PointerArg::Issue { id } => Pointer::Issue { id },
            PointerArg::Run { id } => Pointer::Run { id },
            PointerArg::Workspace { id } => Pointer::Workspace { id },
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
pub struct AddRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// The zone to add it to, last (zon_…, from board_read).
    pub zone: String,
    /// One line, at most 280 characters.
    #[serde(default)]
    pub note: Option<String>,
    /// open, working, needs_input, done, failed or none. Default none.
    #[serde(default)]
    pub status: Option<StatusArg>,
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
    /// The row's rev from board_read: needed with note, not for status.
    #[serde(default)]
    pub rev: Option<u64>,
    /// open, working, needs_input, done, failed, or none to clear it.
    #[serde(default)]
    pub status: Option<StatusArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row (row_…).
    pub row: String,
    /// The zone to move it to (default: its own).
    #[serde(default)]
    pub zone: Option<String>,
    /// Put it just before this row of the zone (default: last).
    #[serde(default)]
    pub before: Option<String>,
    /// The rev it must still have (optional).
    #[serde(default)]
    pub rev: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RemoveRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// A row you added (row_…).
    pub row: String,
}

pub(crate) const WRITE: &[Kind] = &[
    Kind {
        name: "add_row",
        description: "Add a row last in a zone: a note, a status and what it points at.",
        schema: schema_for_type::<AddRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "set",
        description: "Set a row's note (with its rev) and/or status.",
        schema: schema_for_type::<SetRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "move",
        description: "Move a row within its zone or to another.",
        schema: schema_for_type::<MoveRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "remove_row",
        description: "Remove a row you added.",
        schema: schema_for_type::<RemoveRowArgs>,
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

/// `Title (brd_…): 2 zones, 5 rows, 2 done`.
fn line(v: &BoardView) -> String {
    let rows: Vec<_> = v.zones.iter().flat_map(|z| &z.rows).collect();
    let done = rows.iter().filter(|r| r.status == Status::Done).count();
    format!("{} ({}): {} zone(s), {} row(s), {done} done", v.board.title, v.board.id, v.zones.len(), rows.len())
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    async fn owners_boards(&self, what: &str) -> Result<(), String> {
        crate::lab_flags::flag(self.app().control.state_dir(), arugula_proto::flags::BOARDS)?;
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{what} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    async fn boards_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
    }

    /// The board the platform answers a write with, as the tool's result.
    fn board_done(&self, what: &str, v: Value) -> Out {
        let view: BoardView =
            serde_json::from_value(v).map_err(|e| format!("Arugula control sent a board I can't read: {e}"))?;
        let s = format!("{what}, now, as your person's agent on this machine. {}", line(&view));
        let t = text(&view);
        let mut out = serde_json::to_value(&view).map_err(|e| e.to_string())?;
        out["text"] = Value::String(t);
        done(s, out)
    }

    /// `board_read`.
    pub(crate) async fn board_read(&self, a: ReadBoardArgs) -> Out {
        self.owners_boards(READ_TOOL).await?;
        if let Some(id) = a.board.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            let v = self.boards_call(Method::GET, &agent_board(id), None).await?;
            let view: BoardView =
                serde_json::from_value(v).map_err(|e| format!("Arugula control sent a board I can't read: {e}"))?;
            let s = format!("{}; board_write changes it at once", line(&view));
            let t = text(&view);
            let mut out = serde_json::to_value(&view).map_err(|e| e.to_string())?;
            out["text"] = Value::String(t);
            return done(s, out);
        }
        let project = self.current_project(a.project.as_deref()).await?;
        let v = self.boards_call(Method::GET, &agent_project_boards(&project), None).await?;
        let list: BoardList =
            serde_json::from_value(v).map_err(|e| format!("Arugula control sent boards I can't read: {e}"))?;
        let s = match list.boards.len() {
            0 => format!("Project {} has no boards. A person makes one on the platform page.", list.project),
            n => format!(
                "{n} board(s) in project {}: {}. board_read with board reads one.",
                list.project,
                list.boards.iter().map(|b| format!("{} ({})", b.title, b.id)).collect::<Vec<_>>().join("; ")
            ),
        };
        done(s, list)
    }

    /// `board_write`.
    pub(crate) async fn board_write(&self, kind: &str, a: Value) -> Out {
        self.owners_boards(WRITE_TOOL).await?;
        match kind {
            "add_row" => {
                let a: AddRowArgs = args(a)?;
                let put = PutEntry {
                    entry: Some(wire::EntryKind::Row),
                    zone: Some(a.zone),
                    note: a.note,
                    status: a.status.map(Status::from),
                    pointer: a.pointer.map(Pointer::from),
                    ..Default::default()
                };
                let v = self.put(&a.board, &put).await?;
                self.board_done("Added a row", v)
            }
            "set" => {
                let a: SetRowArgs = args(a)?;
                if a.note.is_none() && a.status.is_none() {
                    return Err("set needs note (with rev) or status".into());
                }
                if a.note.is_some() && a.rev.is_none() {
                    return Err("a note needs the row's rev, as board_read shows it".into());
                }
                let put = PutEntry {
                    id: Some(a.row),
                    // A status alone needs no rev, and leaves it be.
                    rev: a.note.as_ref().and(a.rev),
                    note: a.note,
                    status: a.status.map(Status::from),
                    ..Default::default()
                };
                let v = self.put(&a.board, &put).await?;
                self.board_done("Changed a row", v)
            }
            "move" => {
                let a: MoveRowArgs = args(a)?;
                let m = MoveEntry { rev: a.rev, zone: a.zone, before: a.before };
                let body = serde_json::to_value(m).map_err(|e| e.to_string())?;
                let v = self.boards_call(Method::POST, &agent_board_entry_move(&a.board, &a.row), Some(body)).await?;
                self.board_done("Moved a row", v)
            }
            "remove_row" => {
                let a: RemoveRowArgs = args(a)?;
                let v = self.boards_call(Method::DELETE, &agent_board_entry(&a.board, &a.row), None).await?;
                self.board_done("Removed a row", v)
            }
            _ => Err(format!("{WRITE_TOOL} kind {kind} isn't wired up")),
        }
    }

    async fn put(&self, board: &str, put: &PutEntry) -> Result<Value, String> {
        let body = serde_json::to_value(put).map_err(|e| e.to_string())?;
        // Only what is given.
        let mut body = body;
        if let Some(o) = body.as_object_mut() {
            o.retain(|_, v| !v.is_null());
        }
        self.boards_call(Method::PUT, &agent_board_entries(board), Some(body)).await
    }
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
        let no = said(CloudRefused::Said { status: 403, said: "an agent doesn't make zones".into(), reason: None });
        assert!(no.contains("an agent doesn't make zones") && !no.contains("board_read again"), "{no}");
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
