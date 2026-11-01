//! Boards that follow a tracker's epic (Arugula #483, `0086_epics_as_boards`):
//! storage for `crate::boards::epics`, which decides what a board should
//! hold.
//!
//! - A board made from an epic has `source_item`, the epic's `fi_` id, unique
//!   in its project, and is made by the connection: `created_by` is empty and
//!   no agent made it.
//! - A row the sync adds has `source` `plane`, and goes in through the same
//!   rules as any row: ungrouped, one row for a piece of work. A row a person
//!   (or their agent) added is never taken over, counted against the board's
//!   room, or removed here.

use std::collections::HashSet;

use anyhow::Result;
use arugula_control_wire::boards::{Board, Pointer};

use super::{
    Db,
    boards::{BOARD_COLS, lock, new_row_id, renumber, siblings, touch},
    sql::{Optional as _, Row, Sql, params},
};

/// A board to make from an epic.
#[derive(Debug, Clone)]
pub struct NewEpicBoard<'a> {
    pub project: &'a str,
    pub title: &'a str,
    pub goal: &'a str,
    /// The epic's `fi_` id.
    pub source_item: &'a str,
    /// `ARUGU-10`.
    pub key: &'a str,
    pub url: &'a str,
    /// The connection's name.
    pub connection: &'a str,
}

/// What the epic's state asks of its board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow<'a> {
    Open,
    /// Closed, with this done note.
    Closed(&'a str),
}

/// A child of an epic that may be a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    /// The child's `fi_` id.
    pub id: String,
    pub done: bool,
    pub updated_at: u64,
}

/// A row's id, source, target and work item.
type EntryFacts = (String, Option<String>, Option<String>, Option<String>);

/// What [`Db::sync_epic_rows`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    pub added: usize,
    pub removed: usize,
    /// Children the board had no room for.
    pub left_out: u32,
    /// Whether anything on the board changed.
    pub changed: bool,
}

fn board_with_source(r: &Row) -> Result<(Board, String)> {
    Ok((super::boards::board_row(r)?, r.get(17)?))
}

/// Delete a row with what is attached to it.
fn delete_row(c: &impl Sql, id: &str) -> Result<()> {
    c.execute("DELETE FROM board_attachments WHERE row_id = ?1", params![id])?;
    c.execute("DELETE FROM board_asks WHERE row_id = ?1", params![id])?;
    c.execute("DELETE FROM board_entries WHERE id = ?1", params![id])?;
    Ok(())
}

