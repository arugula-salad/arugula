//! Work items (Arugula #405, `0080_work_items`): the one identity behind a
//! native issue, the GitHub issue it was exported to and a Plane item. A
//! work item is made on demand, when a board takes one of its issues and by
//! the one-time move ([`super::boards_work`]); PRs, runs, blocks and links
//! never have one.
//!
//! - **Links.** A native issue with `github_repo_id` and `github_number`
//!   (0009) is the same work as the mirrored GitHub issue of that repo and
//!   number, if the repo is linked to the same project. A Plane item is
//!   linked to nothing, so it is a work item of its own.
//! - **Two become one.** Making a work item for an issue also joins its
//!   counterpart to it; when the two already have work items they are
//!   merged into the older one: its refs and its rows move over, and a
//!   board that has a row for each folds them into one ([`fold_rows`]).
//! - A ref's `target` is in one work item only (`UNIQUE`), so two
//!   transactions making a work item for one issue meet at the insert, and
//!   the later one takes the earlier one's.

use std::collections::HashMap;

use anyhow::Result;

use super::{
    Db,
    boards::{entry, lock, renumber, siblings, touch},
    boards_asks,
    chat::{in_json, json_list},
    sql::{Optional as _, Sql, params},
};
use crate::boards::refs::{IssueKey, Source};

