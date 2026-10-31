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
//!   attached; make, rename, move and delete groups, and put a row in one or
//!   move it among them; remove or repoint a row its own machine added.
//!   Never remove or repoint a row another hand added, or delete a group
//!   that holds one: the platform refuses those (aru-123). **Nobody sets a
//!   lane:** it follows the work, and a move to one and every status are
//!   refused (aru-125). **Asking:** `ask` puts a question on a row for a
//!   person (#367); they answer on the board, and the answer is in
//!   `board_read` (a row's `asked`). An agent can't answer.
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
    self as wire, AGENT_BOARD_REACH, AttachRole, AttachRow, BoardList, BoardView, CloseBoard, EditBoard, MoveEntry,
    NewAsk, NewBoard, Pointer, PutEntry, Status, agent_board, agent_board_attachment, agent_board_attachments,
    agent_board_close, agent_board_entries, agent_board_entry, agent_board_entry_move, agent_board_reopen,
    agent_board_row_ask, agent_project_boards,
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

pub(crate) const READ_DESCRIPTION: &str = "Read a project's board in Arugula control (an epic when it has a goal; with this machine's boards.read grant over its project): a board is rows, each one piece of work: an issue or a PR, or a note, with a one-line note, and what is attached to it (work: PRs, agent blocks, runs; proof: runs, links). It is read as four lanes that follow the work, never placed by anyone: Next, Doing (an agent block that isn't closed, or a run running), Review (a PR is open) and Done (the issue is closed, or the PR merged or closed); a Done row is proven by CI (a PR merged with every check green) or by evidence, or not proven yet. Returns the lanes and rows with their ids and revs (what board_write names), and the board as 72-column text. With board (brd_…): that board. Without: the project's boards (project, else the one this clone's GitHub repo is linked to, else your person's personal project), to pick one from.";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to a project's board in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. Their boards.write grant over the project says you may, never more than their role. A board is rows, one piece of work each, and its lanes (Next, Doing, Review, Done) follow the live state of the work: you can't set a lane or a status; the platform refuses those, \"lanes follow the work\". To get a person's decision, ask them: the ask kind puts a question on a row (who, the question, why, up to four options, a recommendation); they answer on the board, and you read the answer in board_read (the row's asked, newest first), never by waiting on it. What you can set is the plan: groups, the board's named, ordered sections (phases, waves, areas), and the group each row is in. You may make a board (an epic, with a goal), set its title and goal, close it with a note saying what a person can now do, and open it again; make, rename, move and delete groups (add_zone, edit_zone, move_zone, delete_zone), and put a row in a group or out of one (add_row with zone, move); add a row for an issue or a PR (a PR that names the issue of a row is attached to that row, and an item already on the board isn't added again; the answer says so), set a row's note, attach work (a PR, an agent block or a run, which moves the lane) or proof (a run or a link, which marks a Done row proven) to a row, take off an attachment you made, and remove or repoint a row you added. You may not remove or repoint a row someone else added, or delete a group that holds one. A note, a pointer or a group's title names the rev board_read showed for it; if it has changed since, this says so: board_read again and retry. Every kind returns the board as it is now.";

/// The one status a row may be given: none, which clears an old one. A
/// question for a person is an `ask` now.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StatusArg {
    /// Clear it.
    None,
}

