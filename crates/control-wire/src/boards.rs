//! Boards (boards, step 1; the model of aru-125): a project's board is an
//! ordered list of rows, and a row is one piece of work. Its primary item is
//! an issue or a PR; a row with none (or with a link) is a note row. What
//! else the work is (a PR, an agent block, a run) is attached to it, and
//! evidence of it is attached as proof. **Nobody sets a lane:** the lane a
//! row is in (`next`, `doing`, `review`, `done`) follows the live state of
//! its items, and a read answers the board as the four [`ZoneKind::Lane`]
//! zones, always, in that order. What the board's author sets is its
//! **groups**: named, ordered sections (phases, waves, areas) that a row may
//! belong to, answered beside the lanes as [`BoardView::groups`] and
//! [`Row::group`]. The platform keeps the rows
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
/// `GET`: a project's [`BoardSettings`] (members). `PUT` [`BoardSettings`]:
/// set them (owners and editors); answered [`BoardSettings`].
pub const PROJECT_BOARD_SETTINGS: &str = "/api/projects/{project}/board-settings";
/// `POST` [`CloseBoard`]: close the board with a done note (owners and
/// editors); answered [`BoardView`]. Its issues and rows stay as
/// they are, and its rows stay editable.
pub const BOARD_CLOSE: &str = "/api/boards/{id}/close";
/// `POST`, no body: open a closed board again, keeping its done note;
/// answered [`BoardView`].
pub const BOARD_REOPEN: &str = "/api/boards/{id}/reopen";
/// `PUT` [`PutEntry`]: add a row or a group, or change a row's note or
/// pointer or a group's title, answered with the board, [`BoardView`].
pub const BOARD_ENTRIES: &str = "/api/boards/{id}/entries";
/// `DELETE`: a row, or a group and its rows; answered [`BoardView`].
pub const BOARD_ENTRY: &str = "/api/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: change a row's group and its place in it, or move a
/// group among groups (owners and editors); answered [`BoardView`]. A lane
/// is never set: a move to one is refused, 400, "lanes follow the work".
pub const BOARD_ENTRY_MOVE: &str = "/api/boards/{id}/entries/{entry}/move";
/// `POST` [`AttachRow`]: attach work or proof to a row (owners and
/// editors); answered [`BoardView`].
pub const BOARD_ATTACHMENTS: &str = "/api/boards/{id}/attachments";
/// `DELETE`: take an attachment off its row; answered [`BoardView`].
pub const BOARD_ATTACHMENT: &str = "/api/boards/{id}/attachments/{attachment}";
/// `GET` [`WorkQuery`]: a project's work items, [`WorkItems`].
/// `POST` [`NewAsk`]: a person asks a question on a row (owners and
/// editors). One open ask per row, and only on a row with an issue or a PR.
pub const BOARD_ROW_ASK: &str = "/api/boards/{id}/rows/{row}/ask";
/// `POST` [`AnswerAsk`]: the person an ask names answers it, or an owner or
/// editor for them. A machine and its agents never do.
pub const BOARD_ASK_ANSWER: &str = "/api/boards/{id}/asks/{ask}/answer";
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
/// `PUT` [`PutEntry`]: add a row or a group, or change a row's note or
/// pointer (a row the agent's machine added, for a pointer) or a group's
/// title.
pub const AGENT_BOARD_ENTRIES: &str = "/api/daemon/boards/{id}/entries";
/// `DELETE`: a row the agent's machine added, or a group that holds only
/// such rows.
pub const AGENT_BOARD_ENTRY: &str = "/api/daemon/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: change a row's group and its place in it, or move a
/// group among groups. A lane is never set.
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

/// `POST` [`NewAsk`]: an agent asks a person a question on a row
/// (`boards.write`). It reads the answer from the board; there is no route
/// for it to answer.
pub const AGENT_BOARD_ROW_ASK: &str = "/api/daemon/boards/{id}/rows/{row}/ask";

/// `POST` [`ResolveItems`]: an agent asks which issues and PRs of the board's
/// project its references (`#N`, `owner/repo#N`, `IDENT-N`) name, before it
/// adds or attaches them (`boards.write`). Nothing changes.
pub const AGENT_BOARD_RESOLVE: &str = "/api/daemon/boards/{id}/resolve";

/// `PUT` [`PutBlockState`]: a machine tells control the state of one of its
/// own agent blocks, for the rows that point at it. The machine is the one
/// that signed the request, never a field of the body.
pub const AGENT_BLOCK_STATE: &str = "/api/daemon/block-state";
/// `POST` [`AttachBlock`]: a machine's agent block was started on an issue
/// (`start_agent`'s `issue`): attach it, as work, to that issue's row on
/// every board of the project; answered [`BlockAttached`]. Needs the
/// machine's `boards.write` over the project; the block is the signing
/// machine's own.
pub const AGENT_PROJECT_ATTACH_BLOCK: &str = "/api/daemon/projects/{project}/attach-block";

/// `POST` [`CheckClaim`]: before a pane is opened on an issue, ask whether
/// another machine's block holds its row on any board of the project;
/// answered [`ClaimChecked`]. Read-only; needs the machine's `boards.write`.
pub const AGENT_PROJECT_CLAIM_CHECK: &str = "/api/daemon/projects/{project}/claim-check";

/// [`AGENT_PROJECT_CLAIM_CHECK`] for `project`.
pub fn agent_project_claim_check(project: &str) -> String {
    format!("/api/daemon/projects/{project}/claim-check")
}

