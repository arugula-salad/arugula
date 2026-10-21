//! The forge mirror's content (M166, #1212): a linked repo's bodies,
//! comments, reviews' text and review comments, in `forge_content`, only
//! while its project mirrors them (`linked_repos.mirror_content`).
//!
//! - **Only while it's on.** Every write runs in [`Db::mirror`]'s
//!   transaction, which holds the repo's link, and checks the switch there
//!   ([`mirrors`]). Turning it off deletes the repo's rows in the same
//!   transaction as the switch ([`Db::set_mirror_content`]), so a write
//!   racing it either lands first and goes, or finds it off.
//! - **Whole items.** An item is mirrored once it has a `body` row: one
//!   is written only with everything else of the item ([`replace_item`]:
//!   a read, or the poller fetched it all) or for an item just opened.
//!   Until then a read fetches it from GitHub. A webhook's edit of a body
//!   only changes a `body` row that's there; its comments land either way.
//! - **Read through the viewer check.** [`Db::forge_content`] and
//!   [`Db::content_rev`] take M165's [`Readable`], like every forge read.
//! - **Never logged.** [`ContentRow`]'s `Debug` leaves the text out.

use anyhow::Result;

use super::{
    Db,
    forge::LinkedRepo,
    sql::{Optional as _, Row, Sql, params},
};
use crate::forge::viewer::Readable;

/// A body, a comment, a review's text or a review comment, as
/// `forge_content` keeps it. `github_id` is the PR's or issue's own for a
/// body, else the comment's or review's.
#[derive(Clone, PartialEq, Eq)]
pub struct ContentRow {
    pub repo_id: u64,
    pub number: u64,
    /// `body`, `comment`, `review` or `review_comment`.
    pub kind: String,
    pub github_id: u64,
    pub author_login: Option<String>,
    pub author_id: Option<u64>,
    pub body: String,
    /// A review comment's file and line, and the comment it answers.
    pub path: Option<String>,
    pub line: Option<u64>,
    pub reply_to: Option<u64>,
    /// GitHub's times, in ms.
    pub created_at: u64,
    pub updated_at: u64,
}

/// Everything but the text, which is never printed.
impl std::fmt::Debug for ContentRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentRow")
            .field("repo_id", &self.repo_id)
            .field("number", &self.number)
            .field("kind", &self.kind)
            .field("github_id", &self.github_id)
            .field("body", &format_args!("<{} bytes>", self.body.len()))
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

const COLUMNS: &str = "repo_id, number, kind, github_id, author_login, author_id, body, path, line, reply_to, \
                       created_at, updated_at";

fn content_row(r: &Row) -> Result<ContentRow> {
    Ok(ContentRow {
        repo_id: r.get(0)?,
        number: r.get(1)?,
        kind: r.get(2)?,
        github_id: r.get(3)?,
        author_login: r.get(4)?,
        author_id: r.get(5)?,
        body: r.get(6)?,
        path: r.get(7)?,
        line: r.get(8)?,
        reply_to: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

/// What a webhook says of an item's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An item opened: its body, all there is of it so far.
    Opened(ContentRow),
    /// Its body as it is now: changes a `body` row that's there.
    Body(ContentRow),
    /// A comment, a review's text or a review comment, made or edited.
    Upsert(ContentRow),
    /// One deleted: its kind and GitHub's id.
    Deleted(String, u64),
}

/// Whether the repo mirrors content, in the transaction that writes it.
pub fn mirrors(c: &impl Sql, repo_id: u64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT mirror_content FROM linked_repos WHERE repo_id = ?1 AND unlinked_at IS NULL",
        params![repo_id],
        |r| r.get::<bool>(0),
    )
    .optional()?
    .unwrap_or(false))
}

/// A write's error without what Postgres adds after its first line: a
/// `DETAIL: Failing row contains (...)` would print the text, and errors
/// are logged.
fn quiet<T>(r: Result<T>) -> Result<T> {
    r.map_err(|e| {
        let first = format!("{e:#}").lines().next().unwrap_or_default().to_owned();
        anyhow::anyhow!("writing forge content: {first}")
    })
}

/// Write a row, unless the one there is newer.
fn upsert(c: &impl Sql, w: &ContentRow) -> Result<()> {
    quiet(c.execute(
        &format!(
            "INSERT INTO forge_content ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (repo_id, kind, github_id) DO UPDATE SET number = excluded.number,
                 author_login = excluded.author_login, author_id = excluded.author_id, body = excluded.body,
                 path = excluded.path, line = excluded.line, reply_to = excluded.reply_to,
                 created_at = excluded.created_at, updated_at = excluded.updated_at
             WHERE excluded.updated_at >= forge_content.updated_at"
        ),
        params![
            w.repo_id,
            w.number,
            w.kind,
            w.github_id,
            w.author_login,
            w.author_id,
            w.body,
            w.path,
            w.line,
            w.reply_to,
            w.created_at,
            w.updated_at
        ],
    ))?;
    Ok(())
}

