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

use crate::chat::ChatAgent;

/// `GET`: a project's boards, [`BoardList`]; `POST` [`NewBoard`]: make one,
/// answered [`BoardView`].
pub const PROJECT_BOARDS: &str = "/api/projects/{project}/boards";
/// `GET`: a board with its zones and rows in order, [`BoardView`]. `PATCH`
/// [`EditBoard`]: set its title or goal (people only, owners and editors),
/// answered [`BoardView`].
pub const BOARD: &str = "/api/boards/{id}";
/// `POST` [`CloseBoard`]: close the board with a done note (people only,
/// owners and editors); answered [`BoardView`]. Its issues and rows stay as
/// they are, and its rows stay editable.
pub const BOARD_CLOSE: &str = "/api/boards/{id}/close";
/// `POST`, no body: open a closed board again, keeping its done note;
/// answered [`BoardView`].
pub const BOARD_REOPEN: &str = "/api/boards/{id}/reopen";
/// `PUT` [`PutEntry`]: make or change a zone or a row, answered with the
/// board, [`BoardView`].
pub const BOARD_ENTRIES: &str = "/api/boards/{id}/entries";
/// `DELETE`: a row, or a zone and its rows; answered [`BoardView`].
pub const BOARD_ENTRY: &str = "/api/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: reorder a row or a zone; answered [`BoardView`].
pub const BOARD_ENTRY_MOVE: &str = "/api/boards/{id}/entries/{entry}/move";
/// `GET` [`WorkQuery`]: a project's work items, [`WorkItems`].
pub const PROJECT_WORK: &str = "/api/projects/{project}/work";

/// A machine's agents, under `boards.read` / `boards.write` (over the
/// project, given by the machine's owner): the same operations as above,
/// signed by the machine, never past the owner's role. See
/// `arugula_platform::boards::agent` for what they may not do.
///
/// `GET`: a project's boards ([`BoardList`]); `{project}` is an id or `personal`.
pub const AGENT_PROJECT_BOARDS: &str = "/api/daemon/projects/{project}/boards";
/// `GET`: a board ([`BoardView`]).
pub const AGENT_BOARD: &str = "/api/daemon/boards/{id}";
/// `PUT` [`PutEntry`]: add a row, or change a row's note or status.
pub const AGENT_BOARD_ENTRIES: &str = "/api/daemon/boards/{id}/entries";
/// `DELETE`: a row the agent's machine added.
pub const AGENT_BOARD_ENTRY: &str = "/api/daemon/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: move a row.
pub const AGENT_BOARD_ENTRY_MOVE: &str = "/api/daemon/boards/{id}/entries/{entry}/move";

/// [`AGENT_PROJECT_BOARDS`] for `project`.
pub fn agent_project_boards(project: &str) -> String {
    format!("/api/daemon/projects/{project}/boards")
}

/// [`AGENT_BOARD`] for `id`.
pub fn agent_board(id: &str) -> String {
    format!("/api/daemon/boards/{id}")
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

/// The longest title, in characters, and the longest note: one line each.
pub const TITLE_MAX: usize = 120;
/// A board's goal or done note, in characters.
pub const GOAL_MAX: usize = 2000;
pub const NOTE_MAX: usize = 280;
/// The most boards in a project, zones in a board and rows in a zone.
pub const BOARDS_MAX: u64 = 100;
pub const ZONES_MAX: u64 = 30;
pub const ROWS_MAX: u64 = 200;
/// The longest link pointer's URL, in bytes.
pub const LINK_MAX: usize = 2048;

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
    /// An `https://` URL, as evidence (a CI page, a report). The platform
    /// takes only `https`, no userinfo, at most [`LINK_MAX`] bytes.
    Link {
        url: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectBoard"))]
pub struct Board {
    pub id: String,
    pub project: String,
    pub title: String,
    /// The account that made it; empty once that account is deleted.
    pub created_by: String,
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
    /// The account that closed it; empty once that account is deleted.
    #[serde(default)]
    pub closed_by: String,
    /// How many of its issues are closed.
    #[serde(default)]
    pub progress: BoardProgress,
}

/// A board's issues, counted: the rows on a native issue or a mirrored
/// GitHub issue (not a PR, a run or a link), each issue once, and the ones
/// closed at their source. A row the reader can't read counts toward
/// neither; a row's own status counts for nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardProgress"))]
pub struct BoardProgress {
    pub done: u32,
    pub total: u32,
}

/// `PATCH`: what to change; a field left out stays. An empty goal clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditBoard {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
}

/// `POST` to close a board: what was done, not empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CloseBoard {
    pub done_note: String,
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

/// What a row's `fi_`, `iss_` or `run_` pointer is now, from data control
/// already has (the forge mirror, the issues and runs tables): a separate fact, never written
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
}

