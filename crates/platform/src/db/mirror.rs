//! The forge mirror's rows (M164, #1210): a linked repo's items, checks
//! and events, as the App's webhooks and the repo's poller write them, and
//! the poller's lease on the repo (`linked_repos.lease`, `platform_jobs`'
//! way). Nothing here hands items to a person: reading them is M165's,
//! behind its per-viewer check.
//!
//! - **Only a linked repo.** Every write runs in [`Db::mirror`], which
//!   checks in the same transaction that the repo is linked and not
//!   unlinked (and, for the poller, that it still holds the lease). An
//!   unlink stops ingest at the next write.
//! - **Out of order.** A row is only overwritten by an answer whose
//!   GitHub `updated_at` is newer than the row's: a late redelivery, or a
//!   list read before a webhook landed, leaves the newer row be. GitHub's
//!   times are whole seconds, so two answers in the same second that
//!   disagree are a **tie**: the row stays, and the poller reads the item
//!   (or the head's checks) again first thing at the repo's next turn
//!   (`forge_refetch`), whose answer is taken ([`Source::Fetched`]).
//! - **Metadata only.** No bodies or comment text: the rows have nowhere
//!   to put them (`forge_content` is M166's).

use anyhow::Result;
use sha2::{Digest, Sha256};

use super::{
    Db,
    forge::{ForgeCheck, Installation},
    migrate::Dialect,
    sql::{Optional as _, Row, Sql, Tx, params},
};

/// How long a poller's claim leases its repo: what a machine that dies
/// mid-turn costs before another takes the repo. A turn renews it between
/// its reads.
pub const POLL_LEASE_MS: u64 = 120_000;

/// A PR or an issue, as `forge_items` keeps it. `labels`, `assignees` and
/// `reviewers` are JSON lists of plain strings (names, logins, a team as
/// `org/slug`), sorted, so a list that GitHub orders differently isn't a
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub repo_id: u64,
    pub number: u64,
    /// `pr` or `issue`.
    pub kind: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    pub title: String,
    pub labels: String,
    pub author_login: Option<String>,
    pub author_id: Option<u64>,
    pub assignees: String,
    pub reviewers: String,
    pub draft: bool,
    /// GitHub's `mergeable_state`, when the answer has it (a PR's own
    /// read or webhook; not a list). `None` keeps what the row has.
    pub merge_state: Option<String>,
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    pub base_ref: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

const ITEM_COLUMNS: &str = "repo_id, number, kind, state, title, labels, author_login, author_id, assignees, \
                            reviewers, draft, merge_state, head_ref, head_sha, base_ref, created_at, updated_at";

fn item_row(r: &Row) -> Result<ItemRow> {
    Ok(ItemRow {
        repo_id: r.get(0)?,
        number: r.get(1)?,
        kind: r.get(2)?,
        state: r.get(3)?,
        title: r.get(4)?,
        labels: r.get(5)?,
        author_login: r.get(6)?,
        author_id: r.get(7)?,
        assignees: r.get(8)?,
        reviewers: r.get(9)?,
        draft: r.get(10)?,
        merge_state: r.get(11)?,
        head_ref: r.get(12)?,
        head_sha: r.get(13)?,
        base_ref: r.get(14)?,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
    })
}

impl ItemRow {
    /// The fields `self` (the answer) says differently from `was` (the
    /// row): GitHub's `updated_at` aside, which moves with a comment.
    fn differs(&self, was: &ItemRow) -> Vec<&'static str> {
        let mut d = Vec::new();
        let mut field = |name, same: bool| {
            if !same {
                d.push(name);
            }
        };
        field("kind", self.kind == was.kind);
        field("state", self.state == was.state);
        field("title", self.title == was.title);
        field("labels", self.labels == was.labels);
        field("author", self.author_login == was.author_login && self.author_id == was.author_id);
        field("assignees", self.assignees == was.assignees);
        field("reviewers", self.reviewers == was.reviewers);
        field("draft", self.draft == was.draft);
        field("merge_state", self.merge_state.is_none() || self.merge_state == was.merge_state);
        field("head", self.head_ref == was.head_ref && self.head_sha == was.head_sha);
        field("base", self.base_ref == was.base_ref);
        d
    }
}

/// A check run or a commit status on a head, as `forge_checks` keeps it.
/// `status` and `conclusion` are a check run's own; a commit status's
/// `pending` is `pending` with no conclusion, and its other states are
/// `completed` with the state as the conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRow {
    pub repo_id: u64,
    pub head_sha: String,
    /// `check_run` or `status`.
    pub kind: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    pub updated_at: u64,
}

