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
//! - **What `boards.write` may do (#366):** `add` issues and PRs as rows, in
//!   one call (a PR that names the issue of a row is attached to that row
//!   instead), `attach` work (a PR, an agent block, a run) and proof (a run,
//!   a link) to a row and `detach` what it attached, `note` a row, `ask` a
//!   person, `drop` a row it added, put a row in a `group`, and make, rename,
//!   move and delete groups; make a board, set its title and goal, close it
//!   with a done note and open it again. Never remove or repoint a row
//!   another hand added, or delete a group that holds one: the platform
//!   refuses those (aru-123). **Nobody sets a lane:** it follows the work
//!   (aru-125). An agent can't answer an ask.
//! - **Rows are addressed by their item** (`#N`, `owner/repo#N`, `IDENT-N`,
//!   an `iss_`/`fi_` id, or a `row_` id), resolved against the board's rows:
//!   their primary items first, then what is attached. A reference that
//!   names two rows (`#5` a native issue and a GitHub issue) is refused with
//!   both named. Items that aren't on the board yet are resolved by control
//!   (`AGENT_BOARD_RESOLVE`).
//! - **Batches** go one item at a time, so one that fails (nothing by that
//!   number, a refusal) doesn't stop the rest; the answer says which.
//! - **Short answers.** A write answers with what changed, the changed rows'
//!   cards and the board's progress, in about twenty lines whatever the
//!   board's size. The whole board only with `verbose: true`, and then the
//!   JSON once.
//! - **Revs.** A row's `rev` counts changes to its note and pointer. A note
//!   that names a `rev` which has moved on is not written: the answer is the
//!   row's current note.
//! - **Which project** a list means is the issues' rule: `project`, else
//!   the one this clone's GitHub repo is linked to, else the owner's
//!   personal project.
//!
//! The text form of a board (72 columns) is `arugula_control_wire::boards::
//! render`. An agent a guest stands behind may not use these: they act as
//! the owner.

