//! The forge mirror's tables (M162, platform step 4, #1204): linked repos'
//! PRs and issues as platform rows. They're made by `0005_forge_mirror`;
//! their queries come with the features that write them (M161 deliveries,
//! M163 links, M164 items, M167 mints, M170 drafts, M171 the inbox). So
//! far: the webhook's deliveries (M161), the token log (M167), an
//! account's deletion, linked repos, the App's installations and the
//! purge (M163), and reads of items, checks and events, each through the
//! viewer check's [`Readable`], with the access answers it keeps (M165);
//! and drafts (M170), read the same way.

use std::collections::HashMap;

use serde::Serialize;

use super::{
    Db,
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, params},
};
use crate::{Deleted, forge::viewer::Readable};

/// How long an unlinked repo's rows stay before the purge takes them
/// (M163): "everything is purged within 24 hours of an uninstall", and the
/// purge runs every few minutes, so a little under a day.
pub const PURGE_GRACE_MS: u64 = 23 * 3600 * 1000;

/// A merged PR as [`Db::merged_prs`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPr {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    /// The GitHub id of whoever merged it.
    pub merged_by: Option<u64>,
}

/// A repo linked to a project, or unlinked and waiting for the purge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRepo {
    pub repo_id: u64,
    pub full_name: String,
    pub private: bool,
    pub project_id: String,
    pub installation_id: u64,
    pub linked_by: Option<String>,
    pub linked_at: u64,
    pub unlinked_at: Option<u64>,
    pub purge_after: Option<u64>,
    /// The project mirrors its bodies and comments (M166).
    pub mirror_content: bool,
}

const LINK_COLUMNS: &str = "repo_id, full_name, private, project_id, installation_id, linked_by, linked_at, \
                            unlinked_at, purge_after, mirror_content";

fn link_row(r: &Row) -> anyhow::Result<LinkedRepo> {
    Ok(LinkedRepo {
        repo_id: r.get(0)?,
        full_name: r.get(1)?,
        private: r.get(2)?,
        project_id: r.get(3)?,
        installation_id: r.get(4)?,
        linked_by: r.get(5)?,
        linked_at: r.get(6)?,
        unlinked_at: r.get(7)?,
        purge_after: r.get(8)?,
        mirror_content: r.get(9)?,
    })
}

/// Where the App is installed, as GitHub says it in an `installation`
/// event or `GET /repos/{repo}/installation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub id: u64,
    pub account_id: u64,
    pub login: String,
    /// `User` or `Organization`.
    pub account_type: String,
    /// The permissions it was granted, as GitHub's JSON object.
    pub permissions: String,
}

/// What linking did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linked {
    /// Linked now. `again`: it was unlinked from this project and waiting
    /// for the purge, which it no longer is.
    Now { repo: LinkedRepo, again: bool },
    /// It's linked to another project already: a repo is in one project.
    Elsewhere,
}

/// What a claim's `SELECT` of a repo to purge ends with: on Postgres it
/// passes over one another machine is purging (`platform_jobs`' way).
fn skip_locked(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

/// Every row the mirror holds of a repo, but its link: its items and
/// what's on them, its drafts, inbox rows of its items, and the threads on
/// its items (M196).
pub(super) fn purge_rows(c: &impl Sql, repo_id: impl super::sql::Param + Copy) -> anyhow::Result<()> {
    super::objects::drop_item_threads(c, repo_id)?;
    c.execute(
        "DELETE FROM inbox_items WHERE item IN (SELECT id FROM forge_items WHERE repo_id = ?1)",
        params![repo_id],
    )?;
    for table in ["forge_items", "forge_checks", "forge_events", "forge_content", "forge_refetch", "drafts"] {
        c.execute(&format!("DELETE FROM {table} WHERE repo_id = ?1"), params![repo_id])?;
    }
    Ok(())
}

/// Unlink every live link `filter` picks (a `WHERE` on `linked_repos`
/// whose parameters start at `?3`): how many.
fn unlink_where(c: &impl Sql, filter: &str, p: &[&dyn super::sql::Param], now: u64) -> anyhow::Result<usize> {
    let after = now + PURGE_GRACE_MS;
    let mut all: Vec<&dyn super::sql::Param> = vec![&now, &after];
    all.extend_from_slice(p);
    c.execute(
        &format!("UPDATE linked_repos SET unlinked_at = ?1, purge_after = ?2 WHERE unlinked_at IS NULL AND {filter}"),
        &all,
    )
}

/// How long a webhook delivery is remembered (M161): GitHub redelivers by
/// hand for a few days, and a deploy loses nothing.
pub const DELIVERIES_KEPT_MS: u64 = 7 * 24 * 3600 * 1000;

/// How long the token log keeps a mint (M167): a year, so a token GitHub's
/// own audit log names can be traced to who and what it was for.
pub const MINTS_KEPT_MS: u64 = 365 * 24 * 3600 * 1000;

/// Whose a minted token was: the App's installation's, or a person's user
/// token (M168).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MintKind {
    Installation,
    User,
}

/// A mint to write down: never the token.
#[derive(Debug, Clone, Copy)]
pub struct NewMint<'a> {
    pub kind: MintKind,
    pub installation_id: Option<u64>,
    pub repo_id: Option<u64>,
    /// What it was scoped to, as GitHub was asked.
    pub permissions: &'a serde_json::Value,
    /// The account it was for, or `platform`.
    pub caller: &'a str,
    /// What for (`read`, `ingest`, `send:issues`, ...).
    pub purpose: &'a str,
    pub at: u64,
}

/// A row of `github_token_mints`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenMint {
    pub id: String,
    pub kind: String,
    pub installation_id: Option<u64>,
    pub repo_id: Option<u64>,
    pub permissions: String,
    pub caller: String,
    pub purpose: String,
    pub at: u64,
}

fn mint_row(r: &Row) -> anyhow::Result<TokenMint> {
    Ok(TokenMint {
        id: r.get(0)?,
        kind: r.get(1)?,
        installation_id: r.get(2)?,
        repo_id: r.get(3)?,
        permissions: r.get(4)?,
        caller: r.get(5)?,
        purpose: r.get(6)?,
        at: r.get(7)?,
    })
}

impl Db {
    /// Remember a webhook delivery (`X-GitHub-Delivery`): false when it
    /// was here already, a redelivery.
    pub fn first_delivery(&self, delivery: &str, event: &str, now: u64) -> anyhow::Result<bool> {
        let n = self.c().execute(
            "INSERT INTO github_webhook_deliveries (delivery, event, received_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (delivery) DO NOTHING",
            params![delivery, event, now],
        )?;
        Ok(n == 1)
    }

