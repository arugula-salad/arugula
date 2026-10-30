//! Boards (boards, step 1; aru-125): a project's boards, each an ordered
//! list of rows, a row being one piece of work. Who may do what is decided
//! by the routes (`crate::boards`); this is storage. The tables, with notes
//! on their columns, are made by `0058_boards` and `0072_board_lanes`.
//!
//! - Every change to a board takes its row first (`FOR UPDATE` on
//!   Postgres, the write lock on SQLite), so a board's changes run one at a
//!   time: positions are renumbered whole and revs are checked against what
//!   is there.
//! - A row's `rev` counts changes to its pointer and note. A write that
//!   changes those names the rev it read and is refused as
//!   [`Refused::Stale`] if the row moved on.
//! - A row has no zone: its lane is worked out on every read from the live
//!   state of its items ([`crate::boards`]) and is never stored. Zone
//!   entries exist only in a database made before 0072, until
//!   [`Db::lanes_move`] clears them on start.
//! - Work and proof are attached to a row (`board_attachments`,
//!   [`Attached`]); deleting a row takes its attachments.
//! - A pointer is stored, not resolved. A row's `note` and `status` are
//!   separate columns.
//! - Deleting a project (or its account) takes its boards.
//! - A machine's report of one of its own agent blocks (`board_block_states`,
//!   [`BlockState`]) is kept apart from the rows that point at it: one per
//!   (machine, pane), set by the machine and read through a pointer.

use anyhow::{Result, bail};
use arugula_control_wire::{
    boards::{AttachRole, BlockState, BlockWord, Board, Diffstat, Pointer, Status},
    chat::ChatAgent,
};

