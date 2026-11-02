//! A board's Docs (Arugula #517, `0087_board_docs`): the project Docs and
//! the machines' Doc blocks that a board links, in order, in its strip
//! (`row_id` NULL) or on one row, at most [`BOARD_DOCS_MAX`] of each. A row's
//! Docs go with the row ([`delete_of_rows`]). Who may link is the routes'
//! (`crate::boards::docs`); this is storage.
//!
//! - A Doc is in a strip once, and on a row once: linking it again answers
//!   the entry that is there. The limit is counted inside the board's lock.
//! - A project Doc's title and sections are read live, through a LEFT JOIN
//!   on the project's Docs, so a Doc that was deleted reads as missing and
//!   its entry stays. A machine Doc keeps the title it had when linked.

use anyhow::Result;
use arugula_control_wire::boards::{BOARD_DOCS_MAX, Pointer};

use super::{
    Db,
    boards::{Refused, entry, lock, touch},
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A new entry's id: `bdc_` and 32 hex characters.
fn new_id() -> String {
    format!("bdc_{}", hex::encode(arugula_e2e::random::<16>()))
}

fn bad<T>(why: impl Into<String>) -> Result<std::result::Result<T, Refused>> {
    Ok(Err(Refused::Bad(why.into())))
}

/// A project Doc as a chip reads it: its title now, and its sections and
/// how many are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub title: String,
    pub sections: u32,
    pub written: u32,
}

/// One Doc of a board, as stored, with its project Doc's head when it is
/// one and the Doc is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    pub id: String,
    /// The row it is on; none: the board's strip.
    pub row: Option<String>,
    pub pointer: Pointer,
    /// A machine Doc's title when it was linked.
    pub title: String,
    pub head: Option<Head>,
    pub added_by: String,
    pub agent: Option<String>,
    pub created_at: u64,
}

/// A board of the project that lists a Doc: in its strip, and on how many
/// rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub board: String,
    pub title: String,
    pub strip: bool,
    pub rows: u32,
}

/// What [`Db::add_board_doc`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    /// A new entry, last.
    New(String),
    /// The Doc was linked already: that entry.
    Already(String),
}

/// A pointer as an entry stores it: kind, target, machine and title.
fn cols(p: &Pointer) -> Option<(&'static str, &str, &str, &str)> {
    match p {
        Pointer::Doc { id } => Some(("doc", id, "", "")),
        Pointer::MachineDoc { machine, doc, title } => Some(("machine_doc", doc, machine, title)),
        _ => None,
    }
}

