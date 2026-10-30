//! The one-time move of boards to lanes (aru-125, `0072_board_lanes`): run
//! on start, per board that still has zones, and a no-op once none has.
//!
//! A board made before lanes is an ordered list of zones of rows. A board
//! now is rows with attachments, and its lanes follow the work, so each
//! board is rewritten in place:
//! - a PR row that references an issue row of the board becomes a `work`
//!   attachment of it;
//! - a block row, and a run row outside PROOFS, becomes a `work` attachment
//!   of the issue row it references (a block by its reported branch or its
//!   note, a run by its note); one that references none stays, a note row;
//! - a row in the PROOFS zone with a link or run pointer becomes a `proof`
//!   attachment of the issue row its note names (`#N`); one that names
//!   none stays, a note row;
//! - Cloud workspace rows stay as they are: they aren't an issue or a PR,
//!   and a workspace isn't something that is attached;
//! - then the zones go and the rows keep their order, zone by zone.
//!
//! "References" is [`crate::boards::refs`]. Each board is one transaction
//! holding the board's row, and reads what is left of its zones inside it,
//! so two machines starting together don't move a board twice.

use anyhow::Result;
use arugula_control_wire::boards::{AttachRole, Pointer};

use super::{
    Db,
    boards::{Entry, pointer_cols, pointer_of},
    sql::{Optional as _, Sql, params},
};
use crate::boards::refs::{IssueKey, Source, references, references_in};

/// What a start moved.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Moved {
    pub boards: usize,
    pub attached: usize,
    pub proofs: usize,
}

/// A row before the move, with the zone it was in.
struct Old {
    row: Entry,
    proofs: bool,
}

/// What a PR row says of itself: the repo, and what its title and head
/// branch reference.
struct Pr {
    repo: u64,
    refs: crate::boards::refs::Refs,
}

impl Db {
    /// Move every board that has zones to lanes (see the module), once. The
    /// platform's start runs it after the migrations.
    pub fn lanes_move(&self) -> Result<Moved> {
        let boards: Vec<String> = self.c().query_map(
            "SELECT DISTINCT board_id FROM board_entries WHERE kind = 'zone' OR zone_id IS NOT NULL",
            params![],
            |r| r.get(0),
        )?;
        let mut moved = Moved::default();
        for b in boards {
            let (attached, proofs) = self.lanes_move_board(&b)?;
            moved.boards += 1;
            moved.attached += attached;
            moved.proofs += proofs;
        }
        Ok(moved)
    }