/// Why a row the sync added can't be removed by hand: it is a child of its
/// epic in the tracker.
pub(super) fn follows_the_epic(c: &impl Sql, board: &str, row: &Pointer) -> Result<String> {
    let epic: Option<String> =
        c.query_row("SELECT source_key FROM boards WHERE id = ?1", params![board], |r| r.get(0)).optional()?.flatten();
    let child: Option<String> = match row {
        Pointer::ForgeItem { id } => c
            .query_row(
                "SELECT c.identifier || '-' || i.number FROM forge_items i
                 JOIN tracker_connections c ON c.id = i.connection_id WHERE i.id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?,
        _ => None,
    };
    Ok(match (child, epic) {
        (Some(child), Some(epic)) => format!("{child} is a child of {epic} in Plane: change its parent there"),
        _ => "this row is a child of its epic in Plane: change its parent there".to_owned(),
    })
}

impl Db {
    /// The boards of `project` made from one of the epics `items` (`fi_`),
    /// with the epic each follows.
    pub fn epic_boards(&self, project: &str, items: &[String]) -> Result<Vec<(Board, String)>> {
        if items.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT {BOARD_COLS}, source_item FROM boards WHERE project_id = ?1 AND {}",
                super::chat::in_json(&c, "source_item", 2)
            ),
            params![project, super::chat::json_list(items)?],
            board_with_source,
        )
    }

    /// Make a board from an epic, by the connection. `None`: the project is
    /// gone or has `max` boards. A board the epic has already is returned as
    /// it is.
    pub fn create_epic_board(&self, n: &NewEpicBoard<'_>, at: u64, max: u64) -> Result<Option<Board>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", tx.for_update()), params![n.project], |r| {
                r.get(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(None);
        }
        let had = tx
            .query_row(
                &format!("SELECT {BOARD_COLS} FROM boards WHERE project_id = ?1 AND source_item = ?2"),
                params![n.project, n.source_item],
                super::boards::board_row,
            )
            .optional()?;
        if had.is_some() {
            return Ok(had);
        }
        let count: u64 =
            tx.query_row("SELECT count(*) FROM boards WHERE project_id = ?1", params![n.project], |r| r.get(0))?;
        if count >= max {
            return Ok(None);
        }
        let id = format!("brd_{}", hex::encode(arugula_e2e::random::<16>()));
        let board = tx.query_row(
            &format!(
                "INSERT INTO boards (id, project_id, title, goal, created_by, created_at, updated_at, source_item,
                     source_key, source_url, source_name)
                 VALUES (?1, ?2, ?3, ?4, '', ?5, ?5, ?6, ?7, ?8, ?9) RETURNING {BOARD_COLS}"
            ),
            params![id, n.project, n.title, n.goal, at, n.source_item, n.key, n.url, n.connection],
            super::boards::board_row,
        )?;
        tx.commit()?;
        Ok(Some(board))
    }

    /// Make the board follow its epic: its title, and open or closed. A board
    /// is reopened only if the connection closed it, so a person's close
    /// stands. Whether anything changed; `false` too when there is no such
    /// board.
    pub fn follow_epic(&self, board: &str, title: &str, how: Follow<'_>, at: u64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(false);
        }
        let (had, closed_at, closed_by, agent): (String, Option<u64>, String, Option<String>) = tx.query_row(
            "SELECT title, closed_at, closed_by, closed_by_agent FROM boards WHERE id = ?1",
            params![board],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let mut changed = false;
        if had != title {
            tx.execute("UPDATE boards SET title = ?2 WHERE id = ?1", params![board, title])?;
            changed = true;
        }
        match how {
            Follow::Closed(note) if closed_at.is_none() => {
                tx.execute(
                    "UPDATE boards SET done_note = ?2, closed_by = '', closed_by_agent = NULL, closed_at = ?3
                     WHERE id = ?1",
                    params![board, note, at],
                )?;
                changed = true;
            }
            Follow::Open if closed_at.is_some() && closed_by.is_empty() && agent.is_none() => {
                tx.execute("UPDATE boards SET closed_at = NULL WHERE id = ?1", params![board])?;
                changed = true;
            }
            _ => {}
        }
        if changed {
            touch(&tx, board, at)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Make the rows the sync added to `board` the `wanted` children: add
    /// the missing, remove those no longer wanted. A child that is a row of a
    /// person's already stays theirs. Of more children than the board has
    /// room for (`max_rows`, less the person's rows), the open ones go in
    /// first, then the done ones, newest first; the rest are counted as left
    /// out. `None`: no such board.
    pub fn sync_epic_rows(&self, board: &str, wanted: &[Want], at: u64, max_rows: u64) -> Result<Option<Synced>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(None);
        }
        let rows: Vec<EntryFacts> = tx.query_map(
            "SELECT id, source, target, work_item FROM board_entries WHERE board_id = ?1 AND kind = 'row'",
            params![board],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let theirs: HashSet<&str> = rows.iter().filter(|r| r.1.is_none()).filter_map(|r| r.3.as_deref()).collect();
        let people = rows.iter().filter(|r| r.1.is_none()).count() as u64;
        let mut candidates: Vec<&Want> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        for w in wanted {
            let Some(item) = super::work_items::work_item_for(&tx, &w.id, at)? else { continue };
            if theirs.contains(item.as_str()) || !seen.insert(item) {
                continue;
            }
            candidates.push(w);
        }
        let room = max_rows.saturating_sub(people) as usize;
        let mut by_rank = candidates.clone();
        by_rank.sort_by(|a, b| a.done.cmp(&b.done).then(b.updated_at.cmp(&a.updated_at)).then(a.id.cmp(&b.id)));
        let keep: HashSet<&str> = by_rank.iter().take(room).map(|w| w.id.as_str()).collect();
        let left_out = candidates.len().saturating_sub(keep.len()) as u32;

        let mut out = Synced { left_out, ..Default::default() };
        let mut have: HashSet<&str> = HashSet::new();
        for (id, source, target, _) in &rows {
            if source.is_none() {
                continue;
            }
            match target.as_deref() {
                Some(t) if keep.contains(t) => {
                    have.insert(t);
                }
                _ => {
                    delete_row(&tx, id)?;
                    out.removed += 1;
                }
            }
        }
        let first = rows.len() as u64 - out.removed as u64;
        let missing = candidates.iter().filter(|w| keep.contains(w.id.as_str()) && !have.contains(w.id.as_str()));
        for (position, w) in (first..).zip(missing) {
            let item = super::work_items::work_item_for(&tx, &w.id, at)?;
            tx.execute(
                "INSERT INTO board_entries (id, board_id, kind, position, note, target_kind, target, created_at,
                     updated_at, work_item, source)
                 VALUES (?1, ?2, 'row', ?3, '', 'forge_item', ?4, ?5, ?5, ?6, 'plane')",
                params![new_row_id(), board, position, w.id, at, item],
            )?;
            out.added += 1;
        }
        let before: u64 =
            tx.query_row("SELECT source_left_out FROM boards WHERE id = ?1", params![board], |r| r.get(0))?;
        if before != u64::from(left_out) {
            tx.execute("UPDATE boards SET source_left_out = ?2 WHERE id = ?1", params![board, left_out])?;
        }
        out.changed = out.added + out.removed > 0 || before != u64::from(left_out);
        if out.changed {
            renumber(&tx, &siblings(&tx, board)?)?;
            touch(&tx, board, at)?;
        }
        tx.commit()?;
        Ok(Some(out))
    }

    /// Take the rows the sync added for these children off every board but
    /// the one of the epic each now belongs to (`Some`: its `fi_` id; none:
    /// it has no epic, or is gone). Those boards (id, project).
    pub fn drop_moved_rows(&self, children: &[(String, Option<String>)], at: u64) -> Result<Vec<(String, String)>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let mut touched: Vec<(String, String)> = vec![];
        for (child, epic) in children {
            let found: Vec<(String, String, String)> = tx.query_map(
                "SELECT e.id, e.board_id, b.project_id FROM board_entries e JOIN boards b ON b.id = e.board_id
                 WHERE e.source = 'plane' AND e.kind = 'row' AND e.target = ?1
                     AND (b.source_item IS NULL OR b.source_item <> ?2)
                 ORDER BY e.board_id",
                params![child, epic.as_deref().unwrap_or("")],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            for (row, board, project) in found {
                lock(&tx, &board)?;
                delete_row(&tx, &row)?;
                renumber(&tx, &siblings(&tx, &board)?)?;
                touch(&tx, &board, at)?;
                if !touched.iter().any(|t| t.0 == board) {
                    touched.push((board, project));
                }
            }
        }
        tx.commit()?;
        Ok(touched)
    }
}