/// [`AGENT_PROJECT_ATTACH_BLOCK`] for `project`.
pub fn agent_project_attach_block(project: &str) -> String {
    format!("/api/daemon/projects/{project}/attach-block")
}

/// `POST` [`NewRelation`]: relate two items of the project (owners and
/// editors): answered [`Relation`]. An item is an issue (`iss_…`) or a
/// mirrored item (`fi_…`) of the project; a `blocks` edge that closes a
/// cycle, or an edge to the item itself, is refused.
pub const PROJECT_RELATIONS: &str = "/api/projects/{project}/relations";
/// `DELETE`: take a relation off (owners and editors). One that comes from
/// Plane or GitHub is refused, 403: it is changed there.
pub const PROJECT_RELATION: &str = "/api/projects/{project}/relations/{relation}";
/// `POST` [`NewRelation`]: an agent relates two items (`issues.write` over
/// the project); the edge says an agent made it.
pub const AGENT_PROJECT_RELATIONS: &str = "/api/daemon/projects/{project}/relations";
/// `DELETE`: an agent takes off a relation its own machine made.
pub const AGENT_PROJECT_RELATION: &str = "/api/daemon/projects/{project}/relations/{relation}";

/// [`PROJECT_RELATIONS`] for `project`.
pub fn project_relations(project: &str) -> String {
    format!("/api/projects/{project}/relations")
}

/// [`PROJECT_RELATION`] for `relation` of `project`.
pub fn project_relation(project: &str, relation: &str) -> String {
    format!("/api/projects/{project}/relations/{relation}")
}

/// [`AGENT_PROJECT_RELATIONS`] for `project`.
pub fn agent_project_relations(project: &str) -> String {
    format!("/api/daemon/projects/{project}/relations")
}

/// [`AGENT_PROJECT_RELATION`] for `relation` of `project`.
pub fn agent_project_relation(project: &str, relation: &str) -> String {
    format!("/api/daemon/projects/{project}/relations/{relation}")
}

/// [`BOARD_ROW_ASK`] for `row` of `id`.
pub fn board_row_ask(id: &str, row: &str) -> String {
    format!("/api/boards/{id}/rows/{row}/ask")
}

/// [`BOARD_ASK_ANSWER`] for `ask` of `id`.
pub fn board_ask_answer(id: &str, ask: &str) -> String {
    format!("/api/boards/{id}/asks/{ask}/answer")
}

/// [`AGENT_BOARD_ROW_ASK`] for `row` of `id`.
pub fn agent_board_row_ask(id: &str, row: &str) -> String {
    format!("/api/daemon/boards/{id}/rows/{row}/ask")
}

/// [`AGENT_BOARD_RESOLVE`] for `id`.
pub fn agent_board_resolve(id: &str) -> String {
    format!("/api/daemon/boards/{id}/resolve")
}

/// The most references one [`AGENT_BOARD_RESOLVE`] takes.
pub const RESOLVE_MAX: usize = 50;

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

/// What a refused lane or status write says first: the rest of the sentence
/// says what to do instead.
pub const LANES_FOLLOW: &str = "lanes follow the work";

/// The reason of a 409 that refuses an agent block a row another machine's
/// block holds (Arugula #404): the sentence names who holds it. `take`
/// takes the row over.
pub const CLAIMED: &str = "claimed";

/// The longest title, in characters, and the longest note: one line each.
pub const TITLE_MAX: usize = 120;
/// A board's goal or done note, in characters.
pub const GOAL_MAX: usize = 2000;
pub const NOTE_MAX: usize = 280;
/// The most boards in a project, groups in a board, rows in a board and
/// attachments on a row.
pub const BOARDS_MAX: u64 = 100;
pub const GROUPS_MAX: u64 = 30;
pub const ROWS_MAX: u64 = 500;
pub const ATTACHMENTS_MAX: u64 = 20;
/// How long, in minutes, a Doing row's blocks may sit idle before it is
/// stalled: the default, and the most a project may set (a week).
pub const STALL_MINUTES_DEFAULT: u32 = 30;
pub const STALL_MINUTES_MAX: u32 = 10_080;
/// The longest link pointer's URL, in bytes.
pub const LINK_MAX: usize = 2048;
/// An ask's question, one line.
pub const ASK_MAX: usize = 200;
/// An ask's why.
pub const ASK_WHY_MAX: usize = 2000;
/// An ask's options, and the longest one.
pub const ASK_OPTIONS_MAX: usize = 4;
pub const ASK_OPTION_MAX: usize = 80;
/// How many answered asks a row carries, newest first.
pub const ASKED_SHOWN: usize = 3;

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
        /// The block's pane, as `%N` or `N`; stored as `N`.
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

/// Zone (a group, in a database; a lane, on the wire) or row.
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
    /// A PR is open, or the issue is open and its PR merged (see
    /// [`Mark::MergedNotClosed`]).
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

/// What a project says about its boards (Arugula #404).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardSettings"))]
#[serde(deny_unknown_fields)]
pub struct BoardSettings {
    /// A Doing row whose blocks have all been idle for longer than this many
    /// minutes (1 to [`STALL_MINUTES_MAX`]) is stalled.
    pub stall_minutes: u32,
}

