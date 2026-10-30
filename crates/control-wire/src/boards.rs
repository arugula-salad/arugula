//! Boards (boards, step 1; the model of aru-125): a project's board is an
//! ordered list of rows, and a row is one piece of work. Its primary item is
//! an issue or a PR; a row with none (or with a link) is a note row. What
//! else the work is (a PR, an agent block, a run) is attached to it, and
//! evidence of it is attached as proof. **Nobody moves a row:** the lane it
//! is in (`next`, `doing`, `review`, `done`) follows the live state of its
//! items, and a read answers the board as the four [`ZoneKind::Lane`]
//! zones, always, in that order. The platform keeps the rows
//! (`crate::db::boards` there); a board's thread is its object thread
//! (`brd_`, M196).
//!
//! A project in a path is its id (`prj_…`), or `personal` for the reader's
//! own. Behind the `boards` Labs flag: with it off, every path answers 404.
//!
//! **Revs.** A row's `rev` counts changes to its pointer and note. A write
//! that changes those names the `rev` it read; if the row moved on, it is
//! refused with 409 (`stale_rev`) and the client reads the board again.

use serde::{Deserialize, Serialize};

use crate::chat::ChatAgent;

/// `GET`: a project's boards, [`BoardList`]; `POST` [`NewBoard`]: make one,
/// answered [`BoardView`].
pub const PROJECT_BOARDS: &str = "/api/projects/{project}/boards";
/// `GET`: a board with its zones and rows in order, [`BoardView`]. `PATCH`
/// [`EditBoard`]: set its title or goal (owners and editors), answered
/// [`BoardView`].
pub const BOARD: &str = "/api/boards/{id}";
/// `POST` [`CloseBoard`]: close the board with a done note (owners and
/// editors); answered [`BoardView`]. Its issues and rows stay as
/// they are, and its rows stay editable.
pub const BOARD_CLOSE: &str = "/api/boards/{id}/close";
/// `POST`, no body: open a closed board again, keeping its done note;
/// answered [`BoardView`].
pub const BOARD_REOPEN: &str = "/api/boards/{id}/reopen";
/// `PUT` [`PutEntry`]: add a row, or change a row's note or pointer,
/// answered with the board, [`BoardView`].
pub const BOARD_ENTRIES: &str = "/api/boards/{id}/entries";
/// `DELETE`: a row; answered [`BoardView`].
pub const BOARD_ENTRY: &str = "/api/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: refused, 400: lanes follow the work. Kept so an
/// old client is told why; answered with an error, never a [`BoardView`].
pub const BOARD_ENTRY_MOVE: &str = "/api/boards/{id}/entries/{entry}/move";
/// `POST` [`AttachRow`]: attach work or proof to a row (owners and
/// editors); answered [`BoardView`].
pub const BOARD_ATTACHMENTS: &str = "/api/boards/{id}/attachments";
/// `DELETE`: take an attachment off its row; answered [`BoardView`].
pub const BOARD_ATTACHMENT: &str = "/api/boards/{id}/attachments/{attachment}";
/// `GET` [`WorkQuery`]: a project's work items, [`WorkItems`].
pub const PROJECT_WORK: &str = "/api/projects/{project}/work";

/// A machine's agents, under `boards.read` / `boards.write` (over the
/// project, given by the machine's owner): the same operations as above,
/// signed by the machine, never past the owner's role. See
/// `arugula_platform::boards::agent` for what they may not do.
///
/// `GET`: a project's boards ([`BoardList`]); `{project}` is an id or
/// `personal`. `POST` [`NewBoard`]: make one, answered [`BoardView`].
pub const AGENT_PROJECT_BOARDS: &str = "/api/daemon/projects/{project}/boards";
/// `GET`: a board ([`BoardView`]). `PATCH` [`EditBoard`]: set its title or
/// goal.
pub const AGENT_BOARD: &str = "/api/daemon/boards/{id}";
/// `POST` [`CloseBoard`]: close the board with a done note.
pub const AGENT_BOARD_CLOSE: &str = "/api/daemon/boards/{id}/close";
/// `POST`, no body: open a closed board again.
pub const AGENT_BOARD_REOPEN: &str = "/api/daemon/boards/{id}/reopen";
/// `PUT` [`PutEntry`]: add a row, or change a row's note or pointer (a row
/// the agent's machine added, for a pointer).
pub const AGENT_BOARD_ENTRIES: &str = "/api/daemon/boards/{id}/entries";
/// `DELETE`: a row the agent's machine added.
pub const AGENT_BOARD_ENTRY: &str = "/api/daemon/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: refused, 400: lanes follow the work.
pub const AGENT_BOARD_ENTRY_MOVE: &str = "/api/daemon/boards/{id}/entries/{entry}/move";
/// `POST` [`AttachRow`]: attach work or proof to a row.
pub const AGENT_BOARD_ATTACHMENTS: &str = "/api/daemon/boards/{id}/attachments";
/// `DELETE`: take off an attachment this machine's agents made.
pub const AGENT_BOARD_ATTACHMENT: &str = "/api/daemon/boards/{id}/attachments/{attachment}";

