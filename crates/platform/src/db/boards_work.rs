//! The one-time move of board rows to work items (Arugula #405,
//! `0080_work_items`): run on start, per board that has an issue row without
//! one, and a no-op once none has.
//!
//! Each issue row gets the work item of its issue ([`Db::work_item_for`]).
//! Where two rows of a board turn out to be one work item (a native issue
//! and the GitHub issue it was exported to, each a row), the later row is
//! folded into the earlier ([`super::work_items::fold_rows`]): its
//! attachments and asks move onto the earlier row, its note is added to the
//! end of the earlier row's, and the row is dropped. A row whose issue the
//! platform can't place (its repo is no longer linked, say) keeps no work
//! item, and is looked at again next start.
//!
//! Each board is one transaction holding the board's row, so two machines
//! starting together don't move a board twice. PR rows, notes and the rest
//! never have a work item and are not looked at.

use anyhow::Result;

use super::{
    Db,
    boards::lock,
    sql::{Optional as _, Sql, params},
    work_items::{fold_rows, work_item_for},
};

/// What a start moved.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkMoved {
    pub boards: usize,
    /// Rows that got a work item.
    pub rows: usize,
    /// Rows folded into another row of their work item.
    pub folded: usize,
}

impl Db {
    /// Give every issue row of every board its work item (see the module).
    /// The platform's start runs it after the migrations and lanes' move.
    pub fn work_move(&self) -> Result<WorkMoved> {
        let boards: Vec<String> = self.c().query_map(
            "SELECT DISTINCT board_id FROM board_entries
             WHERE kind = 'row' AND work_item IS NULL AND (target_kind = 'issue' OR
                (target_kind = 'forge_item' AND target IN (SELECT id FROM forge_items WHERE kind = 'issue')))
             ORDER BY board_id",
            params![],
            |r| r.get(0),
        )?;
        let mut moved = WorkMoved::default();
        for b in boards {
            let (rows, folded) = self.work_move_board(&b)?;
            if rows + folded > 0 {
                moved.boards += 1;
            }
            moved.rows += rows;
            moved.folded += folded;
        }
        Ok(moved)
    }

    fn work_move_board(&self, board: &str) -> Result<(usize, usize)> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            tx.commit()?;
            return Ok((0, 0));
        }
        let at = arugula_e2e::now_ms();
        let todo: Vec<(String, String)> = tx.query_map(
            "SELECT id, target FROM board_entries
             WHERE board_id = ?1 AND kind = 'row' AND work_item IS NULL AND target_kind IN ('issue', 'forge_item')
             ORDER BY position, id",
            params![board],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let (mut moved, mut folded) = (0, 0);
        for (row, target) in todo {
            // A row is gone when an earlier row's work item merged it away.
            let held = |tx: &super::sql::Tx<'_, '_>| -> Result<Option<Option<String>>> {
                tx.query_row("SELECT work_item FROM board_entries WHERE id = ?1", params![row], |r| {
                    r.get::<Option<String>>(0)
                })
                .optional()
            };
            if !matches!(held(&tx)?, Some(None)) {
                continue;
            }
            let Some(wi) = work_item_for(&tx, &target, at)? else { continue };
            // Merging may have folded this row or given it its work item.
            match held(&tx)? {
                None => continue,
                Some(Some(_)) => {
                    moved += 1;
                    continue;
                }
                Some(None) => {}
            }
            let other: Option<String> = tx
                .query_row(
                    "SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'row' AND work_item = ?2 AND id <> ?3",
                    params![board, wi, row],
                    |r| r.get(0),
                )
                .optional()?;
            let keep = match other {
                Some(other) => {
                    let place = |id: &str| -> Result<(u64, String)> {
                        Ok((
                            tx.query_row("SELECT position FROM board_entries WHERE id = ?1", params![id], |r| {
                                r.get(0)
                            })?,
                            id.to_owned(),
                        ))
                    };
                    let (into, from) = if place(&row)? < place(&other)? { (&row, &other) } else { (&other, &row) };
                    fold_rows(&tx, board, into, from, at)?;
                    folded += 1;
                    into.clone()
                }
                None => row.clone(),
            };
            tx.execute("UPDATE board_entries SET work_item = ?2 WHERE id = ?1", params![keep, wi])?;
            moved += 1;
        }
        tx.commit()?;
        Ok((moved, folded))
    }
}