/// Something that happened to an item, as `forge_events` keeps it: a
/// review (and its state), a comment (that one was made, never what it
/// says), a state change, a review request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub repo_id: u64,
    pub number: u64,
    pub kind: String,
    pub github_id: Option<u64>,
    pub actor_login: Option<String>,
    pub actor_id: Option<u64>,
    pub review_state: Option<String>,
    pub at: u64,
}

impl EventRow {
    /// Its id, from what it is: GitHub's id where there's one, else the
    /// item, kind, actor and time. The same event, redelivered or read
    /// again by the poller, is the same row.
    fn id(&self) -> String {
        let key = match self.github_id {
            Some(g) => format!("{}/{}/{g}", self.repo_id, self.kind),
            None => {
                format!("{}#{}/{}/{}/{}", self.repo_id, self.number, self.kind, self.actor_id.unwrap_or(0), self.at)
            }
        };
        format!("fev_{}", &hex::encode(Sha256::digest(key.as_bytes()))[..32])
    }
}

/// What a write did to a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrote {
    /// There was none.
    New,
    /// It said otherwise: these fields.
    Changed(Vec<&'static str>),
    /// It said the same (its time may have moved).
    Same,
    /// The row is newer than the answer, and stays.
    Older,
    /// The answer is from the same second and says otherwise: the row
    /// stays, and is read again ([`refetch`]).
    Tie,
}

/// Where an answer came from: a webhook or a list, which a tie doesn't
/// overwrite, or the poller's own read of the one item or head, which a
/// tie does: it was asked after the answers that tied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Pushed,
    Fetched,
}

impl Wrote {
    /// Whether the row was missing or wrong.
    pub fn fixed(&self) -> bool {
        matches!(self, Wrote::New | Wrote::Changed(_))
    }
}

/// Write an item, unless the row is newer, or (but for a
/// [`Source::Fetched`] answer) from the same second.
pub fn upsert_item(c: &impl Sql, it: &ItemRow, from: Source, now: u64) -> Result<Wrote> {
    let was = c
        .query_row(
            &format!("SELECT {ITEM_COLUMNS} FROM forge_items WHERE repo_id = ?1 AND number = ?2{}", c.for_update()),
            params![it.repo_id, it.number],
            item_row,
        )
        .optional()?;
    let wrote = match &was {
        None => Wrote::New,
        Some(w) if w.updated_at > it.updated_at => return Ok(Wrote::Older),
        Some(w) => match it.differs(w) {
            d if d.is_empty() && w.updated_at == it.updated_at => return Ok(Wrote::Same),
            d if d.is_empty() => Wrote::Same,
            _ if w.updated_at == it.updated_at && from == Source::Pushed => {
                refetch(c, it.repo_id, &w.kind, &it.number.to_string(), now)?;
                return Ok(Wrote::Tie);
            }
            d => Wrote::Changed(d),
        },
    };
    let fetched = from == Source::Fetched;
    let id = format!("fi_{}", hex::encode(arugula_e2e::random::<16>()));
    c.execute(
        "INSERT INTO forge_items (id, repo_id, number, kind, state, title, labels, author_login, author_id,
             assignees, reviewers, draft, merge_state, head_ref, head_sha, base_ref, created_at, updated_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT (repo_id, number) DO UPDATE SET kind = excluded.kind, state = excluded.state,
             title = excluded.title, labels = excluded.labels, author_login = excluded.author_login,
             author_id = excluded.author_id, assignees = excluded.assignees, reviewers = excluded.reviewers,
             draft = excluded.draft, merge_state = COALESCE(excluded.merge_state, forge_items.merge_state),
             head_ref = excluded.head_ref, head_sha = excluded.head_sha, base_ref = excluded.base_ref,
             created_at = excluded.created_at, updated_at = excluded.updated_at, synced_at = excluded.synced_at
         WHERE excluded.updated_at > forge_items.updated_at
             OR (excluded.updated_at = forge_items.updated_at AND ?20)",
        params![
            id,
            it.repo_id,
            it.number,
            it.kind,
            it.state,
            it.title,
            it.labels,
            it.author_login,
            it.author_id,
            it.assignees,
            it.reviewers,
            it.draft,
            it.merge_state,
            it.head_ref,
            it.head_sha,
            it.base_ref,
            it.created_at,
            it.updated_at,
            now,
            fetched
        ],
    )?;
    Ok(wrote)
}