/// `GET`: the projects whose boards this machine's agents reach, and with
/// which powers, [`BoardReach`].
pub const AGENT_BOARD_REACH: &str = "/api/daemon/boards/reach";

/// [`AGENT_PROJECT_BOARDS`] for `project`.
pub fn agent_project_boards(project: &str) -> String {
    format!("/api/daemon/projects/{project}/boards")
}

/// [`AGENT_BOARD`] for `id`.
pub fn agent_board(id: &str) -> String {
    format!("/api/daemon/boards/{id}")
}

/// [`AGENT_BOARD_CLOSE`] for `id`.
pub fn agent_board_close(id: &str) -> String {
    format!("/api/daemon/boards/{id}/close")
}

/// [`AGENT_BOARD_REOPEN`] for `id`.
pub fn agent_board_reopen(id: &str) -> String {
    format!("/api/daemon/boards/{id}/reopen")
}

/// `PUT` [`PutBlockState`]: a machine tells control the state of one of its
/// own agent blocks, for the rows that point at it. The machine is the one
/// that signed the request, never a field of the body.
pub const AGENT_BLOCK_STATE: &str = "/api/daemon/block-state";

/// [`AGENT_BOARD_ENTRIES`] for `id`.
pub fn agent_board_entries(id: &str) -> String {
    format!("/api/daemon/boards/{id}/entries")
}

/// [`AGENT_BOARD_ENTRY`] for `entry` of `id`.
pub fn agent_board_entry(id: &str, entry: &str) -> String {
    format!("/api/daemon/boards/{id}/entries/{entry}")
}

/// [`AGENT_BOARD_ENTRY_MOVE`] for `entry` of `id`.
pub fn agent_board_entry_move(id: &str, entry: &str) -> String {
    format!("/api/daemon/boards/{id}/entries/{entry}/move")
}

/// [`AGENT_BOARD_ATTACHMENTS`] for `id`.
pub fn agent_board_attachments(id: &str) -> String {
    format!("/api/daemon/boards/{id}/attachments")
}

/// [`AGENT_BOARD_ATTACHMENT`] for `attachment` of `id`.
pub fn agent_board_attachment(id: &str, attachment: &str) -> String {
    format!("/api/daemon/boards/{id}/attachments/{attachment}")
}

/// What a refused move, zone or status write says first: the rest of the
/// sentence says what to do instead.
pub const LANES_FOLLOW: &str = "lanes follow the work";

/// The longest title, in characters, and the longest note: one line each.
pub const TITLE_MAX: usize = 120;
/// A board's goal or done note, in characters.
pub const GOAL_MAX: usize = 2000;
pub const NOTE_MAX: usize = 280;
/// The most boards in a project, rows in a board and attachments on a row.
pub const BOARDS_MAX: u64 = 100;
pub const ROWS_MAX: u64 = 500;
pub const ATTACHMENTS_MAX: u64 = 20;
/// The longest link pointer's URL, in bytes.
pub const LINK_MAX: usize = 2048;

/// What a row points at. `None` is free text. The ids are the platform's:
/// `fi_…` (a mirrored PR or issue), `iss_…`, `run_…`, `hw_…`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardPointer"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pointer {
    #[default]
    None,
    ForgeItem {
        id: String,
    },
    Issue {
        id: String,
    },
    Run {
        id: String,
    },
    Workspace {
        id: String,
    },
    /// An agent block on a machine.
    Block {
        machine: String,
        pane: String,
    },
    /// An `https://` URL, as evidence (a CI page, a report). The platform
    /// takes only `https`, no userinfo, at most [`LINK_MAX`] bytes.
    Link {
        url: String,
    },
}

/// Where a row stands. `None` says nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardStatus"))]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    None,
    Open,
    Working,
    NeedsInput,
    Done,
    Failed,
}

/// Zone or row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardEntryKind"))]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Zone,
    Row,
}

/// A zone's kind: every zone a read answers is a `lane`, one of the four
/// ([`Lane`]). No one makes a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardZoneKind"))]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    Lane,
}

/// Where a row is, worked out on every read from the live state of its
/// items and never stored (aru-125). Each is the furthest that applies, in
/// the order `done`, `review`, `doing`, `next`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardLane"))]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Nothing is under way, or the row is a note.
    #[default]
    Next,
    /// An agent block that isn't closed, or a run that is running.
    Doing,
    /// A PR is open.
    Review,
    /// The issue is closed, or the PR merged or closed.
    Done,
}

