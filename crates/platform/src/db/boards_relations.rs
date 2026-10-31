//! Relations between work items (Arugula #403, `0077_work_relations`): a
//! typed edge from one item of a project to another, each end an issue
//! (`iss_`) or a mirrored item (`fi_`). Who may write one is decided by the
//! routes (`crate::boards::relations`); this is storage.
//!
//! - An edge is stored once: the same (from, to, kind) is a conflict, and so
//!   is the other way round for the kinds that read both ways.
//! - An item never relates to itself, and `blocks` edges never close a
//!   cycle. Both are checked inside the project's lock, so two edges added
//!   together can't make one.
//! - An edge from Plane or GitHub (`source`) is read from there and set by
//!   the mirror ([`Db::plane_relations_set`]); Arugula never deletes it.

use anyhow::Result;
use arugula_control_wire::boards::{RelationKind, RelationSource};

use super::{
    Db,
    boards::Refused,
    chat::{in_json, json_list},
    sql::{Optional as _, Row, Sql, params},
};

/// The most items a cycle check visits before it gives up and refuses.
const WALK_MAX: usize = 5_000;

/// A new edge's id: `rel_` and 32 hex characters.
fn new_id() -> String {
    format!("rel_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// One edge as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelRow {
    pub id: String,
    pub project: String,
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
    pub source: RelationSource,
    pub created_by: String,
    /// The machine whose agent made it; none: a person, or the mirror.
    pub agent: Option<String>,
    pub created_at: u64,
}

const COLS: &str = "id, project_id, from_id, to_id, kind, source, created_by, created_by_agent, created_at";

fn rel_row(r: &Row) -> Result<RelRow> {
    Ok(RelRow {
        id: r.get(0)?,
        project: r.get(1)?,
        from: r.get(2)?,
        to: r.get(3)?,
        kind: RelationKind::parse(&r.get::<String>(4)?).unwrap_or(RelationKind::Relates),
        source: RelationSource::parse(&r.get::<String>(5)?).unwrap_or(RelationSource::Person),
        created_by: r.get(6)?,
        agent: r.get(7)?,
        created_at: r.get(8)?,
    })
}

/// The pointer kind an item id has: `issue` for `iss_`, else `forge_item`.
fn pointer_kind(id: &str) -> &'static str {
    if id.starts_with("iss_") { "issue" } else { "forge_item" }
}

/// What a new edge says, checked by the caller.
pub struct NewRelation<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub kind: RelationKind,
    pub source: RelationSource,
    pub created_by: &'a str,
    pub agent: Option<&'a str>,
}

/// Lock the project, so edges added together are checked one after the other.
fn lock(c: &impl Sql, project: &str) -> Result<bool> {
    let found: Option<String> = c
        .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", c.for_update()), params![project], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// Whether `target` can be reached from `start` along `blocks` edges (from
/// blocker to blocked). `None`: the walk was longer than [`WALK_MAX`].
fn reaches(c: &impl Sql, start: &str, target: &str) -> Result<Option<bool>> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::from([start.to_owned()]);
    let mut frontier = vec![start.to_owned()];
    while let Some(at) = frontier.pop() {
        let next: Vec<String> =
            c.query_map("SELECT to_id FROM work_relations WHERE kind = 'blocks' AND from_id = ?1", params![at], |r| {
                r.get(0)
            })?;
        for n in next {
            if n == target {
                return Ok(Some(true));
            }
            if seen.insert(n.clone()) {
                if seen.len() > WALK_MAX {
                    return Ok(None);
                }
                frontier.push(n);
            }
        }
    }
    Ok(Some(false))
}