use arugula_control_wire::boards::{
    self as wire, AGENT_BOARD_REACH, AttachRole, AttachRow, BoardList, BoardView, CloseBoard, EditBoard, IssueRef,
    Live, MoveEntry, NewAsk, NewBoard, NewRelation, Pointer, PutEntry, ResolveItems, Resolved, Row, agent_board,
    agent_board_attachment, agent_board_attachments, agent_board_close, agent_board_entries, agent_board_entry,
    agent_board_entry_move, agent_board_reopen, agent_board_resolve, agent_board_row_ask, agent_project_boards,
    agent_project_relation, agent_project_relations,
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

pub(crate) const READ_DESCRIPTION: &str = "Read a project's board in Arugula control (an epic when it has a goal; with this machine's boards.read grant over its project). One row is one piece of work: an issue (or a PR with no issue), or a note, with a one-line note and what is attached to it (work: PRs, agent blocks, runs; proof: runs, links). The lanes follow the work and nobody sets them: Next, Doing (an agent block that isn't closed, or a run running), Review (a PR is open) and Done (the issue is closed, or the PR merged or closed); a Done row is proven by CI or by evidence, or not proven yet. Groups are the plan's phases or waves. Returns the board as 72-column text, each row by its item (#N, IDENT-N), and its progress; with verbose: true, also the JSON with ids and revs. With board (brd_…): that board. Without: the project's boards (project, else the one this clone's GitHub repo is linked to, else your person's personal project), to pick one from.";

pub(crate) const WRITE_DESCRIPTION: &str = "Write to a project's board in Arugula control, at once: what you write lands now, labelled as your person's agent on this machine, never as them. Their boards.write grant over the project says you may, never more than their role. One row is one piece of work, addressed by its item (#N, owner/repo#N, IDENT-N or an iss_/fi_ id), never by a row id. The lanes (Next, Doing, Review, Done) follow the live state of the work: there are no statuses, and you can't set a lane. Add issues with add (up to 50 in one call; each is a row, or is attached or already there, and the answer says which). A PR or an agent block is not a row of its own: attach it to the row of its issue (a PR that names that issue is attached for you). When you need a person's decision, ask with options (ask: who, the question, up to four options, a recommendation, why); they answer on the board and you read the answer in board_read, never by waiting. Groups are your phases or waves: add_group, rename_group, move_group, delete_group, and group puts a row in one. note sets a row's one-line note (pass the rev to refuse a stale write), drop removes a row you added, detach takes off an attachment you made. To say what blocks what, relate two items (relate: the item blocks, relates to, duplicates or was discovered from another; only blocks gates the board's Ready line, and a blocked row stays in its lane and says what blocks it) and unrelate one your machine made; relate needs your person's issues.write grant over the project too. You may also make a board (an epic, with a goal), set its title and goal, close it with a note saying what a person can now do, and open it again. A write answers in a few lines: what changed, the rows' cards and the board's progress; pass verbose: true for the whole board. If one item of a batch fails, the others still land and the answer says which failed and why.";

/// The most a batch takes.
const BATCH_MAX: usize = wire::RESOLVE_MAX;
/// The most item lines an answer lists; the rest are counted.
const LINES_MAX: usize = 14;

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
    /// Also the JSON: ids, revs and every attachment. Default: the text and
    /// the progress.
    #[serde(default)]
    pub verbose: bool,
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
    /// The whole board in the answer. Default: a few lines.
    #[serde(default)]
    pub verbose: bool,
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
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct CloseBoardArgs {
    /// The board (brd_…).
    pub board: String,
    /// What a person can now do, not empty.
    pub done_note: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct BoardArgs {
    /// The board (brd_…).
    pub board: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddArgs {
    /// The board (brd_…).
    pub board: String,
    /// Up to 50 issues or PRs: #N (a native issue, else a GitHub one),
    /// owner/repo#N, IDENT-N (Plane), or an iss_… / fi_… id. Each becomes a
    /// row, or is attached to the row of the issue it names (a PR), or is
    /// there already. One that fails doesn't stop the rest.
    pub items: Vec<String>,
    /// The group to put new rows in (its title, or zon_…). Default: none.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct AttachArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row, by its item: #N, owner/repo#N, IDENT-N, or an iss_/fi_ id.
    pub item: String,
    /// What to attach: a PR (#N, owner/repo#N or fi_…), a run (run_…), an
    /// https:// link, or an agent block (block:MACHINE/PANE, the pane as %N or N).
    pub to: Vec<String>,
    /// work moves the row's lane (PRs, blocks and runs are work, the
    /// default); proof marks a Done row proven (a link is proof, the
    /// default, and so may be a run).
    #[serde(default)]
    pub role: Option<AttachRoleArg>,
    #[serde(default)]
    pub verbose: bool,
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
    /// The row, by its item.
    pub item: String,
    /// What to take off: what it was attached as (#N, run_…, the link) or
    /// its att_… id. One you made.
    pub what: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct AskArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row, by its item (an issue or a PR; one open ask per row). A note
    /// row is refused.
    pub item: String,
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
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct RelateArgs {
    /// The board (brd_…): its project is the one the items are in.
    pub board: String,
    /// The first item of the relation: an issue or PR of the project, as
    /// #N, owner/repo#N, IDENT-N, or an iss_… / fi_… id. It is not a row.
    pub item: String,
    /// The second item, written the same way. `item` blocks, relates to,
    /// duplicates or was discovered from it.
    pub to: String,
    /// blocks (the first must be closed before the second can start),
    /// relates, duplicate_of or discovered_from. Only blocks gates.
    pub kind: RelationKindArg,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RelationKindArg {
    Blocks,
    Relates,
    DuplicateOf,
    DiscoveredFrom,
}

impl From<RelationKindArg> for wire::RelationKind {
    fn from(k: RelationKindArg) -> Self {
        match k {
            RelationKindArg::Blocks => wire::RelationKind::Blocks,
            RelationKindArg::Relates => wire::RelationKind::Relates,
            RelationKindArg::DuplicateOf => wire::RelationKind::DuplicateOf,
            RelationKindArg::DiscoveredFrom => wire::RelationKind::DiscoveredFrom,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct NoteArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row, by its item.
    pub item: String,
    /// The new note, one line, at most 280 characters.
    pub text: String,
    /// The row's rev (board_read verbose, or the last write's card). If the
    /// row has changed since, nothing is written and the answer is its
    /// current note. Without it, the note is written.
    #[serde(default)]
    pub rev: Option<u64>,
    #[serde(default)]
    pub verbose: bool,
}

/// A field that is there, `null` or not, is `Some`: so `null` and absent
/// differ.
fn some<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize, JsonSchema)]
pub struct GroupArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row, by its item.
    pub item: String,
    /// The group to put it in (its title, or zon_…); null takes it out of
    /// its group.
    #[serde(default, deserialize_with = "some")]
    #[schemars(required, with = "Option<String>")]
    pub group: Option<Option<String>>,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct DropArgs {
    /// The board (brd_…).
    pub board: String,
    /// The row, by its item: one you added.
    pub item: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddGroupArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group's title: one line, at most 120 characters. It goes last.
    pub title: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct RenameGroupArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (its title, or zon_…).
    pub group: String,
    pub title: String,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveGroupArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (its title, or zon_…).
    pub group: String,
    /// Put it just before this group (default: last).
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteGroupArgs {
    /// The board (brd_…).
    pub board: String,
    /// The group (its title, or zon_…): empty, or holding only rows you
    /// added.
    pub group: String,
    #[serde(default)]
    pub verbose: bool,
}

pub(crate) const WRITE: &[Kind] = &[
    Kind {
        name: "add",
        description: "Add issues or PRs to a board as rows, up to 50 in one call: #N, owner/repo#N, IDENT-N or an iss_/fi_ id. Each is a row, or is attached to the row of the issue it names (a PR), or is there already; one that fails doesn't stop the rest. Optionally into a group.",
        schema: schema_for_type::<AddArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "attach",
        description: "Attach work (a PR, an agent block or a run) or proof (a run or a link) to the row of an item, by the item (#N), not a row id.",
        schema: schema_for_type::<AttachArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "detach",
        description: "Take off an attachment you made, from the row of an item.",
        schema: schema_for_type::<DetachArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "ask",
        description: "Ask a person of the project a question on the row of an item (an issue or a PR; one open ask per row), with up to four options or none for free text. They answer on the board, and the answer is the row's asked in board_read: read it there. You can't answer an ask.",
        schema: schema_for_type::<AskArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "relate",
        description: "Relate two items of the board's project: item blocks, relates to, duplicates (duplicate_of) or was discovered from (discovered_from) to. A blocked row stays in its lane, says what blocks it, and is left out of board_read's Ready line until the blocker is closed. A blocks cycle is refused. Needs the issues.write grant too.",
        schema: schema_for_type::<RelateArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "unrelate",
        description: "Take off a relation your machine made (the same item, to and kind as relate). One a person made, or that comes from Plane, stays. Found through a row of the board that has item or to as its item.",
        schema: schema_for_type::<RelateArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "note",
        description: "Set the one-line note of the row of an item. With rev, a row that changed since is not written and the answer is its current note.",
        schema: schema_for_type::<NoteArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "group",
        description: "Put the row of an item in a group (a phase, a wave), or out of its group (group: null).",
        schema: schema_for_type::<GroupArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "drop",
        description: "Remove the row of an item that you added.",
        schema: schema_for_type::<DropArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "add_group",
        description: "Add a group to a board, last: a named section (a phase, a wave, an area) that rows go in.",
        schema: schema_for_type::<AddGroupArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "rename_group",
        description: "Rename a group.",
        schema: schema_for_type::<RenameGroupArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "move_group",
        description: "Move a group among a board's groups.",
        schema: schema_for_type::<MoveGroupArgs>,
        flag: Some(arugula_proto::flags::BOARDS),
    },
    Kind {
        name: "delete_group",
        description: "Delete a group that is empty or holds only rows you added, with its rows.",
        schema: schema_for_type::<DeleteGroupArgs>,
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

/// `Title (brd_…): 3/11 done, 2 proven, 2 group(s)`; for a board with no
/// issue or PR yet, `no issues yet`.
/// How many rows can start now (the board's ready set).
fn ready_line(v: &BoardView) -> String {
    format!("{} ready.", v.ready.len())
}

fn progress(v: &BoardView) -> String {
    let (b, p) = (&v.board, v.board.progress);
    let state = match p.total {
        0 => "no issues yet".to_owned(),
        _ if p.done == 0 => format!("0/{} done", p.total),
        _ => format!("{}/{} done, {} proven", p.done, p.total, p.proven),
    };
    let groups = if v.groups.is_empty() { String::new() } else { format!(", {} group(s)", v.groups.len()) };
    let closed = if b.closed_at.is_some() { ", closed" } else { "" };
    format!("{} ({}): {state}{groups}{closed}", b.title, b.id)
}

/// `s` on one line, at most `max` characters, ending in `…` if cut.
fn short(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
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

/// A board as the platform sends it.
fn view_of(v: Value) -> Result<BoardView, String> {
    serde_json::from_value(v).map_err(|e| format!("Arugula control sent a board I can't read: {e}"))
}

async fn read_view(c: &Call<'_>, board: &str) -> Result<BoardView, String> {
    view_of(boards_call(c, Method::GET, &agent_board(board.trim()), None).await?)
}

/// The answer to a call: `summary` (its lines), and with `verbose` the board
/// as JSON, once, and its text.
fn answer(summary: String, view: &BoardView, verbose: bool) -> Out {
    if !verbose {
        return done(summary, serde_json::json!({}));
    }
    let mut out = serde_json::to_value(view).map_err(|e| e.to_string())?;
    out["text"] = Value::String(text(view));
    done(summary, out)
}

// ---------------------------------------------------------------- rows

/// The rows of a board, in its lanes' order.
fn rows(v: &BoardView) -> impl Iterator<Item = &Row> {
    v.zones.iter().flat_map(|z| z.rows.iter())
}

fn pointer_id(p: &Pointer) -> Option<&str> {
    match p {
        Pointer::ForgeItem { id } | Pointer::Issue { id } | Pointer::Run { id } | Pointer::Workspace { id } => Some(id),
        _ => None,
    }
}

/// Whether an item as written, its state `live`, is `want`.
fn names(want: &IssueRef, live: Option<&Live>) -> bool {
    let Some(l) = live.filter(|l| l.number > 0) else { return false };
    match want {
        IssueRef::Number(n) => l.number == *n,
        IssueRef::Repo { repo, number } => {
            l.number == *number
                && l.repo.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(repo))
                && l.provider.is_none()
        }
        IssueRef::Plane { key, number } => {
            l.number == *number && l.identifier.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(key))
        }
    }
}

/// What an item is, for a refusal that names candidates.
fn describe(r: &Row) -> String {
    match &r.live {
        Some(l) if l.kind == "native" => format!("the native issue #{}", l.number),
        Some(l) if l.number > 0 => match (&l.repo, &l.identifier) {
            (Some(repo), None) => format!("{repo}#{}", l.number),
            _ => r.item_label(),
        },
        _ => r.item_label(),
    }
}

/// The row an item names on the board: by its primary item first, then by
/// what is attached. A reference that names two rows at one level is
/// refused, naming both.
fn find_row<'a>(v: &'a BoardView, item: &str) -> Result<&'a Row, String> {
    let item = item.trim();
    if item.starts_with("row_") {
        return rows(v).find(|r| r.id == item).ok_or_else(|| format!("no row {item} on this board"));
    }
    let want = IssueRef::parse(item);
    if want.is_none() && !item.starts_with("iss_") && !item.starts_with("fi_") && !item.starts_with("run_") {
        return Err(format!("{item:?} isn't an item: write #N, owner/repo#N, IDENT-N or an iss_/fi_ id"));
    }
    let same = |p: &Pointer, live: Option<&Live>| match &want {
        Some(w) => matches!(p, Pointer::Issue { .. } | Pointer::ForgeItem { .. }) && names(w, live),
        None => pointer_id(p) == Some(item),
    };
    let primary: Vec<&Row> = rows(v).filter(|r| same(&r.pointer, r.live.as_ref())).collect();
    let attached: Vec<&Row> =
        rows(v).filter(|r| r.attachments.iter().any(|a| same(&a.pointer, a.live.as_ref()))).collect();
    for found in [primary, attached] {
        match found.as_slice() {
            [] => {}
            [one] => return Ok(one),
            many => {
                let names: Vec<String> = many.iter().map(|r| describe(r)).collect();
                return Err(format!(
                    "{item} names more than one row on this board ({}): write owner/repo#N, IDENT-N or the iss_/fi_ id of the one you mean",
                    names.join(" and ")
                ));
            }
        }
    }
    Err(format!("no row on this board has {item} (add puts it on)"))
}

/// A group by its title (any case) or `zon_…` id.
fn find_group<'a>(v: &'a BoardView, group: &str) -> Result<&'a wire::Group, String> {
    let group = group.trim();
    v.groups
        .iter()
        .find(|g| g.id == group)
        .or_else(|| v.groups.iter().find(|g| g.title.eq_ignore_ascii_case(group)))
        .ok_or_else(|| {
            let titles: Vec<&str> = v.groups.iter().map(|g| g.title.as_str()).collect();
            match titles.is_empty() {
                true => format!("this board has no group {group:?}: add_group makes one"),
                false => format!("no group {group:?} on this board (its groups: {})", titles.join(", ")),
            }
        })
}

/// A row's card, for an answer: its item, lane, rev and note; its marks; its
/// open ask.
fn card(v: &BoardView, r: &Row) -> Vec<String> {
    let group = r.group.as_deref().and_then(|g| v.groups.iter().find(|x| x.id == g));
    let mut head = format!("{} {} {} · rev {}", r.item_label(), r.lane.glyph(), r.lane.title(), r.rev);
    if let Some(g) = group {
        head += &format!(" · in {}", g.title);
    }
    if !r.note.is_empty() {
        head += &format!(" · {}", short(&r.note, 60));
    }
    let mut out = vec![head];
    if !r.marks.is_empty() {
        out.push(format!("  {}", wire::marks_line(&r.marks)));
    }
    if let Some(a) = &r.ask {
        let by = if a.raised { "the board".to_owned() } else { format!("for {}", a.who_name) };
        out.push(format!("  ? {} ({by})", short(&a.question, 70)));
    }
    out
}

/// A row of the new board by its row id, for its card.
fn card_of(v: &BoardView, row: &str) -> Vec<String> {
    rows(v).find(|r| r.id == row).map(|r| card(v, r)).unwrap_or_default()
}

/// The row in `v` whose primary item is `p`, or that has `p` attached.
fn row_with<'a>(v: &'a BoardView, p: &Pointer) -> (Option<&'a Row>, Option<&'a Row>) {
    (rows(v).find(|r| &r.pointer == p), rows(v).find(|r| r.attachments.iter().any(|a| &a.pointer == p)))
}

/// What a batch did, item by item.
#[derive(Default)]
struct Tally {
    /// (what happened, how many) in the order first seen.
    counts: Vec<(&'static str, usize)>,
    /// (failed, the item's line).
    lines: Vec<(bool, String)>,
}

impl Tally {
    fn did(&mut self, what: &'static str, line: String) {
        self.count(what);
        self.lines.push((what == "failed", line));
    }

    fn count(&mut self, what: &'static str) {
        match self.counts.iter_mut().find(|(w, _)| *w == what) {
            Some((_, n)) => *n += 1,
            None => self.counts.push((what, 1)),
        }
    }

    /// `Added 9, attached 1, failed 1`, then the lines: failures first, at
    /// most [`LINES_MAX`] of them, and the rest counted.
    fn lines(&self, verb: &str) -> Vec<String> {
        let counts: Vec<String> = self.counts.iter().map(|(w, n)| format!("{n} {w}")).collect();
        let mut out = vec![format!("{verb}: {}.", counts.join(", "))];
        let (failed, rest): (Vec<_>, Vec<_>) = self.lines.iter().partition(|(f, _)| *f);
        let ordered: Vec<&String> = failed.into_iter().chain(rest).map(|(_, l)| l).collect();
        out.extend(ordered.iter().take(LINES_MAX).map(|l| format!("  {l}")));
        if ordered.len() > LINES_MAX {
            out.push(format!("  … and {} more", ordered.len() - LINES_MAX));
        }
        out
    }
}

// ---------------------------------------------------------------- items

/// `items` as control resolves them (one call), in order.
async fn resolve(c: &Call<'_>, board: &str, items: &[String]) -> Result<Resolved, String> {
    let body = serde_json::to_value(ResolveItems { items: items.to_vec() }).map_err(|e| e.to_string())?;
    let v = boards_call(c, Method::POST, &agent_board_resolve(board), Some(body)).await?;
    serde_json::from_value(v).map_err(|e| format!("Arugula control sent references I can't read: {e}"))
}

/// What an `attach` target is: the pointer and the role it is attached as by
/// default, or the references control must resolve.
enum Target {
    Done(Pointer, AttachRole),
    Item(String),
}

fn target(to: &str) -> Result<Target, String> {
    let to = to.trim();
    if let Some(rest) = to.strip_prefix("block:") {
        let (machine, pane) = rest
            .split_once('/')
            .filter(|(m, p)| !m.is_empty() && !p.is_empty())
            .ok_or_else(|| format!("{to:?}: an agent block is block:MACHINE/PANE"))?;
        return Ok(Target::Done(
            Pointer::Block { machine: machine.into(), pane: pane.trim().trim_start_matches('%').into() },
            AttachRole::Work,
        ));
    }
    if to.starts_with("run_") {
        return Ok(Target::Done(Pointer::Run { id: to.into() }, AttachRole::Work));
    }
    if to.starts_with("https://") {
        return Ok(Target::Done(Pointer::Link { url: to.into() }, AttachRole::Proof));
    }
    Ok(Target::Item(to.into()))
}

/// The `iss_` or `fi_` ids `item` and `to` name, in the board's project.
async fn relation_ends(c: &Call<'_>, board: &str, item: &str, to: &str) -> Result<(String, String), String> {
    let resolved = resolve(c, board, &[item.to_owned(), to.to_owned()]).await?;
    let mut ends = vec![];
    for (what, r) in [item, to].into_iter().zip(resolved.items) {
        match r.pointer {
            Some(Pointer::Issue { id } | Pointer::ForgeItem { id }) => ends.push(id),
            _ => {
                return Err(format!(
                    "{what}: {}",
                    r.why.unwrap_or_else(|| "control didn't resolve it to an issue or PR".into())
                ));
            }
        }
    }
    let to = ends.pop().unwrap_or_default();
    Ok((ends.pop().unwrap_or_default(), to))
}

/// The id of the relation `from` `kind` `to` that a row of the board shows.
fn relation_between(v: &BoardView, from: &str, to: &str, kind: wire::RelationKind) -> Option<String> {
    rows(v).find_map(|r| {
        let me = pointer_id(&r.pointer)?;
        r.relations.iter().find_map(|x| {
            let other = x.item.as_deref()?;
            let (a, b) = if x.direction == wire::RelationDirection::Out { (me, other) } else { (other, me) };
            (a == from && b == to && x.kind == kind).then(|| x.id.clone())
        })
    })
}

async fn add(c: &Call<'_>, a: AddArgs) -> Out {
    if a.items.is_empty() || a.items.len() > BATCH_MAX {
        return Err(format!("add takes 1 to {BATCH_MAX} items, not {}", a.items.len()));
    }
    let mut view = read_view(c, &a.board).await?;
    let group = a.group.as_deref().map(|g| find_group(&view, g).map(|g| g.id.clone())).transpose()?;
    let resolved = resolve(c, &a.board, &a.items).await?;
    let mut tally = Tally::default();
    for (item, r) in a.items.iter().zip(resolved.items) {
        let Some(pointer) = r.pointer else {
            tally.did(
                "failed",
                format!("{item}: failed, {}", r.why.unwrap_or_else(|| "control didn't resolve it".into())),
            );
            continue;
        };
        let put = PutEntry {
            entry: Some(wire::EntryKind::Row),
            zone: group.clone(),
            pointer: Some(pointer.clone()),
            ..Default::default()
        };
        let body = given(put)?;
        match boards_call(c, Method::PUT, &agent_board_entries(&a.board), Some(body)).await.and_then(view_of) {
            Err(why) => tally.did("failed", format!("{item}: failed, {why}")),
            Ok(after) => {
                match (row_with(&view, &pointer), row_with(&after, &pointer)) {
                    ((Some(r), _), _) => {
                        tally.did("already there", format!("{item}: already on the board as {}", r.item_label()))
                    }
                    ((None, Some(r)), _) => {
                        tally.did("already there", format!("{item}: already attached to {}", r.item_label()))
                    }
                    (_, (Some(r), _)) => {
                        let took = after.said.as_deref().filter(|s| s.contains("PR row(s)")).map(|s| format!("; {s}"));
                        tally.did(
                            "added",
                            format!("{item}: added, {}{}", card(&after, r).remove(0), took.unwrap_or_default()),
                        )
                    }
                    (_, (None, Some(r))) => {
                        tally.did("attached", format!("{item}: attached to {} ({})", r.item_label(), r.lane.title()))
                    }
                    _ => tally.did("failed", format!("{item}: control answered, but it isn't on the board")),
                }
                view = after;
            }
        }
    }
    let mut lines = tally.lines("Added");
    lines.push(progress(&view));
    answer(lines.join("\n"), &view, a.verbose)
}

async fn attach(c: &Call<'_>, a: AttachArgs) -> Out {
    if a.to.is_empty() || a.to.len() > BATCH_MAX {
        return Err(format!("attach takes 1 to {BATCH_MAX} things, not {}", a.to.len()));
    }
    let mut view = read_view(c, &a.board).await?;
    let row = find_row(&view, &a.item)?.clone();
    let targets: Vec<Result<Target, String>> = a.to.iter().map(|t| target(t)).collect();
    let to_resolve: Vec<String> = targets
        .iter()
        .filter_map(|t| match t {
            Ok(Target::Item(i)) => Some(i.clone()),
            _ => None,
        })
        .collect();
    let mut resolved =
        if to_resolve.is_empty() { vec![] } else { resolve(c, &a.board, &to_resolve).await?.items }.into_iter();
    let mut tally = Tally::default();
    for (to, t) in a.to.iter().zip(targets) {
        let (pointer, role) = match t {
            Err(why) => {
                tally.did("failed", format!("{to}: failed, {why}"));
                continue;
            }
            Ok(Target::Done(p, r)) => (p, r),
            Ok(Target::Item(_)) => match resolved.next() {
                Some(r) => match r.pointer {
                    Some(p) => (p, AttachRole::Work),
                    None => {
                        tally.did(
                            "failed",
                            format!("{to}: failed, {}", r.why.unwrap_or_else(|| "control didn't resolve it".into())),
                        );
                        continue;
                    }
                },
                None => {
                    tally.did("failed", format!("{to}: control didn't resolve it"));
                    continue;
                }
            },
        };
        let role = a
            .role
            .map(|r| match r {
                AttachRoleArg::Work => AttachRole::Work,
                AttachRoleArg::Proof => AttachRole::Proof,
            })
            .unwrap_or(role);
        let body = serde_json::to_value(AttachRow { row: row.id.clone(), pointer, role }).map_err(|e| e.to_string())?;
        match boards_call(c, Method::POST, &agent_board_attachments(&a.board), Some(body)).await.and_then(view_of) {
            Err(why) => tally.did("failed", format!("{to}: failed, {why}")),
            Ok(after) => {
                tally.did("attached", format!("{to}: attached as {} to {}", role.as_str(), row.item_label()));
                view = after;
            }
        }
    }
    let mut lines = tally.lines("Attached");
    lines.extend(card_of(&view, &row.id));
    lines.push(progress(&view));
    answer(lines.join("\n"), &view, a.verbose)
}

/// The attachment of `row` that `what` names: its `att_` id, its item as
/// written, its `run_`/`fi_` id or its link.
fn find_attachment<'a>(row: &'a Row, what: &str) -> Result<&'a wire::Attachment, String> {
    let what = what.trim();
    let want = IssueRef::parse(what);
    let found: Vec<&wire::Attachment> = row
        .attachments
        .iter()
        .filter(|a| {
            a.id == what
                || pointer_id(&a.pointer) == Some(what)
                || matches!(&a.pointer, Pointer::Link { url } if url == what)
                || matches!(&a.pointer, Pointer::Block { machine, pane } if format!("block:{machine}/{pane}") == what)
                || want.as_ref().is_some_and(|w| names(w, a.live.as_ref()))
        })
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("{} has nothing attached as {what:?}", row.item_label())),
        many => Err(format!(
            "{what:?} names {} attachments of {}: use an att_ id ({})",
            many.len(),
            row.item_label(),
            many.iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// A write that changes one row: the card of that row and the progress.
fn one_row(view: &BoardView, what: &str, row: &str, verbose: bool) -> Out {
    let mut lines = vec![what.to_owned()];
    lines.extend(card_of(view, row).into_iter().map(|l| format!("  {l}")));
    lines.push(progress(view));
    answer(lines.join("\n"), view, verbose)
}

/// `board_read`.
pub(crate) async fn board_read(c: &Call<'_>, a: ReadBoardArgs) -> Out {
    owners_boards(c, READ_TOOL).await?;
    if let Some(id) = a.board.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        let view = read_view(c, id).await?;
        let s = format!("{}\n{}\nboard_write changes it at once", progress(&view), text(&view));
        return answer(s, &view, a.verbose);
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
    match a.verbose {
        true => done(s, list),
        false => done(s, serde_json::json!({})),
    }
}

/// The board a board-level write ends with: its progress, and what the
/// platform said besides.
fn board_done(what: &str, v: Value, verbose: bool) -> Out {
    let view = view_of(v)?;
    let said = view.said.as_deref().map(|s| format!(" Note: {s}.")).unwrap_or_default();
    let s = format!("{what}, now, as your person's agent on this machine. {}{said}", progress(&view));
    answer(s, &view, verbose)
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
            let mut out = board_done("Made a board", v, a.verbose)?;
            out["summary"] =
                format!("{}. {}", chosen_line(c, &chosen).await, out["summary"].as_str().unwrap_or_default()).into();
            Ok(out)
        }
        "edit_board" => {
            let a: EditBoardArgs = args(a)?;
            if a.title.is_none() && a.goal.is_none() {
                return Err("edit_board needs a title or a goal".into());
            }
            let body = given(EditBoard { title: a.title, goal: a.goal })?;
            let v = boards_call(c, Method::PATCH, &agent_board(&a.board), Some(body)).await?;
            board_done("Changed a board", v, a.verbose)
        }
        "close_board" => {
            let a: CloseBoardArgs = args(a)?;
            let body = given(CloseBoard { done_note: a.done_note })?;
            let v = boards_call(c, Method::POST, &agent_board_close(&a.board), Some(body)).await?;
            board_done("Closed a board", v, a.verbose)
        }
        "reopen_board" => {
            let a: BoardArgs = args(a)?;
            let v = boards_call(c, Method::POST, &agent_board_reopen(&a.board), None).await?;
            board_done("Opened a board again", v, a.verbose)
        }
        "add" => add(c, args(a)?).await,
        "attach" => attach(c, args(a)?).await,
        "detach" => {
            let a: DetachArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let row = find_row(&view, &a.item)?;
            let att = find_attachment(row, &a.what)?;
            let v = boards_call(c, Method::DELETE, &agent_board_attachment(&a.board, &att.id), None).await?;
            one_row(&view_of(v)?, &format!("Took {} off {}.", a.what.trim(), row.item_label()), &row.id, a.verbose)
        }
        "ask" => {
            let a: AskArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let row = find_row(&view, &a.item)?;
            let body = given(NewAsk {
                who: a.who,
                question: a.question,
                why: a.why,
                options: a.options,
                recommend: a.recommend,
            })?;
            let v = boards_call(c, Method::POST, &agent_board_row_ask(&a.board, &row.id), Some(body)).await?;
            one_row(&view_of(v)?, &format!("Asked on {}.", row.item_label()), &row.id, a.verbose)
        }
        "relate" => {
            let a: RelateArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let (from, to) = relation_ends(c, &a.board, &a.item, &a.to).await?;
            let kind = wire::RelationKind::from(a.kind);
            let body = given(NewRelation { from, to, kind })?;
            boards_call(c, Method::POST, &agent_project_relations(&view.board.project), Some(body)).await?;
            let after = read_view(c, &a.board).await?;
            let words = kind.as_str().replace('_', " ");
            answer(
                format!("Related: {} {words} {}.\n{}", a.item.trim(), a.to.trim(), ready_line(&after)),
                &after,
                a.verbose,
            )
        }
        "unrelate" => {
            let a: RelateArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let (from, to) = relation_ends(c, &a.board, &a.item, &a.to).await?;
            let kind = wire::RelationKind::from(a.kind);
            let rel = relation_between(&view, &from, &to, kind).ok_or_else(|| {
                format!(
                    "no such relation on a row of this board: {} and {} need a row with one of them as its item",
                    a.item.trim(),
                    a.to.trim()
                )
            })?;
            boards_call(c, Method::DELETE, &agent_project_relation(&view.board.project, &rel), None).await?;
            let after = read_view(c, &a.board).await?;
            answer(
                format!("Took off the relation between {} and {}.\n{}", a.item.trim(), a.to.trim(), ready_line(&after)),
                &after,
                a.verbose,
            )
        }
        "note" => {
            let a: NoteArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let row = find_row(&view, &a.item)?;
            if let Some(rev) = a.rev.filter(|r| *r != row.rev) {
                return Err(format!(
                    "{} has changed since rev {rev}: it is at rev {} now, and its note is {:?}. Nothing was written; note again with rev {}",
                    row.item_label(),
                    row.rev,
                    row.note,
                    row.rev
                ));
            }
            let put =
                PutEntry { id: Some(row.id.clone()), rev: Some(row.rev), note: Some(a.text), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            one_row(&view_of(v)?, &format!("Noted {}.", row.item_label()), &row.id, a.verbose)
        }
        "group" => {
            let a: GroupArgs = args(a)?;
            let Some(group) = a.group else {
                return Err("group needs a group (its title or zon_…), or null to take the row out of its group".into());
            };
            let view = read_view(c, &a.board).await?;
            let row = find_row(&view, &a.item)?;
            let zone = match group {
                Some(g) => Some(Some(find_group(&view, &g)?.id.clone())),
                None => Some(None),
            };
            // Not `given`: a `zone` of null is the move out of a group.
            let body = serde_json::to_value(MoveEntry { rev: None, zone, before: None }).map_err(|e| e.to_string())?;
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &row.id), Some(body)).await?;
            one_row(&view_of(v)?, &format!("Moved {}.", row.item_label()), &row.id, a.verbose)
        }
        "drop" => {
            let a: DropArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let row = find_row(&view, &a.item)?;
            let v = boards_call(c, Method::DELETE, &agent_board_entry(&a.board, &row.id), None).await?;
            let view = view_of(v)?;
            answer(format!("Dropped {}.\n{}", row.item_label(), progress(&view)), &view, a.verbose)
        }
        "add_group" => {
            let a: AddGroupArgs = args(a)?;
            let put = PutEntry { entry: Some(wire::EntryKind::Zone), title: Some(a.title), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Added a group", v, a.verbose)
        }
        "rename_group" => {
            let a: RenameGroupArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let g = find_group(&view, &a.group)?;
            let put = PutEntry { id: Some(g.id.clone()), rev: Some(g.rev), title: Some(a.title), ..Default::default() };
            let v = put_entry(c, &a.board, &put).await?;
            board_done("Renamed a group", v, a.verbose)
        }
        "move_group" => {
            let a: MoveGroupArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let g = find_group(&view, &a.group)?;
            let before = a.before.as_deref().map(|b| find_group(&view, b).map(|b| b.id.clone())).transpose()?;
            let m = MoveEntry { rev: None, zone: None, before };
            let v = boards_call(c, Method::POST, &agent_board_entry_move(&a.board, &g.id), Some(given(m)?)).await?;
            board_done("Moved a group", v, a.verbose)
        }
        "delete_group" => {
            let a: DeleteGroupArgs = args(a)?;
            let view = read_view(c, &a.board).await?;
            let g = find_group(&view, &a.group)?;
            let v = boards_call(c, Method::DELETE, &agent_board_entry(&a.board, &g.id), None).await?;
            board_done("Deleted a group", v, a.verbose)
        }
        _ => Err(format!("{WRITE_TOOL} kind {kind} isn't wired up")),
    }
}

async fn put_entry(c: &Call<'_>, board: &str, put: &PutEntry) -> Result<Value, String> {
    boards_call(c, Method::PUT, &agent_board_entries(board), Some(given(put)?)).await
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mcp::issues::Rule;

    /// A board of rows by item, for the tests of addressing: each is
    /// (row, item pointer id, live kind, repo, number, attachments).
    fn board(rows: Value) -> BoardView {
        serde_json::from_value(json!({
            "board": { "id": "brd_1", "project": "prj_1", "title": "Launch", "created_by": "a1", "created_at": 1,
                       "updated_at": 1, "progress": { "done": 1, "total": 3, "proven": 1 } },
            "groups": [{ "id": "zon_1", "rev": 4, "title": "Wave 1", "progress": { "done": 0, "total": 1 } }],
            "zones": [{ "id": "lane_next", "rev": 0, "title": "Next", "kind": "lane", "rows": rows }],
        }))
        .unwrap()
    }

    fn row(id: &str, pointer: Value, live: Value, attachments: Value) -> Value {
        json!({ "id": id, "rev": 2, "pointer": pointer, "note": "n", "status": "none", "lane": "next",
                "attachments": attachments, "live": live, "created_at": 1, "updated_at": 1 })
    }

    fn native(n: u64) -> Value {
        json!({ "state": "open", "kind": "native", "number": n, "title": "" })
    }

    fn github(repo: &str, kind: &str, n: u64) -> Value {
        json!({ "state": "open", "kind": kind, "repo": repo, "number": n, "title": "" })
    }

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

    /// A relation is found from what a row shows of it, by its two ends.
    #[test]
    fn unrelate_finds_the_relation_on_a_row() {
        let view: BoardView = serde_json::from_value(json!({
            "board": { "id": "brd_1", "project": "prj_1", "title": "T", "goal": "", "done_note": "", "rev": 1,
                       "created_by": "a", "progress": { "done": 0, "total": 0, "proven": 0 },
                       "created_at": 0, "updated_at": 0 },
            "zones": [{ "id": "lane_next", "rev": 0, "title": "Next", "rows": [
                { "id": "row_b", "rev": 1, "pointer": { "kind": "issue", "id": "iss_b" }, "note": "",
                  "status": "none", "created_at": 0, "updated_at": 0,
                  "relations": [{ "id": "rel_1", "kind": "blocks", "direction": "in", "source": "agent",
                                  "item": "iss_a", "removable": true }] }
            ] }],
            "groups": []
        }))
        .unwrap();
        let k = wire::RelationKind::Blocks;
        assert_eq!(relation_between(&view, "iss_a", "iss_b", k).as_deref(), Some("rel_1"));
        assert_eq!(relation_between(&view, "iss_b", "iss_a", k), None);
        assert_eq!(relation_between(&view, "iss_a", "iss_b", wire::RelationKind::Relates), None);
        assert_eq!(ready_line(&view), "0 ready.");
    }

    /// A board's args take only a title, and leave out what isn't given.
    #[test]
    fn a_board_is_made_from_a_title_alone() {
        let a: CreateBoardArgs = serde_json::from_value(json!({ "title": "Epic" })).unwrap();
        assert!(a.goal.is_none() && a.project.is_none() && !a.verbose);
        assert_eq!(given(NewBoard { title: a.title, goal: a.goal }).unwrap(), json!({ "title": "Epic" }));
    }

    /// The tool offers the verbs of #366 and none of the old ones; a row is
    /// never named by an id, and a move out of a group (`group: null`) is
    /// told from none.
    #[test]
    fn the_tool_offers_the_verbs_and_nothing_lanes_refuse() {
        let names: Vec<&str> = WRITE.iter().map(|k| k.name).collect();
        assert_eq!(
            names,
            [
                "add",
                "attach",
                "detach",
                "ask",
                "relate",
                "unrelate",
                "note",
                "group",
                "drop",
                "add_group",
                "rename_group",
                "move_group",
                "delete_group",
                "create_board",
                "edit_board",
                "close_board",
                "reopen_board"
            ]
        );
        let out: GroupArgs = serde_json::from_value(json!({ "board": "b", "item": "#1", "group": null })).unwrap();
        let stay: GroupArgs = serde_json::from_value(json!({ "board": "b", "item": "#1" })).unwrap();
        let into: GroupArgs = serde_json::from_value(json!({ "board": "b", "item": "#1", "group": "Wave 1" })).unwrap();
        assert_eq!((out.group, stay.group, into.group), (Some(None), None, Some(Some("Wave 1".into()))));
        let body = |zone| serde_json::to_value(MoveEntry { rev: None, zone, before: None }).unwrap();
        assert_eq!(body(Some(None))["zone"], Value::Null, "out of its group goes out as null");
        assert!(body(None).get("zone").is_none(), "no zone: its own");
    }

    /// An ask names who and the question, and leaves out what isn't given.
    #[test]
    fn an_ask_is_sent_as_the_wire_says() {
        let a: AskArgs = serde_json::from_value(
            json!({ "board": "b", "item": "#7", "who": "acc", "question": "Ship?", "options": ["Yes", "No"] }),
        )
        .unwrap();
        let body =
            given(NewAsk { who: a.who, question: a.question, why: a.why, options: a.options, recommend: a.recommend })
                .unwrap();
        assert_eq!(body, json!({ "who": "acc", "question": "Ship?", "options": ["Yes", "No"] }));
        assert!(serde_json::from_value::<AskArgs>(json!({ "board": "b", "item": "#7", "question": "Ship?" })).is_err());
    }

    /// A row is found by its item, in each of its forms, and by what is
    /// attached when no row has it as its own.
    #[test]
    fn a_row_is_found_by_its_item() {
        let v = board(json!([
            row("row_1", json!({ "kind": "issue", "id": "iss_a" }), native(12), json!([])),
            row(
                "row_2",
                json!({ "kind": "forge_item", "id": "fi_b" }),
                github("acme/app", "issue", 30),
                json!([
                { "id": "att_1", "role": "work", "pointer": { "kind": "forge_item", "id": "fi_pr" },
                  "live": github("acme/app", "pr", 31) }])
            ),
            row(
                "row_3",
                json!({ "kind": "forge_item", "id": "fi_c" }),
                json!({ "state": "open", "kind": "issue", "number": 7, "title": "", "provider": "plane", "identifier": "ARUGU" }),
                json!([])
            ),
        ]));
        for (item, row) in [
            ("#12", "row_1"),
            (" #12 ", "row_1"),
            ("iss_a", "row_1"),
            ("row_2", "row_2"),
            ("acme/app#30", "row_2"),
            ("#30", "row_2"),
            ("#31", "row_2"),
            ("acme/app#31", "row_2"),
            ("fi_pr", "row_2"),
            ("ARUGU-7", "row_3"),
            ("arugu-7", "row_3"),
        ] {
            assert_eq!(find_row(&v, item).map(|r| r.id.as_str()), Ok(row), "{item}");
        }
        let none = find_row(&v, "#99").unwrap_err();
        assert!(none.contains("no row on this board has #99") && none.contains("add"), "{none}");
        assert!(find_row(&v, "banana").unwrap_err().contains("isn't an item"));
    }

    /// `#5` that is both a native issue and a GitHub issue on the board is
    /// not guessed: the refusal names both, and the unambiguous forms work.
    #[test]
    fn an_ambiguous_number_names_both_rows() {
        let v = board(json!([
            row("row_1", json!({ "kind": "issue", "id": "iss_a" }), native(5), json!([])),
            row("row_2", json!({ "kind": "forge_item", "id": "fi_b" }), github("acme/app", "issue", 5), json!([])),
        ]));
        let e = find_row(&v, "#5").unwrap_err();
        assert!(
            e.contains("the native issue #5") && e.contains("acme/app#5") && e.contains("more than one row"),
            "{e}"
        );
        assert_eq!(find_row(&v, "acme/app#5").map(|r| r.id.as_str()), Ok("row_2"));
        assert_eq!(find_row(&v, "iss_a").map(|r| r.id.as_str()), Ok("row_1"));
    }

    /// A group is found by its title in any case, or by its id.
    #[test]
    fn a_group_is_found_by_title_or_id() {
        let v = board(json!([]));
        assert_eq!(find_group(&v, "wave 1").map(|g| g.id.as_str()), Ok("zon_1"));
        assert_eq!(find_group(&v, "zon_1").map(|g| g.id.as_str()), Ok("zon_1"));
        assert!(find_group(&v, "Wave 2").unwrap_err().contains("Wave 1"));
    }

    /// A card is the item, its lane, rev and note, its marks and its ask; the
    /// progress line counts the board's issues.
    #[test]
    fn a_card_and_the_progress_are_short() {
        let mut r = row("row_1", json!({ "kind": "issue", "id": "iss_a" }), native(12), json!([]));
        r["group"] = json!("zon_1");
        r["marks"] = json!([{ "kind": "merged_not_closed" }]);
        r["ask"] = json!({ "id": "ask_1", "who": "a2", "who_name": "Sam", "question": "Ship it?", "options": [],
                           "asked_by": "agent", "raised": false, "mine": false, "can_answer": false, "created_at": 1 });
        let v = board(json!([r]));
        let lines = card(&v, rows(&v).next().unwrap());
        assert_eq!(lines, ["#12 ○ Next · rev 2 · in Wave 1 · n", "  merged, not closed", "  ? Ship it? (for Sam)"]);
        assert_eq!(progress(&v), "Launch (brd_1): 1/3 done, 1 proven, 1 group(s)");
    }

    /// A batch's answer lists failures first, caps its lines whatever the
    /// batch's size, and counts what it leaves out.
    #[test]
    fn a_batch_answer_stays_short() {
        let mut t = Tally::default();
        for i in 0..40 {
            t.did("added", format!("#{i}: added"));
        }
        t.did("failed", "#99: failed, nothing is #99 in this project".into());
        let lines = t.lines("Added");
        assert_eq!(lines[0], "Added: 40 added, 1 failed.");
        assert!(lines[1].contains("#99"), "failures first: {lines:?}");
        assert_eq!(lines.len(), 1 + LINES_MAX + 1, "{lines:?}");
        assert!(lines.last().unwrap().contains("and 27 more"), "{lines:?}");
    }

    /// What an attach names: a block, a run, a link, or an item control
    /// resolves.
    #[test]
    fn an_attachment_is_named_by_its_kind() {
        assert!(matches!(target("block:m1/7"), Ok(Target::Done(Pointer::Block { .. }, AttachRole::Work))));
        assert!(matches!(target("run_ab12"), Ok(Target::Done(Pointer::Run { .. }, AttachRole::Work))));
        assert!(matches!(
            target("https://ci.example.com/1"),
            Ok(Target::Done(Pointer::Link { .. }, AttachRole::Proof))
        ));
        assert!(matches!(target("#12"), Ok(Target::Item(_))));
        assert!(target("block:m1").is_err());
    }
}