impl Lane {
    /// The four, in the order a board shows them.
    pub const ALL: [Lane; 4] = [Lane::Next, Lane::Doing, Lane::Review, Lane::Done];

    /// The id of the lane's zone: `lane_next`.
    pub fn id(self) -> &'static str {
        match self {
            Lane::Next => "lane_next",
            Lane::Doing => "lane_doing",
            Lane::Review => "lane_review",
            Lane::Done => "lane_done",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Lane::Next => "Next",
            Lane::Doing => "Doing",
            Lane::Review => "Review",
            Lane::Done => "Done",
        }
    }

    /// The glyph a row in the lane shows.
    pub fn glyph(self) -> char {
        match self {
            Lane::Next => '○',
            Lane::Doing => '◐',
            Lane::Review => '◑',
            Lane::Done => '●',
        }
    }
}

/// What an attachment is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAttachRole"))]
#[serde(rename_all = "snake_case")]
pub enum AttachRole {
    /// The work itself: a PR, an agent block or a run. It moves the lane.
    Work,
    /// Evidence that the work is done: a run or a link.
    Proof,
}

impl AttachRole {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachRole::Work => "work",
            AttachRole::Proof => "proof",
        }
    }

    /// Whether `p` may be attached for this role: a `work` attachment is a
    /// PR (a mirrored item), an agent block or a run; a `proof` one is a run
    /// or a link.
    pub fn takes(self, p: &Pointer) -> bool {
        match self {
            AttachRole::Work => matches!(p, Pointer::ForgeItem { .. } | Pointer::Block { .. } | Pointer::Run { .. }),
            AttachRole::Proof => matches!(p, Pointer::Run { .. } | Pointer::Link { .. }),
        }
    }
}

/// Something attached to a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAttachment"))]
pub struct Attachment {
    /// `att_` and 32 hex characters.
    pub id: String,
    pub role: AttachRole,
    pub pointer: Pointer,
    /// The item's state now, when the reader may see it: an item they can't
    /// read says nothing, as an unreadable row does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// A `Block` pointer's block as its machine last reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockState>,
}

/// How a Done row is proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardProofBy"))]
#[serde(rename_all = "snake_case")]
pub enum ProofBy {
    /// A PR of the row merged with every check on its head completed and
    /// none red.
    Ci,
    /// A `proof` attachment.
    Evidence,
}

/// The mark on a Done row that is proven. A Done row without one is not
/// proven yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardProof"))]
pub struct Proof {
    pub by: ProofBy,
    /// What proves it: the merged PR, or the evidence attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pointer: Option<Pointer>,
}

/// [`BOARD_ATTACHMENTS`]' body: attach `pointer` to `row` as `role`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAttachRow"))]
pub struct AttachRow {
    /// The row (`row_…`).
    pub row: String,
    pub pointer: Pointer,
    pub role: AttachRole,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectBoard"))]
pub struct Board {
    pub id: String,
    pub project: String,
    pub title: String,
    /// The account that made it; empty once that account is deleted. When an
    /// agent made it, the account it acted for.
    pub created_by: String,
    /// The machine whose agent made it, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub created_by_agent: Option<ChatAgent>,
    pub created_at: u64,
    pub updated_at: u64,
    /// What the board is for, in a paragraph; empty means none.
    #[serde(default)]
    pub goal: String,
    /// What was done, written when the board was closed; kept when it is
    /// opened again. Empty before it was ever closed.
    #[serde(default)]
    pub done_note: String,
    /// When it was closed; absent while it is open.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub closed_at: Option<u64>,
    /// The account that closed it; empty once that account is deleted. When
    /// an agent closed it, the account it acted for.
    #[serde(default)]
    pub closed_by: String,
    /// The machine whose agent closed it, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub closed_by_agent: Option<ChatAgent>,
    /// How many of its issues are closed.
    #[serde(default)]
    pub progress: BoardProgress,
}

/// A board's work, counted: the rows with a primary item (an issue or a
/// PR; a note row is never counted). `done` is those in the Done lane, and
/// `proven` those of them with a proof.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardProgress"))]
pub struct BoardProgress {
    pub done: u32,
    pub total: u32,
    #[serde(default)]
    pub proven: u32,
}

/// `PATCH`: what to change; a field left out stays. An empty goal clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditBoard {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
}

/// `POST` to close a board: what was done, not empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloseBoard {
    pub done_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardList {
    pub project: String,
    pub boards: Vec<Board>,
}

/// [`AGENT_BOARD_REACH`]'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoardReach {
    pub projects: Vec<BoardReachProject>,
}

