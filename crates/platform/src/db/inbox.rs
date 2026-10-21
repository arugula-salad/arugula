//! The inbox's rows (M171, #1217): `inbox_items`, one per member, item and
//! reason, kept while the reason holds, with when the member saw it and
//! when they called it done.
//!
//! - **Worked out by the platform** ([`crate::forge::inbox`]) from an
//!   item's mirrored rows ([`Db::inbox_sources`]). That read is the
//!   platform's own, never a person's: what it finds reaches a person only
//!   as their inbox's rows, and only once the viewer check let them read
//!   the item's repo.
//! - **Read through the viewer check:** [`Db::inbox`] and
//!   [`Db::mark_inbox`] take the [`Readable`] it made, and touch rows of
//!   its repos' items only, as every forge read does (M165).

use std::collections::BTreeSet;

use super::{
    Db,
    forge::{ForgeCheck, ForgeEvent, ForgeItem, ITEM_COLUMNS, item_row, repos_in},
    sql::{Optional as _, Sql, params},
};
use crate::forge::viewer::Readable;

/// A body or comment the mirror holds for an item (M166), when its
/// project mirrors content: what a mention is found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    /// `body`, `comment` or `review_comment`.
    pub kind: String,
    pub github_id: u64,
    pub author_login: Option<String>,
    pub body: String,
    pub updated_at: u64,
}

/// What an item's inbox is worked out from: the item, its head's checks,
/// its reviews, its bodies and comments where content is mirrored, and
/// the drafts waiting on it (their authors and ids).
#[derive(Debug, Clone)]
pub struct Sources {
    pub full_name: String,
    pub item: ForgeItem,
    pub checks: Vec<ForgeCheck>,
    pub reviews: Vec<ForgeEvent>,
    pub content: Vec<Content>,
    pub drafts: Vec<(String, String)>,
}

/// One entry of a member's inbox, with what it's about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxRow {
    pub item: String,
    pub reason: String,
    pub seen_at: Option<u64>,
    pub done_at: Option<u64>,
    pub created_at: u64,
    pub full_name: String,
    pub number: u64,
    pub kind: String,
    pub state: String,
    pub title: String,
    pub author_login: Option<String>,
    pub updated_at: u64,
}

/// What a member's inbox is narrowed to.
#[derive(Debug, Clone, Copy, Default)]
pub struct InboxFilter {
    /// Entries called done too.
    pub done: bool,
    pub limit: u32,
}