    /// The event a delivery was, if it was taken and is still kept.
    pub fn delivery_event(&self, delivery: &str) -> anyhow::Result<Option<String>> {
        self.c()
            .query_row("SELECT event FROM github_webhook_deliveries WHERE delivery = ?1", params![delivery], |r| {
                r.get(0)
            })
            .optional()
    }

    /// Forget one, so GitHub's redelivery of it is taken: what it asked
    /// for didn't happen.
    pub fn forget_delivery(&self, delivery: &str) -> anyhow::Result<()> {
        self.c().execute("DELETE FROM github_webhook_deliveries WHERE delivery = ?1", params![delivery])?;
        Ok(())
    }

    /// Forget deliveries older than [`DELIVERIES_KEPT_MS`]: how many went.
    pub fn prune_deliveries(&self, now: u64) -> anyhow::Result<usize> {
        let before = now.saturating_sub(DELIVERIES_KEPT_MS);
        self.c().execute("DELETE FROM github_webhook_deliveries WHERE received_at < ?1", params![before])
    }

    /// Write a GitHub token's mint to the log (M167): its id (`gtm_`).
    pub fn log_mint(&self, m: &NewMint<'_>) -> anyhow::Result<String> {
        let id = format!("gtm_{}", hex::encode(arugula_e2e::random::<16>()));
        let kind = match m.kind {
            MintKind::Installation => "installation",
            MintKind::User => "user",
        };
        self.c().execute(
            "INSERT INTO github_token_mints (id, kind, installation_id, repo_id, permissions, caller, purpose, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, kind, m.installation_id, m.repo_id, m.permissions.to_string(), m.caller, m.purpose, m.at],
        )?;
        Ok(id)
    }

    /// The log's mints since `since`, oldest first.
    pub fn token_mints(&self, since: u64) -> anyhow::Result<Vec<TokenMint>> {
        self.c().query_map(
            "SELECT id, kind, installation_id, repo_id, permissions, caller, purpose, at
             FROM github_token_mints WHERE at >= ?1 ORDER BY at, id",
            params![since],
            mint_row,
        )
    }

    /// Forget mints older than [`MINTS_KEPT_MS`]: how many went.
    pub fn prune_mints(&self, now: u64) -> anyhow::Result<usize> {
        let before = now.saturating_sub(MINTS_KEPT_MS);
        self.c().execute("DELETE FROM github_token_mints WHERE at < ?1", params![before])
    }

    /// A project's linked repos, and those unlinked from it that wait for
    /// the purge, by name.
    pub fn linked_repos(&self, project: &str) -> anyhow::Result<Vec<LinkedRepo>> {
        self.c().query_map(
            &format!(
                "SELECT {LINK_COLUMNS} FROM linked_repos WHERE project_id = ?1 ORDER BY lower(full_name), repo_id"
            ),
            params![project],
            link_row,
        )
    }

    /// Whose a project is: its account's, or its team's. `None` when
    /// there's no such project.
    pub fn project_holder(&self, project: &str) -> anyhow::Result<Option<(Option<String>, Option<String>)>> {
        self.c()
            .query_row("SELECT account_id, team_id FROM projects WHERE id = ?1", params![project], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
    }

    /// The link of a repo, by GitHub's id, live or waiting for the purge.
    pub fn linked_repo(&self, repo_id: u64) -> anyhow::Result<Option<LinkedRepo>> {
        self.c()
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1"),
                params![repo_id],
                link_row,
            )
            .optional()
    }

    /// The merged PRs of `repo` among these `numbers`, as the issues they
    /// close are read (Arugula #432): title, body when the content cache has
    /// it, and GitHub's id for who merged it when a `merged` event says.
    pub fn merged_prs(&self, repo_id: u64, numbers: &[u64]) -> anyhow::Result<Vec<MergedPr>> {
        if numbers.is_empty() {
            return Ok(vec![]);
        }
        // Numbers are the platform's own: written into the statement.
        let nums = numbers.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
        self.c().query_map(
            &format!(
                "SELECT i.number, i.title,
                        (SELECT c.body FROM forge_content c
                         WHERE c.repo_id = i.repo_id AND c.number = i.number AND c.kind = 'body'
                         ORDER BY c.github_id LIMIT 1),
                        (SELECT e.actor_id FROM forge_events e
                         WHERE e.repo_id = i.repo_id AND e.number = i.number AND e.kind = 'merged'
                         ORDER BY e.at DESC LIMIT 1)
                 FROM forge_items i
                 WHERE i.repo_id = ?1 AND i.kind = 'pr' AND i.state = 'merged' AND i.number IN ({nums})"
            ),
            params![repo_id],
            |r| Ok(MergedPr { number: r.get(0)?, title: r.get(1)?, body: r.get(2)?, merged_by: r.get(3)? }),
        )
    }