/// An item's content as GitHub has it now, in place of what was kept: a
/// comment deleted on GitHub goes too.
pub fn replace_item(c: &impl Sql, repo_id: u64, number: u64, rows: &[ContentRow]) -> Result<()> {
    c.execute("DELETE FROM forge_content WHERE repo_id = ?1 AND number = ?2", params![repo_id, number])?;
    for w in rows.iter().filter(|w| w.repo_id == repo_id && w.number == number) {
        upsert(c, w)?;
    }
    Ok(())
}

/// Apply a webhook's change.
pub fn apply(c: &impl Sql, repo_id: u64, change: &Change) -> Result<()> {
    match change {
        Change::Opened(w) | Change::Upsert(w) => upsert(c, w),
        Change::Body(w) => {
            quiet(c.execute(
                "UPDATE forge_content SET body = ?1, author_login = ?2, author_id = ?3, updated_at = ?4
                 WHERE repo_id = ?5 AND number = ?6 AND kind = 'body' AND updated_at <= ?4",
                params![w.body, w.author_login, w.author_id, w.updated_at, repo_id, w.number],
            ))?;
            Ok(())
        }
        Change::Deleted(kind, id) => {
            c.execute(
                "DELETE FROM forge_content WHERE repo_id = ?1 AND kind = ?2 AND github_id = ?3",
                params![repo_id, kind, id],
            )?;
            Ok(())
        }
    }
}

/// An item's content may have changed: what each machine's read cache
/// kept of it is stale.
pub fn bump(c: &impl Sql, repo_id: u64, number: u64) -> Result<()> {
    c.execute(
        "UPDATE forge_items SET content_rev = content_rev + 1 WHERE repo_id = ?1 AND number = ?2",
        params![repo_id, number],
    )?;
    Ok(())
}

impl Db {
    /// Turn a project's linked repo's content mirroring on or off, by name
    /// (any case). On: the poller's next turn backfills, and fetches the
    /// content of what it reads. Off: the repo's content rows go, in the
    /// same transaction. The link and how many rows went, or `None` when
    /// the project has no live link by that name.
    pub fn set_mirror_content(&self, project: &str, full_name: &str, on: bool) -> Result<Option<(LinkedRepo, usize)>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let found = tx
            .query_row(
                &format!(
                    "SELECT repo_id, mirror_content FROM linked_repos
                     WHERE project_id = ?1 AND lower(full_name) = lower(?2) AND unlinked_at IS NULL{}",
                    tx.for_update()
                ),
                params![project, full_name],
                |r| Ok((r.get::<u64>(0)?, r.get::<bool>(1)?)),
            )
            .optional()?;
        let Some((repo_id, was)) = found else { return Ok(None) };
        let mut gone = 0;
        if on && !was {
            tx.execute(
                "UPDATE linked_repos SET mirror_content = ?1, backfilled_at = NULL, poll_cursor = NULL WHERE repo_id = ?2",
                params![true, repo_id],
            )?;
        } else if !on {
            tx.execute("UPDATE linked_repos SET mirror_content = ?1 WHERE repo_id = ?2", params![false, repo_id])?;
            gone = tx.execute("DELETE FROM forge_content WHERE repo_id = ?1", params![repo_id])?;
        }
        tx.commit()?;
        drop(c);
        Ok(self.linked_repo(repo_id)?.map(|l| (l, gone)))
    }

    /// A readable item's content, if the mirror has it whole (a `body`
    /// row): its body first, then the rest as GitHub made them.
    pub fn forge_content(&self, r: &Readable, repo_id: u64, number: u64) -> Result<Option<Vec<ContentRow>>> {
        if !r.has(repo_id) {
            return Ok(None);
        }
        let rows = self.c().query_map(
            &format!(
                "SELECT {COLUMNS} FROM forge_content WHERE repo_id = ?1 AND number = ?2
                 ORDER BY CASE WHEN kind = 'body' THEN 0 ELSE 1 END, created_at, github_id"
            ),
            params![repo_id, number],
            content_row,
        )?;
        Ok(rows.iter().any(|w| w.kind == "body").then_some(rows))
    }

    /// A readable item's content rev ([`bump`]).
    pub fn content_rev(&self, r: &Readable, repo_id: u64, number: u64) -> Result<Option<u64>> {
        if !r.has(repo_id) {
            return Ok(None);
        }
        self.c()
            .query_row(
                "SELECT content_rev FROM forge_items WHERE repo_id = ?1 AND number = ?2",
                params![repo_id, number],
                |r| r.get(0),
            )
            .optional()
    }
}