/// A project a machine's board grants reach, as its owner reads it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoardReachProject {
    pub id: String,
    pub name: String,
    /// `boards.read`, `boards.write`.
    pub powers: Vec<String>,
    /// How many boards it has.
    pub boards: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewBoard {
    pub title: String,
    /// What the board is for; left out: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub goal: Option<String>,
}

/// What a pointer to an item, an issue or a run is now, from data control
/// already has (the forge mirror, the issues and runs tables): a separate fact, never written
/// over the row's stored `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardLiveState"))]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// A PR or an issue, open.
    Open,
    /// A PR, open, with checks still running.
    ChecksRunning,
    /// A PR, open, with red checks.
    ChecksFailed,
    Merged,
    /// A PR closed unmerged, or an issue closed.
    Closed,
    /// A run, running.
    Running,
    Succeeded,
    /// A run that failed.
    RunFailed,
    /// A run that is pending, approved, denied, canceled or expired.
    Other,
}

/// The item a row points at, as it is now: what the page draws beside the
/// row's own status, and the label of its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardLive"))]
pub struct Live {
    pub state: LiveState,
    /// `pr` or `issue` (a mirrored one), `native` for a native issue, or
    /// `run`.
    pub kind: String,
    /// `OWNER/NAME` of a mirrored item; absent for a native issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// A run has none: 0.
    pub number: u64,
    /// A run's: how it started (`manual run`).
    pub title: String,
    /// Who an issue or PR is assigned to: a mirrored item's logins, a native
    /// issue's names. A run has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignees: Vec<String>,
    /// `plane` for a tracker's item; absent for GitHub, native issues and runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// A tracker item's project key (`ARUGU`: the card reads `ARUGU-12`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// A tracker item's own state (`In Progress`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_name: Option<String>,
    /// Where a tracker item is, in the tracker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// What an agent block is doing (aru-116's second ruling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardBlockWord"))]
#[serde(rename_all = "snake_case")]
pub enum BlockWord {
    Working,
    NeedsInput,
    Idle,
    Closed,
}

impl BlockWord {
    /// The word a board shows.
    pub fn word(self) -> &'static str {
        match self {
            BlockWord::Working => "working",
            BlockWord::NeedsInput => "needs input",
            BlockWord::Idle => "idle",
            BlockWord::Closed => "closed",
        }
    }
}

/// A `git diff --stat` summary line as numbers: nothing of the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardDiffstat"))]
#[serde(deny_unknown_fields)]
pub struct Diffstat {
    pub files: u32,
    pub added: u32,
    pub removed: u32,
}

impl Diffstat {
    /// `3 files changed, +40 −2`.
    pub fn line(&self) -> String {
        let s = if self.files == 1 { "" } else { "s" };
        format!("{} file{s} changed, +{} −{}", self.files, self.added, self.removed)
    }
}

/// What a daemon says of one of its own agent blocks, and all it says
/// (aru-116's second ruling): the state word, the branch, the short HEAD,
/// whether it is pushed, how many files are uncommitted and the diffstat.
/// No message text, no file names, no prompts: the type has no field that
/// could carry them, and a body with another field is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardBlockState"))]
#[serde(deny_unknown_fields)]
pub struct BlockState {
    pub state: BlockWord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// HEAD, abbreviated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<bool>,
    /// How many files are uncommitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncommitted: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffstat: Option<Diffstat>,
}

impl BlockState {
    /// `needs input · feat/x@abc1234 · unpushed · 2 uncommitted · 3 files
    /// changed, +40 −2`: what a board shows of a block, in one line.
    pub fn line(&self) -> String {
        let mut parts = vec![self.state.word().to_owned()];
        match (&self.branch, &self.head) {
            (Some(b), Some(h)) => parts.push(format!("{b}@{h}")),
            (Some(x), None) | (None, Some(x)) => parts.push(x.clone()),
            (None, None) => {}
        }
        if let Some(p) = self.pushed {
            parts.push(if p { "pushed" } else { "unpushed" }.to_owned());
        }
        if let Some(n) = self.uncommitted.filter(|n| *n > 0) {
            parts.push(format!("{n} uncommitted"));
        }
        if let Some(d) = &self.diffstat {
            parts.push(d.line());
        }
        parts.join(" · ")
    }
}

