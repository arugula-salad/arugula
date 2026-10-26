//! Boards (live status board, step 1): a project's board is an ordered
//! list of zones, each an ordered list of rows. A row points at something
//! (a forge item, an issue, a run, a Cloud workspace or an agent block) or
//! at nothing, and carries a one-line note and a status. A zone of kind
//! `proofs` is kept last. The platform keeps them (`crate::db::boards`
//! there); a board's thread is its object thread (`brd_`, M196).
//!
//! A project in a path is its id (`prj_…`), or `personal` for the reader's
//! own. Behind the `boards` Labs flag: with it off, every path answers 404.
//!
//! **Revs.** A zone's or row's `rev` counts changes to its title, pointer
//! and note. A write that changes those names the `rev` it read; if the
//! entry moved on, it is refused with 409 (`stale_rev`) and the client
//! reads the board again. A write of `status` alone needs no `rev` and
//! leaves it as it is, so a status update never conflicts with a note. A
//! move may name a `rev` and doesn't change it.

use serde::{Deserialize, Serialize};

/// `GET`: a project's boards, [`BoardList`]; `POST` [`NewBoard`]: make one,
/// answered [`BoardView`].
pub const PROJECT_BOARDS: &str = "/api/projects/{project}/boards";
/// `GET`: a board with its zones and rows in order, [`BoardView`].
pub const BOARD: &str = "/api/boards/{id}";
/// `PUT` [`PutEntry`]: make or change a zone or a row, answered with the
/// board, [`BoardView`].
pub const BOARD_ENTRIES: &str = "/api/boards/{id}/entries";
/// `DELETE`: a row, or a zone and its rows; answered [`BoardView`].
pub const BOARD_ENTRY: &str = "/api/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: reorder a row or a zone; answered [`BoardView`].
pub const BOARD_ENTRY_MOVE: &str = "/api/boards/{id}/entries/{entry}/move";

/// The longest title, in characters, and the longest note: one line each.
pub const TITLE_MAX: usize = 120;
pub const NOTE_MAX: usize = 280;
/// The most boards in a project, zones in a board and rows in a zone.
pub const BOARDS_MAX: u64 = 100;
pub const ZONES_MAX: u64 = 30;
pub const ROWS_MAX: u64 = 200;

/// What a row points at. `None` is free text. The ids are the platform's:
/// `fi_…` (a mirrored PR or issue), `iss_…`, `run_…`, `hw_…`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Where a row stands. `None` says nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardEntryKind"))]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Zone,
    Row,
}

/// A zone's kind: `proofs` is the board's last zone. Absent: a plain one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardZoneKind"))]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    Proofs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectBoard"))]
pub struct Board {
    pub id: String,
    pub project: String,
    pub title: String,
    /// The account that made it; empty once that account is deleted.
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardList {
    pub project: String,
    pub boards: Vec<Board>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewBoard {
    pub title: String,
}

/// What a row's `fi_` or `iss_` pointer is now, from data control already
/// has (the forge mirror, the issues table): a separate fact, never written
/// over the row's stored `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// The item a row points at, as it is now: what the page draws beside the
/// row's own status, and the label of its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardLive"))]
pub struct Live {
    pub state: LiveState,
    /// `pr` or `issue` (a mirrored one), or `native` for a native issue.
    pub kind: String,
    /// `OWNER/NAME` of a mirrored item; absent for a native issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardRow"))]
pub struct Row {
    pub id: String,
    pub rev: u64,
    pub pointer: Pointer,
    pub note: String,
    pub status: Status,
    /// A `fi_` or `iss_` pointer's state now, when the reader may see the
    /// item ([`BoardView`] is read with it; a write's answer has it too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardZone"))]
pub struct Zone {
    pub id: String,
    pub rev: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ZoneKind>,
    pub rows: Vec<Row>,
}

/// A board with its zones and rows, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardView {
    pub board: Board,
    pub zones: Vec<Zone>,
}

/// [`BOARD_ENTRIES`]' body. Without `id` it makes an entry: `entry` says
/// which. A zone is added before the `proofs` zone (a `proofs` one goes
/// last, and a board has one); a row is added last in `zone`. With `id` it
/// changes that entry: what is absent is left as it is. A change to
/// `title`, `pointer` or `note` names `rev`; a change to `status` alone
/// doesn't.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardPutEntry"))]
pub struct PutEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub rev: Option<u64>,
    #[serde(default)]
    pub entry: Option<EntryKind>,
    /// A new row's zone.
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// A new zone's kind.
    #[serde(default)]
    pub kind: Option<ZoneKind>,
    /// A row's pointer; `{"kind": "none"}` clears it.
    #[serde(default)]
    pub pointer: Option<Pointer>,
    #[serde(default)]
    pub note: Option<String>,
    /// A row's status; `"none"` clears it.
    #[serde(default)]
    pub status: Option<Status>,
}

/// [`BOARD_ENTRY_MOVE`]'s body: put the entry just before `before` (a
/// sibling), or last when there is none. A row may go to another `zone`
/// (its own, if absent); a zone moves among zones, never past `proofs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardMoveEntry"))]
pub struct MoveEntry {
    #[serde(default)]
    pub rev: Option<u64>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub before: Option<String>,
}