    /// A live link, by the repo's name (any case).
    pub fn linked_repo_named(&self, full_name: &str) -> anyhow::Result<Option<LinkedRepo>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {LINK_COLUMNS} FROM linked_repos WHERE lower(full_name) = lower(?1) AND unlinked_at IS NULL
                     ORDER BY linked_at DESC LIMIT 1"
                ),
                params![full_name],
                link_row,
            )
            .optional()
    }

    /// Link a repo to a project, from `installation`, as `by`. A repo
    /// unlinked from this project and waiting for the purge is linked
    /// again, and keeps its rows. One unlinked from another project loses
    /// them now (they were that project's), then is linked here. One
    /// linked to another project stays there ([`Linked::Elsewhere`]).
    pub fn link_repo(&self, l: &LinkedRepo, installation: &Installation, by: &str, now: u64) -> anyhow::Result<Linked> {
        let mut c = self.c();
        let tx = c.immediate()?;
        // Kept as it is when the App's events brought it already.
        tx.execute(
            "INSERT INTO github_installations
                 (installation_id, github_account_id, github_login, github_account_type, permissions, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (installation_id) DO NOTHING",
            params![
                installation.id,
                installation.account_id,
                installation.login,
                installation.account_type,
                installation.permissions,
                now
            ],
        )?;
        let was = tx
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1{}", tx.for_update()),
                params![l.repo_id],
                link_row,
            )
            .optional()?;
        let again = match &was {
            Some(w) if w.project_id != l.project_id && w.unlinked_at.is_none() => return Ok(Linked::Elsewhere),
            Some(w) if w.project_id != l.project_id => {
                purge_rows(&tx, w.repo_id)?;
                tx.execute("DELETE FROM linked_repos WHERE repo_id = ?1", params![w.repo_id])?;
                false
            }
            Some(w) => w.unlinked_at.is_some(),
            None => false,
        };
        let Some(repo) = upsert_link(&tx, l, installation.id, by, now)? else { return Ok(Linked::Elsewhere) };
        tx.commit()?;
        Ok(Linked::Now { repo, again })
    }

    /// Unlink a project's repo, by name (any case): ingest stops now, and
    /// its rows go at the purge ([`PURGE_GRACE_MS`]). The link, or `None`
    /// when the project has no live link by that name.
    pub fn unlink_repo(&self, project: &str, full_name: &str, now: u64) -> anyhow::Result<Option<LinkedRepo>> {
        let c = self.c();
        let n = unlink_where(&c, "project_id = ?3 AND lower(full_name) = lower(?4)", params![project, full_name], now)?;
        if n == 0 {
            return Ok(None);
        }
        c.query_row(
            &format!(
                "SELECT {LINK_COLUMNS} FROM linked_repos WHERE project_id = ?1 AND lower(full_name) = lower(?2)
                 AND unlinked_at = ?3"
            ),
            params![project, full_name, now],
            link_row,
        )
        .optional()
    }

    /// The App is installed somewhere, or an installation changed (new
    /// permissions, unsuspended): keep it, not suspended or deleted.
    pub fn installation_seen(&self, i: &Installation, now: u64) -> anyhow::Result<()> {
        upsert_installation(&self.c(), i, now)
    }

    /// An installation was deleted (`deleted`) or suspended: noted, and
    /// every repo linked through it unlinked. How many were.
    pub fn installation_gone(&self, i: &Installation, deleted: bool, now: u64) -> anyhow::Result<usize> {
        let mut c = self.c();
        let tx = c.immediate()?;
        upsert_installation(&tx, i, now)?;
        let column = if deleted { "deleted_at" } else { "suspended_at" };
        tx.execute(
            &format!("UPDATE github_installations SET {column} = ?1, updated_at = ?1 WHERE installation_id = ?2"),
            params![now, i.id],
        )?;
        let n = unlink_where(&tx, "installation_id = ?3", params![i.id], now)?;
        tx.commit()?;
        Ok(n)
    }

    /// Repos were taken out of an installation: each one linked through it
    /// is unlinked. How many were.
    pub fn repos_removed(&self, installation: u64, repo_ids: &[u64], now: u64) -> anyhow::Result<usize> {
        let mut n = 0;
        let mut c = self.c();
        let tx = c.immediate()?;
        for id in repo_ids {
            n += unlink_where(&tx, "installation_id = ?3 AND repo_id = ?4", params![installation, id], now)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Purge one unlinked repo whose `purge_after` has passed and whose
    /// poller holds no lease (M164): every row of it, then its link, in
    /// one transaction. Another machine purging at the same time passes
    /// over it. The repo purged, or `None` when none is due.
    pub fn purge_one(&self, now: u64) -> anyhow::Result<Option<u64>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let due = tx
            .query_row(
                &format!(
                    "SELECT repo_id FROM linked_repos
                     WHERE purge_after IS NOT NULL AND purge_after <= ?1 AND unlinked_at IS NOT NULL AND lease_until <= ?1
                     ORDER BY purge_after LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now],
                |r| r.get::<u64>(0),
            )
            .optional()?;
        let Some(repo_id) = due else { return Ok(None) };
        purge_rows(&tx, repo_id)?;
        tx.execute("DELETE FROM linked_repos WHERE repo_id = ?1", params![repo_id])?;
        tx.commit()?;
        Ok(Some(repo_id))
    }
}

/// Write `l`'s link, or link it again: it keeps what its poller did (M164)
/// and whether the project mirrors its content. A row of another project's
/// is left as it is, and the answer is `None`: on Postgres, another
/// transaction can link a repo nobody had between [`Db::link_repo`]'s
/// look and this write (its `FOR UPDATE` locks no row that isn't there),
/// and the repo stays with the project that linked it first.
fn upsert_link(
    c: &impl Sql,
    l: &LinkedRepo,
    installation: u64,
    by: &str,
    now: u64,
) -> anyhow::Result<Option<LinkedRepo>> {
    c.execute(
        "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (repo_id) DO UPDATE SET full_name = ?2, private = ?3, installation_id = ?5,
             linked_by = CASE WHEN linked_repos.unlinked_at IS NULL THEN linked_repos.linked_by ELSE ?6 END,
             linked_at = CASE WHEN linked_repos.unlinked_at IS NULL THEN linked_repos.linked_at ELSE ?7 END,
             unlinked_at = NULL, purge_after = NULL
         WHERE linked_repos.project_id = ?4",
        params![l.repo_id, l.full_name, l.private, l.project_id, installation, by, now],
    )?;
    let repo = c.query_row(
        &format!("SELECT {LINK_COLUMNS} FROM linked_repos WHERE repo_id = ?1"),
        params![l.repo_id],
        link_row,
    )?;
    Ok((repo.project_id == l.project_id).then_some(repo))
}

/// A PR or an issue as the mirror keeps it (M164 writes them): never its
/// body. `labels`, `assignees` and `reviewers` are JSON lists of names and
/// logins, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeItem {
    pub id: String,
    pub repo_id: u64,
    pub number: u64,
    pub kind: String,
    pub state: String,
    pub title: String,
    pub labels: String,
    pub author_login: Option<String>,
    pub author_id: Option<u64>,
    pub assignees: String,
    pub reviewers: String,
    pub draft: bool,
    pub merge_state: Option<String>,
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    pub base_ref: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

pub(super) const ITEM_COLUMNS: &str = "i.id, i.repo_id, i.number, i.kind, i.state, i.title, i.labels, i.author_login, i.author_id, \
     i.assignees, i.reviewers, i.draft, i.merge_state, i.head_ref, i.head_sha, i.base_ref, i.created_at, i.updated_at";

pub(super) fn item_row(r: &Row) -> anyhow::Result<ForgeItem> {
    Ok(ForgeItem {
        id: r.get(0)?,
        repo_id: r.get(1)?,
        number: r.get(2)?,
        kind: r.get(3)?,
        state: r.get(4)?,
        title: r.get(5)?,
        labels: r.get(6)?,
        author_login: r.get(7)?,
        author_id: r.get(8)?,
        assignees: r.get(9)?,
        reviewers: r.get(10)?,
        draft: r.get(11)?,
        merge_state: r.get(12)?,
        head_ref: r.get(13)?,
        head_sha: r.get(14)?,
        base_ref: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
    })
}

/// A check run or commit status on a head commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeCheck {
    pub kind: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    pub updated_at: u64,
}

/// A check's state alone, on a head commit of a repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckState {
    pub repo_id: u64,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
}

/// What happened to an item, as metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeEvent {
    pub id: String,
    pub kind: String,
    pub actor_login: Option<String>,
    pub review_state: Option<String>,
    pub at: u64,
}