    fn lanes_move_board(&self, board: &str) -> Result<(usize, usize)> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM boards WHERE id = ?1{}", tx.for_update()), params![board], |r| r.get(0))
            .optional()?;
        // The board is gone, or another machine moved it already.
        let left: u64 = tx.query_row(
            "SELECT count(*) FROM board_entries WHERE board_id = ?1 AND (kind = 'zone' OR zone_id IS NOT NULL)",
            params![board],
            |r| r.get(0),
        )?;
        if held.is_none() || left == 0 {
            tx.execute("DELETE FROM board_entries WHERE board_id = ?1 AND kind = 'zone'", params![board])?;
            tx.commit()?;
            return Ok((0, 0));
        }
        let zones: Vec<(String, bool)> = tx.query_map(
            "SELECT id, zone_kind = 'proofs' FROM board_entries WHERE board_id = ?1 AND kind = 'zone'
             ORDER BY position, id",
            params![board],
            |r| Ok((r.get(0)?, r.get::<Option<bool>>(1)?.unwrap_or(false))),
        )?;
        let mut rows: Vec<Old> = vec![];
        for (zone, proofs) in &zones {
            let found = tx.query_map(
                "SELECT id, rev, note, status, target_kind, target, pane, created_at, updated_at, agent
                 FROM board_entries WHERE board_id = ?1 AND kind = 'row' AND zone_id = ?2 ORDER BY position, id",
                params![board, zone],
                |r| {
                    Ok(Entry {
                        id: r.get(0)?,
                        rev: r.get(1)?,
                        note: r.get(2)?,
                        status: super::boards::status_of(r.get(3)?)?,
                        pointer: pointer_of(r.get(4)?, r.get(5)?, r.get(6)?)?,
                        created_at: r.get(7)?,
                        updated_at: r.get(8)?,
                        agent: r.get(9)?,
                    })
                },
            )?;
            rows.extend(found.into_iter().map(|row| Old { row, proofs: *proofs }));
        }
        // Rows whose zone is gone keep their place, last.
        let orphans = tx.query_map(
            "SELECT id, rev, note, status, target_kind, target, pane, created_at, updated_at, agent
             FROM board_entries WHERE board_id = ?1 AND kind = 'row'
               AND (zone_id IS NULL OR zone_id NOT IN (SELECT id FROM board_entries WHERE kind = 'zone'))
             ORDER BY position, id",
            params![board],
            |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    rev: r.get(1)?,
                    note: r.get(2)?,
                    status: super::boards::status_of(r.get(3)?)?,
                    pointer: pointer_of(r.get(4)?, r.get(5)?, r.get(6)?)?,
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                    agent: r.get(9)?,
                })
            },
        )?;
        rows.extend(orphans.into_iter().map(|row| Old { row, proofs: false }));

        // The board's issue rows, as a reference names them.
        let mut issues: Vec<(String, IssueKey)> = vec![];
        for o in &rows {
            if let Some(key) = issue_key(&tx, &o.row.pointer)? {
                issues.push((o.row.id.clone(), key));
            }
        }
        let mut attach: Vec<(String, AttachRole, &Old)> = vec![];
        for o in &rows {
            let into = match &o.row.pointer {
                Pointer::ForgeItem { id } => match pr_of(&tx, id)? {
                    Some(pr) => {
                        issues.iter().find(|(_, k)| pr.refs.name(Some(pr.repo), k)).map(|(r, _)| (r, AttachRole::Work))
                    }
                    None => None,
                },
                Pointer::Block { machine, pane } => {
                    let branch: Option<String> = tx
                        .query_row(
                            "SELECT branch FROM board_block_states WHERE machine = ?1 AND pane = ?2",
                            params![machine, pane],
                            |r| r.get(0),
                        )
                        .optional()?
                        .flatten();
                    let refs = references_in(&format!("{} {}", o.row.note, branch.unwrap_or_default()));
                    issues.iter().find(|(_, k)| refs.name_any(k)).map(|(r, _)| (r, AttachRole::Work))
                }
                Pointer::Run { .. } | Pointer::Link { .. } if o.proofs || matches!(o.row.pointer, Pointer::Run { .. }) => {
                    let role = if o.proofs { AttachRole::Proof } else { AttachRole::Work };
                    let refs = references_in(&o.row.note);
                    issues.iter().find(|(_, k)| refs.name_any(k)).map(|(r, _)| (r, role))
                }
                _ => None,
            };
            if let Some((row, role)) = into {
                // A row is never attached to itself.
                if *row != o.row.id {
                    attach.push((row.clone(), role, o));
                }
            }
        }
        let gone: std::collections::HashSet<&str> = attach.iter().map(|(_, _, o)| o.row.id.as_str()).collect();
        // An issue row that is itself attached stays a row: only a row that
        // isn't an attachment is a target.
        let (mut attached, mut proofs) = (0, 0);
        for (row, role, o) in &attach {
            if gone.contains(row.as_str()) {
                continue;
            }
            let (Some(kind), Some(target), pane) = pointer_cols(&o.row.pointer) else { continue };
            tx.execute(
                "INSERT INTO board_attachments (id, board_id, row_id, role, target_kind, target, pane, agent, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    format!("att_{}", hex::encode(arugula_e2e::random::<16>())),
                    board,
                    row,
                    role.as_str(),
                    kind,
                    target,
                    pane,
                    o.row.agent,
                    o.row.created_at
                ],
            )?;
            tx.execute("DELETE FROM board_entries WHERE id = ?1", params![o.row.id])?;
            if *role == AttachRole::Proof {
                proofs += 1;
            } else {
                attached += 1;
            }
        }
        tx.execute("DELETE FROM board_entries WHERE board_id = ?1 AND kind = 'zone'", params![board])?;
        tx.execute("UPDATE board_entries SET zone_id = NULL WHERE board_id = ?1", params![board])?;
        let mut place = 0u64;
        for o in &rows {
            if tx.execute("UPDATE board_entries SET position = ?2 WHERE id = ?1", params![o.row.id, place])? > 0 {
                place += 1;
            }
        }
        tx.commit()?;
        Ok((attached, proofs))
    }
}