/// What an agent block is doing (aru-116's second ruling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardPutBlockState"))]
#[serde(deny_unknown_fields)]
pub struct PutBlockState {
    pub pane: String,
    pub block: BlockState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardRow"))]
pub struct Row {
    pub id: String,
    pub rev: u64,
    pub pointer: Pointer,
    pub note: String,
    pub status: Status,
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

// ---------------------------------------------------------------- text

/// The widest a board's text form goes, in columns.
pub const TEXT_WIDTH: usize = 72;
/// The progress bar's cells, and the room a row's label has.
const BAR: usize = 10;
const LABEL: usize = 18;

impl Status {
    /// The glyph a row shows: `○` open or none, `◐` working, `?` needs
    /// input, `●` done, `✕` failed.
    pub fn glyph(self) -> char {
        match self {
            Status::None | Status::Open => '○',
            Status::Working => '◐',
            Status::NeedsInput => '?',
            Status::Done => '●',
            Status::Failed => '✕',
        }
    }
}

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

impl Zone {
    /// Whether this is the `proofs` zone.
    pub fn is_proofs(&self) -> bool {
        self.kind == Some(ZoneKind::Proofs)
    }
}

impl Row {
    /// Whether the row proves what it says, in a `proofs` zone: its pointer
    /// is a link, a run that succeeded or a `fi_` that is merged. Derived,
    /// never stored over `status`.
    pub fn proven(&self) -> bool {
        let state = self.live.as_ref().map(|l| l.state);
        match &self.pointer {
            Pointer::Link { .. } => true,
            Pointer::Run { .. } => state == Some(LiveState::Succeeded),
            Pointer::ForgeItem { .. } => state == Some(LiveState::Merged),
            _ => false,
        }
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

/// A board as plain text, no wider than [`TEXT_WIDTH`] columns: each zone
/// is its title and a progress bar counting its done rows, then its rows,
/// each a status glyph, a short pointer label and the note (cut to fit),
/// and under an agent block's row what its machine reported of it. A row
/// in the `proofs` zone starts its note with `[proven]` or `[not proven yet]`
/// ([`Row::proven`]).
/// The `proofs` zone comes last. Ids and revs aren't in it: they are in the
/// [`BoardView`].
///
/// ```text
/// Build                                                  ████░░░░░░ 2/5
///   ● iss_1a2b3c4d     Parser rewrite landed
///   ◐ m1/7             Wiring the new tokenizer through the…
/// ```
pub fn render(view: &BoardView) -> String {
    let mut zones: Vec<&Zone> = view.zones.iter().collect();
    // Stable: a plain zone keeps its place, `proofs` goes after them.
    zones.sort_by_key(|z| z.kind == Some(ZoneKind::Proofs));
    let mut out = String::new();
    for z in zones {
        let done = z.rows.iter().filter(|r| r.status == Status::Done).count();
        let filled = if z.rows.is_empty() { 0 } else { done * BAR / z.rows.len() };
        let count = format!("{done}/{}", z.rows.len());
        let bar = format!("{}{} {count}", "█".repeat(filled), "░".repeat(BAR - filled));
        let bar_w = bar.chars().count();
        let title = fit(&z.title, TEXT_WIDTH - bar_w - 1);
        let pad = TEXT_WIDTH - title.chars().count() - bar_w;
        out += &format!("{title}{}{bar}\n", " ".repeat(pad));
        for r in &z.rows {
            let label = fit(&r.pointer.label(), LABEL);
            let lead = format!("  {} {label:<LABEL$} ", r.status.glyph());
            let mark = match (z.is_proofs(), r.proven()) {
                (false, _) => "",
                (true, true) => "[proven] ",
                (true, false) => "[not proven yet] ",
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardWorkItem"))]
pub struct WorkItem {
    /// What a row takes as its pointer: `Issue`, `ForgeItem` or `Run`.
    pub pointer: Pointer,
    /// `native`, `github` or `run`.
    pub source: String,
    /// `issue`, `pr` or `run`.
    pub kind: String,
    /// `OWNER/NAME` of a mirrored item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// A run has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BoardWorkItems"))]
pub struct WorkItems {
    pub project: String,
    pub items: Vec<WorkItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pointer: Pointer, status: Status, note: &str) -> Row {
        Row {
            id: "row_1".into(),
            rev: 1,
            pointer,
            note: note.into(),
            status,
            live: None,
            block: None,
            agent: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn zone(title: &str, kind: Option<ZoneKind>, rows: Vec<Row>) -> Zone {
        Zone { id: "zon_1".into(), rev: 1, title: title.into(), kind, rows }
    }

    /// A board's text form: a bar per zone counting its done rows, a glyph
    /// per status, notes cut to fit 72 columns, PROOFS last wherever it
    /// came in the list.
    #[test]
    fn a_board_renders_in_72_columns() {
        let board = Board {
            id: "brd_1".into(),
            project: "prj_1".into(),
            title: "Launch".into(),
            created_by: "a".into(),
            created_at: 0,
            updated_at: 0,
            ..Default::default()
        };
        let mut proven_run = row(Pointer::Run { id: "run_aa22".into() }, Status::Done, "Release build");
        proven_run.live = Some(Live {
            state: LiveState::Succeeded,
            kind: "run".into(),
            repo: None,
            number: 0,
            title: "manual run".into(),
            assignees: vec![],
        });
        let long = "Wiring the new tokenizer through every caller, then the benchmarks, then the docs";
        let zones = vec![
            zone(
                "PROOFS",
                Some(ZoneKind::Proofs),
                vec![
                    row(Pointer::None, Status::Done, "Tests green on main"),
                    row(Pointer::Link { url: "https://ci.example.com/runs/42".into() }, Status::Done, "CI on main"),
                    proven_run,
                    row(Pointer::Run { id: "run_ee11".into() }, Status::Done, "Nightly"),
                ],
            ),
            zone(
                "Build",
                None,
                vec![
                    row(Pointer::Issue { id: "iss_1a2b3c4d5e6f7a8b".into() }, Status::Done, "Parser rewrite landed"),
                    row(Pointer::Block { machine: "m1".into(), pane: "7".into() }, Status::Working, long),
                    row(Pointer::Run { id: "run_ff00".into() }, Status::NeedsInput, "Which target?"),
                    row(Pointer::None, Status::Failed, "Flaky\u{7}test"),
                    row(Pointer::None, Status::None, ""),
                ],
            ),
            zone("Empty", None, vec![]),
        ];
        let text = render(&BoardView { board, zones });
        let want = "\
Build                                                     ██░░░░░░░░ 1/5
  ● iss_1a2b3c4d       Parser rewrite landed
  ◐ m1/7               Wiring the new tokenizer through every caller, t…
  ? run_ff00           Which target?
  ✕                    Flaky test
  ○
Empty                                                     ░░░░░░░░░░ 0/0
PROOFS                                                    ██████████ 4/4
  ●                    [not proven yet] Tests green on main
  ● ci.example.com/ru… [proven] CI on main
  ● run_aa22           [proven] Release build
  ● run_ee11           [not proven yet] Nightly";
        assert_eq!(text, want);
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A row on an issue or a PR ends with its assignees as `(@a, @b)`, and
    /// the note is what gives way when the line is full.
    #[test]
    fn a_row_with_assignees_ends_with_them() {
        let board = Board {
            id: "brd_1".into(),
            project: "prj_1".into(),
            title: "Launch".into(),
            created_by: "a".into(),
            created_at: 0,
            updated_at: 0,
            ..Default::default()
        };
        let on = |note: &str, who: &[&str]| {
            let mut r = row(Pointer::Issue { id: "iss_1a2b3c4d5e6f7a8b".into() }, Status::Open, note);
            r.live = Some(Live {
                state: LiveState::Open,
                kind: "native".into(),
                repo: None,
                number: 4,
                title: "Fix login".into(),
                assignees: who.iter().map(|w| (*w).to_owned()).collect(),
            });
            r
        };
        let long = "Fix the login redirect for every browser we support, then the tests";
        let zones = vec![zone(
            "Build",
            None,
            vec![on("Fix login", &["ana", "Bo Li"]), on(long, &["ana"]), on("Fix login", &[])],
        )];
        let text = render(&BoardView { board, zones });
        let want = "\
Build                                                     ░░░░░░░░░░ 0/3
  ○ iss_1a2b3c4d       Fix login (@ana, @Bo_Li)
  ○ iss_1a2b3c4d       Fix the login redirect for every browser… (@ana)
  ○ iss_1a2b3c4d       Fix login";
        assert_eq!(text, want);
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }

    /// A block's row carries what its machine reported under it, and the
    /// body that reports it takes no field the ruling doesn't list.
    #[test]
    fn a_block_row_shows_its_reported_state() {
        let board = Board {
            id: "brd_1".into(),
            project: "prj_1".into(),
            title: "Launch".into(),
            created_by: "a".into(),
            created_at: 0,
            updated_at: 0,
            ..Default::default()
        };
        let mut r = row(Pointer::Block { machine: "m1".into(), pane: "7".into() }, Status::Working, "Tokenizer");
        r.block = Some(BlockState {
            state: BlockWord::NeedsInput,
            branch: Some("feat/tokenizer".into()),
            head: Some("abc1234".into()),
            pushed: Some(false),
            uncommitted: Some(2),
            diffstat: Some(Diffstat { files: 3, added: 40, removed: 2 }),
        });
        let text = render(&BoardView { board, zones: vec![zone("Build", None, vec![r])] });
        let want = "\
Build                                                     ░░░░░░░░░░ 0/1
  ◐ m1/7               Tokenizer
      needs input · feat/tokenizer@abc1234 · unpushed · 2 uncommitted ·…";
        assert_eq!(text, want);
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
        assert_eq!((b.goal.as_str(), b.closed_at, b.progress), ("", None, BoardProgress { done: 0, total: 0 }));
    }
}
