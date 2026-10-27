//! Boards (live status board, step 1): a project's boards, each an ordered
//! list of zones that each hold an ordered list of rows. Who may do what is
//! decided by the routes (`crate::boards`); this is storage. The tables,
//! with notes on their columns, are made by `0058_boards`.
//!
//! - Every change to a board takes its row first (`FOR UPDATE` on
//!   Postgres, the write lock on SQLite), so a board's changes run one at a
//!   time: positions are renumbered whole and revs are checked against what
//!   is there.
//! - An entry's `rev` counts changes to its title, pointer and note. A
//!   write that changes those names the rev it read and is refused as
//!   [`Refused::Stale`] if the entry moved on. A status change leaves the
//!   rev alone and needs none, and so does a move (which may name one).
//! - The PROOFS zone (`kind: proofs`) is the board's last, and there is one.
//! - A pointer is stored, not resolved. A row's `note` and `status` are
//!   separate columns.
//! - Deleting a project (or its account) takes its boards.
//! - A machine's report of one of its own agent blocks (`board_block_states`,
//!   [`BlockState`]) is kept apart from the rows that point at it: one per
//!   (machine, pane), set by the machine and read through a row's pointer.

use anyhow::{Context as _, Result, bail};
use arugula_control_wire::boards::{BlockState, BlockWord, Board, Diffstat, EntryKind, Pointer, Status, ZoneKind};

use super::{
    Db,
    chat::{in_json, json_list},
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A new board's id: `brd_` and 32 hex characters.
fn new_board_id() -> String {
    format!("brd_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A new entry's id: `zon_` or `row_` and 32 hex characters.
fn new_entry_id(kind: EntryKind) -> String {
    let p = match kind {
        EntryKind::Zone => "zon",
        EntryKind::Row => "row",
    };
    format!("{p}_{}", hex::encode(arugula_e2e::random::<16>()))
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
}

fn bad<T>(why: impl Into<String>) -> Result<std::result::Result<T, Refused>> {
    Ok(Err(Refused::Bad(why.into())))
}

/// One zone or row as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub kind: EntryKind,
    /// A row's zone.
    pub zone: Option<String>,
    pub rev: u64,
    pub title: String,
    pub zone_kind: Option<ZoneKind>,
    pub note: String,
    pub status: Status,
    pub pointer: Pointer,
    /// The machine whose agent added a row (`boards.write`); none: a person.
    pub agent: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A board and its entries: zones in order, then each zone's rows in order
/// (a row follows the zone it is in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub board: Board,
    pub entries: Vec<Entry>,
}

/// What a new entry, or a change to one, says. Absent: left as it is.
#[derive(Debug, Default, Clone)]
pub struct Put<'a> {
    /// The entry changed; none: make one, of kind `entry`.
    pub id: Option<&'a str>,
    pub rev: Option<u64>,
    pub entry: Option<EntryKind>,
    pub zone: Option<&'a str>,
    pub title: Option<&'a str>,
    pub kind: Option<ZoneKind>,
    pub pointer: Option<&'a Pointer>,
    pub note: Option<&'a str>,
    pub status: Option<Status>,
    /// The machine whose agent makes a new row; none: a person.
    pub agent: Option<&'a str>,
}

/// Where [`Db::move_entry`] puts an entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct Move<'a> {
    /// A row's new zone; none: its own.
    pub zone: Option<&'a str>,
    /// The sibling it goes before; none: last.
    pub before: Option<&'a str>,
    /// The rev it must still have.
    pub rev: Option<u64>,
}

const BOARD_COLS: &str = "id, project_id, title, created_by, created_at, updated_at";
const ENTRY_COLS: &str = "id, kind, zone_id, rev, title, zone_kind, note, status, target_kind, target, pane, \
                          created_at, updated_at, agent";