impl Default for BoardSettings {
    fn default() -> Self {
        BoardSettings { stall_minutes: STALL_MINUTES_DEFAULT }
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
    /// A block that a newer agent took the row from (`take`): it is an
    /// earlier agent whatever it reports, and holds no claim.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub released: bool,
}

impl Attachment {
    /// Whether this is an agent block holding its row: work, reported and
    /// not closed, and not taken over.
    pub fn holds(&self) -> bool {
        self.role == AttachRole::Work
            && !self.released
            && self.block.as_ref().is_some_and(|b| b.state != BlockWord::Closed)
    }
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

/// A note on a row that its lane doesn't say, worked out on every read and
/// never stored (aru-125, decisions 7 to 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardMark"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mark {
    /// The issue is open and a PR of the row merged, with none open: the
    /// work is in, the issue isn't closed. The row is in Review.
    MergedNotClosed,
    /// An open PR of the row is stacked on another open PR: it targets that
    /// PR's branch, so it waits for it. `repo` is left out when the reader
    /// may not read the repo.
    After {
        number: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        repo: Option<String>,
    },
    /// An agent block attached to the row (`attachment`) was closed, and
    /// another may have taken its place: it no longer counts toward Doing.
    Earlier { attachment: String },
    /// The row is in Doing, every block of it that isn't closed has been
    /// idle for longer than the project's `stall_minutes`, and no PR is on
    /// it (Arugula #404). `since` is when the last of them went idle, in
    /// milliseconds. It names no block: who holds the row is the block's
    /// chip, for a reader who may see it.
    Stalled { since: u64 },
    /// The row's item is blocked by an open item (a `blocks` edge, Arugula
    /// #403): the row stays in its lane. `number`, `title` and `repo` are
    /// left out when the reader may not read the blocker (one such mark
    /// stands for all of them: "something you can't see").
    BlockedBy {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        number: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        repo: Option<String>,
    },
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
    /// For an agent block as work on a row another machine's block holds:
    /// take the row over. The old block is folded under `earlier`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub take: bool,
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

/// Where an item is in its own life, the same words for every source
/// (Arugula #405): Plane's state groups, which a native issue's and a GitHub
/// issue's open and closed map onto. A lane reads it: `completed` and
/// `canceled` are Done, every other category is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardCategory"))]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Backlog => "backlog",
            Self::Unstarted => "unstarted",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Canceled => "canceled",
        }
    }

    /// Whether the work is over: completed or canceled.
    pub fn is_done(self) -> bool {
        matches!(self, Self::Completed | Self::Canceled)
    }
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
    /// An issue's status category; a PR and a run have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub category: Option<Category>,
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
    /// The name its starter or its person gave the block: a board's chip
    /// for it. Never the first words of a prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    /// When `state` last changed, in milliseconds: control's to keep, set
    /// on a read and never on the wire. A daemon's report can't carry it,
    /// and a released daemon refuses a block state that has it.
    #[serde(skip)]
    pub since: Option<u64>,
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

/// A question on a row that a person answers on the board (Arugula #367):
/// asked by a person or an agent, or raised by the board itself from the
/// state of the row's items (`raised`, never stored, and with no answer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAsk"))]
pub struct Ask {
    /// `ask_…`; a raised ask has none.
    pub id: String,
    /// The account it is for; empty for a raised ask.
    pub who: String,
    /// Who it is for, as the reader's page names them.
    pub who_name: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub why: Option<String>,
    /// None: free text.
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub recommend: Option<String>,
    /// Who asked, as a name: a person, or an agent and its machine. Empty
    /// for a raised ask.
    pub asked_by: String,
    /// The board raised it, from its items' state, and it clears with its
    /// cause: nobody answers it.
    #[serde(default)]
    pub raised: bool,
    /// It is for the reader (a raised ask is for whoever reads the row).
    #[serde(default)]
    pub mine: bool,
    /// The reader may answer it: the person it names, or an owner or editor.
    #[serde(default)]
    pub can_answer: bool,
    pub created_at: u64,
}

/// An ask that was answered, kept in the row's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAnsweredAsk"))]
pub struct AnsweredAsk {
    pub id: String,
    pub question: String,
    pub answer: String,
    pub asked_by: String,
    pub answered_by: String,
    pub answered_at: u64,
}

/// What an edge between two work items says (Arugula #403). Only `blocks`
/// gates: it keeps the blocked item's row out of the ready set until the
/// blocker is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "RelationKind"))]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// The first item blocks the second.
    Blocks,
    Relates,
    /// The first item duplicates the second.
    DuplicateOf,
    /// The first item was found while working on the second.
    DiscoveredFrom,
}

impl RelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationKind::Blocks => "blocks",
            RelationKind::Relates => "relates",
            RelationKind::DuplicateOf => "duplicate_of",
            RelationKind::DiscoveredFrom => "discovered_from",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "blocks" => RelationKind::Blocks,
            "relates" => RelationKind::Relates,
            "duplicate_of" => RelationKind::DuplicateOf,
            "discovered_from" => RelationKind::DiscoveredFrom,
            _ => return None,
        })
    }

    /// Whether an open item on the first end keeps the second out of the
    /// ready set.
    pub fn blocks_ready(self) -> bool {
        self == RelationKind::Blocks
    }
}

/// Who made an edge. `plane` and `github` edges are read from there, and
/// Arugula doesn't change them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "RelationSource"))]
#[serde(rename_all = "snake_case")]
pub enum RelationSource {
    Person,
    Agent,
    Plane,
    Github,
}