/// Write a check, unless the row is newer, or (but for a
/// [`Source::Fetched`] answer) from the same second.
pub fn upsert_check(c: &impl Sql, ck: &CheckRow, from: Source, now: u64) -> Result<Wrote> {
    let was = c
        .query_row(
            &format!(
                "SELECT status, conclusion, url, updated_at FROM forge_checks
                 WHERE repo_id = ?1 AND head_sha = ?2 AND kind = ?3 AND name = ?4{}",
                c.for_update()
            ),
            params![ck.repo_id, ck.head_sha, ck.kind, ck.name],
            |r| Ok((r.get::<String>(0)?, r.get::<Option<String>>(1)?, r.get::<Option<String>>(2)?, r.get::<u64>(3)?)),
        )
        .optional()?;
    let wrote = match was {
        None => Wrote::New,
        Some((.., at)) if at > ck.updated_at => return Ok(Wrote::Older),
        Some((status, conclusion, url, at)) => {
            let mut d = Vec::new();
            if status != ck.status || conclusion != ck.conclusion {
                d.push("status");
            }
            if url != ck.url {
                d.push("url");
            }
            match d.is_empty() {
                true if at == ck.updated_at => return Ok(Wrote::Same),
                true => Wrote::Same,
                false if at == ck.updated_at && from == Source::Pushed => {
                    refetch(c, ck.repo_id, "head", &ck.head_sha, now)?;
                    return Ok(Wrote::Tie);
                }
                false => Wrote::Changed(d),
            }
        }
    };
    c.execute(
        "INSERT INTO forge_checks (repo_id, head_sha, kind, name, status, conclusion, url, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (repo_id, head_sha, kind, name) DO UPDATE SET status = excluded.status,
             conclusion = excluded.conclusion, url = excluded.url, updated_at = excluded.updated_at
         WHERE excluded.updated_at > forge_checks.updated_at
             OR (excluded.updated_at = forge_checks.updated_at AND ?9)",
        params![
            ck.repo_id,
            ck.head_sha,
            ck.kind,
            ck.name,
            ck.status,
            ck.conclusion,
            ck.url,
            ck.updated_at,
            from == Source::Fetched
        ],
    )?;
    Ok(wrote)
}

/// Write an event once ([`EventRow::id`]). A review's state follows
/// GitHub's (dismissed later), but a dismissed review stays dismissed.
pub fn add_event(c: &impl Sql, e: &EventRow) -> Result<Wrote> {
    let id = e.id();
    let was = c
        .query_row(
            &format!("SELECT review_state FROM forge_events WHERE id = ?1{}", c.for_update()),
            params![id],
            |r| r.get::<Option<String>>(0),
        )
        .optional()?;
    let wrote = match &was {
        None => Wrote::New,
        Some(s) if *s == e.review_state || s.as_deref() == Some("dismissed") => return Ok(Wrote::Same),
        Some(_) => Wrote::Changed(vec!["review_state"]),
    };
    c.execute(
        "INSERT INTO forge_events (id, repo_id, number, kind, github_id, actor_login, actor_id, review_state, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (id) DO UPDATE SET review_state = CASE WHEN forge_events.review_state = 'dismissed'
             THEN forge_events.review_state ELSE excluded.review_state END",
        params![id, e.repo_id, e.number, e.kind, e.github_id, e.actor_login, e.actor_id, e.review_state, e.at],
    )?;
    Ok(wrote)
}

/// Read `kind` (`pr`, `issue`, `head`) `key` (a number, a SHA) again
/// at the repo's next turn.
pub fn refetch(c: &impl Sql, repo_id: u64, kind: &str, key: &str, now: u64) -> Result<()> {
    c.execute(
        "INSERT INTO forge_refetch (repo_id, kind, key, at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
        params![repo_id, kind, key, now],
    )?;
    Ok(())
}

/// It was read again.
pub fn refetched(c: &impl Sql, repo_id: u64, kind: &str, key: &str) -> Result<()> {
    c.execute("DELETE FROM forge_refetch WHERE repo_id = ?1 AND kind = ?2 AND key = ?3", params![repo_id, kind, key])?;
    Ok(())
}

/// An issue deleted, or transferred to another repo: its row, its
/// events, the checks of a head no other item has, its inbox rows and
/// any read again it waited for. Its drafts stay (M170's): a send to it
/// fails and says so. Whether there was one.
pub fn delete_item(c: &impl Sql, repo_id: u64, number: u64) -> Result<bool> {
    let found = c
        .query_row(
            &format!("SELECT id, head_sha FROM forge_items WHERE repo_id = ?1 AND number = ?2{}", c.for_update()),
            params![repo_id, number],
            |r| Ok((r.get::<String>(0)?, r.get::<Option<String>>(1)?)),
        )
        .optional()?;
    let Some((id, head)) = found else { return Ok(false) };
    c.execute("DELETE FROM inbox_items WHERE item = ?1", params![id])?;
    c.execute("DELETE FROM forge_events WHERE repo_id = ?1 AND number = ?2", params![repo_id, number])?;
    if let Some(sha) = head {
        c.execute(
            "DELETE FROM forge_checks WHERE repo_id = ?1 AND head_sha = ?2
             AND NOT EXISTS (SELECT 1 FROM forge_items WHERE repo_id = ?1 AND head_sha = ?2 AND number != ?3)",
            params![repo_id, sha, number],
        )?;
    }
    c.execute(
        "DELETE FROM forge_refetch WHERE repo_id = ?1 AND kind IN ('pr', 'issue') AND key = ?2",
        params![repo_id, number.to_string()],
    )?;
    c.execute("DELETE FROM forge_items WHERE id = ?1", params![id])?;
    Ok(true)
}

