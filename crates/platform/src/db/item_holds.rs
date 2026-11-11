//! Tickets marked Not finished (Arugula #841, `0111_item_holds`): a native
//! issue (`iss_`) or a Plane item (`fi_`) with what is left to do. While a
//! ticket has one, a merged PR that names it doesn't close it
//! ([`crate::forge::closes`], [`crate::trackers::done`]). It is on the
//! ticket, not a board's row, so a ticket on several boards is held on all of
//! them. A hold on a closed ticket is ignored, and kept until a person or an
//! agent releases it.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Deleted;

use super::{
    Db,
    forge::{MergedPr, fold, like_anywhere},
    issues::Actor,
    sql::{Optional as _, Sql, params},
};

/// The longest reason, in characters.
pub const REASON_MAX: usize = 200;

/// The merge a hold kept from closing its ticket, kept so that taking the
/// mark off closes it whatever the mirror holds of the PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    /// The PR's id in the mirror (`fi_…`).
    pub pr: String,
    /// `owner/repo#123`.
    pub reference: String,
    /// The words of the link that would have closed a native issue.
    pub words: Option<String>,
    /// GitHub's id of whoever merged it.
    pub merged_by: Option<u64>,
}

/// A ticket's hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    /// The ticket: an `iss_` or `fi_` id.
    pub item: String,
    pub reason: String,
    /// The account that marked it.
    pub by: String,
    /// The machine of the agent that did, for a person's agent.
    pub by_agent: Option<String>,
    pub at: u64,
    /// The newest merged PR that would have closed it, if one has.
    pub skipped: Option<Skipped>,
}

/// A merged PR that may have closed a ticket, with the repo it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub repo: u64,
    pub full_name: String,
    pub pr: MergedPr,
}

/// The most candidate PRs one release reads.
const CANDIDATES_MAX: usize = 100;

const MERGED_COLUMNS: &str = "i.repo_id, l.full_name, i.id, i.number, i.title, i.head_ref,
    (SELECT c.body FROM forge_content c
     WHERE c.repo_id = i.repo_id AND c.number = i.number AND c.kind = 'body'
     ORDER BY c.github_id LIMIT 1),
    (SELECT e.actor_id FROM forge_events e
     WHERE e.repo_id = i.repo_id AND e.number = i.number AND e.kind = 'merged'
     ORDER BY e.at DESC LIMIT 1)";

fn merged_row(r: &super::sql::Row) -> Result<Merged> {
    Ok(Merged {
        repo: r.get(0)?,
        full_name: r.get(1)?,
        pr: MergedPr {
            id: r.get(2)?,
            number: r.get(3)?,
            title: r.get(4)?,
            head_ref: r.get(5)?,
            body: r.get(6)?,
            merged_by: r.get(7)?,
        },
    })
}

fn hold_row(r: &super::sql::Row) -> Result<Hold> {
    let skipped: Option<String> = r.get(5)?;
    Ok(Hold {
        item: r.get(0)?,
        reason: r.get(1)?,
        by: r.get(2)?,
        by_agent: r.get(3)?,
        at: r.get(4)?,
        skipped: skipped.and_then(|j| serde_json::from_str(&j).ok()),
    })
}

/// What deleting an account takes from holds: those of the projects that
/// went with it, and its name where it only marked one.
pub(super) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("DELETE FROM item_holds WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE item_holds SET held_by = '' WHERE held_by = ?1", params![d.account])?;
    Ok(())
}

impl Db {
    /// Mark `item` of `project` Not finished with `reason` (already checked),
    /// as `by`: a ticket held already takes the new reason and writer.
    pub fn hold_set(&self, project: &str, item: &str, reason: &str, by: Actor<'_>, now: u64) -> Result<()> {
        self.c().execute(
            "INSERT INTO item_holds (project_id, item, reason, held_by, held_by_agent, held_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project_id, item) DO UPDATE SET
                 reason = excluded.reason, held_by = excluded.held_by,
                 held_by_agent = excluded.held_by_agent, held_at = excluded.held_at",
            params![project, item, reason, by.account, by.agent, now],
        )?;
        Ok(())
    }