impl RelationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationSource::Person => "person",
            RelationSource::Agent => "agent",
            RelationSource::Plane => "plane",
            RelationSource::Github => "github",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "person" => RelationSource::Person,
            "agent" => RelationSource::Agent,
            "plane" => RelationSource::Plane,
            "github" => RelationSource::Github,
            _ => return None,
        })
    }
}

/// [`PROJECT_RELATIONS`]' body: `from` `kind` `to`, each end an `iss_` or
/// `fi_` id of the project (`a` blocks `b`: from a, to b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "NewRelation"))]
#[serde(deny_unknown_fields)]
pub struct NewRelation {
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
}

/// An edge made, as [`PROJECT_RELATIONS`] answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "Relation"))]
pub struct Relation {
    /// `rel_…`.
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
    pub source: RelationSource,
    pub created_at: u64,
}

/// Which end of an edge a row's item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "RelationDirection"))]
#[serde(rename_all = "snake_case")]
pub enum RelationDirection {
    /// The row's item is the first end: it blocks, duplicates, was found from.
    Out,
    /// The row's item is the second end: it is blocked by the other.
    In,
}

/// An edge of a row's item, as the reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardRowRelation"))]
pub struct RowRelation {
    /// `rel_…`, to take it off.
    pub id: String,
    pub kind: RelationKind,
    pub direction: RelationDirection,
    pub source: RelationSource,
    /// The item at the other end. Left out when the reader may not read it:
    /// the edge is "something you can't see", with no title or number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub other: Option<Live>,
    /// The `iss_` or `fi_` id of the item at the other end, with `other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub item: Option<String>,
    /// The reader may take it off: an owner or editor, and it was made here.
    pub removable: bool,
}

/// [`BOARD_ROW_ASK`]'s body: `who` is an account of the project. `options`
/// are 0 to [`ASK_OPTIONS_MAX`] (none: free text), and `recommend` is one
/// of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardNewAsk"))]
#[serde(deny_unknown_fields)]
pub struct NewAsk {
    pub who: String,
    pub question: String,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub recommend: Option<String>,
}

/// [`BOARD_ASK_ANSWER`]'s body: one of the ask's `option`s, or `text` when
/// it has none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAnswerAsk"))]
#[serde(deny_unknown_fields)]
pub struct AnswerAsk {
    #[serde(default)]
    pub option: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

/// An issue as a starter writes it: `#N`, `owner/repo#N` or `IDENT-N`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueRef {
    /// `#N`: a native issue of the project, else a GitHub issue of one of
    /// its repos.
    Number(u64),
    /// `owner/repo#N`.
    Repo { repo: String, number: u64 },
    /// `IDENT-N`: a Plane item; the key in capitals.
    Plane { key: String, number: u64 },
}

impl IssueRef {
    /// The reference `text` writes, or none when it isn't one.
    pub fn parse(text: &str) -> Option<IssueRef> {
        let text = text.trim();
        let number = |n: &str| n.parse::<u64>().ok().filter(|n| *n > 0);
        let digits = |n: &str| !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit());
        if let Some((left, n)) = text.rsplit_once('#') {
            if !digits(n) {
                return None;
            }
            let number = number(n)?;
            if left.is_empty() {
                return Some(IssueRef::Number(number));
            }
            let name = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
            let (owner, repo) = left.split_once('/')?;
            return (name(owner) && name(repo)).then(|| IssueRef::Repo { repo: left.to_owned(), number });
        }
        let (key, n) = text.rsplit_once('-')?;
        let key_ok = key.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
            && key.bytes().all(|b| b.is_ascii_alphanumeric());
        if !(key_ok && digits(n)) {
            return None;
        }
        Some(IssueRef::Plane { key: key.to_ascii_uppercase(), number: number(n)? })
    }

    /// The issue's number, for a branch name.
    pub fn number(&self) -> u64 {
        match self {
            IssueRef::Number(n) | IssueRef::Repo { number: n, .. } | IssueRef::Plane { number: n, .. } => *n,
        }
    }
}

/// [`AGENT_PROJECT_ATTACH_BLOCK`]'s body: the issue as the starter wrote it
/// (`#N`, `owner/repo#N` or `IDENT-N`) and the block, which is on the
/// signing machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardAttachBlock"))]
#[serde(deny_unknown_fields)]
pub struct AttachBlock {
    pub issue: String,
    pub machine: String,
    pub pane: String,
    /// Take a row that another machine's block holds, folding that block
    /// under `earlier`; without it the attach is refused, 409 `claimed`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub take: bool,
}

/// [`AGENT_PROJECT_CLAIM_CHECK`]'s body: the issue as the starter wrote it
/// and the asking machine (the signer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardCheckClaim"))]
#[serde(deny_unknown_fields)]
pub struct CheckClaim {
    pub issue: String,
    pub machine: String,
}

/// [`AGENT_PROJECT_CLAIM_CHECK`]'s answer: `held` says who holds the row
/// (the sentence of the 409 at attach), none when nothing does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardClaimChecked"))]
pub struct ClaimChecked {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub held: Option<String>,
}

/// [`AGENT_BOARD_RESOLVE`]'s body: up to [`RESOLVE_MAX`] references, each
/// `#N`, `owner/repo#N`, `IDENT-N` or an `iss_`/`fi_` id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardResolveItems"))]
#[serde(deny_unknown_fields)]
pub struct ResolveItems {
    pub items: Vec<String>,
}

