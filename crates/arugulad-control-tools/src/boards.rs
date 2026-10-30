//! A project's boards from agents (boards, step 3): the board in
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
//!   board or a row it makes says which machine's agent made it.
//! - **What `boards.write` may do:** make a board (with a goal and zones),
//!   set its title and goal, close it with a done note and open it again;
//!   make, rename, move and delete zones; add rows, set any row's note and
//!   status, and move rows; remove or repoint a row its own machine added.
//!   Never delete a zone that holds a row another hand added, or remove or
//!   repoint such a row: the platform refuses those (aru-123).
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
    self as wire, AGENT_BOARD_REACH, BoardList, BoardView, CloseBoard, EditBoard, MoveEntry, NewBoard, Pointer,
    PutEntry, Status, ZoneKind, agent_board, agent_board_close, agent_board_entries, agent_board_entry,
    agent_board_entry_move, agent_board_reopen, agent_project_boards,
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::{
    issues::Chosen,
    results::{BoardsProject, BoardsReach},
    tools::{Call, Kind, Out, done},
};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const READ_TOOL: &str = "board_read";
pub(crate) const WRITE_TOOL: &str = "board_write";

pub(crate) const READ_DESCRIPTION: &str = "Read a project's board in Arugula control (an epic when it has a goal; with this machine's boards.read grant over its project): a board is zones (a title and a progress bar of how many rows are done) of rows (a pointer to an issue, PR, run, workspace or agent block, or to nothing; a one-line note; a status). Returns the zones and rows with their ids and revs (what board_write names), and the board as 72-column text. With board (brd_…): that board. Without: the project's boards (project, else the one this clone's GitHub repo is linked to, else your person's personal project), to pick one from.";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to a project's board in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. Their boards.write grant over the project says you may, never more than their role. You may make a board (an epic, with a goal and its zones), set its title and goal, close it with a note saying what a person can now do, and open it again; make, rename, move and delete zones (PROOFS too); add rows, set any row's note and status (a PROOFS row's too), move rows, and remove or repoint a row you added. You may not delete a zone that holds a row someone else added, or remove or repoint such a row. A note, a pointer or a zone's title names the rev board_read showed for it; if it has changed since, this says so: board_read again and retry. A status or a move needs no rev. Every kind returns the board as it is now.";

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
pub struct CreateBoardArgs {
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
    /// One line, at most 120 characters.
    pub title: String,
    /// What the board is for, in a paragraph (at most 2000 characters): a
    /// board with a goal is an epic.
    #[serde(default)]
    pub goal: Option<String>,
    /// Zones to make, in order, by title.
    #[serde(default)]
    pub zones: Vec<String>,
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
pub struct AddZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// One line, at most 120 characters.
    pub title: String,
    /// Make it the PROOFS zone: the board's last, and it has one.
    #[serde(default)]
    pub proofs: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The zone (zon_…, from board_read).
    pub zone: String,
    /// The zone's rev from board_read.
    pub rev: u64,
    pub title: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The zone (zon_…).
    pub zone: String,
    /// Put it just before this zone (default: last, before PROOFS).
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The zone (zon_…): empty, or holding only rows you added.
    pub zone: String,
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
    /// What the row points at now, for a row you added. Needs rev.
    #[serde(default)]
    pub pointer: Option<PointerArg>,
    /// The row's rev from board_read: needed with note or pointer, not for
    /// status.
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
        description: "Set a row's note or pointer (with its rev) and/or status.",
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
    Kind {
        name: "create_board",
        description: "Make a board in a project, with a goal (an epic) and its zones in order; returns it.",
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
    Kind {
        name: "add_zone",
        description: "Add a zone to a board, before PROOFS (or make the PROOFS zone, last).",
        schema: schema_for_type::<AddZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "edit_zone",
        description: "Rename a zone (with its rev).",
        schema: schema_for_type::<EditZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "move_zone",
        description: "Move a zone among a board's zones; PROOFS stays last.",
        schema: schema_for_type::<MoveZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "delete_zone",
        description: "Delete a zone that is empty or holds only rows you added, with its rows.",
        schema: schema_for_type::<DeleteZoneArgs>,
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
    let s = format!("{what}, now, as your person's agent on this machine. {}", line(&view));
    let t = text(&view);
    let mut out = serde_json::to_value(&view).map_err(|e| e.to_string())?;
    out["text"] = Value::String(t);
    done(s, out)
}

/// The projects whose boards this machine's grants reach, for
/// `capabilities` and for `board_read` to name where else to look. None
/// (with a note) where control can't say.
pub(crate) async fn boards_reach(app: &crate::server::App) -> BoardsReach {
    let asked = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        app.control.cloud_now(Method::GET, AGENT_BOARD_REACH, None),
    );
    match asked.await {
        Ok(Ok(v)) => BoardsReach {
            projects: serde_json::from_value::<Vec<BoardsProject>>(v["projects"].clone()).unwrap_or_default(),
            note: String::new(),
        },
        Ok(Err(e)) => BoardsReach { projects: vec![], note: said(e) },
        Err(_) => BoardsReach { projects: vec![], note: "Arugula control didn't answer in time".into() },
    }
}

/// What a result says of the project a call meant; when that is the
/// personal one by default, also the projects with boards this machine
/// reaches, for the agent to pass as `project`.
async fn chosen_line(c: &Call<'_>, chosen: &Chosen) -> String {
    let reach = match chosen.is_personal_by_default() {
        true => crate::mcp::registry::boards_reach(c.app()).await.projects,
        false => vec![],
    };
    with_other_boards(chosen, &reach)
}

fn with_other_boards(chosen: &Chosen, reach: &[BoardsProject]) -> String {
    let mut s = chosen.says();
    let others: Vec<String> = reach
        .iter()
        .filter(|p| chosen.is_personal_by_default() && p.boards > 0 && p.id != chosen.id)
        .map(|p| format!("{} ({})", p.name, p.id))
        .collect();
    if !others.is_empty() {
        s += &format!(". Projects with boards (pass project): {}", others.join(", "));
    }
    s
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
    let chosen = c.chosen_project(a.project.as_deref()).await?;
    let v = boards_call(c, Method::GET, &agent_project_boards(&chosen.id), None).await?;
    let list: BoardList =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent boards I can't read: {e}"))?;
    let which = chosen_line(c, &chosen).await;
    let s = format!(
        "{which}. {}",
        match list.boards.len() {
            0 => format!("Project {} has no boards. board_write create_board makes one.", list.project),
            n => format!(
                "{n} board(s) in project {}: {}. board_read with board reads one.",
                list.project,
                list.boards.iter().map(|b| format!("{} ({})", b.title, b.id)).collect::<Vec<_>>().join("; ")
            ),
        }
    );
    done(s, list)
}

/// `board_write`.
pub(crate) async fn board_write(c: &Call<'_>, kind: &str, a: Value) -> Out {
    owners_boards(c, WRITE_TOOL).await?;
    match kind {
        "create_board" => {
            let a: CreateBoardArgs = args(a)?;
            let chosen = c.chosen_project(a.project.as_deref()).await?;
            let new = NewBoard { title: a.title, goal: a.goal };
            let v = boards_call(c, Method::POST, &agent_project_boards(&chosen.id), Some(given(new)?)).await?;
            let mut view: Value = v;
            let board = serde_json::from_value::<BoardView>(view.clone()).map(|b| b.board.id).unwrap_or_default();
            for title in a.zones {
                let put =
                    PutEntry { entry: Some(wire::EntryKind::Zone), title: Some(title.clone()), ..Default::default() };
                view = put_entry(c, &board, &put)
                    .await
                    .map_err(|e| format!("made board {board}, but not its zone {title:?}: {e}"))?;
            }
            let mut out = board_done("Made a board", view)?;
            out["summary"] =
                format!("{}. {}", chosen_line(c, &chosen).await, out["summary"].as_str().unwrap_or_default()).into();
            Ok(out)
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
        "add_zone" => {
            let a: AddZoneArgs = args(a)?;
            let put = PutEntry {
                entry: Some(wire::EntryKind::Zone),
                title: Some(a.title),
                kind: a.proofs.then_some(ZoneKind::Proofs),
                ..Default::default()
            };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Added a zone", v)
        }
        "edit_zone" => {
            let a: EditZoneArgs = args(a)?;
            let put = PutEntry { id: Some(a.zone), rev: Some(a.rev), title: Some(a.title), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Renamed a zone", v)
        }
        "move_zone" => {
            let a: MoveZoneArgs = args(a)?;
            let m = MoveEntry { rev: None, zone: None, before: a.before };
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &a.zone), Some(given(m)?)).await?;
            board_done("Moved a zone", v)
        }
        "delete_zone" => {
            let a: DeleteZoneArgs = args(a)?;
            let v = boards_call(c, Method::DELETE, &agent_board_entry(&a.board, &a.zone), None).await?;
            board_done("Deleted a zone", v)
        }
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
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Added a row", v)
        }
        "set" => {
            let a: SetRowArgs = args(a)?;
            if a.note.is_none() && a.pointer.is_none() && a.status.is_none() {
                return Err("set needs note or pointer (with rev) or status".into());
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
        "move" => {
            let a: MoveRowArgs = args(a)?;
            let m = MoveEntry { rev: a.rev, zone: a.zone, before: a.before };
            let body = serde_json::to_value(m).map_err(|e| e.to_string())?;
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &a.row), Some(body)).await?;
            board_done("Moved a row", v)
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
    use crate::mcp::issues::Rule;

    /// Falling back to the personal project also names the other projects
    /// with boards to pass; a project that was named or found says only
    /// itself, and one with no boards isn't offered.
    #[test]
    fn the_personal_fallback_names_the_projects_with_boards() {
        let project = |id: &str, name: &str, boards| BoardsProject {
            id: id.into(),
            name: name.into(),
            powers: vec!["boards.read".into()],
            boards,
        };
        let reach = [project("prj_1", "Arugula", 3), project("prj_2", "Empty", 0), project("prj_me", "Me", 1)];
        let chosen = |rule| Chosen { id: "prj_me".into(), name: "Me".into(), rule };
        let s = with_other_boards(&chosen(Rule::Personal), &reach);
        assert!(s.starts_with("Your personal project Me"), "{s}");
        assert!(s.ends_with("Projects with boards (pass project): Arugula (prj_1)"), "{s}");
        for rule in [Rule::Asked, Rule::Linked("acme/app".into())] {
            let s = with_other_boards(&chosen(rule), &reach);
            assert!(!s.contains("pass project"), "{s}");
        }
    }

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
        assert!(a.zones.is_empty() && a.goal.is_none() && a.project.is_none());
        assert_eq!(given(NewBoard { title: a.title, goal: a.goal }).unwrap(), json!({ "title": "Epic" }));
        let m = MoveEntry { rev: None, zone: None, before: Some("zon_2".into()) };
        assert_eq!(given(m).unwrap(), json!({ "before": "zon_2" }));
        assert!(
            serde_json::from_value::<EditZoneArgs>(json!({ "board": "brd_1", "zone": "zon_1", "title": "x" })).is_err()
        );
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