use super::{
    Db,
    chat::{in_json, json_list},
    forge::CheckState,
    issues::Actor,
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A new board's id: `brd_` and 32 hex characters.
fn new_board_id() -> String {
    format!("brd_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A new row's id: `row_` and 32 hex characters.
fn new_row_id() -> String {
    format!("row_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A new attachment's id: `att_` and 32 hex characters.
fn new_attachment_id() -> String {
    format!("att_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// Why a change to a board wasn't made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The board, entry or zone isn't there.
    Missing(&'static str),
    /// The entry's rev isn't the one named: read the board again.
    Stale { rev: u64 },
    /// The change isn't allowed; why.
    Bad(String),
    /// The change is something its author may not do; why.
    Forbidden(String),
}

fn bad<T>(why: impl Into<String>) -> Result<std::result::Result<T, Refused>> {
    Ok(Err(Refused::Bad(why.into())))
}

/// One row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub rev: u64,
    pub note: String,
    pub status: Status,
    pub pointer: Pointer,
    /// The machine whose agent added a row (`boards.write`); none: a person.
    pub agent: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// One thing attached to a row, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub id: String,
    pub row: String,
    pub role: AttachRole,
    pub pointer: Pointer,
    /// The machine whose agent attached it; none: a person.
    pub agent: Option<String>,
    pub created_at: u64,
}

/// A board and its rows in order, with what is attached to them (oldest
/// first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub board: Board,
    pub rows: Vec<Entry>,
    pub attachments: Vec<Attached>,
}

/// What a new row, or a change to one, says. Absent: left as it is.
#[derive(Debug, Default, Clone)]
pub struct Put<'a> {
    /// The row changed; none: add one, last.
    pub id: Option<&'a str>,
    pub rev: Option<u64>,
    pub pointer: Option<&'a Pointer>,
    pub note: Option<&'a str>,
    pub status: Option<Status>,
    /// The machine whose agent adds a new row; none: a person.
    pub agent: Option<&'a str>,
}

const BOARD_COLS: &str = "id, project_id, title, created_by, created_at, updated_at, goal, done_note, closed_at, \
                          closed_by, created_by_agent, closed_by_agent";
const ENTRY_COLS: &str = "id, rev, note, status, target_kind, target, pane, created_at, updated_at, agent";
const ATTACHED_COLS: &str = "id, row_id, role, target_kind, target, pane, agent, created_at";

fn board_row(r: &Row) -> Result<Board> {
    Ok(Board {
        id: r.get(0)?,
        project: r.get(1)?,
        title: r.get(2)?,
        created_by: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        goal: r.get(6)?,
        done_note: r.get(7)?,
        closed_at: r.get(8)?,
        closed_by: r.get(9)?,
        // The names are filled in by the reader (`crate::boards`).
        created_by_agent: machine_of(r.get(10)?),
        closed_by_agent: machine_of(r.get(11)?),
        progress: Default::default(),
    })
}

/// A machine stored on a board, not yet named.
fn machine_of(m: Option<String>) -> Option<ChatAgent> {
    m.map(|machine| ChatAgent { machine, machine_name: String::new() })
}

fn status_str(s: Status) -> Option<&'static str> {
    Some(match s {
        Status::None => return None,
        Status::Open => "open",
        Status::Working => "working",
        Status::NeedsInput => "needs_input",
        Status::Done => "done",
        Status::Failed => "failed",
    })
}

fn word_str(w: BlockWord) -> &'static str {
    match w {
        BlockWord::Working => "working",
        BlockWord::NeedsInput => "needs_input",
        BlockWord::Idle => "idle",
        BlockWord::Closed => "closed",
    }
}

fn word_of(s: &str) -> Result<BlockWord> {
    Ok(match s {
        "working" => BlockWord::Working,
        "needs_input" => BlockWord::NeedsInput,
        "idle" => BlockWord::Idle,
        "closed" => BlockWord::Closed,
        other => bail!("unknown block state {other}"),
    })
}

pub(super) fn status_of(s: Option<String>) -> Result<Status> {
    Ok(match s.as_deref() {
        None => Status::None,
        Some("open") => Status::Open,
        Some("working") => Status::Working,
        Some("needs_input") => Status::NeedsInput,
        Some("done") => Status::Done,
        Some("failed") => Status::Failed,
        Some(s) => bail!("a board row in status {s:?}"),
    })
}

/// A pointer as its three columns: kind, target and pane.
pub(super) fn pointer_cols(p: &Pointer) -> (Option<&'static str>, Option<&str>, Option<&str>) {
    match p {
        Pointer::None => (None, None, None),
        Pointer::ForgeItem { id } => (Some("forge_item"), Some(id), None),
        Pointer::Issue { id } => (Some("issue"), Some(id), None),
        Pointer::Run { id } => (Some("run"), Some(id), None),
        Pointer::Workspace { id } => (Some("workspace"), Some(id), None),
        Pointer::Block { machine, pane } => (Some("block"), Some(machine), Some(pane)),
        Pointer::Link { url } => (Some("link"), Some(url), None),
    }
}

/// A kind this build doesn't know (one a newer build wrote, before a
/// rollback) reads as no pointer, so the board still reads; its row keeps
/// the stored columns, which a status change or a move leaves alone.
pub(super) fn pointer_of(kind: Option<String>, target: Option<String>, pane: Option<String>) -> Result<Pointer> {
    Ok(match (kind.as_deref(), target, pane) {
        (None, ..) => Pointer::None,
        (Some("forge_item"), Some(id), _) => Pointer::ForgeItem { id },
        (Some("issue"), Some(id), _) => Pointer::Issue { id },
        (Some("run"), Some(id), _) => Pointer::Run { id },
        (Some("workspace"), Some(id), _) => Pointer::Workspace { id },
        (Some("block"), Some(machine), Some(pane)) => Pointer::Block { machine, pane },
        (Some("link"), Some(url), _) => Pointer::Link { url },
        (Some("forge_item" | "issue" | "run" | "workspace" | "block" | "link"), ..) => {
            bail!("a board row's pointer is missing its target")
        }
        (Some(_), ..) => Pointer::None,
    })
}

fn entry_row(r: &Row) -> Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        rev: r.get(1)?,
        note: r.get(2)?,
        status: status_of(r.get(3)?)?,
        pointer: pointer_of(r.get(4)?, r.get(5)?, r.get(6)?)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        agent: r.get(9)?,
    })
}