/// The issue a row's pointer is, as a reference names it: a native issue by
/// its number, a GitHub issue by repo and number, a Plane item by its key.
fn issue_key(c: &impl Sql, p: &Pointer) -> Result<Option<IssueKey>> {
    Ok(match p {
        Pointer::Issue { id } => c
            .query_row("SELECT number FROM issues WHERE id = ?1", params![id], |r| r.get::<u64>(0))
            .optional()?
            .map(|number| IssueKey { source: Source::Native, number }),
        Pointer::ForgeItem { id } => c
            .query_row(
                "SELECT i.kind, i.number, i.repo_id, i.provider, k.identifier FROM forge_items i
                 LEFT JOIN tracker_connections k ON k.id = i.connection_id WHERE i.id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get::<String>(0)?,
                        r.get::<u64>(1)?,
                        r.get::<i64>(2)?,
                        r.get::<String>(3)?,
                        r.get::<Option<String>>(4)?,
                    ))
                },
            )
            .optional()?
            .and_then(|(kind, number, repo, provider, ident)| {
                if kind != "issue" {
                    return None;
                }
                let source = match (provider.as_str(), ident) {
                    ("plane", Some(key)) => Source::Plane(key),
                    _ => Source::Repo(repo.max(0) as u64),
                };
                Some(IssueKey { source, number })
            }),
        _ => None,
    })
}