fn strip_row(r: &Row) -> Result<Strip> {
    let (kind, target, machine, title): (String, String, String, String) = (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
    let (pointer, head) = match kind.as_str() {
        "doc" => {
            let head = match r.get::<Option<String>>(8)? {
                Some(t) => Some(Head { title: t, sections: r.get(9)?, written: r.get(10)? }),
                None => None,
            };
            (Pointer::Doc { id: target }, head)
        }
        _ => (Pointer::MachineDoc { machine, doc: target, title: title.clone() }, None),
    };
    Ok(Strip {
        id: r.get(0)?,
        row: r.get(11)?,
        pointer,
        title,
        head,
        added_by: r.get(5)?,
        agent: r.get(6)?,
        created_at: r.get(7)?,
    })
}

const STRIP_COLS: &str = "b.id, b.target_kind, b.target, b.machine, b.title, b.added_by, b.agent, b.created_at, \
     d.title, (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id), \
     (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id AND s.body IS NOT NULL), b.row_id";

const STRIP_FROM: &str = "board_docs b LEFT JOIN project_docs d ON b.target_kind = 'doc' AND d.id = b.target";

impl Db {
    /// Every Doc of a board, its strip's and its rows', in order: the
    /// strip's first, then each row's, each in its own order.
    pub fn board_docs(&self, board: &str) -> Result<Vec<Strip>> {
        self.c().query_map(
            &format!(
                "SELECT {STRIP_COLS} FROM {STRIP_FROM} WHERE b.board_id = ?1
                 ORDER BY b.row_id IS NOT NULL, b.row_id, b.position, b.id"
            ),
            params![board],
            strip_row,
        )
    }

    /// One Doc of a board, on its strip or a row.
    pub fn board_doc(&self, board: &str, id: &str) -> Result<Option<Strip>> {
        self.c()
            .query_row(
                &format!("SELECT {STRIP_COLS} FROM {STRIP_FROM} WHERE b.board_id = ?1 AND b.id = ?2"),
                params![board, id],
                strip_row,
            )
            .optional()
    }

    /// Link a Doc last, in the board's strip or (`row`) on that row. A Doc
    /// linked there already answers its entry; past [`BOARD_DOCS_MAX`] is
    /// refused.
    pub fn add_board_doc(
        &self,
        board: &str,
        row: Option<&str>,
        pointer: &Pointer,
        by: &str,
        agent: Option<&str>,
        at: u64,
    ) -> Result<std::result::Result<Added, Refused>> {
        let Some((kind, target, machine, title)) = cols(pointer) else { return bad("a Doc is linked by its pointer") };
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        if let Some(row) = row
            && entry(&tx, board, row)?.is_none()
        {
            return Ok(Err(Refused::Missing("no such row")));
        }
        let scope = row.unwrap_or("");
        let there: Option<String> = tx
            .query_row(
                "SELECT id FROM board_docs WHERE board_id = ?1 AND COALESCE(row_id, '') = ?5
                   AND target_kind = ?2 AND target = ?3 AND machine = ?4",
                params![board, kind, target, machine, scope],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = there {
            return Ok(Ok(Added::Already(id)));
        }
        let (n, last): (u64, Option<i64>) = tx.query_row(
            "SELECT count(*), max(position) FROM board_docs WHERE board_id = ?1 AND COALESCE(row_id, '') = ?2",
            params![board, scope],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if n >= BOARD_DOCS_MAX {
            let (what, has) = if row.is_some() { ("a row", "has") } else { ("a board", "lists") };
            return bad(format!("{what} {has} at most {BOARD_DOCS_MAX} Docs"));
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO board_docs (id, board_id, row_id, position, target_kind, target, machine, title, added_by,
                                    agent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![id, board, row, last.map_or(0, |p| p + 1), kind, target, machine, title, by, agent, at],
        )?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(Added::New(id)))
    }

    /// Take a Doc off its strip or row.
    pub fn remove_board_doc(&self, board: &str, id: &str, at: u64) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(scope) = scope_of(&tx, board, id)? else {
            return Ok(Err(Refused::Missing("no such Doc on this board")));
        };
        tx.execute("DELETE FROM board_docs WHERE board_id = ?1 AND id = ?2", params![board, id])?;
        renumber(&tx, &in_scope(&tx, board, scope.as_deref())?)?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// Move a Doc before another of its strip or row; none: last.
    pub fn move_board_doc(
        &self,
        board: &str,
        id: &str,
        before: Option<&str>,
        at: u64,
    ) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(scope) = scope_of(&tx, board, id)? else {
            return Ok(Err(Refused::Missing("no such Doc on this board")));
        };
        let mut order = in_scope(&tx, board, scope.as_deref())?;
        let Some(from) = order.iter().position(|d| d == id) else {
            return Ok(Err(Refused::Missing("no such Doc on this board")));
        };
        let moved = order.remove(from);
        let to = match before {
            None => order.len(),
            Some(b) if b == id => return bad("a Doc can't go before itself"),
            Some(b) => match order.iter().position(|d| d == b) {
                Some(i) => i,
                None => return Ok(Err(Refused::Missing("no such Doc to go before, in the same strip or row"))),
            },
        };
        order.insert(to, moved);
        renumber(&tx, &order)?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// The boards (id, project) that list a project Doc, in the strip or on
    /// a row: who to tell that it changed.
    pub fn boards_on_doc(&self, doc: &str) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT DISTINCT b.id, b.project_id FROM boards b WHERE b.id IN
                 (SELECT board_id FROM board_docs WHERE target_kind = 'doc' AND target = ?1)",
            params![doc],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The boards of `project` that list a project Doc, open ones first.
    pub fn doc_listings(&self, project: &str, doc: &str) -> Result<Vec<Listing>> {
        self.c().query_map(
            "SELECT b.id, b.title,
                    EXISTS (SELECT 1 FROM board_docs d WHERE d.board_id = b.id AND d.row_id IS NULL
                            AND d.target_kind = 'doc' AND d.target = ?2),
                    (SELECT count(*) FROM board_docs d WHERE d.board_id = b.id AND d.row_id IS NOT NULL
                     AND d.target_kind = 'doc' AND d.target = ?2)
             FROM boards b WHERE b.project_id = ?1 AND
                 EXISTS (SELECT 1 FROM board_docs d WHERE d.board_id = b.id AND d.target_kind = 'doc'
                         AND d.target = ?2)
             ORDER BY b.closed_at IS NOT NULL, b.created_at, b.id",
            params![project, doc],
            |r| Ok(Listing { board: r.get(0)?, title: r.get(1)?, strip: r.get(2)?, rows: r.get(3)? }),
        )
    }
}

/// The strip or row a Doc is in: `Some(None)` is the strip, none: no such
/// Doc on the board.
fn scope_of(c: &impl Sql, board: &str, id: &str) -> Result<Option<Option<String>>> {
    c.query_row("SELECT row_id FROM board_docs WHERE board_id = ?1 AND id = ?2", params![board, id], |r| r.get(0))
        .optional()
}

/// The ids of the Docs of one strip (`row` none) or row, in order.
fn in_scope(c: &impl Sql, board: &str, row: Option<&str>) -> Result<Vec<String>> {
    c.query_map(
        "SELECT id FROM board_docs WHERE board_id = ?1 AND COALESCE(row_id, '') = ?2 ORDER BY position, id",
        params![board, row.unwrap_or("")],
        |r| r.get(0),
    )
}

/// Number `ids` 0, 1, 2... as their strip's or row's order.
fn renumber(c: &impl Sql, ids: &[String]) -> Result<()> {
    for (i, id) in ids.iter().enumerate() {
        c.execute("UPDATE board_docs SET position = ?2 WHERE id = ?1 AND position != ?2", params![id, i])?;
    }
    Ok(())
}

/// Take the Docs of these rows with them, inside the transaction that
/// deletes the rows.
pub(super) fn delete_of_rows(c: &impl Sql, rows: &[String]) -> Result<()> {
    for r in rows {
        c.execute("DELETE FROM board_docs WHERE row_id = ?1", params![r])?;
    }
    Ok(())
}

/// The Docs of every row of a group (a group goes with its rows).
pub(super) fn delete_of_group(c: &impl Sql, board: &str, group: &str) -> Result<()> {
    c.execute(
        "DELETE FROM board_docs WHERE row_id IN
             (SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'row' AND zone_id = ?2)",
        params![board, group],
    )?;
    Ok(())
}

/// Fold row `from`'s Docs into row `into` (two rows of one work item): those
/// `into` has already go, the rest are `into`'s, after its own.
pub(super) fn fold_rows(c: &impl Sql, into: &str, from: &str) -> Result<()> {
    c.execute(
        "DELETE FROM board_docs WHERE row_id = ?2 AND EXISTS
            (SELECT 1 FROM board_docs k WHERE k.row_id = ?1 AND k.target_kind = board_docs.target_kind
               AND k.target = board_docs.target AND k.machine = board_docs.machine)",
        params![into, from],
    )?;
    c.execute(
        "UPDATE board_docs SET row_id = ?1,
             position = position + (SELECT count(*) FROM board_docs k WHERE k.row_id = ?1)
         WHERE row_id = ?2",
        params![into, from],
    )?;
    Ok(())
}

/// What deleting an account leaves of Docs, inside the platform's
/// `account_deleted` transaction: an entry it added stays, with no one who
/// added it. (Those of projects that went are taken by
/// [`super::boards::erase_account`].)
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("UPDATE board_docs SET added_by = '' WHERE added_by = ?1", params![d.account])?;
    Ok(())
}