impl From<StatusArg> for Status {
    fn from(s: StatusArg) -> Self {
        match s {
            StatusArg::None => Status::None,
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
    /// The group to add it to, last (zon_…, from board_read). Default: none.
    #[serde(default)]
    pub zone: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group's title: one line, at most 120 characters. It goes last.
    pub title: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (zon_…, from board_read).
    pub zone: String,
    /// The group's rev from board_read.
    pub rev: u64,
    pub title: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (zon_…).
    pub zone: String,
    /// Put it just before this group (default: last).
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteZoneArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (zon_…): empty, or holding only rows you added.
    pub zone: String,
}

/// A field that is there, `null` or not, is `Some`: so `null` and absent
/// differ.
fn some<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveRowArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row (row_…, from board_read).
    pub row: String,
    /// The group to move it to (zon_…); null takes it out of its group.
    /// Default: its own. A lane is never set.
    #[serde(default, deserialize_with = "some")]
    pub zone: Option<Option<String>>,
    /// Put it just before this row of the group (default: last).
    #[serde(default)]
    pub before: Option<String>,
    /// The row's rev, if you want the move refused when it has changed.
    #[serde(default)]
    pub rev: Option<u64>,
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
    /// none, to clear an old status. Lanes follow the work: no other status
    /// is taken, and a question for a person is an ask.
    #[serde(default)]
    pub status: Option<StatusArg>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AskArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row (row_…, from board_read): one with an issue or a PR as its
    /// item, with no open ask. A note row is refused.
    pub row: String,
    /// The person it is for: an account of the project (the issues'
    /// assignees and a board's people are named in board_read).
    pub who: String,
    /// One line, at most 200 characters.
    pub question: String,
    /// Why you ask, at most 2000 characters. Folded on the card.
    #[serde(default)]
    pub why: Option<String>,
    /// Up to four answers, each at most 80 characters, as buttons. None: they
    /// type an answer.
    #[serde(default)]
    pub options: Vec<String>,
    /// One of the options, marked as what you would pick.
    #[serde(default)]
    pub recommend: Option<String>,
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
        description: "Add a row last (in a group, if you name one): an issue or a PR as its item (or a note), with a one-line note. A PR that names the issue of a row is attached to that row; an item already on the board isn't added again.",
        schema: schema_for_type::<AddRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "set",
        description: "Set a row's note or pointer (with its rev), or clear an old status.",
        schema: schema_for_type::<SetRowArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "move",
        description: "Move a row to a group (zone), or out of its group (zone: null), and set its place in it. A lane is never set.",
        schema: schema_for_type::<MoveRowArgs>,
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
        name: "ask",
        description: "Ask a person of the project a question on a row (an issue or a PR; one open ask per row), with up to four options or none for free text. They answer on the board, and the answer is the row's asked in board_read: read it there. You can't answer an ask.",
        schema: schema_for_type::<AskArgs>,
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
    Kind {
        name: "add_zone",
        description: "Add a group to a board, last: a named section (a phase, a wave, an area) that rows go in.",
        schema: schema_for_type::<AddZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "edit_zone",
        description: "Rename a group (with its rev).",
        schema: schema_for_type::<EditZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "move_zone",
        description: "Move a group among a board's groups.",
        schema: schema_for_type::<MoveZoneArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "delete_zone",
        description: "Delete a group that is empty or holds only rows you added, with its rows.",
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

/// `Title (brd_…): 5 row(s): 2 next, 1 doing, 1 review, 1 done`, and
/// `, 2 group(s)` when it has any.
fn line(v: &BoardView) -> String {
    let n = v.zones.iter().map(|z| z.rows.len()).sum::<usize>();
    let by: Vec<String> = v.zones.iter().map(|z| format!("{} {}", z.rows.len(), z.title.to_lowercase())).collect();
    let groups = if v.groups.is_empty() { String::new() } else { format!(", {} group(s)", v.groups.len()) };
    format!("{} ({}): {n} row(s): {}{groups}", v.board.title, v.board.id, by.join(", "))
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
            let mut out = board_done("Made a board", v)?;
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
        "add_row" => {
            let a: AddRowArgs = args(a)?;
            let put = PutEntry {
                entry: Some(wire::EntryKind::Row),
                zone: a.zone,
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
                return Err("set needs note or pointer (with rev) or status (none)".into());
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
            // Not `given`: a `zone` of null is the move out of a group.
            let body = serde_json::to_value(m).map_err(|e| e.to_string())?;
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &a.row), Some(body)).await?;
            board_done("Moved a row", v)
        }
        "add_zone" => {
            let a: AddZoneArgs = args(a)?;
            let put = PutEntry { entry: Some(wire::EntryKind::Zone), title: Some(a.title), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Added a group", v)
        }
        "edit_zone" => {
            let a: EditZoneArgs = args(a)?;
            let put = PutEntry { id: Some(a.zone), rev: Some(a.rev), title: Some(a.title), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Renamed a group", v)
        }
        "move_zone" => {
            let a: MoveZoneArgs = args(a)?;
            let m = MoveEntry { rev: None, zone: None, before: a.before };
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &a.zone), Some(given(m)?)).await?;
            board_done("Moved a group", v)
        }
        "delete_zone" => {
            let a: DeleteZoneArgs = args(a)?;
            let v = boards_call(c, Method::DELETE, &agent_board_entry(&a.board, &a.zone), None).await?;
            board_done("Deleted a group", v)
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
        "ask" => {
            let a: AskArgs = args(a)?;
            let body = given(NewAsk {
                who: a.who,
                question: a.question,
                why: a.why,
                options: a.options,
                recommend: a.recommend,
            })?;
            let v = boards_call(c, Method::POST, &agent_board_row_ask(&a.board, &a.row), Some(body)).await?;
            board_done("Asked", v)
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
        assert!(a.goal.is_none() && a.project.is_none());
        assert_eq!(given(NewBoard { title: a.title, goal: a.goal }).unwrap(), json!({ "title": "Epic" }));
    }

    /// What lanes refuse isn't offered: no lane, and the one status; groups
    /// are, and a move out of a group (`zone: null`) is told from none.
    #[test]
    fn the_tool_offers_nothing_lanes_refuse() {
        let names: Vec<&str> = WRITE.iter().map(|k| k.name).collect();
        assert_eq!(
            names,
            [
                "add_row",
                "set",
                "move",
                "attach",
                "detach",
                "ask",
                "remove_row",
                "create_board",
                "edit_board",
                "close_board",
                "reopen_board",
                "add_zone",
                "edit_zone",
                "move_zone",
                "delete_zone"
            ]
        );
        let out: MoveRowArgs = serde_json::from_value(json!({ "board": "b", "row": "r", "zone": null })).unwrap();
        let stay: MoveRowArgs = serde_json::from_value(json!({ "board": "b", "row": "r" })).unwrap();
        let into: MoveRowArgs = serde_json::from_value(json!({ "board": "b", "row": "r", "zone": "zon_1" })).unwrap();
        assert_eq!((out.zone, stay.zone, into.zone), (Some(None), None, Some(Some("zon_1".into()))));
        let body = |zone| serde_json::to_value(MoveEntry { rev: None, zone, before: None }).unwrap();
        assert_eq!(body(Some(None))["zone"], Value::Null, "out of its group goes out as null");
        assert!(body(None).get("zone").is_none(), "no zone: its own");
        assert!(serde_json::from_value::<SetRowArgs>(json!({ "board": "b", "row": "r", "status": "done" })).is_err());
        assert!(
            serde_json::from_value::<SetRowArgs>(json!({ "board": "b", "row": "r", "status": "needs_input" })).is_err(),
            "a question for a person is an ask"
        );
        assert!(serde_json::from_value::<SetRowArgs>(json!({ "board": "b", "row": "r", "status": "none" })).is_ok());
        assert!(serde_json::from_value::<PointerArg>(json!({ "kind": "workspace", "id": "hw_1" })).is_err());
    }

    /// An ask names who and the question, and leaves out what isn't given.
    #[test]
    fn an_ask_is_sent_as_the_wire_says() {
        let a: AskArgs = serde_json::from_value(
            json!({ "board": "b", "row": "r", "who": "acc", "question": "Ship?", "options": ["Yes", "No"] }),
        )
        .unwrap();
        let body =
            given(NewAsk { who: a.who, question: a.question, why: a.why, options: a.options, recommend: a.recommend })
                .unwrap();
        assert_eq!(body, json!({ "who": "acc", "question": "Ship?", "options": ["Yes", "No"] }));
        assert!(serde_json::from_value::<AskArgs>(json!({ "board": "b", "row": "r", "question": "Ship?" })).is_err());
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