/// [`AGENT_BLOCK_STATE`]'s body: the pane (of the machine that signs it) and
/// its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardPutBlockState"))]
#[serde(deny_unknown_fields)]
pub struct PutBlockState {
    pub pane: String,
    pub block: BlockState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardRow"))]
pub struct Row {
    pub id: String,
    pub rev: u64,
    pub pointer: Pointer,
    pub note: String,
    /// Kept for old clients; nothing writes it any more (aru-125).
    pub status: Status,
    /// Where the row is, from the live state of its items.
    #[serde(default)]
    pub lane: Lane,
    /// Work and proof attached to the row, oldest first.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Set only on a Done row that is proven.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub proof: Option<Proof>,
    /// A `fi_`, `iss_` or `run_` pointer's state now, when the reader may see the
    /// item ([`BoardView`] is read with it; a write's answer has it too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// A `Block` pointer's block as its machine last reported it, when that
    /// machine holds `boards.write` over the board's project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockState>,
    /// The machine whose agent added the row, if one did (it may remove
    /// it; nobody else's agent may).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ChatAgent>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardZone"))]
pub struct Zone {
    pub id: String,
    pub rev: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ZoneKind>,
    pub rows: Vec<Row>,
}

/// A board with its four lanes, in order (`lane_next`, `lane_doing`,
/// `lane_review`, `lane_done`), each with its rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardView {
    pub board: Board,
    pub zones: Vec<Zone>,
    /// What a write did that the board alone doesn't say: a PR attached to
    /// the row of the issue it closes, an item that was there already.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub said: Option<String>,
}

/// [`BOARD_ENTRIES`]' body. Without `id` it adds a row, last (`entry` is
/// `row` or absent); with `id` it changes that row: what is absent is left
/// as it is, and a change to `pointer` or `note` names `rev`. `zone`, `title`,
/// `kind`, `entry: zone` and `status` are refused (aru-125: lanes follow the
/// work), except `status: needs_input` on a row, which stays until asks
/// replace it. The row's `pointer` is an issue or a PR (or a link, for a
/// note).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardPutEntry"))]
pub struct PutEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub rev: Option<u64>,
    #[serde(default)]
    pub entry: Option<EntryKind>,
    /// Refused: rows have no zone.
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Refused: no one makes a zone.
    #[serde(default)]
    pub kind: Option<ZoneKind>,
    /// A row's pointer; `{"kind": "none"}` clears it.
    #[serde(default)]
    pub pointer: Option<Pointer>,
    #[serde(default)]
    pub note: Option<String>,
    /// Refused, except `"needs_input"`.
    #[serde(default)]
    pub status: Option<Status>,
}

/// [`BOARD_ENTRY_MOVE`]'s body. Every move is refused (aru-125): lanes
/// follow the work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardMoveEntry"))]
pub struct MoveEntry {
    #[serde(default)]
    pub rev: Option<u64>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub before: Option<String>,
}

// ---------------------------------------------------------------- text

/// The widest a board's text form goes, in columns.
pub const TEXT_WIDTH: usize = 72;
/// The progress bar's cells, and the room a row's label has.
const BAR: usize = 10;
const LABEL: usize = 18;

impl Pointer {
    /// A short label: the id's first characters (`iss_1a2b3c4d`), or
    /// `machine/pane` for an agent block; empty for none.
    pub fn label(&self) -> String {
        let short = |id: &str| id.chars().take(12).collect::<String>();
        match self {
            Pointer::None => String::new(),
            Pointer::ForgeItem { id } | Pointer::Issue { id } | Pointer::Run { id } | Pointer::Workspace { id } => {
                short(id)
            }
            Pointer::Block { machine, pane } => format!("{machine}/{pane}"),
            Pointer::Link { url } => url.strip_prefix("https://").unwrap_or(url).to_owned(),
        }
    }
}

impl Row {
    /// Whether the row has a primary item to count: not a note row.
    pub fn counted(&self) -> bool {
        matches!(self.pointer, Pointer::Issue { .. } | Pointer::ForgeItem { .. })
    }
}

/// `s` on one line, at most `max` characters, ending in `…` if cut.
fn fit(s: &str, max: usize) -> String {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut cut: String = s.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// ` (@a, @b)`, or empty for none.
fn assignees_suffix(who: &[String]) -> String {
    if who.is_empty() {
        return String::new();
    }
    let tagged: Vec<String> = who.iter().map(|w| format!("@{}", w.replace(char::is_whitespace, "_"))).collect();
    format!(" ({})", tagged.join(", "))
}

impl Proof {
    /// `[proven by CI]` or `[proven]`.
    fn mark(&self) -> &'static str {
        match self.by {
            ProofBy::Ci => "[proven by CI] ",
            ProofBy::Evidence => "[proven] ",
        }
    }
}