/// One reference, resolved: the `pointer` it names, or `why` it names none
/// (nothing by that number, or more than one thing: the answer names them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardResolution"))]
pub struct Resolution {
    /// The reference as it was written.
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pointer: Option<Pointer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub why: Option<String>,
}

/// [`AGENT_BOARD_RESOLVE`]'s answer, one for each reference, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardResolved"))]
pub struct Resolved {
    pub items: Vec<Resolution>,
}

/// One row a block was attached to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardBlockOn"))]
pub struct BlockOn {
    pub board: String,
    pub title: String,
    pub row: String,
}

/// [`AGENT_PROJECT_ATTACH_BLOCK`]'s answer: the rows the block is on now
/// (also those it was on already), and none when no board of the project
/// has a row for the issue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardBlockAttached"))]
pub struct BlockAttached {
    pub on: Vec<BlockOn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardRow"))]
pub struct Row {
    pub id: String,
    pub rev: u64,
    pub pointer: Pointer,
    pub note: String,
    /// Kept for old clients: `needs_input` when the row has an open ask
    /// (#367), `none` otherwise. Nothing writes it.
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
    /// The group (`zon_…`) the row belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub group: Option<String>,
    /// What the lane doesn't say, in a fixed order: `merged_not_closed`,
    /// `after`, `blocked_by`, then one `earlier` for each closed block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    /// The row's one open question: asked, else raised by the board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ask: Option<Ask>,
    /// The last [`ASKED_SHOWN`] asks answered, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<AnsweredAsk>,
    /// The edges of the row's item, both ways, for the drawer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RowRelation>,
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

/// A named, ordered section of a board that its author sets: a phase, a
/// wave, an area. A row belongs to at most one ([`Row::group`]); a group
/// holds no lane, and a row's lane is the same in any group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardGroup"))]
pub struct Group {
    /// `zon_…`.
    pub id: String,
    /// Counts changes to its title.
    pub rev: u64,
    pub title: String,
    /// Its rows with an issue or a PR, counted as a board's are.
    #[serde(default)]
    pub progress: BoardProgress,
}

/// A board with its four lanes, in order (`lane_next`, `lane_doing`,
/// `lane_review`, `lane_done`), each with its rows, and its groups, in
/// order. The rows of a group are those with [`Row::group`] set to its id; a
/// row without one comes after the grouped ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardView {
    pub board: Board,
    pub zones: Vec<Zone>,
    /// Empty for a board with none; an old client ignores it.
    #[serde(default)]
    pub groups: Vec<Group>,
    /// The ready set (Arugula #403): the rows (`row_…`) with an issue or a
    /// PR as their item that are in Next and have no open blocker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ready: Vec<String>,
    /// What a write did that the board alone doesn't say: a PR attached to
    /// the row of the issue it closes, an item that was there already.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub said: Option<String>,
}

/// [`BOARD_ENTRIES`]' body. Without `id` it adds a row, last (`entry` is
/// `row` or absent), or, with `entry: zone`, a group with a `title`, last;
/// with `id` it changes that row or group: what is absent is left as it is,
/// and a change to `pointer`, `note` or a group's `title` names `rev`. A
/// new row's `zone` is the group it goes in. `kind`, and a `zone` that is a
/// lane, are refused, and so is every `status` (aru-125: lanes follow the work;
/// a question for a person is an ask, #367). The row's
/// `pointer` is an issue or a PR (or a link, for a note).
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
    /// A new row's group (`zon_…`).
    #[serde(default)]
    pub zone: Option<String>,
    /// A group's title.
    #[serde(default)]
    pub title: Option<String>,
    /// Refused: a group has no kind.
    #[serde(default)]
    pub kind: Option<ZoneKind>,
    /// A row's pointer; `{"kind": "none"}` clears it.
    #[serde(default)]
    pub pointer: Option<Pointer>,
    #[serde(default)]
    pub note: Option<String>,
    /// Refused: ask with a question instead.
    #[serde(default)]
    pub status: Option<Status>,
}

/// [`BOARD_ENTRY_MOVE`]'s body. A row moves to a group (`zone`) or, with
/// `zone: null`, out of its group; with `zone` absent it stays where it is.
/// It goes before `before` (a row of the place it goes to), last without
/// one. A group moves among groups:
/// `before` is a group, `zone` is absent. A lane is never set (aru-125): a
/// `zone` that is a lane is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardMoveEntry"))]
pub struct MoveEntry {
    #[serde(default)]
    pub rev: Option<u64>,
    /// The group the row goes to; `null`: none. Absent: its own.
    #[serde(default, deserialize_with = "some")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "string | null"))]
    pub zone: Option<Option<String>>,
    #[serde(default)]
    pub before: Option<String>,
}

/// A field that is there, `null` or not, is `Some`: so `null` and absent
/// differ.
fn some<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
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
    /// How the row's item is written to address it: `#N` for a native or a
    /// GitHub issue or PR, `IDENT-N` for a Plane item, else the pointer's
    /// short label.
    pub fn item_label(&self) -> String {
        item_label(&self.pointer, self.live.as_ref())
    }

    /// Whether the row has a primary item to count: not a note row.
    pub fn counted(&self) -> bool {
        matches!(self.pointer, Pointer::Issue { .. } | Pointer::ForgeItem { .. })
    }
}