/// Checks on a commit no item has as its head, from before `before`: a
/// status that came before its PR's new head (kept a while, so the
/// poller doesn't find it missing), an old head's, a branch's. How many.
pub fn prune_checks(c: &impl Sql, repo_id: u64, before: u64) -> Result<usize> {
    c.execute(
        "DELETE FROM forge_checks WHERE repo_id = ?1 AND updated_at < ?2 AND head_sha NOT IN
             (SELECT head_sha FROM forge_items WHERE repo_id = ?1 AND head_sha IS NOT NULL)",
        params![repo_id, before],
    )
}

/// An open PR whose merge state the poller may read again
/// ([`Db::merge_candidates`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeCandidate {
    pub number: u64,
    /// GitHub's `mergeable_state`, as the row has it.
    pub merge_state: Option<String>,
    /// When the row was last written, or its merge state read again.
    pub synced_at: u64,
    pub head_sha: String,
    pub checks: Vec<ForgeCheck>,
}

/// The poller read a PR again for its merge state at `now`: its row is as
/// fresh as that, whether or not the answer changed it (the next
/// rechecks go stalest first).
pub fn merge_checked(c: &impl Sql, repo_id: u64, number: u64, now: u64) -> Result<()> {
    c.execute(
        "UPDATE forge_items SET synced_at = ?1 WHERE repo_id = ?2 AND number = ?3 AND synced_at < ?1",
        params![now, repo_id, number],
    )?;
    Ok(())
}

/// GitHub's name and privacy for a linked repo, as the poller read them.
pub fn refresh_link(c: &impl Sql, repo_id: u64, full_name: &str, private: bool) -> Result<()> {
    c.execute(
        "UPDATE linked_repos SET full_name = ?1, private = ?2 WHERE repo_id = ?3",
        params![full_name, private, repo_id],
    )?;
    Ok(())
}

/// A poller's claim on a linked repo, and what it did last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollJob {
    pub repo_id: u64,
    pub full_name: String,
    pub installation_id: u64,
    pub linked_at: u64,
    pub backfilled_at: Option<u64>,
    pub reconciled_at: Option<u64>,
    /// Where the last turn stopped, when it ran out of pages.
    pub cursor: Option<String>,
    /// Turns that failed in a row.
    pub failures: u64,
    /// A backfill or a reconcile is due (else only reads again, or the
    /// rest of a cursor's).
    pub due: bool,
    pub lease: String,
}

/// How a turn ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    /// Backfilled (`true`) or reconciled.
    Reconciled(bool),
    /// Only read again what tied.
    Refetched,
    /// Ran out of pages: carry on from here.
    Cursor(String),
}

/// The longest a failing repo waits.
pub const MAX_BACKOFF_MS: u64 = 3_600_000;

/// What a claim's `SELECT` ends with: on Postgres it passes over a repo
/// another machine's claim holds (`platform_jobs`' way). SQLite's
/// `BEGIN IMMEDIATE` makes claims take turns.
fn skip_locked(c: &impl Sql) -> &'static str {
    match c.dialect() {
        Dialect::Sqlite => "",
        Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
    }
}