fn board_row(r: &Row) -> Result<Board> {
    Ok(Board {
        id: r.get(0)?,
        project: r.get(1)?,
        title: r.get(2)?,
        created_by: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
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

fn status_of(s: Option<String>) -> Result<Status> {
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
fn pointer_cols(p: &Pointer) -> (Option<&'static str>, Option<&str>, Option<&str>) {
    match p {
        Pointer::None => (None, None, None),
        Pointer::ForgeItem { id } => (Some("forge_item"), Some(id), None),
        Pointer::Issue { id } => (Some("issue"), Some(id), None),
        Pointer::Run { id } => (Some("run"), Some(id), None),
        Pointer::Workspace { id } => (Some("workspace"), Some(id), None),
        Pointer::Block { machine, pane } => (Some("block"), Some(machine), Some(pane)),
    }
}

fn pointer_of(kind: Option<String>, target: Option<String>, pane: Option<String>) -> Result<Pointer> {
    Ok(match (kind.as_deref(), target, pane) {
        (None, ..) => Pointer::None,
        (Some("forge_item"), Some(id), _) => Pointer::ForgeItem { id },
        (Some("issue"), Some(id), _) => Pointer::Issue { id },
        (Some("run"), Some(id), _) => Pointer::Run { id },
        (Some("workspace"), Some(id), _) => Pointer::Workspace { id },
        (Some("block"), Some(machine), Some(pane)) => Pointer::Block { machine, pane },
        (Some(k), ..) => bail!("a board row pointing at {k:?}"),
    })
}

fn entry_row(r: &Row) -> Result<Entry> {
    let kind = match r.get::<String>(1)?.as_str() {
        "zone" => EntryKind::Zone,
        "row" => EntryKind::Row,
        k => bail!("a board entry of kind {k:?}"),
    };
    let zone_kind = match r.get::<Option<String>>(5)?.as_deref() {
        None => None,
        Some("proofs") => Some(ZoneKind::Proofs),
        Some(k) => bail!("a board zone of kind {k:?}"),
    };
    Ok(Entry {
        id: r.get(0)?,
        kind,
        zone: r.get(2)?,
        rev: r.get(3)?,
        title: r.get(4)?,
        zone_kind,
        note: r.get(6)?,
        status: status_of(r.get(7)?)?,
        pointer: pointer_of(r.get(8)?, r.get(9)?, r.get(10)?)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
        agent: r.get(13)?,
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
        &format!("SELECT {ENTRY_COLS} FROM board_entries WHERE board_id = ?1 AND id = ?2"),
        params![board, id],
        entry_row,
    )
    .optional()
}

/// The ids of a board's zones, or of a zone's rows, in order.
fn siblings(c: &impl Sql, board: &str, zone: Option<&str>) -> Result<Vec<String>> {
    match zone {
        None => c.query_map(
            "SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'zone' ORDER BY position, id",
            params![board],
            |r| r.get(0),
        ),
        Some(z) => c.query_map(
            "SELECT id FROM board_entries WHERE board_id = ?1 AND zone_id = ?2 ORDER BY position, id",
            params![board, z],
            |r| r.get(0),
        ),
    }
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

/// A zone of `board`: its entry, or why not.
fn zone_of(c: &impl Sql, board: &str, id: &str) -> Result<Option<Entry>> {
    Ok(entry(c, board, id)?.filter(|e| e.kind == EntryKind::Zone))
}

fn is_proofs(e: &Entry) -> bool {
    e.zone_kind == Some(ZoneKind::Proofs)
}

impl Db {
    /// Make a board in `project` as `by`. `None`: the project already has
    /// `max` boards. The project is the caller's to have checked.
    pub fn create_board(&self, project: &str, title: &str, by: &str, at: u64, max: u64) -> Result<Option<Board>> {
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
                "INSERT INTO boards (id, project_id, title, created_by, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5) RETURNING {BOARD_COLS}"
            ),
            params![new_board_id(), project, title, by, at],
            board_row,
        )?;
        tx.commit()?;
        Ok(Some(board))
    }

    /// One entry of a board (a zone or a row), as stored.
    pub fn board_entry(&self, board: &str, id: &str) -> Result<Option<Entry>> {
        entry(&self.c(), board, id)
    }

    pub fn board(&self, id: &str) -> Result<Option<Board>> {
        self.c().query_row(&format!("SELECT {BOARD_COLS} FROM boards WHERE id = ?1"), params![id], board_row).optional()
    }

    /// A project's boards, oldest first.
    pub fn boards(&self, project: &str) -> Result<Vec<Board>> {
        self.c().query_map(
            &format!("SELECT {BOARD_COLS} FROM boards WHERE project_id = ?1 ORDER BY created_at, id"),
            params![project],
            board_row,
        )
    }

    /// A board with its entries in order, from one read.
    pub fn board_stored(&self, id: &str) -> Result<Option<Stored>> {
        let c = self.c();
        let Some(board) = c
            .query_row(&format!("SELECT {BOARD_COLS} FROM boards WHERE id = ?1"), params![id], board_row)
            .optional()?
        else {
            return Ok(None);
        };
        let all = c.query_map(
            &format!("SELECT {ENTRY_COLS}, position FROM board_entries WHERE board_id = ?1"),
            params![id],
            |r| Ok((entry_row(r)?, r.get::<u64>(14)?)),
        )?;
        let (mut zones, mut rows): (Vec<_>, Vec<_>) = all.into_iter().partition(|(e, _)| e.kind == EntryKind::Zone);
        zones.sort_by(|a, b| (a.1, &a.0.id).cmp(&(b.1, &b.0.id)));
        rows.sort_by(|a, b| (a.1, &a.0.id).cmp(&(b.1, &b.0.id)));
        let mut entries = Vec::with_capacity(zones.len() + rows.len());
        for (z, _) in zones {
            entries.push(z.clone());
            entries.extend(rows.iter().filter(|(r, _)| r.zone.as_deref() == Some(&z.id)).map(|(r, _)| r.clone()));
        }
        Ok(Some(Stored { board, entries }))
    }

    /// Make or change a zone or a row (see [`Put`]).
    pub fn put_entry(
        &self,
        board: &str,
        p: &Put,
        at: u64,
        max_zones: u64,
        max_rows: u64,
    ) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        match p.id {
            Some(id) => {
                let Some(old) = entry(&tx, board, id)? else { return Ok(Err(Refused::Missing("no such entry"))) };
                let text = p.title.is_some() || p.pointer.is_some() || p.note.is_some();
                if old.kind == EntryKind::Zone && (p.pointer.is_some() || p.note.is_some() || p.status.is_some()) {
                    return bad("a zone has a title, and nothing else to change");
                }
                if old.kind == EntryKind::Row && p.title.is_some() {
                    return bad("a row has a note, not a title");
                }
                if p.kind.is_some_and(|k| Some(k) != old.zone_kind) || p.entry.is_some_and(|e| e != old.kind) {
                    return bad("an entry's kind can't change");
                }
                if p.zone.is_some_and(|z| Some(z) != old.zone.as_deref()) {
                    return bad("a row changes zone by a move");
                }
                if !text && p.status.is_none() {
                    return bad("nothing to change");
                }
                if text {
                    let Some(rev) = p.rev else { return bad("name the rev you read (\"rev\")") };
                    if rev != old.rev {
                        return Ok(Err(Refused::Stale { rev: old.rev }));
                    }
                    let (kind, target, pane) =
                        p.pointer.map(pointer_cols).unwrap_or_else(|| pointer_cols(&old.pointer));
                    tx.execute(
                        "UPDATE board_entries SET rev = rev + 1, title = ?2, note = ?3, target_kind = ?4, target = ?5, pane = ?6,
                             updated_at = ?7 WHERE id = ?1",
                        params![id, p.title.unwrap_or(&old.title), p.note.unwrap_or(&old.note), kind, target, pane, at],
                    )?;
                }
                if let Some(s) = p.status {
                    tx.execute(
                        "UPDATE board_entries SET status = ?2, updated_at = ?3 WHERE id = ?1",
                        params![id, status_str(s), at],
                    )?;
                }
            }
            None => match p.entry {
                Some(EntryKind::Zone) => {
                    let Some(title) = p.title else { return bad("a zone needs a title") };
                    if p.zone.is_some() || p.pointer.is_some() || p.note.is_some() || p.status.is_some() {
                        return bad("a zone has a title, and nothing else");
                    }
                    let mut ids = siblings(&tx, board, None)?;
                    if ids.len() as u64 >= max_zones {
                        return bad(format!("a board has at most {max_zones} zones"));
                    }
                    let last_is_proofs = match ids.last() {
                        Some(l) => entry(&tx, board, l)?.is_some_and(|e| is_proofs(&e)),
                        None => false,
                    };
                    if p.kind == Some(ZoneKind::Proofs) && last_is_proofs {
                        return bad("a board has one proofs zone");
                    }
                    let id = new_entry_id(EntryKind::Zone);
                    tx.execute(
                        "INSERT INTO board_entries (id, board_id, kind, position, title, zone_kind, created_at, updated_at)
                         VALUES (?1, ?2, 'zone', 0, ?3, ?4, ?5, ?5)",
                        params![id, board, title, p.kind.map(|_| "proofs"), at],
                    )?;
                    // Before the proofs zone, which stays last; a new proofs
                    // zone goes last.
                    let at_index = if last_is_proofs && p.kind.is_none() { ids.len() - 1 } else { ids.len() };
                    ids.insert(at_index, id);
                    renumber(&tx, &ids)?;
                }
                Some(EntryKind::Row) => {
                    let Some(zone) = p.zone else { return bad("a row needs its zone (\"zone\")") };
                    if p.title.is_some() || p.kind.is_some() {
                        return bad("a row has a note, not a title");
                    }
                    if zone_of(&tx, board, zone)?.is_none() {
                        return Ok(Err(Refused::Missing("no such zone")));
                    }
                    let n: u64 = tx.query_row(
                        "SELECT count(*) FROM board_entries WHERE board_id = ?1 AND zone_id = ?2",
                        params![board, zone],
                        |r| r.get(0),
                    )?;
                    if n >= max_rows {
                        return bad(format!("a zone has at most {max_rows} rows"));
                    }
                    let (kind, target, pane) = p.pointer.map(pointer_cols).unwrap_or((None, None, None));
                    tx.execute(
                        "INSERT INTO board_entries (id, board_id, kind, zone_id, position, note, status, target_kind, target,
                             pane, created_at, updated_at, agent)
                         VALUES (?1, ?2, 'row', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11)",
                        params![
                            new_entry_id(EntryKind::Row),
                            board,
                            zone,
                            n,
                            p.note.unwrap_or(""),
                            p.status.and_then(status_str),
                            kind,
                            target,
                            pane,
                            at,
                            p.agent
                        ],
                    )?;
                }
                None => return bad("say what to make (\"entry\": zone or row), or which entry to change (\"id\")"),
            },
        }
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Put `id` just before `before` among its siblings (last without one),
    /// a row in `zone` (its own without one). A zone stays before the
    /// proofs zone, which stays last. `rev`, if named, must be the entry's.
    pub fn move_entry(
        &self,
        board: &str,
        id: &str,
        m: &Move,
        at: u64,
        max_rows: u64,
    ) -> Result<std::result::Result<(), Refused>> {
        let Move { zone, before, rev } = *m;
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(e) = entry(&tx, board, id)? else { return Ok(Err(Refused::Missing("no such entry"))) };
        if let Some(r) = rev
            && r != e.rev
        {
            return Ok(Err(Refused::Stale { rev: e.rev }));
        }
        if before == Some(id) {
            return bad("an entry can't go before itself");
        }
        match e.kind {
            EntryKind::Zone => {
                if zone.is_some() {
                    return bad("a zone moves among zones; \"zone\" is for rows");
                }
                if is_proofs(&e) {
                    return bad("the proofs zone stays last");
                }
                let mut ids = siblings(&tx, board, None)?;
                ids.retain(|i| i != id);
                let last_is_proofs = match ids.last() {
                    Some(l) => entry(&tx, board, l)?.is_some_and(|z| is_proofs(&z)),
                    None => false,
                };
                let index = match before {
                    Some(b) => match ids.iter().position(|i| i == b) {
                        Some(i) => i,
                        None => return bad("\"before\" is a zone of this board"),
                    },
                    None if last_is_proofs => ids.len() - 1,
                    None => ids.len(),
                };
                ids.insert(index, id.to_owned());
                renumber(&tx, &ids)?;
            }
            EntryKind::Row => {
                let from = e.zone.clone().context("a row without a zone")?;
                let to = zone.unwrap_or(&from);
                if zone_of(&tx, board, to)?.is_none() {
                    return Ok(Err(Refused::Missing("no such zone")));
                }
                let mut ids = siblings(&tx, board, Some(to))?;
                ids.retain(|i| i != id);
                if to != from && ids.len() as u64 >= max_rows {
                    return bad(format!("a zone has at most {max_rows} rows"));
                }
                let index = match before {
                    Some(b) => match ids.iter().position(|i| i == b) {
                        Some(i) => i,
                        None => return bad("\"before\" is a row of the zone it goes to"),
                    },
                    None => ids.len(),
                };
                if to != from {
                    tx.execute("UPDATE board_entries SET zone_id = ?2 WHERE id = ?1", params![id, to])?;
                }
                ids.insert(index, id.to_owned());
                renumber(&tx, &ids)?;
                if to != from {
                    let mut old = siblings(&tx, board, Some(&from))?;
                    old.retain(|i| i != id);
                    renumber(&tx, &old)?;
                }
            }
        }
        tx.execute("UPDATE board_entries SET updated_at = ?2 WHERE id = ?1", params![id, at])?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Delete a row, or a zone and its rows.
    pub fn delete_entry(&self, board: &str, id: &str, at: u64) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(e) = entry(&tx, board, id)? else { return Ok(Err(Refused::Missing("no such entry"))) };
        match e.kind {
            EntryKind::Zone => {
                tx.execute("DELETE FROM board_entries WHERE board_id = ?1 AND zone_id = ?2", params![board, id])?;
                tx.execute("DELETE FROM board_entries WHERE id = ?1", params![id])?;
                renumber(&tx, &siblings(&tx, board, None)?)?;
            }
            EntryKind::Row => {
                tx.execute("DELETE FROM board_entries WHERE id = ?1", params![id])?;
                let zone = e.zone.context("a row without a zone")?;
                renumber(&tx, &siblings(&tx, board, Some(&zone))?)?;
            }
        }
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// The boards with these ids (`brd_…`): each one's id, project and
    /// title. One statement; an id that isn't there isn't in the list.
    /// The `fi_` and `iss_` ids that the rows of `board` point at, by kind
    /// (forge items, then issues): what a read resolves in one query each.
    pub fn board_targets(&self, board: &str) -> Result<(Vec<String>, Vec<String>)> {
        let rows: Vec<(String, String)> = self.c().query_map(
            "SELECT target_kind, target FROM board_entries
             WHERE board_id = ?1 AND target_kind IN ('forge_item', 'issue')",
            params![board],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let (mut items, mut issues) = (vec![], vec![]);
        for (k, t) in rows {
            if k == "issue" { issues.push(t) } else { items.push(t) }
        }
        Ok((items, issues))
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
                "SELECT DISTINCT b.id, b.project_id FROM board_entries e
                 JOIN boards b ON b.id = e.board_id
                 JOIN forge_items i ON i.id = e.target
                 WHERE e.target_kind = 'forge_item' AND i.repo_id = ?1 AND ({nums} OR {heads})"
            ),
            params![repo, json_list(shas)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The native issues among `ids` in `project`, as (id, number, title,
    /// closed): one read for any number (a board's rows).
    pub fn issue_states(&self, project: &str, ids: &[&str]) -> Result<Vec<(String, u64, String, bool)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT id, number, title, closed_at IS NOT NULL FROM issues WHERE project_id = ?1 AND {}",
                in_json(&c, "id", 2)
            ),
            params![project, json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
    }

    /// The boards (id, project) with a row that points at native issue `id`.
    pub fn boards_on_issue(&self, id: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT DISTINCT b.id, b.project_id FROM board_entries e JOIN boards b ON b.id = e.board_id
             WHERE e.target_kind = 'issue' AND e.target = ?1",
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
            "SELECT DISTINCT b.id, b.project_id FROM board_entries e JOIN boards b ON b.id = e.board_id
             WHERE e.target_kind = 'block' AND e.target = ?1 AND e.pane = ?2",
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
        "DELETE FROM board_entries WHERE board_id IN
             (SELECT id FROM boards WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM boards WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE boards SET created_by = '' WHERE created_by = ?1", params![d.account])?;
    c.execute("DELETE FROM board_block_states WHERE account = ?1", params![d.account])?;
    Ok(())
}