fn attached_row(r: &Row) -> Result<Attached> {
    let role = match r.get::<String>(2)?.as_str() {
        "work" => AttachRole::Work,
        "proof" => AttachRole::Proof,
        k => bail!("a board attachment of role {k:?}"),
    };
    Ok(Attached {
        id: r.get(0)?,
        row: r.get(1)?,
        role,
        pointer: pointer_of(r.get(3)?, r.get(4)?, r.get(5)?)?,
        agent: r.get(6)?,
        created_at: r.get(7)?,
    })
}

/// Lock the board's row for the rest of the transaction: its changes take
/// turns.
fn lock(c: &impl Sql, board: &str) -> Result<bool> {
    let found: Option<String> = c
        .query_row(&format!("SELECT id FROM boards WHERE id = ?1{}", c.for_update()), params![board], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

fn entry(c: &impl Sql, board: &str, id: &str) -> Result<Option<Entry>> {
    c.query_row(
        &format!("SELECT {ENTRY_COLS} FROM board_entries WHERE board_id = ?1 AND id = ?2 AND kind = 'row'"),
        params![board, id],
        entry_row,
    )
    .optional()
}

/// The ids of a board's rows, in order.
fn siblings(c: &impl Sql, board: &str) -> Result<Vec<String>> {
    c.query_map(
        "SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'row' ORDER BY position, id",
        params![board],
        |r| r.get(0),
    )
}

/// Number `ids` 0, 1, 2... as the order of their siblings.
fn renumber(c: &impl Sql, ids: &[String]) -> Result<()> {
    for (i, id) in ids.iter().enumerate() {
        c.execute("UPDATE board_entries SET position = ?2 WHERE id = ?1 AND position != ?2", params![id, i])?;
    }
    Ok(())
}

fn touch(c: &impl Sql, board: &str, at: u64) -> Result<()> {
    c.execute("UPDATE boards SET updated_at = ?2 WHERE id = ?1", params![board, at])?;
    Ok(())
}

/// Every row's pointer and every attachment's, as one table: what a change
/// to an item, a block or a run tells the boards of.
const POINTED: &str = "(SELECT board_id, target_kind, target, pane FROM board_entries WHERE kind = 'row'
     UNION ALL SELECT board_id, target_kind, target, pane FROM board_attachments)";

/// A row's primary pointer, or an attachment's (`role`), on a board: one
/// thing that [`Db::project_things`] counts progress from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thing {
    pub board: String,
    pub row: String,
    /// `None`: the row's own pointer.
    pub role: Option<AttachRole>,
    pub pointer: Pointer,
}

/// What a lane needs of a mirrored item, and nothing a reader couldn't be
/// told once they may not read it: no title, no author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFact {
    pub id: String,
    /// `pr` or `issue`.
    pub kind: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    /// A tracker's items have a negative one.
    pub repo_id: i64,
    pub head_sha: Option<String>,
}

/// A native issue as a board row reads it.
pub struct IssueState {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub closed: bool,
    /// Accounts; a list that doesn't parse reads as none.
    pub assignees: Vec<String>,
}