/// A board as plain text, no wider than [`TEXT_WIDTH`] columns: before the
/// lanes, `Goal:` with the goal's first line, `Issues` with the board's
/// progress when it has any, and `Done:` with the done note's first line
/// when it is closed. Then each lane is its title and how many rows it holds
/// (the Done lane also how many are proven), and its rows, each a glyph, a
/// short pointer label and the note (cut to fit). Under a row: what is
/// attached to it, and under an agent block's row what its machine reported
/// of it. A Done row starts its note with `[proven by CI]`, `[proven]` or
/// `[not proven yet]`, and a note row (no issue or PR) has no mark. Ids and
/// revs aren't in it: they are in the [`BoardView`].
///
/// ```text
/// Doing                                                                1
///   ◐ iss_1a2b3c4d     Parser rewrite
///       with m1/7
/// ```
pub fn render(view: &BoardView) -> String {
    let mut out = String::new();
    let first = |s: &str| s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_owned();
    let b = &view.board;
    if !first(&b.goal).is_empty() {
        out += &format!("{}\n", fit(&format!("Goal: {}", first(&b.goal)), TEXT_WIDTH));
    }
    if b.progress.total > 0 {
        let (done, total) = (b.progress.done as usize, b.progress.total as usize);
        let filled = (done * BAR / total).min(BAR);
        let proven = if done > 0 { format!(", {} proven", b.progress.proven) } else { String::new() };
        out += &format!("Issues  {}{} {done}/{total}{proven}\n", "█".repeat(filled), "░".repeat(BAR - filled));
    }
    if b.closed_at.is_some() {
        out += &format!("{}\n", fit(&format!("Done: {}", first(&b.done_note)), TEXT_WIDTH));
    }
    for z in &view.zones {
        let proven = z.rows.iter().filter(|r| r.proof.is_some()).count();
        let count = if z.rows.iter().any(|r| r.lane == Lane::Done) && proven > 0 {
            format!("{} ({proven} proven)", z.rows.len())
        } else {
            z.rows.len().to_string()
        };
        let title = fit(&z.title, TEXT_WIDTH - count.chars().count() - 1);
        let pad = TEXT_WIDTH - title.chars().count() - count.chars().count();
        out += &format!("{title}{}{count}\n", " ".repeat(pad));
        for r in &z.rows {
            let label = fit(&r.pointer.label(), LABEL);
            let lead = format!("  {} {label:<LABEL$} ", r.lane.glyph());
            let mark = match (r.lane, r.counted(), &r.proof) {
                (Lane::Done, true, Some(p)) => p.mark(),
                (Lane::Done, true, None) => "[not proven yet] ",
                _ => "",
            };
            let room = TEXT_WIDTH - lead.chars().count();
            let who = r.live.as_ref().map(|l| assignees_suffix(&l.assignees)).unwrap_or_default();
            let note = if who.is_empty() || room < who.chars().count() + 8 {
                fit(&format!("{mark}{}", r.note), room)
            } else {
                // The note gives way, so the assignees stay whole.
                format!("{}{who}", fit(&format!("{mark}{}", r.note), room - who.chars().count()))
            };
            out += format!("{lead}{note}").trim_end();
            out.push('\n');
            if !r.attachments.is_empty() {
                let with: Vec<String> = r
                    .attachments
                    .iter()
                    .map(|a| {
                        let role = if a.role == AttachRole::Proof { "proof " } else { "" };
                        format!("{role}{}", a.pointer.label())
                    })
                    .collect();
                out += &format!("      {}\n", fit(&format!("with {}", with.join(", ")), TEXT_WIDTH - 6));
            }
            if let Some(b) = &r.block {
                // Under the row, in line with its label.
                out += &format!("      {}\n", fit(&b.line(), TEXT_WIDTH - 6));
            }
        }
    }
    out.trim_end().to_owned()
}

// ---------------------------------------------------------------- work