    /// Note the merged PR `s` that `item`'s hold kept from closing it, in
    /// place of an older one. Nothing if the ticket isn't held.
    pub fn hold_skipped(&self, project: &str, item: &str, s: &Skipped) -> Result<()> {
        self.c().execute(
            "UPDATE item_holds SET skipped = ?3 WHERE project_id = ?1 AND item = ?2",
            params![project, item, serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    /// Take the mark off `item`; false if it had none.
    pub fn hold_clear(&self, project: &str, item: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM item_holds WHERE project_id = ?1 AND item = ?2", params![project, item])? > 0)
    }

    /// `item`'s hold, if it has one.
    pub fn hold_of(&self, project: &str, item: &str) -> Result<Option<Hold>> {
        self.c()
            .query_row(
                "SELECT item, reason, held_by, held_by_agent, held_at, skipped FROM item_holds
                 WHERE project_id = ?1 AND item = ?2",
                params![project, item],
                hold_row,
            )
            .optional()
    }

    /// Every hold of `project`.
    pub fn holds(&self, project: &str) -> Result<Vec<Hold>> {
        self.c().query_map(
            "SELECT item, reason, held_by, held_by_agent, held_at, skipped FROM item_holds WHERE project_id = ?1 ORDER BY item",
            params![project],
            hold_row,
        )
    }

    /// The merged PRs of `project`'s linked repos whose title, branch or body
    /// has `needle` in it (any case), newest first: a prefilter for the PRs a
    /// ticket's release looks at, which read their references as a merge
    /// does. The body is the mirror's, kept only while the project mirrors
    /// content.
    pub fn merged_prs_mentioning(&self, project: &str, needle: &str) -> Result<Vec<Merged>> {
        let c = self.c();
        let (title, head, body) = (fold(&c, "i.title"), fold(&c, "coalesce(i.head_ref, '')"), fold(&c, "c.body"));
        c.query_map(
            &format!(
                r"SELECT {MERGED_COLUMNS}
                 FROM forge_items i JOIN linked_repos l ON l.repo_id = i.repo_id
                 WHERE l.project_id = ?1 AND l.unlinked_at IS NULL AND i.kind = 'pr' AND i.state = 'merged'
                   AND ({title} LIKE ?2 ESCAPE '\' OR {head} LIKE ?2 ESCAPE '\'
                        OR EXISTS (SELECT 1 FROM forge_content c
                                   WHERE c.repo_id = i.repo_id AND c.number = i.number AND c.kind = 'body'
                                     AND {body} LIKE ?2 ESCAPE '\'))
                 ORDER BY i.number DESC LIMIT {CANDIDATES_MAX}"
            ),
            params![project, like_anywhere(needle)],
            merged_row,
        )
    }

    /// The merged PRs attached as work, by a person or an agent, to a row of
    /// `project` whose item is `item` (an `iss_` or `fi_` id).
    pub fn merged_prs_attached_to(&self, project: &str, item: &str) -> Result<Vec<Merged>> {
        self.c().query_map(
            &format!(
                "SELECT DISTINCT {MERGED_COLUMNS}
                 FROM board_attachments a
                 JOIN boards b ON b.id = a.board_id
                 JOIN board_entries e ON e.board_id = a.board_id AND e.id = a.row_id
                 JOIN forge_items i ON i.id = a.target
                 JOIN linked_repos l ON l.repo_id = i.repo_id
                 WHERE b.project_id = ?1 AND l.project_id = ?1 AND l.unlinked_at IS NULL
                   AND a.target_kind = 'forge_item' AND a.role = 'work' AND a.source IS NULL
                   AND e.target = ?2 AND e.target_kind IN ('issue', 'forge_item')
                   AND i.kind = 'pr' AND i.state = 'merged'
                 ORDER BY i.number DESC LIMIT {CANDIDATES_MAX}"
            ),
            params![project, item],
            merged_row,
        )
    }
}