impl Db {
    /// Run `f` on `repo_id`'s mirror rows in one transaction, if the repo
    /// is linked and not unlinked, and (`lease`) its poller's claim is
    /// still that one. `None`, and nothing written, otherwise.
    pub fn mirror<T>(
        &self,
        repo_id: u64,
        lease: Option<&str>,
        f: impl FnOnce(&Tx<'_, '_>) -> Result<T>,
    ) -> Result<Option<T>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let sql = "SELECT repo_id FROM linked_repos WHERE repo_id = ?1 AND unlinked_at IS NULL";
        let live = match lease {
            None => tx.query_row(&format!("{sql}{}", tx.for_update()), params![repo_id], |r| r.get::<u64>(0)),
            Some(l) => tx.query_row(&format!("{sql} AND lease = ?2{}", tx.for_update()), params![repo_id, l], |r| {
                r.get::<u64>(0)
            }),
        }
        .optional()?;
        if live.is_none() {
            return Ok(None);
        }
        let out = f(&tx)?;
        tx.commit()?;
        Ok(Some(out))
    }

    /// Claim a linked repo whose poller is due: something to read again
    /// (first), a turn to carry on, never backfilled, or not reconciled for
    /// `every_ms`. Two machines never claim the same one (a lease of
    /// [`POLL_LEASE_MS`]).
    pub fn claim_poll(&self, now: u64, every_ms: u64) -> Result<Option<PollJob>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let due = now.saturating_sub(every_ms);
        let found = tx
            .query_row(
                &format!(
                    "SELECT repo_id, full_name, installation_id, linked_at, backfilled_at, reconciled_at,
                         poll_cursor, poll_failures
                     FROM linked_repos l
                     WHERE unlinked_at IS NULL AND lease_until <= ?1
                         AND (backfilled_at IS NULL OR reconciled_at IS NULL OR reconciled_at <= ?2
                             OR poll_cursor IS NOT NULL
                             OR EXISTS (SELECT 1 FROM forge_refetch f WHERE f.repo_id = l.repo_id))
                     ORDER BY CASE WHEN EXISTS (SELECT 1 FROM forge_refetch f WHERE f.repo_id = l.repo_id)
                         THEN 0 ELSE 1 END, COALESCE(reconciled_at, 0), repo_id LIMIT 1{}",
                    skip_locked(&tx)
                ),
                params![now, due],
                |r| {
                    Ok(PollJob {
                        repo_id: r.get(0)?,
                        full_name: r.get(1)?,
                        installation_id: r.get(2)?,
                        linked_at: r.get(3)?,
                        backfilled_at: r.get(4)?,
                        reconciled_at: r.get(5)?,
                        cursor: r.get(6)?,
                        failures: r.get(7)?,
                        due: false,
                        lease: String::new(),
                    })
                },
            )
            .optional()?;
        let Some(mut job) = found else { return Ok(None) };
        job.due = job.backfilled_at.is_none() || job.cursor.is_some() || job.reconciled_at.is_none_or(|r| r <= due);
        job.lease = hex::encode(arugula_e2e::random::<16>());
        let n = tx.execute(
            "UPDATE linked_repos SET lease = ?1, lease_until = ?2
             WHERE repo_id = ?3 AND lease_until <= ?4 AND unlinked_at IS NULL",
            params![job.lease, now + POLL_LEASE_MS, job.repo_id, now],
        )?;
        if n != 1 {
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(job))
    }

    /// Hold the claim for another [`POLL_LEASE_MS`]: false when it's lost,
    /// or the repo was unlinked, and the turn stops.
    pub fn renew_poll(&self, job: &PollJob, now: u64) -> Result<bool> {
        let n = self.c().execute(
            "UPDATE linked_repos SET lease_until = ?1 WHERE repo_id = ?2 AND lease = ?3 AND unlinked_at IS NULL",
            params![now + POLL_LEASE_MS, job.repo_id, job.lease],
        )?;
        Ok(n == 1)
    }

    /// A turn is done, and the claim goes: reconciled (and backfilled)
    /// at `now`, or only read again, or to carry on from a cursor. False
    /// when the claim was lost.
    pub fn finish_poll(&self, job: &PollJob, how: &Finish, now: u64) -> Result<bool> {
        let c = self.c();
        let n = match how {
            Finish::Reconciled(backfilled) => c.execute(
                "UPDATE linked_repos SET backfilled_at = CASE WHEN ?1 THEN ?2 ELSE backfilled_at END,
                     reconciled_at = ?2, poll_cursor = NULL, poll_failures = 0, lease = NULL, lease_until = ?2
                 WHERE repo_id = ?3 AND lease = ?4",
                params![backfilled, now, job.repo_id, job.lease],
            )?,
            Finish::Refetched => c.execute(
                "UPDATE linked_repos SET poll_failures = 0, lease = NULL, lease_until = ?1
                 WHERE repo_id = ?2 AND lease = ?3",
                params![now, job.repo_id, job.lease],
            )?,
            Finish::Cursor(cursor) => c.execute(
                "UPDATE linked_repos SET poll_cursor = ?1, poll_failures = 0, lease = NULL, lease_until = ?2
                 WHERE repo_id = ?3 AND lease = ?4",
                params![cursor, now, job.repo_id, job.lease],
            )?,
        };
        Ok(n == 1)
    }

    /// A turn failed: the claim goes, and the repo waits `backoff_ms`
    /// doubled for each failure in a row before it, up to
    /// [`MAX_BACKOFF_MS`]. How long it waits.
    pub fn fail_poll(&self, job: &PollJob, backoff_ms: u64, now: u64) -> Result<u64> {
        let wait = backoff_ms.saturating_mul(1u64 << job.failures.min(16)).min(MAX_BACKOFF_MS);
        self.c().execute(
            "UPDATE linked_repos SET poll_failures = poll_failures + 1, lease = NULL, lease_until = ?1
             WHERE repo_id = ?2 AND lease = ?3",
            params![now + wait, job.repo_id, job.lease],
        )?;
        Ok(wait)
    }

    /// Give the claim up without a turn done: no other machine takes the
    /// repo before `until` (the rate budget's back-off).
    pub fn release_poll(&self, job: &PollJob, until: u64) -> Result<()> {
        self.c().execute(
            "UPDATE linked_repos SET lease = NULL, lease_until = ?1 WHERE repo_id = ?2 AND lease = ?3",
            params![until, job.repo_id, job.lease],
        )?;
        Ok(())
    }

    /// The open PRs' heads the poller reads checks for: `(number, sha)`.
    pub fn open_heads(&self, repo_id: u64) -> Result<Vec<(u64, String)>> {
        self.c().query_map(
            "SELECT number, head_sha FROM forge_items
             WHERE repo_id = ?1 AND kind = 'pr' AND state = 'open' AND head_sha IS NOT NULL ORDER BY number",
            params![repo_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The open PRs, not drafts, whose merge state the poller may read
    /// again (`forge::poller`'s merge rechecks): each with its state, when
    /// its row was last written or read, and its head's checks.
    pub fn merge_candidates(&self, repo_id: u64) -> Result<Vec<MergeCandidate>> {
        let c = self.c();
        let mut out: Vec<MergeCandidate> = c
            .query_map(
                "SELECT number, merge_state, synced_at, head_sha, draft FROM forge_items
                 WHERE repo_id = ?1 AND kind = 'pr' AND state = 'open' AND head_sha IS NOT NULL ORDER BY number",
                params![repo_id],
                |r| {
                    Ok((
                        MergeCandidate {
                            number: r.get(0)?,
                            merge_state: r.get(1)?,
                            synced_at: r.get(2)?,
                            head_sha: r.get(3)?,
                            checks: vec![],
                        },
                        r.get::<bool>(4)?,
                    ))
                },
            )?
            .into_iter()
            .filter_map(|(m, draft)| (!draft).then_some(m))
            .collect();
        let checks: Vec<(String, ForgeCheck)> = c.query_map(
            "SELECT c.head_sha, c.kind, c.name, c.status, c.conclusion, c.url, c.updated_at FROM forge_checks c
             WHERE c.repo_id = ?1 AND c.head_sha IN
                 (SELECT head_sha FROM forge_items WHERE repo_id = ?1 AND kind = 'pr' AND state = 'open')
             ORDER BY c.name, c.kind",
            params![repo_id],
            |r| {
                Ok((
                    r.get(0)?,
                    ForgeCheck {
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        status: r.get(3)?,
                        conclusion: r.get(4)?,
                        url: r.get(5)?,
                        updated_at: r.get(6)?,
                    },
                ))
            },
        )?;
        for (sha, check) in checks {
            for m in out.iter_mut().filter(|m| m.head_sha == sha) {
                m.checks.push(check.clone());
            }
        }
        Ok(out)
    }

    /// What waits to be read again for a repo: `(kind, key)`.
    pub fn refetches(&self, repo_id: u64) -> Result<Vec<(String, String)>> {
        self.c().query_map(
            "SELECT kind, key FROM forge_refetch WHERE repo_id = ?1 ORDER BY at, kind, key",
            params![repo_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// The installations live links go through, for the sweep.
    pub fn linked_installations(&self) -> Result<Vec<u64>> {
        self.c().query_map(
            "SELECT DISTINCT installation_id FROM linked_repos WHERE unlinked_at IS NULL ORDER BY installation_id",
            params![],
            |r| r.get(0),
        )
    }

    /// An installation as the platform last kept it.
    pub fn installation(&self, id: u64) -> Result<Option<Installation>> {
        self.c()
            .query_row(
                "SELECT installation_id, github_account_id, github_login, github_account_type, permissions
                 FROM github_installations WHERE installation_id = ?1",
                params![id],
                |r| {
                    Ok(Installation {
                        id: r.get(0)?,
                        account_id: r.get(1)?,
                        login: r.get(2)?,
                        account_type: r.get(3)?,
                        permissions: r.get(4)?,
                    })
                },
            )
            .optional()
    }

    /// Each linked repo's item count, for drift per thousand items.
    pub fn mirror_sizes(&self) -> Result<Vec<(u64, u64)>> {
        self.c().query_map(
            "SELECT l.repo_id, (SELECT count(*) FROM forge_items i WHERE i.repo_id = l.repo_id)
             FROM linked_repos l WHERE l.unlinked_at IS NULL ORDER BY l.repo_id",
            params![],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::forge::{Installation, LinkedRepo};

    fn link(db: &Db, repo: u64) {
        let l = LinkedRepo {
            repo_id: repo,
            full_name: format!("acme/r{repo}"),
            private: true,
            project_id: "prj_a".into(),
            installation_id: 9,
            linked_by: Some("alice".into()),
            linked_at: 0,
            unlinked_at: None,
            purge_after: None,
            mirror_content: false,
        };
        let i = Installation {
            id: 9,
            account_id: 500,
            login: "acme".into(),
            account_type: "Organization".into(),
            permissions: "{}".into(),
        };
        db.link_repo(&l, &i, "alice", 1).unwrap();
    }

    fn item(updated_at: u64, title: &str) -> ItemRow {
        ItemRow {
            repo_id: 7,
            number: 1,
            kind: "pr".into(),
            state: "open".into(),
            title: title.into(),
            labels: "[]".into(),
            author_login: Some("bob".into()),
            author_id: Some(2),
            assignees: "[]".into(),
            reviewers: "[]".into(),
            draft: false,
            merge_state: Some("clean".into()),
            head_ref: Some("b".into()),
            head_sha: Some("abc".into()),
            base_ref: Some("main".into()),
            created_at: 1,
            updated_at,
        }
    }

    /// An item is written once per time GitHub gives it: an older answer
    /// doesn't overwrite, and one with no `mergeable_state` keeps the row's.
    #[test]
    fn items_never_go_back_in_time() {
        let db = Db::test();
        link(&db, 7);
        let w = |it: ItemRow| db.mirror(7, None, |tx| upsert_item(tx, &it, Source::Pushed, 5)).unwrap().unwrap();
        assert_eq!(w(item(10, "one")), Wrote::New);
        assert_eq!(w(item(10, "one")), Wrote::Same);
        // The same second, saying otherwise: kept, and read again.
        assert_eq!(w(item(10, "uno")), Wrote::Tie);
        assert_eq!(db.refetches(7).unwrap(), [("pr".to_owned(), "1".to_owned())]);
        let fetched = db.mirror(7, None, |tx| upsert_item(tx, &item(10, "uno"), Source::Fetched, 5)).unwrap();
        assert_eq!(fetched, Some(Wrote::Changed(vec!["title"])), "the read again is taken");
        assert_eq!(w(item(20, "two")), Wrote::Changed(vec!["title"]));
        assert_eq!(w(item(15, "old")), Wrote::Older);
        assert_eq!(w(item(30, "two")), Wrote::Same, "a comment moved its time");
        assert_eq!(w(ItemRow { merge_state: None, ..item(40, "two") }), Wrote::Same);
        let ms: Option<String> =
            db.c().query_row("SELECT merge_state FROM forge_items WHERE repo_id = 7", params![], |r| r.get(0)).unwrap();
        assert_eq!(ms.as_deref(), Some("clean"));
        let (title, at): (String, u64) = db
            .c()
            .query_row("SELECT title, updated_at FROM forge_items WHERE repo_id = 7", params![], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((title.as_str(), at), ("two", 40));
    }

    /// Events land once, by GitHub's id or by what they are.
    #[test]
    fn events_land_once() {
        let db = Db::test();
        link(&db, 7);
        let review = EventRow {
            repo_id: 7,
            number: 1,
            kind: "review".into(),
            github_id: Some(55),
            actor_login: Some("carol".into()),
            actor_id: Some(3),
            review_state: Some("approved".into()),
            at: 10,
        };
        let w = |e: &EventRow| db.mirror(7, None, |tx| add_event(tx, e)).unwrap().unwrap();
        assert_eq!(w(&review), Wrote::New);
        assert_eq!(w(&review), Wrote::Same);
        let dismissed = EventRow { review_state: Some("dismissed".into()), ..review.clone() };
        assert_eq!(w(&dismissed), Wrote::Changed(vec!["review_state"]));
        assert_eq!(w(&review), Wrote::Same, "a late `submitted` doesn't undo a dismissal");
        let closed = EventRow { kind: "closed".into(), github_id: None, review_state: None, ..review };
        assert_eq!(w(&closed), Wrote::New);
        assert_eq!(w(&closed), Wrote::Same);
        let n: u64 = db.c().query_row("SELECT count(*) FROM forge_events", params![], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    /// Nothing is written to a repo that isn't linked, or was unlinked, or
    /// whose lease another claim holds.
    #[test]
    fn only_a_linked_repo_and_its_lease() {
        let db = Db::test();
        assert_eq!(db.mirror(7, None, |tx| upsert_item(tx, &item(1, "x"), Source::Pushed, 1)).unwrap(), None);
        link(&db, 7);
        let job = db.claim_poll(100, 1).unwrap().unwrap();
        assert!(db.mirror(7, Some("other"), |_| Ok(())).unwrap().is_none());
        assert!(db.mirror(7, Some(&job.lease), |_| Ok(())).unwrap().is_some());
        db.unlink_repo("prj_a", "acme/r7", 200).unwrap().unwrap();
        assert!(db.mirror(7, None, |_| Ok(())).unwrap().is_none());
        assert!(db.mirror(7, Some(&job.lease), |_| Ok(())).unwrap().is_none());
        assert!(!db.renew_poll(&job, 201).unwrap(), "an unlinked repo's turn stops");
    }

    /// A claim waits for the repo's turn: backfill first, then every
    /// `every` after the last reconcile; a back-off holds it.
    #[test]
    fn claims_come_when_due() {
        let db = Db::test();
        link(&db, 7);
        let job = db.claim_poll(1_000, 300).unwrap().unwrap();
        assert_eq!((job.backfilled_at, job.reconciled_at), (None, None));
        assert!(db.claim_poll(1_000, 300).unwrap().is_none(), "leased");
        assert!(job.due);
        assert!(db.finish_poll(&job, &Finish::Reconciled(true), 1_100).unwrap());
        assert!(db.claim_poll(1_200, 300).unwrap().is_none(), "not due");
        let job = db.claim_poll(1_400, 300).unwrap().unwrap();
        assert_eq!((job.backfilled_at, job.reconciled_at), (Some(1_100), Some(1_100)));
        db.release_poll(&job, 2_000).unwrap();
        assert!(db.claim_poll(1_999, 300).unwrap().is_none(), "backing off");
        let job = db.claim_poll(2_000, 300).unwrap().unwrap();
        // A cursor, or something to read again, makes it due at once; only
        // a cursor makes a reconcile due.
        assert!(db.finish_poll(&job, &Finish::Cursor("{}".into()), 2_000).unwrap());
        let job = db.claim_poll(2_001, 300).unwrap().unwrap();
        assert_eq!((job.cursor.as_deref(), job.due), (Some("{}"), true));
        assert!(db.finish_poll(&job, &Finish::Reconciled(false), 2_001).unwrap());
        db.mirror(7, None, |tx| refetch(tx, 7, "head", "abc", 2_002)).unwrap();
        let job = db.claim_poll(2_002, 300).unwrap().unwrap();
        assert!(!job.due);
        assert!(db.finish_poll(&job, &Finish::Refetched, 2_002).unwrap());
        // Failures wait longer each time, up to an hour.
        db.mirror(7, None, |tx| refetched(tx, 7, "head", "abc")).unwrap();
        let mut at = 2_002 + 300;
        let mut waits = vec![];
        for _ in 0..9 {
            let job = db.claim_poll(at, 300).unwrap().unwrap();
            let wait = db.fail_poll(&job, 60_000, at).unwrap();
            waits.push(wait / 60_000);
            assert!(db.claim_poll(at + wait - 1, 300).unwrap().is_none());
            at += wait;
        }
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
        let job = db.claim_poll(at, 300).unwrap().unwrap();
        assert_eq!(job.failures, 9);
        db.finish_poll(&job, &Finish::Reconciled(false), at).unwrap();
        assert_eq!(db.claim_poll(at + 300, 300).unwrap().unwrap().failures, 0);
    }

    /// Two machines' pollers, four each, claiming at once: each repo is
    /// claimed by one, until its lease runs out.
    #[test]
    fn two_machines_never_claim_the_same_repo() {
        let a = Db::test();
        let (b, file) = match a.second() {
            Some(b) => (b, None),
            None => {
                let path =
                    std::env::temp_dir().join(format!("arugula-poll-{}.db", hex::encode(arugula_e2e::random::<8>())));
                (Db::open(&path).unwrap(), Some(path))
            }
        };
        let a = match &file {
            Some(path) => Db::open(path).unwrap(),
            None => a,
        };
        for r in 1..=20 {
            link(&a, r);
        }
        let claimed = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for db in [&a, &b] {
                for _ in 0..4 {
                    scope.spawn(|| {
                        while let Some(j) = db.claim_poll(1_000, 300).unwrap() {
                            claimed.lock().unwrap().push(j.repo_id);
                        }
                    });
                }
            }
        });
        let mut claimed = claimed.into_inner().unwrap();
        claimed.sort_unstable();
        assert_eq!(claimed, (1..=20).collect::<Vec<_>>(), "every repo, once");
        assert!(b.claim_poll(1_000 + POLL_LEASE_MS - 1, 300).unwrap().is_none());
        assert!(b.claim_poll(1_000 + POLL_LEASE_MS, 300).unwrap().is_some());
        drop((a, b));
        if let Some(f) = file {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", f.display()));
            }
        }
    }
}