/// A mirrored PR's repo and references, or `None` for anything else.
fn pr_of(c: &impl Sql, id: &str) -> Result<Option<Pr>> {
    Ok(c.query_row(
        "SELECT repo_id, number, title, head_ref FROM forge_items WHERE id = ?1 AND kind = 'pr'",
        params![id],
        |r| Ok((r.get::<i64>(0)?, r.get::<u64>(1)?, r.get::<String>(2)?, r.get::<Option<String>>(3)?)),
    )
    .optional()?
    .map(|(repo, number, title, branch)| Pr {
        repo: repo.max(0) as u64,
        refs: references(&title, branch.as_deref().unwrap_or(""), "", Some(number)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board as the phase 3 board was: Next, Doing, Review and PROOFS
    /// zones with issue rows, PR rows, an agent block row and rows of
    /// evidence.
    fn old_board(db: &Db) {
        let c = db.c();
        c.execute(
            "INSERT INTO boards (id, project_id, title, created_by, created_at, updated_at)
             VALUES ('brd_1', 'prj_1', 'Phase 3', 'a', 1, 1)",
            params![],
        )
        .unwrap();
        for (id, number, kind, title, head) in [
            ("fi_i5", 5u64, "issue", "Parser", None),
            ("fi_i6", 6, "issue", "Lexer", None),
            ("fi_p1", 10, "pr", "Fix parser (#5)", Some("feat/5-parser")),
            ("fi_p2", 11, "pr", "Unrelated", Some("chore/tidy")),
            ("fi_p3", 12, "pr", "Something", Some("fix/6-thing")),
        ] {
            c.execute(
                "INSERT INTO forge_items (id, repo_id, number, kind, state, title, head_ref, created_at, updated_at, synced_at)
                 VALUES (?1, 88, ?2, ?3, 'open', ?4, ?5, 1, 1, 1)",
                params![id, number, kind, title, head],
            )
            .unwrap();
        }
        for (n, (id, title, kind)) in
            [("zon_n", "Next", None), ("zon_d", "Doing", None), ("zon_r", "Review", None), ("zon_p", "PROOFS", Some("proofs"))]
                .into_iter()
                .enumerate()
        {
            c.execute(
                "INSERT INTO board_entries (id, board_id, kind, position, title, zone_kind, created_at, updated_at)
                 VALUES (?1, 'brd_1', 'zone', ?2, ?3, ?4, 1, 1)",
                params![id, n, title, kind],
            )
            .unwrap();
        }
        let rows: [(&str, &str, &str, &str, Option<&str>, Option<&str>, Option<&str>); 10] = [
            ("zon_n", "row_i5", "", "forge_item", Some("fi_i5"), None, Some("working")),
            ("zon_n", "row_i6", "", "forge_item", Some("fi_i6"), None, None),
            ("zon_n", "row_note", "decide naming", "", None, None, None),
            ("zon_d", "row_blk", "agent on #6", "block", Some("m1"), Some("7"), Some("working")),
            ("zon_r", "row_p1", "", "forge_item", Some("fi_p1"), None, None),
            ("zon_r", "row_p2", "", "forge_item", Some("fi_p2"), None, None),
            ("zon_r", "row_p3", "", "forge_item", Some("fi_p3"), None, None),
            ("zon_p", "row_pl", "CI green for #5", "link", Some("https://ci.example.com/1"), None, Some("done")),
            ("zon_p", "row_pr", "nightly", "run", Some("run_9"), None, None),
            ("zon_p", "row_pn", "no number", "link", Some("https://ci.example.com/2"), None, None),
        ];
        let mut place: std::collections::HashMap<&str, u64> = Default::default();
        for (zone, id, note, kind, target, pane, status) in rows {
            let n = place.entry(zone).or_default();
            *n += 1;
            let n = *n;
            c.execute(
                "INSERT INTO board_entries (id, board_id, kind, zone_id, position, note, status, target_kind, target, pane,
                     created_at, updated_at)
                 VALUES (?1, 'brd_1', 'row', ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1)",
                params![id, zone, n, note, status, Some(kind).filter(|k| !k.is_empty()), target, pane],
            )
            .unwrap();
        }
    }

    /// The move on the phase 3 board's shape: zones go, a PR that names an
    /// issue row, the block and the evidence become that row's attachments,
    /// the rest stay as rows; and a second run moves nothing.
    #[test]
    fn a_board_of_zones_moves_to_rows_with_attachments() {
        let db = Db::memory();
        old_board(&db);
        let moved = db.lanes_move().unwrap();
        assert_eq!(moved, Moved { boards: 1, attached: 3, proofs: 1 });

        let c = db.c();
        let zones: u64 = c.query_row("SELECT count(*) FROM board_entries WHERE kind = 'zone'", params![], |r| r.get(0)).unwrap();
        let zoned: u64 = c.query_row("SELECT count(*) FROM board_entries WHERE zone_id IS NOT NULL", params![], |r| r.get(0)).unwrap();
        assert_eq!((zones, zoned), (0, 0));
        drop(c);
        let stored = db.board_stored("brd_1").unwrap().unwrap();
        let ids: Vec<&str> = stored.rows.iter().map(|r| r.id.as_str()).collect();
        // Zone by zone, row by row; the attached ones are gone.
        assert_eq!(ids, ["row_i5", "row_i6", "row_note", "row_p2", "row_pr", "row_pn"]);
        // Made in one moment, so by role and what they point at.
        let on = |row: &str| -> Vec<(AttachRole, Pointer)> {
            let mut on: Vec<_> =
                stored.attachments.iter().filter(|a| a.row == row).map(|a| (a.role, a.pointer.clone())).collect();
            on.sort_by_key(|(r, p)| (r.as_str(), p.label()));
            on
        };
        assert_eq!(
            on("row_i5"),
            [
                (AttachRole::Proof, Pointer::Link { url: "https://ci.example.com/1".into() }),
                (AttachRole::Work, Pointer::ForgeItem { id: "fi_p1".into() })
            ]
        );
        assert_eq!(
            on("row_i6"),
            [
                (AttachRole::Work, Pointer::ForgeItem { id: "fi_p3".into() }),
                (AttachRole::Work, Pointer::Block { machine: "m1".into(), pane: "7".into() })
            ]
        );
        assert!(on("row_p2").is_empty() && on("row_pr").is_empty() && on("row_pn").is_empty());
        // A PR that names no issue row, and evidence that names none, stay.
        assert_eq!(stored.rows[3].pointer, Pointer::ForgeItem { id: "fi_p2".into() });
        assert_eq!(stored.rows[5].pointer, Pointer::Link { url: "https://ci.example.com/2".into() });
        // The order is kept, and what was in a row is too.
        assert_eq!(stored.rows[0].status, arugula_control_wire::boards::Status::Working);
        assert_eq!(stored.rows[2].note, "decide naming");

        assert_eq!(db.lanes_move().unwrap(), Moved::default(), "idempotent");
        assert_eq!(db.board_stored("brd_1").unwrap().unwrap(), stored);
    }
}