impl Db {
    /// Store an edge of `project`. Refused for an edge to itself, one that
    /// is stored already (either way round, for a kind that reads both
    /// ways), and a `blocks` edge that would close a cycle.
    pub fn relation_add(
        &self,
        project: &str,
        n: &NewRelation<'_>,
        at: u64,
    ) -> Result<std::result::Result<RelRow, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock(&tx, project)? {
            return Ok(Err(Refused::Missing("no such project")));
        }
        if n.from == n.to {
            return Ok(Err(Refused::Bad("an item can't relate to itself".to_owned())));
        }
        let both = matches!(n.kind, RelationKind::Relates);
        let there: Option<String> = tx
            .query_row(
                "SELECT id FROM work_relations WHERE kind = ?3
                 AND ((from_id = ?1 AND to_id = ?2) OR (CAST(?4 AS BIGINT) = 1 AND from_id = ?2 AND to_id = ?1))",
                params![n.from, n.to, n.kind.as_str(), i64::from(both)],
                |r| r.get(0),
            )
            .optional()?;
        if there.is_some() {
            return Ok(Err(Refused::Conflict("that relation is there already".to_owned())));
        }
        if n.kind == RelationKind::Blocks {
            match reaches(&tx, n.to, n.from)? {
                Some(false) => {}
                Some(true) => {
                    return Ok(Err(Refused::Conflict(
                        "that would make a cycle: the second item blocks the first, directly or through others"
                            .to_owned(),
                    )));
                }
                None => {
                    return Ok(Err(Refused::Unprocessable(
                        "too many blocking relations to check for a cycle".to_owned(),
                    )));
                }
            }
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO work_relations (id, project_id, from_kind, from_id, to_kind, to_id, kind, source,
                 created_by, created_by_agent, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                project,
                pointer_kind(n.from),
                n.from,
                pointer_kind(n.to),
                n.to,
                n.kind.as_str(),
                n.source.as_str(),
                n.created_by,
                n.agent,
                at
            ],
        )?;
        let row = RelRow {
            id,
            project: project.to_owned(),
            from: n.from.to_owned(),
            to: n.to.to_owned(),
            kind: n.kind,
            source: n.source,
            created_by: n.created_by.to_owned(),
            agent: n.agent.map(str::to_owned),
            created_at: at,
        };
        tx.commit()?;
        Ok(Ok(row))
    }

    /// One edge of a project.
    pub fn relation(&self, project: &str, id: &str) -> Result<Option<RelRow>> {
        self.c()
            .query_row(
                &format!("SELECT {COLS} FROM work_relations WHERE project_id = ?1 AND id = ?2"),
                params![project, id],
                rel_row,
            )
            .optional()
    }

    /// Delete an edge made here. Refused when it isn't there, or when it
    /// comes from Plane or GitHub, which only they change. The edge deleted.
    pub fn relation_delete(&self, project: &str, id: &str) -> Result<std::result::Result<RelRow, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let found: Option<RelRow> = tx
            .query_row(
                &format!("SELECT {COLS} FROM work_relations WHERE project_id = ?1 AND id = ?2{}", tx.for_update()),
                params![project, id],
                rel_row,
            )
            .optional()?;
        let Some(r) = found else { return Ok(Err(Refused::Missing("no such relation"))) };
        match r.source {
            RelationSource::Plane => return Ok(Err(Refused::Forbidden("it comes from Plane".to_owned()))),
            RelationSource::Github => return Ok(Err(Refused::Forbidden("it comes from GitHub".to_owned()))),
            RelationSource::Person | RelationSource::Agent => {}
        }
        tx.execute("DELETE FROM work_relations WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(Ok(r))
    }

    /// Every edge of `project` that touches one of `ids`, either end.
    pub fn relations_among(&self, project: &str, ids: &[&str]) -> Result<Vec<RelRow>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT {COLS} FROM work_relations WHERE project_id = ?2 AND ({} OR {}) ORDER BY created_at, id",
                in_json(&c, "from_id", 1),
                in_json(&c, "to_id", 1)
            ),
            params![json_list(ids)?, project],
            rel_row,
        )
    }

    /// Make the `blocks` edges `source` says of one mirrored item (`item`,
    /// the tracker's id for it, on `connection`) the ones stored: those
    /// from `blocked_by` (the tracker's ids of items that block it) and to
    /// `blocking` (those it blocks). Edges whose other end isn't mirrored on
    /// the connection are left out. The `fi_` ids of the items whose edges
    /// changed (none when nothing did).
    pub fn plane_relations_set(
        &self,
        connection: &str,
        item: &str,
        blocked_by: &[String],
        blocking: &[String],
        at: u64,
    ) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let fi_of = |src: &str| -> Result<Option<String>> {
            tx.query_row(
                "SELECT id FROM forge_items WHERE connection_id = ?1 AND source_item_id = ?2",
                params![connection, src],
                |r| r.get(0),
            )
            .optional()
        };
        let Some(me) = fi_of(item)? else { return Ok(vec![]) };
        let project: Option<String> = tx
            .query_row("SELECT project_id FROM tracker_connections WHERE id = ?1", params![connection], |r| r.get(0))
            .optional()?;
        let Some(project) = project else { return Ok(vec![]) };
        let (mut from, mut to) = (vec![], vec![]);
        for s in blocked_by {
            from.extend(fi_of(s)?);
        }
        for s in blocking {
            to.extend(fi_of(s)?);
        }
        let mut want: Vec<(String, String)> = vec![];
        want.extend(from.into_iter().map(|f| (f, me.clone())));
        want.extend(to.into_iter().map(|t| (me.clone(), t)));
        want.retain(|(f, t)| f != t);
        want.sort();
        want.dedup();
        let have: Vec<(String, String, String)> = tx.query_map(
            "SELECT id, from_id, to_id FROM work_relations
             WHERE source = 'plane' AND kind = 'blocks' AND (from_id = ?1 OR to_id = ?1)",
            params![me],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut changed: Vec<String> = vec![];
        for (id, f, t) in &have {
            if !want.iter().any(|(wf, wt)| wf == f && wt == t) {
                tx.execute("DELETE FROM work_relations WHERE id = ?1", params![id])?;
                changed.extend([f.clone(), t.clone()]);
            }
        }
        for (f, t) in &want {
            if have.iter().any(|(_, hf, ht)| hf == f && ht == t) {
                continue;
            }
            // Plane's edge stands beside one made here: not stored twice.
            let n = tx.execute(
                "INSERT INTO work_relations (id, project_id, from_kind, from_id, to_kind, to_id, kind, source,
                     created_by, created_at)
                 SELECT CAST(?1 AS TEXT), CAST(?2 AS TEXT), 'forge_item', CAST(?3 AS TEXT), 'forge_item', CAST(?4 AS TEXT),
                        'blocks', 'plane', '', CAST(?5 AS BIGINT)
                 WHERE NOT EXISTS (SELECT 1 FROM work_relations WHERE from_id = ?3 AND to_id = ?4 AND kind = 'blocks')",
                params![new_id(), project, f, t, at],
            )?;
            if n > 0 {
                changed.extend([f.clone(), t.clone()]);
            }
        }
        tx.commit()?;
        changed.sort();
        changed.dedup();
        Ok(changed)
    }

    /// The boards (id, project) with a row that points at one of these
    /// items: those a change to an edge between them shows on.
    pub fn boards_on_pointers(&self, project: &str, ids: &[&str]) -> Result<Vec<(String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT DISTINCT b.id, b.project_id FROM board_entries e JOIN boards b ON b.id = e.board_id
                 WHERE e.kind = 'row' AND b.project_id = ?2 AND {}",
                in_json(&c, "e.target", 1)
            ),
            params![json_list(ids)?, project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }
}

/// What deleting an account takes from relations, inside `boards`'
/// `erase_account`: the edges of the projects that went with it, and its
/// name where it only made one.
pub(super) fn erase_account(c: &impl Sql, d: &crate::Deleted) -> Result<()> {
    c.execute("DELETE FROM work_relations WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE work_relations SET created_by = '' WHERE created_by = ?1", params![d.account])?;
    Ok(())
}