/// What a list of items is narrowed to. Each is matched without regard to
/// ASCII case, the same on both databases ([`fold`]); `text` is text
/// anywhere in the title or, for a repo that mirrors content (M166), in its
/// body, comments and reviews (the list's search).
#[derive(Debug, Clone, Copy, Default)]
pub struct ItemFilter<'a> {
    pub kind: Option<&'a str>,
    pub state: Option<&'a str>,
    pub author: Option<&'a str>,
    /// A login among the requested reviewers.
    pub review_requested: Option<&'a str>,
    pub text: Option<&'a str>,
    /// One PR's or issue's number (M169's `pr.get`).
    pub number: Option<u64>,
    pub limit: u32,
}

/// `IN (...)` of the repos a [`Readable`] holds: GitHub's ids, numbers the
/// platform keeps, so written into the statement.
pub(super) fn repos_in(r: &Readable) -> String {
    r.repos().iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
}

/// `%text%` for `LIKE ... ESCAPE '\'`, with the text's own `%`, `_` and `\`
/// taken as themselves.
pub(super) fn like_anywhere(text: &str) -> String {
    let mut s = String::from("%");
    for c in text.to_ascii_lowercase().chars() {
        if matches!(c, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

/// `expr` with ASCII letters in lower case and nothing else changed, as
/// SQLite's `lower` does (Postgres's folds every letter), so a filter
/// matches the same on both. Its value is folded in Rust the same way.
pub(super) fn fold(c: &impl Sql, expr: &str) -> String {
    match c.dialect() {
        Dialect::Sqlite => format!("lower({expr})"),
        Dialect::Postgres => format!("translate({expr}, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz')"),
    }
}

/// The database's clock, in milliseconds, as SQL: what every machine's
/// access answers and drops are timed by, so their clocks never matter.
fn clock(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "CAST((julianday('now') - 2440587.5) * 86400000.0 AS INTEGER)",
        Dialect::Postgres => "CAST(floor(extract(epoch FROM clock_timestamp()) * 1000) AS BIGINT)",
    }
}

/// What drops kept access answers ([`Db::drop_access`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessScope {
    /// A repo, by `owner/name`.
    Repo,
    /// Every repo of a user or an organization, by login.
    Owner,
    /// A person's answers, by GitHub id.
    Person,
}

/// Forge reads (M165): every one that returns a forge row takes the
/// [`Readable`] the viewer check made, and returns rows of its repos only.
/// Nothing else here reads `forge_items`, `forge_checks` or `forge_events`
/// for a person.
impl Db {
    /// The repo an item is in, and nothing else of it: what the viewer
    /// check needs to decide on it. A tracker's item (a negative
    /// `repo_id`, `0071_tracker_connections`) isn't a repo's: not found.
    pub fn forge_item_repo(&self, id: &str) -> anyhow::Result<Option<u64>> {
        self.c()
            .query_row("SELECT repo_id FROM forge_items WHERE id = ?1 AND repo_id > 0", params![id], |r| r.get(0))
            .optional()
    }

    /// The items of the readable repos that match, last updated first.
    pub fn forge_items(&self, r: &Readable, f: &ItemFilter<'_>) -> anyhow::Result<Vec<ForgeItem>> {
        if r.repos().is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let mut wh = vec![format!("i.repo_id IN ({})", repos_in(r))];
        let mut args: Vec<String> = Vec::new();
        let mut eq = |col: &str, v: Option<&str>| {
            if let Some(v) = v {
                wh.push(format!("{} = ?", fold(&c, col)));
                args.push(v.to_ascii_lowercase());
            }
        };
        eq("i.kind", f.kind);
        eq("i.state", f.state);
        eq("i.author_login", f.author);
        if let Some(login) = f.review_requested {
            // A row whose reviewers aren't a JSON list matches nothing,
            // rather than failing the whole list.
            wh.push(match c.dialect() {
                Dialect::Sqlite => format!(
                    "CASE WHEN json_valid(i.reviewers) AND json_type(i.reviewers) = 'array' THEN EXISTS (
                         SELECT 1 FROM json_each(i.reviewers) WHERE {} = ?) ELSE FALSE END",
                    fold(&c, "value")
                ),
                Dialect::Postgres => format!(
                    "CASE WHEN jsonb_typeof(CAST(i.reviewers AS jsonb)) = 'array' THEN EXISTS (
                         SELECT 1 FROM jsonb_array_elements_text(CAST(i.reviewers AS jsonb)) AS r(v) WHERE {} = ?)
                     ELSE FALSE END",
                    fold(&c, "r.v")
                ),
            });
            args.push(login.to_ascii_lowercase());
        }
        if let Some(n) = f.number {
            // A number: written into the statement, as the repo ids are.
            wh.push(format!("i.number = {n}"));
        }
        if let Some(t) = f.text.filter(|t| !t.trim().is_empty()) {
            // Content only of a repo that mirrors it: turning it off takes
            // the rows, and this holds while that commits.
            wh.push(format!(
                r"({} LIKE ? ESCAPE '\' OR EXISTS (SELECT 1 FROM forge_content c
                     JOIN linked_repos l ON l.repo_id = c.repo_id AND l.mirror_content = TRUE
                     WHERE c.repo_id = i.repo_id AND c.number = i.number AND {} LIKE ? ESCAPE '\'))",
                fold(&c, "i.title"),
                fold(&c, "c.body")
            ));
            args.extend([like_anywhere(t.trim()), like_anywhere(t.trim())]);
        }
        c.query_map(
            &format!(
                "SELECT {ITEM_COLUMNS} FROM forge_items i WHERE {} ORDER BY i.updated_at DESC, i.id LIMIT {}",
                wh.join(" AND "),
                f.limit
            ),
            &super::sql::list(&args),
            item_row,
        )
    }

    /// One item, if it's in a readable repo.
    pub fn forge_item(&self, r: &Readable, id: &str) -> anyhow::Result<Option<ForgeItem>> {
        if r.repos().is_empty() {
            return Ok(None);
        }
        self.c()
            .query_row(
                &format!("SELECT {ITEM_COLUMNS} FROM forge_items i WHERE i.id = ?1 AND i.repo_id IN ({})", repos_in(r)),
                params![id],
                item_row,
            )
            .optional()
    }

    /// The items among `ids` that are in a readable repo: one read for any
    /// number (a board's rows).
    pub fn forge_items_among(&self, r: &Readable, ids: &[&str]) -> anyhow::Result<Vec<ForgeItem>> {
        if r.repos().is_empty() || ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT {ITEM_COLUMNS} FROM forge_items i WHERE i.repo_id IN ({}) AND {}",
                repos_in(r),
                super::chat::in_json(&c, "i.id", 1)
            ),
            params![super::chat::json_list(ids)?],
            item_row,
        )
    }

    /// The state of every check on the given heads of the readable repos:
    /// one read for any number of heads.
    pub fn forge_checks_on_heads(&self, r: &Readable, heads: &[&str]) -> anyhow::Result<Vec<CheckState>> {
        if r.repos().is_empty() || heads.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!(
                "SELECT repo_id, head_sha, status, conclusion FROM forge_checks WHERE repo_id IN ({}) AND {}",
                repos_in(r),
                super::chat::in_json(&c, "head_sha", 1)
            ),
            params![super::chat::json_list(heads)?],
            |r| Ok(CheckState { repo_id: r.get(0)?, head_sha: r.get(1)?, status: r.get(2)?, conclusion: r.get(3)? }),
        )
    }

    /// A readable repo's checks on a head commit, by name.
    pub fn forge_checks(&self, r: &Readable, repo_id: u64, head_sha: &str) -> anyhow::Result<Vec<ForgeCheck>> {
        if !r.has(repo_id) {
            return Ok(vec![]);
        }
        self.c().query_map(
            "SELECT kind, name, status, conclusion, url, updated_at FROM forge_checks
             WHERE repo_id = ?1 AND head_sha = ?2 ORDER BY name, kind",
            params![repo_id, head_sha],
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
        )
    }

    /// What happened to a readable repo's item, oldest first.
    pub fn forge_events(&self, r: &Readable, repo_id: u64, number: u64) -> anyhow::Result<Vec<ForgeEvent>> {
        if !r.has(repo_id) {
            return Ok(vec![]);
        }
        self.c().query_map(
            "SELECT id, kind, actor_login, review_state, at FROM forge_events
             WHERE repo_id = ?1 AND number = ?2 ORDER BY at, id",
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
        )
    }

    /// The database's clock now, in milliseconds: when a person's access
    /// is asked ([`Db::keep_access`]), so a drop on another machine is
    /// timed by the same clock.
    pub fn clock_ms(&self) -> anyhow::Result<u64> {
        let c = self.c();
        c.query_row(&format!("SELECT {}", clock(&c)), params![], |r| r.get(0))
    }

    /// What GitHub said about a person's access, asked less than `keep` ago
    /// by the database's clock and not dropped since: by `owner/name` (in
    /// lower case), why not, or `None` for access.
    pub fn kept_access(&self, github_id: u64, keep: u64) -> anyhow::Result<HashMap<String, Option<String>>> {
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT a.repo, a.why FROM forge_access a
                 WHERE a.github_id = ?1 AND a.asked_at > {} - ?2 AND NOT EXISTS (
                     SELECT 1 FROM forge_access_drops d WHERE d.at >= a.asked_at AND (
                         (d.scope = 'repo' AND d.subject = a.repo)
                         OR (d.scope = 'owner' AND d.subject = a.owner)
                         OR (d.scope = 'person' AND d.subject = CAST(a.github_id AS TEXT))))",
                clock(&c)
            ),
            params![github_id, keep],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Keep GitHub's answer for a person on `owner/name` (`why` not, or
    /// `None`), asked at `asked_at` ([`Db::clock_ms`]), unless a newer one
    /// is kept already.
    pub fn keep_access(&self, github_id: u64, repo: &str, why: Option<&str>, asked_at: u64) -> anyhow::Result<()> {
        let repo = repo.to_lowercase();
        let owner = repo.split('/').next().unwrap_or_default().to_owned();
        self.c().execute(
            "INSERT INTO forge_access (github_id, repo, owner, why, asked_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (github_id, repo) DO UPDATE SET owner = ?3, why = ?4, asked_at = ?5
             WHERE forge_access.asked_at <= ?5",
            params![github_id, repo, owner, why, asked_at],
        )?;
        Ok(())
    }

    /// Drop every answer asked until now (the database's clock) for a repo
    /// (`owner/name`), an owner (a login) or a person (a GitHub id), on
    /// every machine.
    pub fn drop_access(&self, scope: AccessScope, subject: &str) -> anyhow::Result<()> {
        let scope = match scope {
            AccessScope::Repo => "repo",
            AccessScope::Owner => "owner",
            AccessScope::Person => "person",
        };
        let c = self.c();
        c.execute(
            &format!(
                "INSERT INTO forge_access_drops (scope, subject, at) VALUES (?1, ?2, {})
                 ON CONFLICT (scope, subject) DO UPDATE SET at = CASE WHEN forge_access_drops.at > excluded.at
                     THEN forge_access_drops.at ELSE excluded.at END",
                clock(&c)
            ),
            params![scope, subject.to_lowercase()],
        )?;
        Ok(())
    }

    /// Forget answers and drops older than `keep` by the database's
    /// clock: how many went.
    pub fn prune_access(&self, keep: u64) -> anyhow::Result<usize> {
        let c = self.c();
        let now = clock(&c);
        Ok(c.execute(&format!("DELETE FROM forge_access WHERE asked_at < {now} - ?1"), params![keep])?
            + c.execute(&format!("DELETE FROM forge_access_drops WHERE at < {now} - ?1"), params![keep])?)
    }

    /// GitHub says a repo was made private or public: a linked one says so
    /// from now on. Whether a link changed.
    pub fn set_repo_private(&self, repo_id: u64, private: bool) -> anyhow::Result<bool> {
        let n = self.c().execute(
            "UPDATE linked_repos SET private = ?1 WHERE repo_id = ?2 AND private != ?1",
            params![private, repo_id],
        )?;
        Ok(n > 0)
    }
}

