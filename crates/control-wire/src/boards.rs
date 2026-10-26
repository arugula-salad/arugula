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
/// `GET`: a board with its zones and rows in order, [`BoardView`].
pub const BOARD: &str = "/api/boards/{id}";
/// `PUT` [`PutEntry`]: make or change a zone or a row, answered with the
/// board, [`BoardView`].
pub const BOARD_ENTRIES: &str = "/api/boards/{id}/entries";
/// `DELETE`: a row, or a zone and its rows; answered [`BoardView`].
pub const BOARD_ENTRY: &str = "/api/boards/{id}/entries/{entry}";
/// `POST` [`MoveEntry`]: reorder a row or a zone; answered [`BoardView`].
pub const BOARD_ENTRY_MOVE: &str = "/api/boards/{id}/entries/{entry}/move";

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

/// A board as plain text, no wider than [`TEXT_WIDTH`] columns: each zone
/// is its title and a progress bar counting its done rows, then its rows,
/// each a status glyph, a short pointer label and the note (cut to fit).
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
            let note = fit(&r.note, TEXT_WIDTH - lead.chars().count());
            out += format!("{lead}{note}").trim_end();
            out.push('\n');
        }
    }
    out.trim_end().to_owned()
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
        };
        let long = "Wiring the new tokenizer through every caller, then the benchmarks, then the docs";
        let zones = vec![
            zone("PROOFS", Some(ZoneKind::Proofs), vec![row(Pointer::None, Status::Done, "Tests green on main")]),
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
PROOFS                                                    ██████████ 1/1
  ●                    Tests green on main";
        assert_eq!(text, want);
        assert!(text.lines().all(|l| l.chars().count() <= TEXT_WIDTH), "{text}");
    }
}