/// [`Row::item_label`] for any pointer and its state.
pub fn item_label(pointer: &Pointer, live: Option<&Live>) -> String {
    match (pointer, live) {
        (Pointer::Issue { .. } | Pointer::ForgeItem { .. }, Some(l)) if l.number > 0 => {
            match (&l.provider, &l.identifier) {
                (Some(_), Some(key)) => format!("{key}-{}", l.number),
                _ => format!("#{}", l.number),
            }
        }
        _ => pointer.label(),
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

/// `42m` or `3h5m` or `2d`: how long ago `since` was, at `now` (both in
/// milliseconds), to the minute.
pub fn ago(since: u64, now: u64) -> String {
    let m = now.saturating_sub(since) / 60_000;
    match (m / 1440, m / 60 % 24, m % 60) {
        (0, 0, m) => format!("{m}m"),
        (0, h, 0) => format!("{h}h"),
        (0, h, m) => format!("{h}h{m}m"),
        (d, _, _) => format!("{d}d"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Mark {
    /// `merged, not closed`, `after #1859`, `blocked by #1860`, `earlier agent`,
    /// `stalled 42m`.
    fn words(&self) -> String {
        match self {
            Mark::MergedNotClosed => "merged, not closed".to_owned(),
            Mark::After { number, .. } => format!("after #{number}"),
            Mark::Earlier { .. } => "earlier agent".to_owned(),
            Mark::Stalled { since } => format!("stalled {}", ago(*since, now_ms())),
            Mark::BlockedBy { number: Some(n), .. } => format!("blocked by #{n}"),
            Mark::BlockedBy { number: None, .. } => "blocked by something you can't see".to_owned(),
        }
    }
}

/// The words of a row's marks, with its closed blocks counted:
/// `merged, not closed · after #1859 · earlier agents (2)`.
pub fn marks_line(marks: &[Mark]) -> String {
    let earlier = marks.iter().filter(|m| matches!(m, Mark::Earlier { .. })).count();
    let mut parts: Vec<String> = marks.iter().filter(|m| !matches!(m, Mark::Earlier { .. })).map(Mark::words).collect();
    if earlier > 0 {
        parts.push(format!("earlier agents ({earlier})"));
    }
    parts.join(" · ")
}

/// One lane of a board's text: its title and count, then its rows.
fn lane_text<'a>(out: &mut String, title: &str, rows: impl Iterator<Item = &'a Row>) {
    let rows: Vec<&Row> = rows.collect();
    let proven = rows.iter().filter(|r| r.proof.is_some()).count();
    let count = if rows.iter().any(|r| r.lane == Lane::Done) && proven > 0 {
        format!("{} ({proven} proven)", rows.len())
    } else {
        rows.len().to_string()
    };
    let title = fit(title, TEXT_WIDTH - count.chars().count() - 1);
    let pad = TEXT_WIDTH - title.chars().count() - count.chars().count();
    *out += &format!("{title}{}{count}\n", " ".repeat(pad));
    for r in rows {
        let label = fit(&r.item_label(), LABEL);
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
        *out += format!("{lead}{note}").trim_end();
        out.push('\n');
        if !r.marks.is_empty() {
            *out += &format!("      {}\n", fit(&marks_line(&r.marks), TEXT_WIDTH - 6));
        }
        if let Some(b) = r.attachments.iter().find(|a| a.holds()).and_then(|a| a.block.as_ref()) {
            let who = b.title.as_deref().unwrap_or("an agent");
            *out += &format!("      {}\n", fit(&format!("claimed by {who}"), TEXT_WIDTH - 6));
        }
        if !r.attachments.is_empty() {
            let with: Vec<String> = r
                .attachments
                .iter()
                .map(|a| {
                    let role = if a.role == AttachRole::Proof { "proof " } else { "" };
                    format!("{role}{}", a.pointer.label())
                })
                .collect();
            *out += &format!("      {}\n", fit(&format!("with {}", with.join(", ")), TEXT_WIDTH - 6));
        }
        if let Some(a) = &r.ask {
            let by = if a.raised { "the board".to_owned() } else { format!("for {}", a.who_name) };
            *out += &format!("      {}\n", fit(&format!("? {} ({by})", a.question), TEXT_WIDTH - 6));
        }
        if r.ask.is_none()
            && let Some(a) = r.asked.first()
        {
            let line = format!("answered: {} → {}", a.question, a.answer);
            *out += &format!("      {}\n", fit(&line, TEXT_WIDTH - 6));
        }
        if let Some(b) = &r.block {
            // Under the row, in line with its label.
            *out += &format!("      {}\n", fit(&b.line(), TEXT_WIDTH - 6));
        }
    }
}

/// A board as plain text, no wider than [`TEXT_WIDTH`] columns: before the
/// lanes, `Goal:` with the goal's first line, `Issues` with the board's
/// progress when it has any, and `Done:` with the done note's first line
/// when it is closed, and `Ready (n):` with the labels of the rows that can
/// start now when there are any (#403). Then each lane is its title and how many rows it holds
/// (the Done lane also how many are proven), and its rows, each a glyph, a
/// short pointer label and the note (cut to fit). Under a row: what is
/// attached to it, and under an agent block's row what its machine reported
/// of it. A Done row starts its note with `[proven by CI]`, `[proven]` or
/// `[not proven yet]`, and a note row (no issue or PR) has no mark. Ids and
/// revs aren't in it: they are in the [`BoardView`]. Under a row, its marks
/// ([`marks_line`]). A board with groups is a `# Title  done/total` header
/// for each group, and the lanes that hold its rows under it; rows with no
/// group come last under `# No group`.
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
    let ready: Vec<String> = view
        .ready
        .iter()
        .filter_map(|id| view.zones.iter().flat_map(|z| &z.rows).find(|r| &r.id == id))
        .map(|r| r.pointer.label())
        .collect();
    if !ready.is_empty() {
        out += &format!("{}\n", fit(&format!("Ready ({}): {}", ready.len(), ready.join(", ")), TEXT_WIDTH));
    }
    if view.groups.is_empty() {
        for z in &view.zones {
            lane_text(&mut out, &z.title, z.rows.iter());
        }
    } else {
        // Each group, then the rows with none; under each, the lanes that
        // hold any of its rows.
        let sections =
            view.groups.iter().map(|g| (Some(g), g.title.as_str())).chain(std::iter::once((None, "No group")));
        for (group, title) in sections {
            let mine = |r: &&Row| r.group.as_deref() == group.map(|g| g.id.as_str());
            let n = view.zones.iter().flat_map(|z| &z.rows).filter(mine).count();
            if group.is_none() && n == 0 {
                continue;
            }
            let count = match group.map(|g| g.progress).filter(|p| p.total > 0) {
                Some(p) => format!("{}/{}", p.done, p.total),
                None => n.to_string(),
            };
            let head = fit(title, TEXT_WIDTH - count.chars().count() - 3);
            let pad = TEXT_WIDTH - head.chars().count() - count.chars().count() - 3;
            out += &format!("# {head}{}{count}\n", " ".repeat(pad + 1));
            for z in &view.zones {
                if z.rows.iter().any(|r| mine(&r)) {
                    lane_text(&mut out, &z.title, z.rows.iter().filter(mine));
                }
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

    /// The three forms a starter writes an issue in, and what isn't one.
    #[test]
    fn an_issue_is_written_as_a_number_a_repo_number_or_a_key() {
        assert_eq!(IssueRef::parse(" #372 "), Some(IssueRef::Number(372)));
        let repo = IssueRef::parse("arugula-salad/arugula#12");
        assert_eq!(repo, Some(IssueRef::Repo { repo: "arugula-salad/arugula".into(), number: 12 }));
        assert_eq!(IssueRef::parse("arugu-7"), Some(IssueRef::Plane { key: "ARUGU".into(), number: 7 }));
        assert_eq!(IssueRef::parse("ARUGU-7").map(|r| r.number()), Some(7));
        for no in ["372", "#", "#0", "#x", "a/b/c#1", "/r#1", "o/#1", "-5", "1A-5", "A-", "A B-5", "#1234567890"] {
            assert_eq!(IssueRef::parse(no), None, "{no}");
        }
    }

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
            group: None,
            marks: vec![],
            ask: None,
            asked: vec![],
            relations: vec![],
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
        BoardView { board: board(), zones, groups: vec![], ready: vec![], said: None }
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
            released: false,
        }];
        let mut proof_att = row(Pointer::Issue { id: "iss_cc22".into() }, Lane::Done, "Docs");
        proof_att.attachments = vec![Attachment {
            id: "att_2".into(),
            role: AttachRole::Proof,
            pointer: Pointer::Link { url: "https://ci.example.com/runs/42".into() },
            live: None,
            block: None,
            released: false,
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
    /// the note is what gives way when the line is full. It is labelled by
    /// its item (`#4`), as an agent addresses it.
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
                category: None,
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
            "{}\n  ○ #4                 Fix login (@ana, @Bo_Li)\n  ○ #4                 Fix the login redirect for every browser… (@ana)\n  ○ #4                 Fix login\n",
            head("Next", "3")
        );
        assert!(text.starts_with(&want), "{text}");
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A block's row carries what its machine reported under it, and the
    /// body that reports it takes no field the ruling doesn't list.
    #[test]
    fn a_row_shows_its_ask_and_then_its_last_answer() {
        let mut r = row(Pointer::Issue { id: "iss_1a2b3c4d".into() }, Lane::Review, "Parser");
        r.ask = Some(Ask {
            id: "ask_1".into(),
            who: "acc".into(),
            who_name: "sam".into(),
            question: "Ship it?".into(),
            why: None,
            options: vec!["Yes".into(), "No".into()],
            recommend: None,
            asked_by: "an agent on m1".into(),
            raised: false,
            mine: false,
            can_answer: false,
            created_at: 0,
        });
        let text = render(&view(lanes(vec![(Lane::Review, vec![r.clone()])])));
        assert!(text.contains("      ? Ship it? (for sam)\n"), "{text}");
        r.ask = None;
        r.asked = vec![AnsweredAsk {
            id: "ask_1".into(),
            question: "Ship it?".into(),
            answer: "Yes".into(),
            asked_by: "an agent on m1".into(),
            answered_by: "sam".into(),
            answered_at: 1,
        }];
        let text = render(&view(lanes(vec![(Lane::Review, vec![r])])));
        assert!(text.contains("      answered: Ship it? → Yes\n"), "{text}");
    }

    #[test]
    fn a_block_row_shows_its_reported_state() {
        let mut r = row(Pointer::Block { machine: "m1".into(), pane: "7".into() }, Lane::Doing, "Tokenizer");
        r.block = Some(BlockState {
            title: None,
            state: BlockWord::NeedsInput,
            branch: Some("feat/tokenizer".into()),
            head: Some("abc1234".into()),
            pushed: Some(false),
            uncommitted: Some(2),
            diffstat: Some(Diffstat { files: 3, added: 40, removed: 2 }),
            since: None,
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

    /// A row's marks are a line under it, with the closed blocks counted.
    #[test]
    fn marks_are_a_line_under_the_row() {
        let mut r = row(Pointer::Issue { id: "iss_1".into() }, Lane::Review, "Fix login");
        r.marks = vec![
            Mark::MergedNotClosed,
            Mark::After { number: 1859, repo: Some("acme/app".into()) },
            Mark::Earlier { attachment: "att_1".into() },
            Mark::Earlier { attachment: "att_2".into() },
        ];
        let text = render(&view(lanes(vec![(Lane::Review, vec![r])])));
        assert!(text.contains("Fix login\n      merged, not closed · after #1859 · earlier agents (2)\n"), "{text}");
        assert_eq!(marks_line(&[Mark::Earlier { attachment: "a".into() }]), "earlier agents (1)");
        assert_eq!(marks_line(&[]), "");
    }

    /// With groups the text is a header for each group (with its progress)
    /// and the lanes that hold its rows under it, then the rows with none.
    #[test]
    fn groups_head_their_lanes_in_the_text() {
        let mut a = row(Pointer::Issue { id: "iss_a".into() }, Lane::Done, "Decided");
        a.group = Some("zon_1".into());
        let mut b = row(Pointer::Issue { id: "iss_b".into() }, Lane::Next, "Build it");
        b.group = Some("zon_2".into());
        let loose = row(Pointer::None, Lane::Next, "Loose end");
        let mut v = view(lanes(vec![(Lane::Next, vec![b, loose]), (Lane::Done, vec![a])]));
        v.groups = vec![
            Group {
                id: "zon_1".into(),
                rev: 1,
                title: "Decide".into(),
                progress: BoardProgress { done: 1, total: 1, proven: 0 },
            },
            Group { id: "zon_2".into(), rev: 1, title: "Build".into(), progress: BoardProgress::default() },
        ];
        let text = render(&v);
        let h = |t: &str, c: &str| format!("{t}{c:>w$}", w = TEXT_WIDTH - t.chars().count());
        let want = [
            h("# Decide", "1/1"),
            h("Done", "1"),
            "  ● iss_a              [not proven yet] Decided".to_owned(),
            h("# Build", "1"),
            h("Next", "1"),
            "  ○ iss_b              Build it".to_owned(),
            h("# No group", "1"),
            h("Next", "1"),
            "  ○                    Loose end".to_owned(),
        ]
        .join("\n");
        assert_eq!(text, want);
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A move's `zone` tells null (out of its group) from absent (stay).
    #[test]
    fn a_move_tells_null_from_absent() {
        let parse = |v: serde_json::Value| serde_json::from_value::<MoveEntry>(v).unwrap();
        assert_eq!(parse(serde_json::json!({})).zone, None);
        assert_eq!(parse(serde_json::json!({ "zone": null })).zone, Some(None));
        assert_eq!(parse(serde_json::json!({ "zone": "zon_1" })).zone, Some(Some("zon_1".into())));
        assert_eq!(
            serde_json::to_value(parse(serde_json::json!({ "zone": null }))).unwrap()["zone"],
            serde_json::Value::Null
        );
        assert!(serde_json::to_value(parse(serde_json::json!({}))).unwrap().get("zone").is_none());
    }

    /// The marks' wire shapes.
    #[test]
    fn marks_are_tagged_by_kind() {
        use serde_json::json;
        let marks = [
            (Mark::MergedNotClosed, json!({ "kind": "merged_not_closed" })),
            (Mark::After { number: 3, repo: None }, json!({ "kind": "after", "number": 3 })),
            (
                Mark::After { number: 3, repo: Some("a/b".into()) },
                json!({ "kind": "after", "number": 3, "repo": "a/b" }),
            ),
            (Mark::Earlier { attachment: "att_1".into() }, json!({ "kind": "earlier", "attachment": "att_1" })),
        ];
        for (m, j) in marks {
            assert_eq!(serde_json::to_value(&m).unwrap(), j);
            assert_eq!(serde_json::from_value::<Mark>(j).unwrap(), m);
        }
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

    /// A blocked row says what blocks it (or that it can't say), and the
    /// ready set is a line of the rows that can start; only `blocks` gates.
    #[test]
    fn the_text_says_what_blocks_a_row_and_what_is_ready() {
        let mut blocked = row(Pointer::Issue { id: "iss_blocked".into() }, Lane::Next, "Build");
        blocked.id = "row_b".into();
        blocked.marks = vec![
            Mark::BlockedBy { number: Some(7), title: Some("Design".into()), repo: None },
            Mark::BlockedBy { number: None, title: None, repo: None },
        ];
        let mut free = row(Pointer::Issue { id: "iss_free".into() }, Lane::Next, "Design");
        free.id = "row_f".into();
        let mut v = view(lanes(vec![(Lane::Next, vec![blocked, free])]));
        v.ready = vec!["row_f".into()];
        let text = render(&v);
        assert!(text.contains("blocked by #7 · blocked by something you can't see"), "{text}");
        assert!(text.contains("Ready (1): iss_free"), "{text}");
        assert!(!text.contains("Ready (2)"), "{text}");
        assert!(RelationKind::Blocks.blocks_ready());
        assert!(
            ![RelationKind::Relates, RelationKind::DuplicateOf, RelationKind::DiscoveredFrom]
                .iter()
                .any(|k| k.blocks_ready())
        );
        v.ready.clear();
        assert!(!render(&v).contains("Ready"), "no ready set, no line");
    }
}