/// A draft (M170): a comment, review or new issue on a linked repo, waiting
/// for a person to send it. `full_name` is its repo's, and `item` and
/// `item_title` the mirrored item it's on, when the mirror has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRow {
    pub id: String,
    pub project_id: String,
    pub author: String,
    pub agent_name: Option<String>,
    pub agent_machine: Option<String>,
    pub repo_id: u64,
    pub full_name: String,
    pub number: Option<u64>,
    pub kind: String,
    pub event: Option<String>,
    pub title: Option<String>,
    pub body: String,
    pub state: String,
    pub sent_at: Option<u64>,
    pub sent_url: Option<String>,
    pub error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub item: Option<String>,
    pub item_title: Option<String>,
}

const DRAFT_COLUMNS: &str = "d.id, d.project_id, d.author, d.agent_name, d.agent_machine, d.repo_id, l.full_name, \
     d.number, d.kind, d.event, d.title, d.body, d.state, d.sent_at, d.sent_url, d.error, d.created_at, d.updated_at, \
     i.id, i.title";

const DRAFT_FROM: &str = "drafts d JOIN linked_repos l ON l.repo_id = d.repo_id \
     LEFT JOIN forge_items i ON i.repo_id = d.repo_id AND i.number = d.number";

fn draft_row(r: &Row) -> anyhow::Result<DraftRow> {
    Ok(DraftRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        author: r.get(2)?,
        agent_name: r.get(3)?,
        agent_machine: r.get(4)?,
        repo_id: r.get(5)?,
        full_name: r.get(6)?,
        number: r.get(7)?,
        kind: r.get(8)?,
        event: r.get(9)?,
        title: r.get(10)?,
        body: r.get(11)?,
        state: r.get(12)?,
        sent_at: r.get(13)?,
        sent_url: r.get(14)?,
        error: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
        item: r.get(18)?,
        item_title: r.get(19)?,
    })
}