/// A new work item's id: `wi_` and 32 hex characters.
fn new_id() -> String {
    format!("wi_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// What an issue says of itself: where it lives, and the issue that is the
/// same work, if it is mirrored or exported.
struct Subject {
    project: String,
    /// `native`, `github` or `plane`.
    system: &'static str,
    instance: String,
    key: String,
    url: Option<String>,
    /// The `fi_` or `iss_` this is linked to.
    other: Option<String>,
}

/// The issue `target` (an `iss_` or an `fi_` of an issue) as a ref is
/// written. `None` for a PR, an unknown id, and a mirrored issue whose repo
/// or connection no project has.
fn subject(c: &impl Sql, target: &str) -> Result<Option<Subject>> {
    if target.starts_with("iss_") {
        let Some((project, number, repo, gh)) = c
            .query_row(
                "SELECT project_id, number, github_repo_id, github_number FROM issues WHERE id = ?1",
                params![target],
                |r| Ok((r.get::<String>(0)?, r.get::<u64>(1)?, r.get::<Option<i64>>(2)?, r.get::<Option<u64>>(3)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let other = match (repo, gh) {
            (Some(repo), Some(gh)) => c
                .query_row(
                    "SELECT f.id FROM forge_items f JOIN linked_repos l ON l.repo_id = f.repo_id
                     WHERE f.repo_id = ?1 AND f.number = ?2 AND f.kind = 'issue' AND l.project_id = ?3",
                    params![repo, gh, project],
                    |r| r.get::<String>(0),
                )
                .optional()?,
            _ => None,
        };
        return Ok(Some(Subject {
            instance: project.clone(),
            project,
            system: "native",
            key: number.to_string(),
            url: None,
            other,
        }));
    }
    let Some((repo, number, provider, connection)) = c
        .query_row(
            "SELECT repo_id, number, provider, connection_id FROM forge_items WHERE id = ?1 AND kind = 'issue'",
            params![target],
            |r| Ok((r.get::<i64>(0)?, r.get::<u64>(1)?, r.get::<String>(2)?, r.get::<Option<String>>(3)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    if provider == "plane" {
        let Some(connection) = connection else { return Ok(None) };
        return Ok(c
            .query_row(
                "SELECT project_id, identifier, workspace_slug FROM tracker_connections WHERE id = ?1",
                params![connection],
                |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?, r.get::<String>(2)?)),
            )
            .optional()?
            .map(|(project, identifier, slug)| Subject {
                project,
                system: "plane",
                instance: connection,
                key: format!("{identifier}-{number}"),
                url: Some(crate::trackers::item_url(&slug, &identifier, number)),
                other: None,
            }));
    }
    let Some((project, full_name)) = c
        .query_row("SELECT project_id, full_name FROM linked_repos WHERE repo_id = ?1", params![repo], |r| {
            Ok((r.get::<String>(0)?, r.get::<String>(1)?))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let other = c
        .query_row(
            "SELECT id FROM issues WHERE project_id = ?1 AND github_repo_id = ?2 AND github_number = ?3",
            params![project, repo, number],
            |r| r.get::<String>(0),
        )
        .optional()?;
    Ok(Some(Subject {
        project,
        system: "github",
        instance: repo.to_string(),
        key: format!("{full_name}#{number}"),
        url: Some(format!("https://github.com/{full_name}/issues/{number}")),
        other,
    }))
}

/// The work item `target` is a ref of.
fn ref_item(c: &impl Sql, target: &str) -> Result<Option<String>> {
    c.query_row("SELECT work_item FROM work_item_refs WHERE target = ?1", params![target], |r| r.get(0)).optional()
}

/// Make `target` a ref of `work_item`; `false` when it was a ref already.
fn add_ref(c: &impl Sql, work_item: &str, target: &str, s: &Subject) -> Result<bool> {
    Ok(c.execute(
        "INSERT INTO work_item_refs (work_item, system, target, instance, key, url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (target) DO NOTHING",
        params![work_item, s.system, target, s.instance, s.key, s.url],
    )? > 0)
}

/// The work item of the issue `target` (an `iss_` or an `fi_`), made if it
/// has none, in the caller's transaction. Its counterpart joins it; if the
/// counterpart has a work item of its own, the two are merged into the older
/// (see the module). `None`: not an issue the platform can place.
pub(super) fn work_item_for(c: &impl Sql, target: &str, at: u64) -> Result<Option<String>> {
    let Some(s) = subject(c, target)? else { return Ok(None) };
    let other = match &s.other {
        Some(o) => subject(c, o)?.map(|x| (o.as_str(), x)),
        None => None,
    };
    let mine = ref_item(c, target)?;
    let theirs = match &other {
        Some((o, _)) => ref_item(c, o)?,
        None => None,
    };
    let keep = match (mine, theirs) {
        (Some(a), Some(b)) if a != b => merge(c, &a, &b, at)?,
        (Some(a), _) | (None, Some(a)) => a,
        (None, None) => {
            let id = new_id();
            c.execute(
                "INSERT INTO work_items (id, project_id, created_at) VALUES (?1, ?2, ?3)",
                params![id, s.project, at],
            )?;
            id
        }
    };
    if !add_ref(c, &keep, target, &s)?
        && let Some(won) = ref_item(c, target)?
        && won != keep
    {
        // Another transaction gave the issue its work item first.
        c.execute(
            "DELETE FROM work_items WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM work_item_refs WHERE work_item = ?1)",
            params![keep],
        )?;
        return Ok(Some(won));
    }
    if let Some((o, x)) = &other {
        add_ref(c, &keep, o, x)?;
    }
    Ok(Some(keep))
}

/// Merge work items `a` and `b` into the older, and say which that is. A
/// board with a row for each folds them into one row first (the unique
/// index would refuse the move otherwise); then every row and ref of the
/// younger is the older's, and the younger goes.
fn merge(c: &impl Sql, a: &str, b: &str, at: u64) -> Result<String> {
    let older: Vec<String> =
        c.query_map("SELECT id FROM work_items WHERE id IN (?1, ?2) ORDER BY created_at, id", params![a, b], |r| {
            r.get(0)
        })?;
    let keep = older.into_iter().next().unwrap_or_else(|| a.to_owned());
    let gone = if keep == a { b } else { a };
    let boards: Vec<String> = c.query_map(
        "SELECT DISTINCT x.board_id FROM board_entries x
         JOIN board_entries y ON y.board_id = x.board_id AND y.kind = 'row' AND y.work_item = ?2
         WHERE x.kind = 'row' AND x.work_item = ?1 ORDER BY x.board_id",
        params![keep, gone],
        |r| r.get(0),
    )?;
    for board in boards {
        if !lock(c, &board)? {
            continue;
        }
        let rows: Vec<String> = c.query_map(
            "SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'row' AND work_item IN (?2, ?3)
             ORDER BY position, id",
            params![board, keep, gone],
            |r| r.get(0),
        )?;
        if let Some((first, later)) = rows.split_first() {
            for row in later {
                fold_rows(c, &board, first, row, at)?;
            }
        }
    }
    c.execute("UPDATE board_entries SET work_item = ?1 WHERE work_item = ?2", params![keep, gone])?;
    c.execute("UPDATE work_item_refs SET work_item = ?1 WHERE work_item = ?2", params![keep, gone])?;
    c.execute("DELETE FROM work_items WHERE id = ?1", params![gone])?;
    Ok(keep)
}

/// Fold row `from` of `board` into row `into`, two rows of one work item:
/// its attachments (those `into` has already go) and asks move over, its
/// note is added to the end of `into`'s, and the row is dropped.
pub(super) fn fold_rows(c: &impl Sql, board: &str, into: &str, from: &str, at: u64) -> Result<()> {
    let (Some(keep), Some(drop)) = (entry(c, board, into)?, entry(c, board, from)?) else { return Ok(()) };
    c.execute(
        "DELETE FROM board_attachments WHERE row_id = ?2 AND EXISTS
            (SELECT 1 FROM board_attachments k WHERE k.row_id = ?1 AND k.role = board_attachments.role
               AND k.target_kind = board_attachments.target_kind AND k.target = board_attachments.target
               AND (k.pane = board_attachments.pane OR (k.pane IS NULL AND board_attachments.pane IS NULL)))",
        params![into, from],
    )?;
    c.execute("UPDATE board_attachments SET row_id = ?1 WHERE row_id = ?2", params![into, from])?;
    boards_asks::absorb(c, from, into)?;
    if !drop.note.is_empty() {
        let note = if keep.note.is_empty() { drop.note } else { format!("{}\n{}", keep.note, drop.note) };
        c.execute(
            "UPDATE board_entries SET rev = rev + 1, note = ?2, updated_at = ?3 WHERE id = ?1",
            params![into, note, at],
        )?;
    }
    c.execute("DELETE FROM board_entries WHERE id = ?1", params![from])?;
    renumber(c, &siblings(c, board)?)?;
    touch(c, board, at)
}

/// How a reference names each of `ids` (`iss_` and `fi_` ids of issues; any
/// other is not in the answer).
fn issue_keys(c: &impl Sql, ids: &[&str]) -> Result<HashMap<String, IssueKey>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let list = json_list(ids)?;
    for (id, number) in
        c.query_map(&format!("SELECT id, number FROM issues WHERE {}", in_json(c, "id", 1)), params![list], |r| {
            Ok((r.get::<String>(0)?, r.get::<u64>(1)?))
        })?
    {
        out.insert(id, IssueKey { source: Source::Native, number });
    }
    for (id, number, repo, ident) in c.query_map(
        &format!(
            "SELECT i.id, i.number, i.repo_id, k.identifier FROM forge_items i
             LEFT JOIN tracker_connections k ON k.id = i.connection_id AND i.provider = 'plane'
             WHERE i.kind = 'issue' AND {}",
            in_json(c, "i.id", 1)
        ),
        params![list],
        |r| Ok((r.get::<String>(0)?, r.get::<u64>(1)?, r.get::<i64>(2)?, r.get::<Option<String>>(3)?)),
    )? {
        let source = match ident {
            Some(key) => Source::Plane(key),
            None => Source::Repo(repo.max(0) as u64),
        };
        out.insert(id, IssueKey { source, number });
    }
    Ok(out)
}

/// A board's issue row as a PR's references see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKeys {
    pub row: String,
    pub work_item: Option<String>,
    /// Every way a reference names the row's work: its own issue, and the
    /// issues of its work item.
    pub keys: Vec<IssueKey>,
}

/// The [`RowKeys`] of rows given as (row, id it points at, work item): one
/// read of the work items' refs and one of the issues. A row whose id is
/// not an issue has no keys.
fn row_keys(c: &impl Sql, rows: &[(String, Option<String>, Option<String>)]) -> Result<Vec<RowKeys>> {
    let items: Vec<&str> = rows.iter().filter_map(|(_, _, wi)| wi.as_deref()).collect();
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    if !items.is_empty() {
        for (wi, target) in c.query_map(
            &format!(
                "SELECT work_item, target FROM work_item_refs WHERE {} ORDER BY target",
                in_json(c, "work_item", 1)
            ),
            params![json_list(&items)?],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )? {
            refs.entry(wi).or_default().push(target);
        }
    }
    let mut ids: Vec<&str> = rows.iter().filter_map(|(_, id, _)| id.as_deref()).collect();
    ids.extend(refs.values().flatten().map(String::as_str));
    ids.sort_unstable();
    ids.dedup();
    let found = issue_keys(c, &ids)?;
    Ok(rows
        .iter()
        .map(|(row, id, wi)| {
            let mut keys: Vec<IssueKey> = vec![];
            let own = id.iter().map(String::as_str);
            let linked = wi.iter().filter_map(|w| refs.get(w)).flatten().map(String::as_str);
            for key in own.chain(linked).filter_map(|t| found.get(t)) {
                if !keys.contains(key) {
                    keys.push(key.clone());
                }
            }
            RowKeys { row: row.clone(), work_item: wi.clone(), keys }
        })
        .collect())
}

/// [`row_keys`] for the rows `ids`, in the caller's transaction.
pub(super) fn keys_of_rows(c: &impl Sql, ids: &[&str]) -> Result<Vec<RowKeys>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let rows = c.query_map(
        &format!(
            "SELECT id, CASE WHEN target_kind IN ('issue', 'forge_item') THEN target END, work_item
             FROM board_entries WHERE kind = 'row' AND {}",
            in_json(c, "id", 1)
        ),
        params![json_list(ids)?],
        |r| Ok((r.get::<String>(0)?, r.get::<Option<String>>(1)?, r.get::<Option<String>>(2)?)),
    )?;
    row_keys(c, &rows)
}

impl Db {
    /// The work item of the issue `target`, made if it has none (see the
    /// module). `None`: not an issue the platform can place.
    pub fn work_item_for(&self, target: &str) -> Result<Option<String>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let found = work_item_for(&tx, target, arugula_e2e::now_ms())?;
        tx.commit()?;
        Ok(found)
    }

    /// The row of `board` that is `work_item`, if there is one.
    pub fn row_of_work_item(&self, board: &str, work_item: &str) -> Result<Option<String>> {
        self.c()
            .query_row(
                "SELECT id FROM board_entries WHERE board_id = ?1 AND kind = 'row' AND work_item = ?2",
                params![board, work_item],
                |r| r.get(0),
            )
            .optional()
    }

    /// The keys of every issue row of `board`, in the board's order.
    pub fn board_row_keys(&self, board: &str) -> Result<Vec<RowKeys>> {
        let c = self.c();
        let rows = c.query_map(
            "SELECT id, target, work_item FROM board_entries
             WHERE board_id = ?1 AND kind = 'row' AND target_kind IN ('issue', 'forge_item')
             ORDER BY position, id",
            params![board],
            |r| Ok((r.get::<String>(0)?, r.get::<Option<String>>(1)?, r.get::<Option<String>>(2)?)),
        )?;
        row_keys(&c, &rows)
    }

    /// The keys of the issue `target` and of the work item it is in.
    pub fn issue_keys_of(&self, target: &str, work_item: &str) -> Result<Vec<IssueKey>> {
        let c = self.c();
        let one = [(String::new(), Some(target.to_owned()), Some(work_item.to_owned()))];
        Ok(row_keys(&c, &one)?.pop().map(|r| r.keys).unwrap_or_default())
    }

    /// The work of each of `targets`, as one id that two issues share when
    /// they are the same work: the work item if there is one, else the
    /// native issue of an exported pair, else the issue itself. Read only:
    /// no work item is made. This is how the picker lists a piece of work
    /// once. A target that is no issue is not in the answer.
    pub fn work_identities(&self, targets: &[&str]) -> Result<HashMap<String, String>> {
        let c = self.c();
        let mut out = HashMap::new();
        for t in targets {
            let Some(s) = subject(&c, t)? else { continue };
            let mut found = ref_item(&c, t)?;
            if found.is_none()
                && let Some(o) = &s.other
            {
                found = ref_item(&c, o)?.or_else(|| (!t.starts_with("iss_")).then(|| o.clone()));
            }
            out.insert((*t).to_owned(), found.unwrap_or_else(|| (*t).to_owned()));
        }
        Ok(out)
    }

    /// The work items that have a row on any board of `project`.
    pub fn project_board_work_items(&self, project: &str) -> Result<std::collections::HashSet<String>> {
        Ok(self
            .c()
            .query_map(
                "SELECT DISTINCT e.work_item FROM board_entries e JOIN boards b ON b.id = e.board_id
                 WHERE b.project_id = ?1 AND e.kind = 'row' AND e.work_item IS NOT NULL",
                params![project],
                |r| r.get::<String>(0),
            )?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
#[path = "work_items_tests.rs"]
mod tests;
