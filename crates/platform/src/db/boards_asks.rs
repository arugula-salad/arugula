//! Asks on boards (Arugula #367, `0074_board_asks`): a question on a row
//! that a person answers. Who may ask and answer is decided by the routes
//! (`crate::boards::asks`); this is storage.
//!
//! - A row has at most one open ask (an `answer` that is NULL), and only a
//!   row with an issue or a PR as its item can have one. Both are checked
//!   inside the board's lock, so two asks made together can't both open.
//! - An answer closes the ask and keeps it: the ask stays as the row's
//!   history, with who answered and when.
//! - The asks a board raises itself are worked out on a read and not kept.

use anyhow::Result;
use arugula_control_wire::boards::Pointer;

use super::{
    Db,
    boards::{Refused, entry, lock, touch},
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A new ask's id: `ask_` and 32 hex characters.
fn new_ask_id() -> String {
    format!("ask_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// One ask as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskRow {
    pub id: String,
    pub row: String,
    /// The account it is for.
    pub who: String,
    pub question: String,
    pub why: String,
    pub options: Vec<String>,
    pub recommend: Option<String>,
    /// The account that asked.
    pub asked_by: String,
    /// The machine whose agent asked for them; none: the person.
    pub asked_by_agent: Option<String>,
    pub answer: Option<String>,
    pub answered_by: Option<String>,
    pub answered_at: Option<u64>,
    pub created_at: u64,
}

/// What a new ask says, checked by the caller.
pub struct NewAsk<'a> {
    pub who: &'a str,
    pub question: &'a str,
    pub why: &'a str,
    pub options: &'a [String],
    pub recommend: Option<&'a str>,
    pub asked_by: &'a str,
    pub asked_by_agent: Option<&'a str>,
}

const ASK_COLS: &str = "id, row_id, who, question, why, options, recommend, asked_by, asked_by_agent, answer, \
                        answered_by, answered_at, created_at";

fn ask_row(r: &Row) -> Result<AskRow> {
    Ok(AskRow {
        id: r.get(0)?,
        row: r.get(1)?,
        who: r.get(2)?,
        question: r.get(3)?,
        why: r.get(4)?,
        options: serde_json::from_str(&r.get::<String>(5)?).unwrap_or_default(),
        recommend: r.get(6)?,
        asked_by: r.get(7)?,
        asked_by_agent: r.get(8)?,
        answer: r.get(9)?,
        answered_by: r.get(10)?,
        answered_at: r.get(11)?,
        created_at: r.get(12)?,
    })
}

fn find(c: &impl Sql, board: &str, id: &str) -> Result<Option<AskRow>> {
    c.query_row(
        &format!("SELECT {ASK_COLS} FROM board_asks WHERE board_id = ?1 AND id = ?2"),
        params![board, id],
        ask_row,
    )
    .optional()
}

/// An ask that `from`, a row being taken into `to`, leaves behind: its
/// answered ones follow it, and its open one too unless `to` has one.
pub(super) fn absorb(c: &impl Sql, from: &str, to: &str) -> Result<()> {
    c.execute(
        "UPDATE board_asks SET row_id = ?2 WHERE row_id = ?1
         AND (answer IS NOT NULL OR NOT EXISTS (SELECT 1 FROM board_asks WHERE row_id = ?2 AND answer IS NULL))",
        params![from, to],
    )?;
    c.execute("DELETE FROM board_asks WHERE row_id = ?1", params![from])?;
    Ok(())
}

/// What deleting an account takes from asks: those that name it, and its name
/// where it only asked or answered.
pub(super) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("DELETE FROM board_asks WHERE who = ?1", params![d.account])?;
    c.execute("UPDATE board_asks SET asked_by = '' WHERE asked_by = ?1", params![d.account])?;
    c.execute("UPDATE board_asks SET answered_by = '' WHERE answered_by = ?1", params![d.account])?;
    Ok(())
}

impl Db {
    /// Open an ask on `row`: its id. Refused when the row isn't there, is a
    /// note (no issue or PR as its item), or has an open ask already.
    pub fn ask_add(
        &self,
        board: &str,
        row: &str,
        a: &NewAsk<'_>,
        at: u64,
    ) -> Result<std::result::Result<String, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(e) = entry(&tx, board, row)? else { return Ok(Err(Refused::Missing("no such row"))) };
        if !matches!(e.pointer, Pointer::Issue { .. } | Pointer::ForgeItem { .. }) {
            return Ok(Err(Refused::Unprocessable(
                "a note row has nothing to act on: ask on the issue's row".to_owned(),
            )));
        }
        let open: Option<String> = tx
            .query_row("SELECT question FROM board_asks WHERE row_id = ?1 AND answer IS NULL", params![row], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(q) = open {
            return Ok(Err(Refused::Conflict(format!("this row already asks {q:?}: it is answered first"))));
        }
        let id = new_ask_id();
        tx.execute(
            "INSERT INTO board_asks (id, board_id, row_id, who, question, why, options, recommend, asked_by,
                 asked_by_agent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                board,
                row,
                a.who,
                a.question,
                a.why,
                serde_json::to_string(a.options)?,
                a.recommend,
                a.asked_by,
                a.asked_by_agent,
                at
            ],
        )?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(id))
    }

    /// One ask of a board, answered or not.
    pub fn board_ask(&self, board: &str, id: &str) -> Result<Option<AskRow>> {
        find(&self.c(), board, id)
    }

    /// Every ask of a board, oldest first.
    pub fn board_asks(&self, board: &str) -> Result<Vec<AskRow>> {
        self.c().query_map(
            &format!("SELECT {ASK_COLS} FROM board_asks WHERE board_id = ?1 ORDER BY created_at, id"),
            params![board],
            ask_row,
        )
    }

    /// Answer an open ask with `answer`, by account `by`. Refused when it
    /// isn't there or was answered already (the first answer stands).
    pub fn ask_answer(
        &self,
        board: &str,
        id: &str,
        answer: &str,
        by: &str,
        at: u64,
    ) -> Result<std::result::Result<(), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, board)? {
            return Ok(Err(Refused::Missing("no such board")));
        }
        let Some(ask) = find(&tx, board, id)? else { return Ok(Err(Refused::Missing("no such ask"))) };
        if ask.answer.is_some() {
            return Ok(Err(Refused::Conflict("this ask was answered already".to_owned())));
        }
        tx.execute(
            "UPDATE board_asks SET answer = ?2, answered_by = ?3, answered_at = ?4 WHERE id = ?1",
            params![id, answer, by, at],
        )?;
        touch(&tx, board, at)?;
        tx.commit()?;
        Ok(Ok(()))
    }
}