impl Db {
    /// An item's id by its repo and number.
    pub fn forge_item_id(&self, repo_id: u64, number: u64) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT id FROM forge_items WHERE repo_id = ?1 AND number = ?2", params![repo_id, number], |r| {
                r.get(0)
            })
            .optional()
    }

    /// The platform's own read of what an item's inbox is worked out from
    /// (module docs): `None` when the item isn't mirrored, or its repo
    /// isn't linked.
    pub fn inbox_sources(&self, repo_id: u64, number: u64) -> anyhow::Result<Option<Sources>> {
        let c = self.c();
        let Some((item, full_name)) = c
            .query_row(
                &format!(
                    "SELECT {ITEM_COLUMNS}, l.full_name FROM forge_items i JOIN linked_repos l ON l.repo_id = i.repo_id
                     WHERE i.repo_id = ?1 AND i.number = ?2 AND l.unlinked_at IS NULL"
                ),
                params![repo_id, number],
                |r| Ok((item_row(r)?, r.get::<String>(18)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let checks = match &item.head_sha {
            Some(sha) => c.query_map(
                "SELECT kind, name, status, conclusion, url, updated_at FROM forge_checks
                 WHERE repo_id = ?1 AND head_sha = ?2 ORDER BY name, kind",
                params![repo_id, sha],
                |r| {
                    Ok(ForgeCheck {
                        kind: r.get(0)?,
                        name: r.get(1)?,
                        status: r.get(2)?,
                        conclusion: r.get(3)?,
                        url: r.get(4)?,
                        updated_at: r.get(5)?,
                    })
                },
            )?,
            None => vec![],
        };
        let reviews = c.query_map(
            "SELECT id, kind, actor_login, review_state, at FROM forge_events
             WHERE repo_id = ?1 AND number = ?2 AND kind = 'review' ORDER BY at, id",
            params![repo_id, number],
            |r| {
                Ok(ForgeEvent {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    actor_login: r.get(2)?,
                    review_state: r.get(3)?,
                    at: r.get(4)?,
                })
            },
        )?;
        let content = c.query_map(
            "SELECT kind, github_id, author_login, body, updated_at FROM forge_content
             WHERE repo_id = ?1 AND number = ?2 ORDER BY updated_at, github_id",
            params![repo_id, number],
            |r| {
                Ok(Content {
                    kind: r.get(0)?,
                    github_id: r.get(1)?,
                    author_login: r.get(2)?,
                    body: r.get(3)?,
                    updated_at: r.get(4)?,
                })
            },
        )?;
        let drafts = c.query_map(
            "SELECT author, id FROM drafts WHERE repo_id = ?1 AND number = ?2 AND state = 'waiting'
             ORDER BY created_at, id",
            params![repo_id, number],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(Some(Sources { full_name, item, checks, reviews, content, drafts }))
    }

    /// A linked repo's items whose inbox a sweep works out again: the open
    /// ones, and every one that's in someone's inbox now.
    pub fn inbox_numbers(&self, repo_id: u64) -> anyhow::Result<Vec<u64>> {
        self.c().query_map(
            "SELECT number FROM forge_items i WHERE i.repo_id = ?1
                 AND (i.state = 'open' OR EXISTS (SELECT 1 FROM inbox_items x WHERE x.item = i.id))
             ORDER BY number",
            params![repo_id],
            |r| r.get(0),
        )
    }

    /// The repo's items with one of these heads: whose checks changed.
    pub fn head_numbers(&self, repo_id: u64, shas: &[String]) -> anyhow::Result<Vec<u64>> {
        let mut out = Vec::new();
        for sha in shas {
            out.extend(self.c().query_map(
                "SELECT number FROM forge_items WHERE repo_id = ?1 AND head_sha = ?2",
                params![repo_id, sha],
                |r| r.get::<u64>(0),
            )?);
        }
        Ok(out)
    }

    /// An item's inbox as it should be: `want` is every `(account,
    /// reason)` that holds now. Rows that no longer hold go (an account
    /// not in `want` has none left); a reason that holds keeps its row, so
    /// what was seen and done stays. The rows made now, which are what a
    /// push is for: on two machines at once, only one makes each.
    pub fn set_inbox(&self, item: &str, want: &[(String, String)], now: u64) -> anyhow::Result<Vec<(String, String)>> {
        let want: BTreeSet<&(String, String)> = want.iter().collect();
        let mut c = self.c();
        let tx = c.immediate()?;
        let have: Vec<(String, String)> = tx.query_map(
            &format!("SELECT account, reason FROM inbox_items WHERE item = ?1{}", tx.for_update()),
            params![item],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        for (a, r) in have.iter().filter(|h| !want.contains(h)) {
            tx.execute(
                "DELETE FROM inbox_items WHERE account = ?1 AND item = ?2 AND reason = ?3",
                params![a, item, r],
            )?;
        }
        let mut made = Vec::new();
        for (a, r) in want.into_iter().filter(|w| !have.contains(w)) {
            let n = tx.execute(
                "INSERT INTO inbox_items (account, item, reason, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT (account, item, reason) DO NOTHING",
                params![a, item, r, now],
            )?;
            if n == 1 {
                made.push((a.clone(), r.clone()));
            }
        }
        tx.commit()?;
        Ok(made)
    }

    /// Every reason an item is in an account's inbox for, seen, done or
    /// not: what the shadow comparison holds against a block's.
    pub fn inbox_reasons(&self, account: &str, item: &str) -> anyhow::Result<Vec<String>> {
        self.c().query_map(
            "SELECT reason FROM inbox_items WHERE account = ?1 AND item = ?2 ORDER BY reason",
            params![account, item],
            |r| r.get(0),
        )
    }

    /// The repos an account's inbox has items of: what a read of it checks.
    pub fn inbox_repos(&self, account: &str) -> anyhow::Result<Vec<u64>> {
        self.c().query_map(
            "SELECT DISTINCT i.repo_id FROM inbox_items x JOIN forge_items i ON i.id = x.item WHERE x.account = ?1",
            params![account],
            |r| r.get(0),
        )
    }

    /// A draft's repo and number: what its inbox reason is on.
    pub fn draft_item(&self, id: &str) -> anyhow::Result<Option<(u64, Option<u64>)>> {
        self.c()
            .query_row("SELECT repo_id, number FROM drafts WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
    }

    /// Of these drafts, the waiting ones that are `author`'s.
    pub fn waiting_drafts_of(&self, author: &str, ids: &[String]) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.c().query_map(
                "SELECT id FROM drafts WHERE id = ?1 AND author = ?2 AND state = 'waiting'",
                params![id, author],
                |r| r.get::<String>(0),
            )?);
        }
        Ok(out)
    }

    /// An account's inbox on the readable repos, newest first, with what
    /// each entry is about.
    pub fn inbox(&self, r: &Readable, account: &str, f: &InboxFilter) -> anyhow::Result<Vec<InboxRow>> {
        if r.repos().is_empty() {
            return Ok(vec![]);
        }
        let done = if f.done { "" } else { " AND x.done_at IS NULL" };
        self.c().query_map(
            &format!(
                "SELECT x.item, x.reason, x.seen_at, x.done_at, x.created_at, l.full_name, i.number, i.kind, i.state,
                     i.title, i.author_login, i.updated_at
                 FROM inbox_items x JOIN forge_items i ON i.id = x.item JOIN linked_repos l ON l.repo_id = i.repo_id
                 WHERE x.account = ?1 AND i.repo_id IN ({}){done}
                 ORDER BY x.created_at DESC, x.item, x.reason LIMIT {}",
                repos_in(r),
                f.limit
            ),
            params![account],
            |r| {
                Ok(InboxRow {
                    item: r.get(0)?,
                    reason: r.get(1)?,
                    seen_at: r.get(2)?,
                    done_at: r.get(3)?,
                    created_at: r.get(4)?,
                    full_name: r.get(5)?,
                    number: r.get(6)?,
                    kind: r.get(7)?,
                    state: r.get(8)?,
                    title: r.get(9)?,
                    author_login: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            },
        )
    }

    /// Mark an account's entries for an item (one reason, or all) seen or
    /// not, done or not, if the item is in a readable repo: how many.
    #[allow(clippy::too_many_arguments)]
    pub fn mark_inbox(
        &self,
        r: &Readable,
        account: &str,
        item: &str,
        reason: Option<&str>,
        seen: Option<bool>,
        done: Option<bool>,
        now: u64,
    ) -> anyhow::Result<usize> {
        if r.repos().is_empty() {
            return Ok(0);
        }
        let at = |on: Option<bool>, col: &str| match on {
            Some(true) => format!(", {col} = COALESCE({col}, ?1)"),
            Some(false) => format!(", {col} = NULL"),
            None => String::new(),
        };
        let reason = reason.unwrap_or("");
        self.c().execute(
            &format!(
                "UPDATE inbox_items SET updated_at = ?1{}{} WHERE account = ?2 AND item = ?3
                     AND (?4 = '' OR reason = ?4)
                     AND item IN (SELECT id FROM forge_items WHERE repo_id IN ({}))",
                at(seen, "seen_at"),
                at(done, "done_at"),
                repos_in(r)
            ),
            params![now, account, item, reason],
        )
    }
}