impl Db {
    /// Make a board in `project` as `by` (a person, or their agent on a
    /// machine), with `goal` if any. `None`: the project
    /// already has `max` boards. The project is the caller's to have checked.
    pub fn create_board(
        &self,
        project: &str,
        title: &str,
        goal: &str,
        by: Actor<'_>,
        at: u64,
        max: u64,
    ) -> Result<Option<Board>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        // Counted in the same transaction as the insert, with the project's
        // row held, so two at once can't both take the last place.
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", tx.for_update()), params![project], |r| {
                r.get(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(None);
        }
        let n: u64 =
            tx.query_row("SELECT count(*) FROM boards WHERE project_id = ?1", params![project], |r| r.get(0))?;
        if n >= max {
            return Ok(None);
        }
        let board = tx.query_row(
            &format!(
                "INSERT INTO boards (id, project_id, title, goal, created_by, created_by_agent, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7) RETURNING {BOARD_COLS}"
            ),
            params![new_board_id(), project, title, goal, by.account, by.agent, at],
            board_row,
        )?;
        tx.commit()?;
        Ok(Some(board))
    }

    /// One row of a board, as stored.
    pub fn board_entry(&self, board: &str, id: &str) -> Result<Option<Entry>> {
        entry(&self.c(), board, id)
    }

    pub fn board(&self, id: &str) -> Result<Option<Board>> {
        self.c().query_row(&format!("SELECT {BOARD_COLS} FROM boards WHERE id = ?1"), params![id], board_row).optional()
    }

    /// A project's boards: open ones first, then closed, each oldest first.
    pub fn boards(&self, project: &str) -> Result<Vec<Board>> {
        self.c().query_map(
            &format!(
                "SELECT {BOARD_COLS} FROM boards WHERE project_id = ?1
                 ORDER BY closed_at IS NOT NULL, created_at, id"
            ),
            params![project],
            board_row,
        )
    }

    /// A board with its rows in order and their attachments, from one read.
    pub fn board_stored(&self, id: &str) -> Result<Option<Stored>> {
        let c = self.c();
        let Some(board) = c
            .query_row(&format!("SELECT {BOARD_COLS} FROM boards WHERE id = ?1"), params![id], board_row)
            .optional()?
        else {
            return Ok(None);
        };
        let all = c.query_map(
            &format!("SELECT {ENTRY_COLS}, position FROM board_entries WHERE board_id = ?1 AND kind = 'row'"),
            params![id],
            |r| Ok((entry_row(r)?, r.get::<u64>(10)?)),
        )?;
        let mut rows = all;
        rows.sort_by(|a, b| (a.1, &a.0.id).cmp(&(b.1, &b.0.id)));
        let attachments = c.query_map(
            &format!("SELECT {ATTACHED_COLS} FROM board_attachments WHERE board_id = ?1 ORDER BY created_at, id"),
            params![id],
            attached_row,
        )?;
        Ok(Some(Stored { board, rows: rows.into_iter().map(|(e, _)| e).collect(), attachments }))
    }

    /// Set a board's title and goal (each only when given). `false`: no such
    /// board.
    pub fn edit_board(&self, id: &str, title: Option<&str>, goal: Option<&str>, at: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, id)? {
            return Ok(false);
        }
        if let Some(t) = title {
            tx.execute("UPDATE boards SET title = ?2 WHERE id = ?1", params![id, t])?;
        }
        if let Some(g) = goal {
            tx.execute("UPDATE boards SET goal = ?2 WHERE id = ?1", params![id, g])?;
        }
        touch(&tx, id, at)?;
        tx.commit()?;
        Ok(true)
    }

    /// Close a board with `note` as `by` (a person, or their agent on a
    /// machine). A board closed already takes the new note and keeps
    /// its first `closed_at`, `closed_by` and `closed_by_agent`. `false`: no
    /// such board.
    pub fn close_board(&self, id: &str, note: &str, by: Actor<'_>, at: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, id)? {
            return Ok(false);
        }
        tx.execute(
            "UPDATE boards SET done_note = ?2,
                 closed_by = CASE WHEN closed_at IS NULL THEN ?3 ELSE closed_by END,
                 closed_by_agent = CASE WHEN closed_at IS NULL THEN ?4 ELSE closed_by_agent END,
                 closed_at = COALESCE(closed_at, ?5)
             WHERE id = ?1",
            params![id, note, by.account, by.agent, at],
        )?;
        touch(&tx, id, at)?;
        tx.commit()?;
        Ok(true)
    }

    /// Open a board again; its done note stays. `false`: no such board.
    pub fn reopen_board(&self, id: &str, at: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, id)? {
            return Ok(false);
        }
        tx.execute(
            "UPDATE boards SET closed_at = NULL, closed_by = '', closed_by_agent = NULL WHERE id = ?1",
            params![id],
        )?;
        touch(&tx, id, at)?;
        tx.commit()?;
        Ok(true)
    }

    /// Every row of `project`'s boards that has a pointer, and every
    /// attachment: what progress is counted from, in one read.
    pub fn project_things(&self, project: &str) -> Result<Vec<Thing>> {
        let c = self.c();
        let mut out: Vec<Thing> = c.query_map(
            "SELECT e.board_id, e.id, e.target_kind, e.target, e.pane FROM board_entries e
             JOIN boards b ON b.id = e.board_id
             WHERE b.project_id = ?1 AND e.kind = 'row' AND e.target_kind IS NOT NULL",
            params![project],
            |r| {
                Ok(Thing {
                    board: r.get(0)?,
                    row: r.get(1)?,
                    role: None,
                    pointer: pointer_of(r.get(2)?, r.get(3)?, r.get(4)?)?,
                })
            },
        )?;
        out.extend(c.query_map(
            "SELECT a.board_id, a.row_id, a.role, a.target_kind, a.target, a.pane FROM board_attachments a
             JOIN boards b ON b.id = a.board_id WHERE b.project_id = ?1",
            params![project],
            |r| {
                let role = if r.get::<String>(2)? == "proof" { AttachRole::Proof } else { AttachRole::Work };
                Ok(Thing {
                    board: r.get(0)?,
                    row: r.get(1)?,
                    role: Some(role),
                    pointer: pointer_of(r.get(3)?, r.get(4)?, r.get(5)?)?,
                })
            },
        )?);
        Ok(out)
    }

    /// Add a row last, or change one (see [`Put`]); the id of the row.
    /// `absorb`: rows of the board that the new one takes in, in the same
    /// transaction: each becomes a `work` attachment of it (a PR row of the
    /// issue it closes), with the attachments it had, and its place is gone.
    pub fn put_entry(
        &self,
        board: &str,
        p: &Put,
        absorb: &[String],
        at: u64,
        max_rows: u64,
    ) -> Result<std::result::Result<String, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let row = match p.id {
            Some(id) => {
                let Some(old) = entry(&tx, board, id)? else { return Ok(Err(Refused::Missing("no such row"))) };
                let text = p.pointer.is_some() || p.note.is_some();
                if !text && p.status.is_none() {
                    return bad("nothing to change");
                }
                if text {
                    let Some(rev) = p.rev else { return bad("name the rev you read (\"rev\")") };
                    if rev != old.rev {
                        return Ok(Err(Refused::Stale { rev: old.rev }));
                    }
                    let note = p.note.unwrap_or(&old.note);
                    // Without a new pointer, the stored one is left as it is
                    // (one this build can't read included).
                    if let Some((kind, target, pane)) = p.pointer.map(pointer_cols) {
                        tx.execute(
                            "UPDATE board_entries SET rev = rev + 1, note = ?2, target_kind = ?3, target = ?4, pane = ?5,
                                 updated_at = ?6 WHERE id = ?1",
                            params![id, note, kind, target, pane, at],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE board_entries SET rev = rev + 1, note = ?2, updated_at = ?3 WHERE id = ?1",
                            params![id, note, at],
                        )?;
                    }
                }
                if let Some(s) = p.status {
                    tx.execute(
                        "UPDATE board_entries SET status = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, status_str(s), at],
                    )?;
                }
                id.to_owned()
            }
            None => {
                let n: u64 = tx.query_row(
                    "SELECT count(*) FROM board_entries WHERE board_id = ?1 AND kind = 'row'",
                    params![board],
                    |r| r.get(0),
                )?;
                if n >= max_rows {
                    return bad(format!("a board has at most {max_rows} rows"));
                }
                let (kind, target, pane) = p.pointer.map(pointer_cols).unwrap_or((None, None, None));
                let id = new_row_id();
                tx.execute(
                    "INSERT INTO board_entries (id, board_id, kind, zone_id, position, note, status, target_kind, target,
                         pane, created_at, updated_at, agent)
                     VALUES (?1, ?2, 'row', NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
                    params![id, board, n, p.note.unwrap_or(""), p.status.and_then(status_str), kind, target, pane, at, p.agent],
                )?;
                for from in absorb {
                    let Some(taken) = entry(&tx, board, from)? else { continue };
                    let (kind, target, pane) = pointer_cols(&taken.pointer);
                    if let (Some(kind), Some(target)) = (kind, target) {
                        tx.execute(
                            "INSERT INTO board_attachments (id, board_id, row_id, role, target_kind, target, pane, agent,
                                 created_at)
                             VALUES (?1, ?2, ?3, 'work', ?4, ?5, ?6, ?7, ?8)",
                            params![new_attachment_id(), board, id, kind, target, pane, taken.agent, at],
                        )?;
                    }
                    tx.execute("UPDATE board_attachments SET row_id = ?2 WHERE row_id = ?1", params![from, id])?;
                    tx.execute("DELETE FROM board_entries WHERE id = ?1", params![from])?;
                }
                renumber(&tx, &siblings(&tx, board)?)?;
                id
            }
        };
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(row))
    }

    /// Delete a row and what is attached to it.
    pub fn delete_entry(&self, board: &str, id: &str, at: u64) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        if entry(&tx, board, id)?.is_none() {
            return Ok(Err(Refused::Missing("no such row")));
        }
        tx.execute("DELETE FROM board_attachments WHERE row_id = ?1", params![id])?;
        tx.execute("DELETE FROM board_entries WHERE id = ?1", params![id])?;
        renumber(&tx, &siblings(&tx, board)?)?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Attach `pointer` to `row` as `role`, by `agent` (a machine) or a
    /// person; the attachment's id. The pointer's shape for the role is the
    /// caller's to have checked.
    #[allow(clippy::too_many_arguments)]
    pub fn attach(
        &self,
        board: &str,
        row: &str,
        role: AttachRole,
        pointer: &Pointer,
        agent: Option<&str>,
        at: u64,
        max: u64,
    ) -> Result<std::result::Result<String, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        if entry(&tx, board, row)?.is_none() {
            return Ok(Err(Refused::Missing("no such row")));
        }
        let n: u64 =
            tx.query_row("SELECT count(*) FROM board_attachments WHERE row_id = ?1", params![row], |r| r.get(0))?;
        if n >= max {
            return bad(format!("a row has at most {max} attachments"));
        }
        let (Some(kind), Some(target), pane) = pointer_cols(pointer) else {
            return bad("an attachment points at something");
        };
        let id = new_attachment_id();
        tx.execute(
            "INSERT INTO board_attachments (id, board_id, row_id, role, target_kind, target, pane, agent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, board, row, role.as_str(), kind, target, pane, agent, at],
        )?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(id))
    }

    /// One attachment of a board, as stored.
    pub fn attachment(&self, board: &str, id: &str) -> Result<Option<Attached>> {
        self.c()
            .query_row(
                &format!("SELECT {ATTACHED_COLS} FROM board_attachments WHERE board_id = ?1 AND id = ?2"),
                params![board, id],
                attached_row,
            )
            .optional()
    }

    /// Take an attachment off its row.
    pub fn detach(&self, board: &str, id: &str, at: u64) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        if tx.execute("DELETE FROM board_attachments WHERE board_id = ?1 AND id = ?2", params![board, id])? == 0 {
            return Ok(Err(Refused::Missing("no such attachment")));
        }
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// The boards (id, project) with a row that points at an item of `repo`
    /// with one of these `numbers`, or whose head is one of these `shas`
    /// (a check landed): who to tell that an item changed.
    pub fn boards_on_items(&self, repo: u64, numbers: &[u64], shas: &[String]) -> Result<Vec<(String, String)>> {
        if numbers.is_empty() && shas.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        // Numbers are the platform's own: written into the statement, as the
        // mirror's reads write repo ids.
        let nums = if numbers.is_empty() {
            "FALSE".to_owned()
        } else {
            format!("i.number IN ({})", numbers.iter().map(u64::to_string).collect::<Vec<_>>().join(", "))
        };
        let heads = in_json(&c, "i.head_sha", 2);
        c.query_map(
            &format!(
                "SELECT DISTINCT b.id, b.project_id FROM {POINTED} e
                 JOIN boards b ON b.id = e.board_id
                 JOIN forge_items i ON i.id = e.target
                 WHERE e.target_kind = 'forge_item' AND i.repo_id = ?1 AND ({nums} OR {heads})"
            ),
            params![repo, json_list(shas)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The boards (id, project) with a row that points at one of these
    /// `forge_items` ids: who to tell that a tracker's items changed.
    pub fn boards_on_item_ids(&self, ids: &[String]) -> Result<Vec<(String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT DISTINCT b.id, b.project_id FROM {POINTED} e JOIN boards b ON b.id = e.board_id
                 WHERE e.target_kind = 'forge_item' AND {}",
                in_json(&c, "e.target", 1)
            ),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The native issues among `ids` in `project`, as (id, number, title,
    /// closed, assignees): one read for any number (a board's rows).
    pub fn issue_states(&self, project: &str, ids: &[&str]) -> Result<Vec<IssueState>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT id, number, title, closed_at IS NOT NULL, assignees FROM issues WHERE project_id = ?1 AND {}",
                in_json(&c, "id", 2)
            ),
            params![project, json_list(ids)?],
            |r| {
                Ok(IssueState {
                    id: r.get(0)?,
                    number: r.get(1)?,
                    title: r.get(2)?,
                    closed: r.get(3)?,
                    assignees: serde_json::from_str(&r.get::<String>(4)?).unwrap_or_default(),
                })
            },
        )
    }

    /// The state of the mirrored items among `ids` (GitHub's and a tracker's)
    /// with every check on their heads, whoever may read them. This is how a
    /// lane is worked out for a reader who may not read an item: it holds
    /// what a lane and a proof are made of, and the reader's own chips still
    /// go through the viewer check ([`crate::forge::viewer`]).
    /// Only items of `project`'s linked repos and connected trackers.
    pub fn item_facts(&self, project: &str, ids: &[&str]) -> Result<(Vec<ItemFact>, Vec<CheckState>)> {
        if ids.is_empty() {
            return Ok((vec![], vec![]));
        }
        let c = self.c();
        let items = c.query_map(
            &format!(
                "SELECT i.id, i.kind, i.state, i.repo_id, i.head_sha FROM forge_items i
                 WHERE (i.repo_id IN (SELECT repo_id FROM linked_repos WHERE project_id = ?2 AND unlinked_at IS NULL)
                        OR i.connection_id IN
                           (SELECT id FROM tracker_connections WHERE project_id = ?2 AND unlinked_at IS NULL))
                   AND {}",
                in_json(&c, "i.id", 1)
            ),
            params![json_list(ids)?, project],
            |r| {
                Ok(ItemFact {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    state: r.get(2)?,
                    repo_id: r.get(3)?,
                    head_sha: r.get(4)?,
                })
            },
        )?;
        let heads: Vec<&str> = items.iter().filter_map(|i| i.head_sha.as_deref()).collect();
        let checks = if heads.is_empty() {
            vec![]
        } else {
            c.query_map(
                &format!(
                    "SELECT repo_id, head_sha, status, conclusion FROM forge_checks WHERE {}",
                    in_json(&c, "head_sha", 1)
                ),
                params![json_list(&heads)?],
                |r| {
                    Ok(CheckState { repo_id: r.get(0)?, head_sha: r.get(1)?, status: r.get(2)?, conclusion: r.get(3)? })
                },
            )?
        };
        Ok((items, checks))
    }

    /// The boards (id, project) with a row that points at native issue `id`.
    pub fn boards_on_issue(&self, id: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT DISTINCT b.id, b.project_id FROM board_entries e JOIN boards b ON b.id = e.board_id
             WHERE e.kind = 'row' AND e.target_kind = 'issue' AND e.target = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The projects over which `machine` holds a live `boards.write` grant,
    /// each with the account that gave it: whether the owner still reads the
    /// project is for [`crate::grants::agent_may`].
    pub fn machine_write_projects(&self, machine: &str, power: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT scope_id, granted_by FROM grants WHERE subject_kind = ?3 AND subject_id = ?1 AND power = ?2
             AND scope_kind = 'project' AND revoked_at IS NULL",
            params![machine, power, super::projects::subject_kind(machine)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// Keep what `machine` says of its block `pane`, replacing the last.
    pub fn put_block_state(&self, machine: &str, account: &str, pane: &str, b: &BlockState, at: u64) -> Result<()> {
        let d = b.diffstat;
        self.c().execute(
            "INSERT INTO board_block_states (machine, pane, account, state, branch, head, pushed, uncommitted,
                 diff_files, diff_added, diff_removed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (machine, pane) DO UPDATE SET account = excluded.account, state = excluded.state,
                 branch = excluded.branch, head = excluded.head, pushed = excluded.pushed,
                 uncommitted = excluded.uncommitted, diff_files = excluded.diff_files,
                 diff_added = excluded.diff_added, diff_removed = excluded.diff_removed,
                 updated_at = excluded.updated_at",
            params![
                machine,
                pane,
                account,
                word_str(b.state),
                b.branch,
                b.head,
                b.pushed,
                b.uncommitted,
                d.map(|d| d.files),
                d.map(|d| d.added),
                d.map(|d| d.removed),
                at
            ],
        )?;
        Ok(())
    }

    /// What `machine` last said of these `panes`, by pane.
    pub fn block_states(&self, machine: &str, panes: &[&str]) -> Result<Vec<(String, BlockState)>> {
        if panes.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT pane, state, branch, head, pushed, uncommitted, diff_files, diff_added, diff_removed
                 FROM board_block_states WHERE machine = ?1 AND {}",
                in_json(&c, "pane", 2)
            ),
            params![machine, json_list(panes)?],
            |r| {
                let files: Option<u32> = r.get(6)?;
                let state = BlockState {
                    state: word_of(&r.get::<String>(1)?)?,
                    branch: r.get(2)?,
                    head: r.get(3)?,
                    pushed: r.get(4)?,
                    uncommitted: r.get(5)?,
                    diffstat: match (files, r.get(7)?, r.get(8)?) {
                        (Some(files), Some(added), Some(removed)) => Some(Diffstat { files, added, removed }),
                        _ => None,
                    },
                };
                Ok((r.get(0)?, state))
            },
        )
    }

    /// The boards (id, project) with a row that points at `machine`'s block
    /// `pane`: who to tell that it changed.
    pub fn boards_on_block(&self, machine: &str, pane: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            &format!(
                "SELECT DISTINCT b.id, b.project_id FROM {POINTED} e JOIN boards b ON b.id = e.board_id
                 WHERE e.target_kind = 'block' AND e.target = ?1 AND e.pane = ?2"
            ),
            params![machine, pane],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    pub fn board_places(&self, ids: &[&str]) -> Result<Vec<(String, String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!("SELECT id, project_id, title FROM boards WHERE {}", in_json(&c, "id", 1)),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }
}

/// What deleting an account takes from boards, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// the boards of the projects that went with it, with their entries;
/// elsewhere its boards stay, with no creator.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute(
        "DELETE FROM board_attachments WHERE board_id IN
             (SELECT id FROM boards WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute(
        "DELETE FROM board_entries WHERE board_id IN
             (SELECT id FROM boards WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM boards WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE boards SET created_by = '' WHERE created_by = ?1", params![d.account])?;
    c.execute("UPDATE boards SET closed_by = '' WHERE closed_by = ?1", params![d.account])?;
    c.execute("DELETE FROM board_block_states WHERE account = ?1", params![d.account])?;
    Ok(())
}

impl Db {
    /// The `fi_` and `iss_` ids that any row of any board of `project` points
    /// at: what makes an issue triaged.
    pub fn project_board_targets(&self, project: &str) -> Result<std::collections::HashSet<String>> {
        let rows: Vec<String> = self.c().query_map(
            &format!(
                "SELECT DISTINCT e.target FROM {POINTED} e JOIN boards b ON b.id = e.board_id
                 WHERE b.project_id = ?1 AND e.target_kind IN ('forge_item', 'issue')"
            ),
            params![project],
            |r| r.get(0),
        )?;
        Ok(rows.into_iter().collect())
    }
}