/// A draft to keep. `import_key`, a machine's own id for one it uploads,
/// makes a second upload of it answer with the first.
#[derive(Debug, Clone, Copy)]
pub struct NewDraftRow<'a> {
    pub project_id: &'a str,
    pub author: &'a str,
    pub agent_name: Option<&'a str>,
    pub agent_machine: Option<&'a str>,
    pub repo_id: u64,
    pub number: Option<u64>,
    pub kind: &'a str,
    pub event: Option<&'a str>,
    pub title: Option<&'a str>,
    pub body: &'a str,
    pub import_key: Option<&'a str>,
    pub now: u64,
}

/// What a list of drafts is narrowed to, within the readable repos.
#[derive(Debug, Clone, Copy, Default)]
pub struct DraftFilter<'a> {
    pub project: Option<&'a str>,
    pub repo_id: Option<u64>,
    pub number: Option<u64>,
    pub author: Option<&'a str>,
    /// `None`: every state.
    pub state: Option<&'a str>,
    pub limit: u32,
}

/// Who discards a draft: one who may read its repo (the viewer check's
/// [`Readable`]), or its author, who may whether or not they still can.
#[derive(Debug, Clone, Copy)]
pub enum Discarder<'a> {
    Reader(&'a Readable),
    Author(&'a str),
}

/// How long a send holds its draft (M168): a waiting or failed draft with
/// `sent_at` set is being sent since then ([`Db::hold_draft`]), and nobody
/// sends or discards it until the send ends or this has passed (a platform
/// machine that stopped mid-send lets go of it). The wire says `sending`.
pub const SENDING_MS: u64 = 2 * 60 * 1000;

/// How a send ended ([`Db::end_send`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendEnd<'a> {
    /// On GitHub, at this URL when it said.
    Sent(Option<&'a str>),
    /// GitHub refused it, or the call failed: `failed`, to send again.
    Failed(&'a str),
    /// It didn't go anywhere: `waiting` again, saying why.
    Waits(&'a str),
}

impl DraftRow {
    /// Whether a send holds it now ([`SENDING_MS`]).
    pub fn sending(&self, now: u64) -> bool {
        self.state != "sent" && self.sent_at.is_some_and(|at| at + SENDING_MS > now)
    }
}

/// Drafts (M170): every read takes the [`Readable`] the viewer check made,
/// as items' do, and returns drafts on its repos only.
impl Db {
    /// A draft's repo and author, and nothing else of it: what the viewer
    /// check needs to decide on it.
    pub fn draft_target(&self, id: &str) -> anyhow::Result<Option<(u64, String)>> {
        self.c()
            .query_row("SELECT repo_id, author FROM drafts WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
    }

    /// The repos an account's drafts are on: what a list of its own checks.
    pub fn draft_repos(&self, author: &str) -> anyhow::Result<Vec<u64>> {
        self.c().query_map("SELECT DISTINCT repo_id FROM drafts WHERE author = ?1", params![author], |r| r.get(0))
    }

    /// Keep a draft on a readable repo: it, and whether it's new. One with
    /// an `import_key` kept before is answered with that one, unless it's
    /// another author's ([`Taken`](super::Taken)). `None` when the repo
    /// isn't readable.
    pub fn add_draft(&self, r: &Readable, d: &NewDraftRow<'_>) -> anyhow::Result<Option<(DraftRow, bool)>> {
        if !r.has(d.repo_id) {
            return Ok(None);
        }
        let id = format!("dft_{}", hex::encode(arugula_e2e::random::<16>()));
        let c = self.c();
        let n = c.execute(
            "INSERT INTO drafts (id, project_id, author, agent_name, agent_machine, repo_id, number, kind, event, title,
                 body, import_key, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
             ON CONFLICT (import_key) DO NOTHING",
            params![
                id,
                d.project_id,
                d.author,
                d.agent_name,
                d.agent_machine,
                d.repo_id,
                d.number,
                d.kind,
                d.event,
                d.title,
                d.body,
                d.import_key,
                d.now
            ],
        )?;
        let row = if n == 1 {
            c.query_row(&format!("SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE d.id = ?1"), params![id], draft_row)?
        } else {
            c.query_row(
                &format!("SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE d.import_key = ?1"),
                params![d.import_key],
                draft_row,
            )?
        };
        if n != 1 && (row.author != d.author || !r.has(row.repo_id)) {
            return Err(super::Taken.into());
        }
        Ok(Some((row, n == 1)))
    }

    /// The drafts on the readable repos that match, oldest first.
    pub fn drafts(&self, r: &Readable, f: &DraftFilter<'_>) -> anyhow::Result<Vec<DraftRow>> {
        if r.repos().is_empty() {
            return Ok(vec![]);
        }
        let mut wh = vec![format!("d.repo_id IN ({})", repos_in(r))];
        let (project, author, state) = (f.project.unwrap_or(""), f.author.unwrap_or(""), f.state.unwrap_or(""));
        let mut args: Vec<&dyn super::sql::Param> = Vec::new();
        if f.project.is_some() {
            wh.push("d.project_id = ?".into());
            args.push(&project);
        }
        if let Some(repo) = &f.repo_id {
            wh.push("d.repo_id = ?".into());
            args.push(repo);
        }
        if let Some(number) = &f.number {
            wh.push("d.number = ?".into());
            args.push(number);
        }
        if f.author.is_some() {
            wh.push("d.author = ?".into());
            args.push(&author);
        }
        if f.state.is_some() {
            wh.push("d.state = ?".into());
            args.push(&state);
        }
        self.c().query_map(
            &format!(
                "SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE {} ORDER BY d.created_at, d.id LIMIT {}",
                wh.join(" AND "),
                f.limit
            ),
            &args,
            draft_row,
        )
    }

    /// One draft, if it's on a readable repo.
    pub fn draft(&self, r: &Readable, id: &str) -> anyhow::Result<Option<DraftRow>> {
        if r.repos().is_empty() {
            return Ok(None);
        }
        self.c()
            .query_row(
                &format!("SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE d.id = ?1 AND d.repo_id IN ({})", repos_in(r)),
                params![id],
                draft_row,
            )
            .optional()
    }

    /// Discard a waiting (or failed) draft: its state after, or `None`
    /// when there's no such draft that `who` may discard. A sent one stays
    /// sent, and one discarded already stays as it was.
    pub fn discard_draft(&self, who: Discarder<'_>, id: &str, now: u64) -> anyhow::Result<Option<String>> {
        let (may, author) = match who {
            Discarder::Reader(r) if r.repos().is_empty() => return Ok(None),
            Discarder::Reader(r) => (format!("repo_id IN ({})", repos_in(r)), None),
            Discarder::Author(a) => ("author = ?".to_owned(), Some(a)),
        };
        let mut c = self.c();
        let tx = c.immediate()?;
        // Not while a send holds it (M168): that one is `sending`.
        let held = now.saturating_sub(SENDING_MS);
        let mut p: Vec<&dyn super::sql::Param> = vec![&now, &held, &id];
        if let Some(a) = &author {
            p.push(a);
        }
        tx.execute(
            &format!(
                "UPDATE drafts SET state = 'discarded', updated_at = ? WHERE (sent_at IS NULL OR sent_at < ?)
                 AND id = ? AND state IN ('waiting', 'failed') AND {may}"
            ),
            &p,
        )?;
        let state = tx
            .query_row(
                &format!(
                    "SELECT CASE WHEN state <> 'sent' AND sent_at >= ? THEN 'sending' ELSE state END
                     FROM drafts WHERE id = ? AND {may}"
                ),
                &p[1..],
                |r| r.get(0),
            )
            .optional()?;
        tx.commit()?;
        Ok(state)
    }
}

/// Sends (M168): a send holds its draft while it goes ([`SENDING_MS`]), and
/// ends it once.
impl Db {
    /// Hold an author's waiting (or failed) draft on a readable repo for a
    /// send: the draft, and the hold (its `sent_at`, which ends it). `None`
    /// when it isn't theirs, isn't readable, was sent or discarded, or
    /// another send holds it.
    pub fn hold_draft(&self, r: &Readable, id: &str, author: &str, now: u64) -> anyhow::Result<Option<DraftRow>> {
        if r.repos().is_empty() {
            return Ok(None);
        }
        let held = now.saturating_sub(SENDING_MS);
        let mut c = self.c();
        let tx = c.immediate()?;
        let n = tx.execute(
            &format!(
                "UPDATE drafts SET sent_at = ?1, error = NULL, updated_at = ?1
                 WHERE id = ?2 AND author = ?3 AND state IN ('waiting', 'failed') AND (sent_at IS NULL OR sent_at < ?4)
                 AND repo_id IN ({})",
                repos_in(r)
            ),
            params![now, id, author, held],
        )?;
        let row = if n == 1 {
            Some(tx.query_row(
                &format!("SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE d.id = ?1"),
                params![id],
                draft_row,
            )?)
        } else {
            None
        };
        tx.commit()?;
        Ok(row)
    }

    /// End the send that holds a draft since `held`: the draft after, or
    /// `None` when that hold isn't its any more (it ran out and another
    /// send took it).
    pub fn end_send(&self, id: &str, held: u64, end: SendEnd<'_>, now: u64) -> anyhow::Result<Option<DraftRow>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let (state, sent_at, url, error) = match end {
            SendEnd::Sent(url) => ("sent", Some(now), url, None),
            SendEnd::Failed(e) => ("failed", None, None, Some(e)),
            SendEnd::Waits(e) => ("waiting", None, None, Some(e)),
        };
        let n = tx.execute(
            "UPDATE drafts SET state = ?1, sent_at = ?2, sent_url = ?3, error = ?4, updated_at = ?5
             WHERE id = ?6 AND sent_at = ?7 AND state IN ('waiting', 'failed')",
            params![state, sent_at, url, error, now, id, held],
        )?;
        let row = if n == 1 {
            Some(tx.query_row(
                &format!("SELECT {DRAFT_COLUMNS} FROM {DRAFT_FROM} WHERE d.id = ?1"),
                params![id],
                draft_row,
            )?)
        } else {
            None
        };
        tx.commit()?;
        Ok(row)
    }
}

/// An installation as GitHub says it now, not suspended or deleted.
fn upsert_installation(c: &impl Sql, i: &Installation, now: u64) -> anyhow::Result<()> {
    c.execute(
        "INSERT INTO github_installations
             (installation_id, github_account_id, github_login, github_account_type, permissions, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (installation_id) DO UPDATE SET github_account_id = ?2, github_login = ?3,
             github_account_type = ?4, permissions = ?5, suspended_at = NULL, deleted_at = NULL, updated_at = ?6",
        params![i.id, i.account_id, i.login, i.account_type, i.permissions, now],
    )?;
    Ok(())
}

/// What deleting an account takes from the mirror, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// its drafts and inbox, and its name as who linked a repo. A repo linked
/// to a project that went with it is unlinked now, and M163's purge job
/// takes its rows. The token log keeps its mints (`tables.toml`).
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted, now: u64) -> anyhow::Result<()> {
    let account = &d.account;
    c.execute("DELETE FROM drafts WHERE author = ?1 OR project_id NOT IN (SELECT id FROM projects)", params![account])?;
    c.execute("DELETE FROM inbox_items WHERE account = ?1", params![account])?;
    c.execute("UPDATE linked_repos SET linked_by = NULL WHERE linked_by = ?1", params![account])?;
    c.execute(
        "UPDATE linked_repos SET unlinked_at = ?1, purge_after = ?1
         WHERE unlinked_at IS NULL AND project_id NOT IN (SELECT id FROM projects)",
        params![now],
    )?;
    super::trackers::erase_account(c, account, now)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        Db,
        sql::{Sql, params},
    };
    use crate::Deleted;

    /// A delivery is taken once, until it's forgotten or a week old.
    #[test]
    fn deliveries_once_for_a_week() {
        let db = Db::test();
        assert!(db.first_delivery("d1", "pull_request", 1_000).unwrap());
        assert!(!db.first_delivery("d1", "pull_request", 2_000).unwrap(), "a redelivery");
        db.forget_delivery("d1").unwrap();
        assert!(db.first_delivery("d1", "pull_request", 3_000).unwrap(), "forgotten: taken again");
        assert!(db.first_delivery("d2", "issues", 3_000 + super::DELIVERIES_KEPT_MS).unwrap());
        assert_eq!(db.prune_deliveries(3_001 + super::DELIVERIES_KEPT_MS).unwrap(), 1);
        assert!(db.first_delivery("d1", "pull_request", 5_000).unwrap(), "a week on, it's new");
        assert!(!db.first_delivery("d2", "issues", 5_000).unwrap());
    }

    /// A mint is a row with what it was scoped to, and a year on it goes.
    #[test]
    fn mints_are_logged_for_a_year() {
        use super::{MINTS_KEPT_MS, MintKind, NewMint};
        let db = Db::test();
        let perms = serde_json::json!({ "metadata": "read" });
        let m = NewMint {
            kind: MintKind::Installation,
            installation_id: Some(11),
            repo_id: Some(101),
            permissions: &perms,
            caller: "a1",
            purpose: "read",
            at: 1_000,
        };
        let id = db.log_mint(&m).unwrap();
        assert!(id.starts_with("gtm_"), "{id}");
        db.log_mint(&NewMint { kind: MintKind::User, installation_id: None, at: 2_000, ..m }).unwrap();
        let rows = db.token_mints(0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].kind.as_str(), rows[0].installation_id, rows[0].repo_id, rows[0].permissions.as_str()),
            ("installation", Some(11), Some(101), r#"{"metadata":"read"}"#)
        );
        assert_eq!((rows[1].kind.as_str(), rows[1].installation_id), ("user", None));
        assert_eq!(db.prune_mints(1_500 + MINTS_KEPT_MS).unwrap(), 1);
        assert_eq!(db.token_mints(0).unwrap().len(), 1);
    }

    fn link(repo: u64, project: &str) -> super::LinkedRepo {
        super::LinkedRepo {
            repo_id: repo,
            full_name: format!("acme/r{repo}"),
            private: false,
            project_id: project.into(),
            installation_id: 9,
            linked_by: None,
            linked_at: 0,
            unlinked_at: None,
            purge_after: None,
            mirror_content: false,
        }
    }

    fn install() -> super::Installation {
        super::Installation {
            id: 9,
            account_id: 500,
            login: "acme".into(),
            account_type: "Organization".into(),
            permissions: "{}".into(),
        }
    }

    /// The race on Postgres, as the second linker meets it: it looked and
    /// found no link, then another project's commits before its write.
    /// The write leaves that link alone, and the answer is "elsewhere".
    #[test]
    fn a_link_written_meanwhile_by_another_project_stays_its() {
        let db = Db::test();
        assert!(matches!(db.link_repo(&link(1, "prj_a"), &install(), "alice", 10).unwrap(), super::Linked::Now { .. }));
        {
            let mut c = db.c();
            let tx = c.immediate().unwrap();
            assert_eq!(super::upsert_link(&tx, &link(1, "prj_b"), 9, "bob", 20).unwrap(), None);
            tx.commit().unwrap();
        }
        let kept = db.linked_repo(1).unwrap().unwrap();
        assert_eq!((kept.project_id.as_str(), kept.linked_by.as_deref(), kept.linked_at), ("prj_a", Some("alice"), 10));
        // The same project's write still links it again.
        db.unlink_repo("prj_a", "acme/r1", 30).unwrap();
        let mut c = db.c();
        let tx = c.immediate().unwrap();
        let again = super::upsert_link(&tx, &link(1, "prj_a"), 9, "carol", 40).unwrap().unwrap();
        tx.commit().unwrap();
        assert_eq!((again.unlinked_at, again.linked_by.as_deref()), (None, Some("carol")));
    }

    /// Two projects link a repo nobody had, at once, from two machines
    /// (Postgres; SQLite's writers take turns): one links it, the other is
    /// told it's elsewhere, and the link is the first's.
    #[test]
    fn two_projects_linking_at_once_one_wins() {
        let db = Db::test();
        let Some(other) = db.second() else { return };
        for repo in 100..120u64 {
            let go = std::sync::Barrier::new(2);
            let (a, b) = std::thread::scope(|s| {
                let a = s.spawn(|| {
                    go.wait();
                    db.link_repo(&link(repo, "prj_a"), &install(), "alice", 1).unwrap()
                });
                let b = s.spawn(|| {
                    go.wait();
                    other.link_repo(&link(repo, "prj_b"), &install(), "bob", 1).unwrap()
                });
                (a.join().unwrap(), b.join().unwrap())
            });
            let stored = db.linked_repo(repo).unwrap().unwrap().project_id;
            match (a, b) {
                (super::Linked::Now { repo: r, .. }, super::Linked::Elsewhere)
                | (super::Linked::Elsewhere, super::Linked::Now { repo: r, .. }) => {
                    assert_eq!(r.project_id, stored, "repo {repo}")
                }
                (a, b) => panic!("repo {repo}: {a:?} and {b:?}"),
            }
        }
    }

    fn count(db: &Db, sql: &str) -> u64 {
        db.c().query_row(sql, params![], |r| r.get(0)).unwrap()
    }

    /// Deleting an account takes its drafts and inbox and its name on a
    /// link, and unlinks what was linked to its personal project, for the
    /// purge. A team's repo stays linked, and others' drafts stay.
    #[test]
    fn deleting_an_account_takes_its_drafts_and_inbox() {
        let db = Db::test();
        let mine = db.personal_project("alice", 1).unwrap();
        db.c()
            .execute_batch(&format!(
                "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES ('prj_t', 't1', 't1', TRUE, 1);
                 INSERT INTO linked_repos (repo_id, full_name, project_id, installation_id, linked_by, linked_at)
                     VALUES (1, 'octo/dots', '{mine}', 9, 'alice', 1), (2, 'acme/app', 'prj_t', 9, 'alice', 1);
                 INSERT INTO drafts (id, project_id, author, repo_id, number, kind, body, created_at, updated_at)
                     VALUES ('dft_a', 'prj_t', 'alice', 2, 7, 'comment', 'lgtm', 1, 1),
                            ('dft_b', 'prj_t', 'bob', 2, 7, 'comment', 'nit', 1, 1);
                 INSERT INTO inbox_items (account, item, reason, created_at, updated_at)
                     VALUES ('alice', 'fi_1', 'review_requested', 1, 1), ('bob', 'fi_1', 'review_requested', 1, 1);"
            ))
            .unwrap();
        let gone = Deleted { account: "alice".into(), machines: vec![], disbanded: vec![] };
        for _ in 0..2 {
            let mut c = db.c();
            let tx = c.transaction().unwrap();
            super::super::projects::erase_account(&tx, &gone).unwrap();
            super::erase_account(&tx, &gone, 50).unwrap();
            tx.commit().unwrap();
        }
        assert!(db.mentions("alice").is_empty(), "{:?}", db.mentions("alice"));
        assert_eq!(count(&db, "SELECT count(*) FROM drafts WHERE author = 'bob'"), 1);
        assert_eq!(count(&db, "SELECT count(*) FROM inbox_items"), 1);
        assert_eq!(count(&db, "SELECT count(*) FROM linked_repos WHERE purge_after = 50"), 1, "its own repo");
        assert_eq!(count(&db, "SELECT count(*) FROM linked_repos WHERE repo_id = 2 AND unlinked_at IS NULL"), 1);
    }
}