/// [`PROJECT_WORK`]'s query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardWorkQuery"))]
pub struct WorkQuery {
    /// Search words: an issue's title or body, a mirrored item's title or
    /// mirrored content, a run's trigger (`manual`).
    #[serde(default)]
    pub q: Option<String>,
    /// `issue`, `pr` or `run`; absent means all.
    #[serde(default)]
    pub kind: Option<String>,
    /// Only open issues (native and GitHub) that no row of any of the
    /// project's boards points at.
    #[serde(default)]
    pub untriaged: bool,
    /// At most this many (default 30, at most 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// One thing a row may point at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardWorkItem"))]
pub struct WorkItem {
    /// What a row takes as its pointer: `Issue`, `ForgeItem` or `Run`.
    pub pointer: Pointer,
    /// `native`, `github`, `plane` or `run`.
    pub source: String,
    /// `issue`, `pr` or `run`.
    pub kind: String,
    /// `OWNER/NAME` of a mirrored item; a tracker project's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// A tracker item's project key (`ARUGU`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// A run has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// Where a tracker item is, in the tracker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A run's: how it started (`manual run`).
    pub title: String,
    /// `open`, `closed` or `merged` for an issue or a PR; a run's own state.
    pub state: String,
    /// GitHub logins for a mirrored item; account ids for a native issue.
    pub assignees: Vec<String>,
    pub updated_at: u64,
}

/// [`PROJECT_WORK`]'s answer: the newest first, across sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardWorkItems"))]
pub struct WorkItems {
    pub project: String,
    pub items: Vec<WorkItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pointer: Pointer, lane: Lane, note: &str) -> Row {
        Row {
            id: "row_1".into(),
            rev: 1,
            pointer,
            note: note.into(),
            status: Status::None,
            lane,
            attachments: vec![],
            proof: None,
            live: None,
            block: None,
            agent: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn lane(l: Lane, rows: Vec<Row>) -> Zone {
        Zone { id: l.id().into(), rev: 0, title: l.title().into(), kind: Some(ZoneKind::Lane), rows }
    }

    fn lanes(mut by: Vec<(Lane, Vec<Row>)>) -> Vec<Zone> {
        Lane::ALL
            .map(|l| lane(l, by.iter_mut().find(|(k, _)| *k == l).map(|(_, r)| std::mem::take(r)).unwrap_or_default()))
            .into()
    }

    fn board() -> Board {
        Board {
            id: "brd_1".into(),
            project: "prj_1".into(),
            title: "Launch".into(),
            created_by: "a".into(),
            created_at: 0,
            updated_at: 0,
            ..Default::default()
        }
    }

    /// A lane's heading line: its title and count, 72 columns.
    fn head(title: &str, count: &str) -> String {
        format!("{title}{count:>w$}", w = TEXT_WIDTH - title.chars().count())
    }

    fn view(zones: Vec<Zone>) -> BoardView {
        BoardView { board: board(), zones, said: None }
    }

    /// A board's text form: the four lanes with their counts, a glyph per
    /// lane, notes cut to fit 72 columns, a Done row's proof as a mark, and
    /// what is attached under a row.
    #[test]
    fn a_board_renders_in_72_columns() {
        let long = "Wiring the new tokenizer through every caller, then the benchmarks, then the docs";
        let mut ci = row(Pointer::ForgeItem { id: "fi_aa22".into() }, Lane::Done, "Release build");
        ci.proof = Some(Proof { by: ProofBy::Ci, pointer: Some(Pointer::ForgeItem { id: "fi_aa22".into() }) });
        let mut by_hand =
            row(Pointer::Issue { id: "iss_1a2b3c4d5e6f7a8b".into() }, Lane::Done, "Parser rewrite landed");
        by_hand.proof = Some(Proof { by: ProofBy::Evidence, pointer: None });
        let mut doing = row(Pointer::Issue { id: "iss_bb11".into() }, Lane::Doing, long);
        doing.attachments = vec![Attachment {
            id: "att_1".into(),
            role: AttachRole::Work,
            pointer: Pointer::Block { machine: "m1".into(), pane: "7".into() },
            live: None,
            block: None,
        }];
        let mut proof_att = row(Pointer::Issue { id: "iss_cc22".into() }, Lane::Done, "Docs");
        proof_att.attachments = vec![Attachment {
            id: "att_2".into(),
            role: AttachRole::Proof,
            pointer: Pointer::Link { url: "https://ci.example.com/runs/42".into() },
            live: None,
            block: None,
        }];
        let zones = lanes(vec![
            (Lane::Next, vec![row(Pointer::None, Lane::Next, "Decide the name"), row(Pointer::None, Lane::Next, "")]),
            (Lane::Doing, vec![doing]),
            (Lane::Done, vec![ci, by_hand, proof_att]),
        ]);
        let text = render(&view(zones));
        let want = format!(
            "{}\n  ○                    Decide the name\n  ○\n{}\n  ◐ iss_bb11           Wiring the new tokenizer through every caller, t…\n      with m1/7\n{}\n{}\n  ● fi_aa22            [proven by CI] Release build\n  ● iss_1a2b3c4d       [proven] Parser rewrite landed\n  ● iss_cc22           [not proven yet] Docs\n      with proof ci.example.com/runs/42",
            head("Next", "2"),
            head("Doing", "1"),
            head("Review", "0"),
            head("Done", "3 (2 proven)")
        );
        assert_eq!(text, want);
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A board's goal, issue progress and done note head its text, and a
    /// board with none of them renders as before.
    #[test]
    fn a_goal_progress_and_done_note_head_the_text() {
        let mut v = view(lanes(vec![(Lane::Next, vec![row(Pointer::None, Lane::Next, "x")])]));
        let plain = render(&v);
        assert!(plain.starts_with("Next "), "{plain}");
        v.board.goal =
            "\nShip the thing to everyone, and make it easy to find the thing again later on\nsecond line".into();
        v.board.progress = BoardProgress { done: 3, total: 5, proven: 2 };
        v.board.closed_at = Some(9);
        v.board.done_note = "People can sign up.\nMore.".into();
        let text = render(&v);
        let want = "\
Goal: Ship the thing to everyone, and make it easy to find the thing ag…
Issues  ██████░░░░ 3/5, 2 proven
Done: People can sign up.
";
        assert!(text.starts_with(want), "{text}");
        assert_eq!(text.strip_prefix(want), Some(plain.as_str()));
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A row on an issue or a PR ends with its assignees as `(@a, @b)`, and
    /// the note is what gives way when the line is full.
    #[test]
    fn a_row_with_assignees_ends_with_them() {
        let on = |note: &str, who: &[&str]| {
            let mut r = row(Pointer::Issue { id: "iss_1a2b3c4d5e6f7a8b".into() }, Lane::Next, note);
            r.live = Some(Live {
                state: LiveState::Open,
                kind: "native".into(),
                repo: None,
                number: 4,
                title: "Fix login".into(),
                assignees: who.iter().map(|w| (*w).to_owned()).collect(),
                provider: None,
                identifier: None,
                state_name: None,
                url: None,
            });
            r
        };
        let long = "Fix the login redirect for every browser we support, then the tests";
        let zones = lanes(vec![(
            Lane::Next,
            vec![on("Fix login", &["ana", "Bo Li"]), on(long, &["ana"]), on("Fix login", &[])],
        )]);
        let text = render(&view(zones));
        let want = format!(
            "{}\n  ○ iss_1a2b3c4d       Fix login (@ana, @Bo_Li)\n  ○ iss_1a2b3c4d       Fix the login redirect for every browser… (@ana)\n  ○ iss_1a2b3c4d       Fix login\n",
            head("Next", "3")
        );
        assert!(text.starts_with(&want), "{text}");
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A block's row carries what its machine reported under it, and the
    /// body that reports it takes no field the ruling doesn't list.
    #[test]
    fn a_block_row_shows_its_reported_state() {
        let mut r = row(Pointer::Block { machine: "m1".into(), pane: "7".into() }, Lane::Doing, "Tokenizer");
        r.block = Some(BlockState {
            state: BlockWord::NeedsInput,
            branch: Some("feat/tokenizer".into()),
            head: Some("abc1234".into()),
            pushed: Some(false),
            uncommitted: Some(2),
            diffstat: Some(Diffstat { files: 3, added: 40, removed: 2 }),
        });
        let text = render(&view(lanes(vec![(Lane::Doing, vec![r])])));
        let want = format!(
            "{}\n  ◐ m1/7               Tokenizer\n      needs input · feat/tokenizer@abc1234 · unpushed · 2 uncommitted ·…",
            head("Doing", "1")
        );
        assert!(text.contains(&want), "{text}");
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");

        let extra = serde_json::json!({ "pane": "7", "block": { "state": "idle", "message": "hello" } });
        assert!(serde_json::from_value::<PutBlockState>(extra).is_err(), "no field for what an agent said");
    }

    /// A board from a control before epics has no goal, note or progress,
    /// and reads as open.
    #[test]
    fn a_board_from_before_epics_reads_as_open() {
        let old = serde_json::json!({
            "id": "brd_1", "project": "prj_1", "title": "Launch", "created_by": "a", "created_at": 1, "updated_at": 2
        });
        let b: Board = serde_json::from_value(old).unwrap();
        assert_eq!(
            (b.goal.as_str(), b.closed_at, b.progress),
            ("", None, BoardProgress { done: 0, total: 0, proven: 0 })
        );
    }

    /// A row from a control before lanes has no lane, attachments or proof,
    /// and reads as next.
    #[test]
    fn a_row_from_before_lanes_reads_as_next() {
        let old = serde_json::json!({
            "id": "row_1", "rev": 1, "pointer": { "kind": "none" }, "note": "x", "status": "done",
            "created_at": 1, "updated_at": 2
        });
        let r: Row = serde_json::from_value(old).unwrap();
        assert_eq!((r.lane, r.attachments.len(), r.proof), (Lane::Next, 0, None));
    }

    /// What each role takes: work is a PR, a block or a run; proof is a run
    /// or a link.
    #[test]
    fn a_role_takes_its_pointers() {
        let fi = Pointer::ForgeItem { id: "fi_1".into() };
        let block = Pointer::Block { machine: "m".into(), pane: "1".into() };
        let run = Pointer::Run { id: "run_1".into() };
        let link = Pointer::Link { url: "https://x.example/1".into() };
        assert!([&fi, &block, &run].iter().all(|p| AttachRole::Work.takes(p)));
        assert!(
            ![&link, &Pointer::Issue { id: "iss_1".into() }, &Pointer::None].iter().any(|p| AttachRole::Work.takes(p))
        );
        assert!([&run, &link].iter().all(|p| AttachRole::Proof.takes(p)));
        assert!(![&fi, &block].iter().any(|p| AttachRole::Proof.takes(p)));
    }
}
