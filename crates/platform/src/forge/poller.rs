//! One leased poller per linked repo (M164, #1210), in the job worker's
//! way ([`crate::jobs`]): each machine claims a repo whose turn it is (`Db::claim_poll`; on
//! Postgres `FOR UPDATE SKIP LOCKED`, so two machines never poll the same
//! one), renews the lease between reads, and lets it go when it's done.
//!
//! Each turn first reads the repo itself, keeping its name and privacy
//! current (`linked_repos.full_name`, `private`), then what two answers
//! tied on (`forge_refetch`), taking GitHub's answer now. Then, when it's
//! due:
//!
//! - **Backfill**, on the first turns after linking: open PRs and issues,
//!   and everything updated in the last [`BACKFILL_DAYS`] days; each PR's
//!   reviews, as events; the checks on open PRs' heads.
//! - **Reconcile**, every few minutes (`Settings::every_ms`): what GitHub
//!   lists as updated since the last reconcile (less [`OVERLAP_MS`]), each
//!   such PR's reviews, and the checks on every open PR's head, compared
//!   with the rows. A row that was missing or wrong is **drift**: written
//!   now, counted (per repo per day, [`super::stats`]) and logged, when
//!   GitHub's change came after the last reconcile and more than
//!   [`GRACE_MS`] ago (its webhook may be on its way). An old item's
//!   history (reviews of a PR updated again) is written, not counted, and
//!   so is a check run that started: GitHub sends `check_run` when one is
//!   made and when it completes, never when it starts.
//! - **The full pass**, about once a day per repo, after a reconcile
//!   ([`full_due`]; each repo's time of day is its own, so an
//!   installation's repos don't all go at once). A reconcile only sees
//!   what GitHub says was updated, and a deleted or transferred item never
//!   is, so this lists what's open on GitHub and reads again each open row
//!   it doesn't list (at the next turn, through `forge_refetch`; none when
//!   they're over [`FULL_MISSING`] or a quarter of the open rows, which
//!   says the list is wrong, not the rows). Then it reads the
//!   [`FULL_READS`] rows written or read longest ago again, with their
//!   text for a repo that mirrors it, so a comment deleted or edited while
//!   its webhook was lost catches up. The reconcile and the pass's start
//!   (`linked_repos.full_at`) are recorded before it runs, and it's best
//!   effort: however it stops, the reconcile stands and the next pass is a
//!   day off. Its cost, per repo per day: one list page per 100 open items
//!   (at most [`FULL_PAGES`]); for each open row GitHub didn't list, one
//!   read (two for a PR, with its reviews); and for the stalest rows, one
//!   read per issue and two per PR (its reviews), at most 60. A repo that
//!   mirrors text reads each of those whole as well: an issue again and
//!   its comments, a PR again with its comments, reviews and review
//!   comments, so at most 180. These rows were last read a day or more
//!   ago, so the ETag cache mostly won't make them free. A repo with under
//!   100 open items and none missing: at most 61 reads a day, or 181 with
//!   text, against an installation's 5,000 an hour or more. All of it
//!   within the rate reserve, as the rest.
//! - **An item read again** is gone when GitHub answers
//!   404 or 410, or with an item that names another number or repo (a
//!   transfer's redirect; one that names only another repo name is checked
//!   against the repo's name read again, as a rename mid-turn looks the
//!   same), and the repo itself still reads as this one.
//!   It then goes from the mirror with its text. An answer that names no
//!   item (an empty or garbled body) and any other refusal of one item or
//!   head (422 for a commit GitHub no longer has, say) are logged and
//!   left: neither deletes a row nor stops the repo's turn.
//! - A turn reads at most `Settings::pages` list pages. One with more
//!   keeps a cursor (`linked_repos.poll_cursor`), and the next turn, at
//!   once, carries on from it: nothing is dropped.
//! - **Reads** go through `arugula-forge-github`'s client, with
//!   installation tokens for the one repo, read only, minted as
//!   `Purpose::Ingest` (M167's helper, logged). Conditional GETs: an
//!   unchanged answer costs no rate limit.
//! - **Rate budget**: before each read, if less than `Settings::reserve`
//!   percent of the installation's limit is left, the turn stops, gives
//!   its lease up and the repo waits [`BACKOFF_MS`]: the rest is for
//!   reads on demand and sends.
//! - **A turn that fails** waits [`BACKOFF_MS`], doubled for each failure
//!   in a row, up to an hour. When GitHub won't mint a token for the repo
//!   or says it's gone (404 or 451), the App's own view of the
//!   installation decides: deleted or suspended unlinks its repos, and
//!   otherwise the repo is taken out of it ([`gone`]); as the
//!   installation's events would, so the purge follows. A sweep of the
//!   App's installations every [`SWEEP_EVERY`] does the same for an
//!   uninstall whose webhook never came ([`sweep`]).
//! - **Content** (M166): for a repo whose project mirrors it, each item
//!   a list's page names is fetched whole (body, comments, reviews' text,
//!   review comments) and replaces what `forge_content` had of it.
//!   Turning mirroring on makes the next turn a backfill.
//! - **Merge states**: lists never carry a PR's `mergeable_state`,
//!   webhooks often say `unknown` (GitHub works it out lazily), and a base
//!   branch that moves changes it with no event the App hears. So at the
//!   end of each backfill and reconcile the poller reads, one PR at a time
//!   (`GET pulls/N`, which makes GitHub work it out), the open PRs that
//!   aren't drafts whose state it doesn't know, and the green ones whose
//!   row is older than [`MERGE_RECHECK_MS`]: unknown first, then stalest,
//!   at most [`MERGE_CHECKS`] a turn ([`merge_rechecks`]). Within the rate
//!   reserve (one that reaches it leaves the rest for the next turn), as
//!   ingest, and never drift: GitHub sends no webhook for it. The inbox
//!   calls a green PR done only once the state is known (M171).
//! - **Unlinking** stops a turn at its next read or write (`Db::mirror`
//!   and `Db::renew_poll` check the link), and M163's purge waits for the
//!   lease to run out.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, bail};
use arugula_e2e::now_ms;
use arugula_forge_github::{Error as GhError, Github, Purpose as GhPurpose, Token, TokenSource, unreadable};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use super::{
    Caller, GithubApp, Purpose, Repo, content,
    ingest::{self, Answer},
};
use crate::{
    Platform,
    db::{
        forge::Installation,
        mirror::{
            self, CheckRow, EventRow, Finish, ItemRow, MergeCandidate, PollJob, Source, Wrote, add_event, upsert_check,
            upsert_item,
        },
        sql::Tx,
    },
};

/// How far back a backfill reads closed items.
pub const BACKFILL_DAYS: u64 = 90;
/// How far before the last reconcile the next one reads from.
pub const OVERLAP_MS: u64 = 120_000;
/// A change GitHub made this recently isn't drift yet.
pub const GRACE_MS: u64 = 60_000;
/// How long a repo waits when its installation's budget is reserved, and
/// after its first failure.
pub const BACKOFF_MS: u64 = 60_000;
/// How long checks on a commit no item has as its head are kept.
pub const KEEP_UNHEADED_MS: u64 = 3_600_000;
/// How often the App's installations are swept for ones GitHub no longer
/// lists.
pub const SWEEP_EVERY: Duration = Duration::from_secs(6 * 3600);
/// How often each machine looks for a repo whose turn it is.
const TICK: Duration = Duration::from_secs(5);
/// Most pages of one item's reviews, or one head's checks.
const SMALL_PAGES: usize = 10;
/// Most PRs a turn reads again for their merge state.
pub const MERGE_CHECKS: usize = 10;
/// How old a green PR's row is before its merge state is read again (its
/// base may have moved).
pub const MERGE_RECHECK_MS: u64 = 15 * 60_000;

/// About how often each repo gets a full pass.
pub const FULL_EVERY_MS: u64 = 86_400_000;
/// Most pages of open items a full pass lists: past it, it compares
/// nothing that day, and says so.
pub const FULL_PAGES: usize = 50;
/// How many of the stalest rows a full pass reads again.
pub const FULL_READS: usize = 30;
/// Most open rows GitHub didn't list that a full pass reads again: past
/// it, or past a quarter of the open rows, it reads none and says so.
pub const FULL_MISSING: usize = 100;

/// Whether a reconcile at `now` of a repo whose last full pass started at
/// `last` (`linked_repos.full_at`, else its backfill) is followed by one,
/// one `every` ms ([`FULL_EVERY_MS`]; 0, never): the first since the
/// repo's own point in that period passed. Never after a backfill (it
/// lists what's open itself), nor before one (`last` none).
pub fn full_due(repo_id: u64, last: Option<u64>, now: u64, every: u64) -> bool {
    if every == 0 {
        return false;
    }
    // Each repo's point in the period, spread by its id.
    let offset = repo_id.wrapping_mul(2_654_435_761) % every;
    let period = |t: u64| (t + offset) / every;
    last.is_some_and(|last| period(last) < period(now))
}

/// GitHub says it isn't there: 404, or 410 (deleted).
fn not_there(e: &GhError) -> bool {
    matches!(e, GhError::NotFound(_)) || matches!(e, GhError::Http(m) if m.starts_with("410"))
}

/// GitHub refused this one read for itself (a 4xx but 401, 403 and 429),
/// as for a commit it no longer has (422): nothing about the repo or the
/// token, so the turn goes on.
fn refused_one(e: &GhError) -> bool {
    let GhError::Http(m) = e else { return false };
    let code: Option<u16> = m.get(..3).and_then(|c| c.parse().ok());
    code.is_some_and(|c| (400..500).contains(&c) && !matches!(c, 401 | 403 | 429))
}

/// Which open PRs (not drafts) a turn at `now` reads again for their merge
/// state, in order: every one whose state the row doesn't know (none, or
/// `unknown`), then green ones whose row is older than
/// [`MERGE_RECHECK_MS`], stalest first; at most [`MERGE_CHECKS`].
pub fn merge_rechecks(open: &[MergeCandidate], now: u64) -> Vec<u64> {
    let unknown = |m: &MergeCandidate| matches!(m.merge_state.as_deref(), None | Some("unknown"));
    let mut due: Vec<&MergeCandidate> = open
        .iter()
        .filter(|m| unknown(m) || (m.synced_at + MERGE_RECHECK_MS <= now && super::inbox::green(&m.checks)))
        .collect();
    due.sort_by_key(|m| (!unknown(m), m.synced_at, m.number));
    due.into_iter().take(MERGE_CHECKS).map(|m| m.number).collect()
}

/// Installation tokens for the one repo, read only: the poller's
/// (`Purpose::Ingest`, for `platform`), or a person's read (M166).
pub(super) struct RepoTokens<'p> {
    pub p: &'p Platform,
    pub caller: &'p str,
    pub purpose: Purpose,
    pub installation: u64,
    pub repo_id: u64,
}

impl TokenSource for RepoTokens<'_> {
    fn token<'a>(
        &'a self,
        _repo: &'a str,
        purpose: GhPurpose,
        _fresh: bool,
    ) -> BoxFuture<'a, Result<Option<Token>, String>> {
        Box::pin(async move {
            // Only reads.
            if purpose != GhPurpose::Read {
                return Ok(None);
            }
            let gh = self.p.github.as_ref().ok_or(super::NO_APP)?;
            let caller = Caller::of(self.p, self.caller);
            let t = gh
                .read_token(&caller, self.installation, Repo::Id(self.repo_id), self.purpose)
                .await
                .map_err(|e| format!("{e:#}"))?;
            Ok(Some(Token { secret: t.secret().to_owned(), login: None }))
        })
    }
}

/// Why a turn stopped before it was done.
#[derive(Debug)]
enum Stop {
    /// The installation's budget is reserved until then (epoch s).
    Budget(u64),
    /// The lease went, or the repo was unlinked.
    Lost,
    /// GitHub said no, to a read of `path`; `repo` when it was the repo's own.
    Gh {
        path: String,
        error: GhError,
        repo: bool,
    },
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for Stop {
    fn from(e: anyhow::Error) -> Self {
        Stop::Failed(e)
    }
}

/// What a turn did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    /// Backfilled (or reconciled), with the rows it fixed and the drift
    /// among them.
    Done { repo: u64, backfill: bool, fixed: usize, drift: u64 },
    /// Ran out of pages, and carries on at the next turn.
    More { repo: u64, fixed: usize },
    /// Only read again what tied.
    Refetched { repo: u64, fixed: usize },
    /// Left the installation's reserve be.
    BackedOff { repo: u64 },
    /// Lost its lease, or the repo was unlinked.
    Lost { repo: u64 },
    /// The installation or the repo is gone from the App: `unlinked` repos.
    Gone { repo: u64, unlinked: usize },
}

/// Where a turn that ran out of pages stopped: the list, and its next page.
#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    list: usize,
    next: String,
}

/// A list a turn reads: its path, the update it stops before, and whether
/// it lists PRs.
type List = (String, Option<u64>, bool);

/// A turn on one repo.
struct Poll<'p> {
    p: &'p Platform,
    job: PollJob,
    gh: Github<RepoTokens<'p>>,
    /// Count drift from GitHub changes since then (the last reconcile),
    /// when this is a reconcile after one since linking.
    drift_from: Option<u64>,
    now: u64,
    pages: usize,
    fixed: usize,
    drift: u64,
    /// The PRs it wrote or fixed, for the boards to attach (#368) once
    /// their content is in.
    prs: Vec<u64>,
    /// The issues whose rows it changed: an exported one's comments are
    /// read (M205).
    issues: Vec<u64>,
}

impl<'p> Poll<'p> {
    /// Before each read: the lease, and the budget.
    fn may_read(&self) -> Result<(), Stop> {
        let Some(gh) = &self.p.github else { return Err(Stop::Lost) };
        let reserve = gh.ingest.settings.reserve;
        if let Some(reset) = gh.ingest.stats.reserved(self.job.installation_id, reserve, now_ms() / 1000) {
            return Err(Stop::Budget(reset));
        }
        if !self.p.db.renew_poll(&self.job, now_ms())? {
            return Err(Stop::Lost);
        }
        Ok(())
    }

    fn heard(&self) {
        if let Some(gh) = &self.p.github {
            let r = self.gh.rate();
            gh.ingest.stats.heard(self.job.installation_id, r.limit, r.remaining, r.reset);
        }
    }

    fn refused(path: &str, error: GhError) -> Stop {
        Stop::Gh { path: path.split('?').next().unwrap_or_default().to_owned(), error, repo: false }
    }

    async fn read(&self, path: &str) -> Result<Value, Stop> {
        self.may_read()?;
        let r = self.gh.read(&self.job.full_name, path).await;
        self.heard();
        r.map_err(|e| Self::refused(path, e))
    }

    async fn page(&self, path: &str) -> Result<(Value, Option<String>), Stop> {
        self.may_read()?;
        let r = self.gh.page(&self.job.full_name, path).await;
        self.heard();
        r.map_err(|e| Self::refused(path, e))
    }

    /// The body of each PR among `self.prs` that is merged and whose body the
    /// mirror does not keep (the project's content is off), read once from
    /// GitHub for the closer (#467). Never stored. One that can't be read is
    /// left out, and the budget ends the reads.
    async fn merged_bodies(&self) -> HashMap<u64, String> {
        let id = self.job.repo_id;
        let mut out = HashMap::new();
        let prs = match self.p.db.merged_prs(id, &self.prs) {
            Ok(prs) => prs,
            Err(e) => {
                warn!(repo = id, error = %e, "forge mirror: finding the merged PRs without a body");
                return out;
            }
        };
        for pr in prs.into_iter().filter(|pr| pr.body.is_none()) {
            match self.read(&format!("repositories/{id}/pulls/{}", pr.number)).await {
                Ok(v) => out.extend(v["body"].as_str().map(|b| (pr.number, b.to_owned()))),
                Err(Stop::Gh { path, error, .. }) => {
                    warn!(repo = id, number = pr.number, path, error = %error, "forge mirror: reading a merged PR's body");
                }
                Err(_) => break,
            }
        }
        out
    }

    /// Every page of a short list (an item's reviews, a head's checks):
    /// GitHub's list, or the one under `key` (`check_runs`).
    async fn all(&self, path: &str, key: Option<&str>) -> Result<Vec<Value>, Stop> {
        let mut out = Vec::new();
        let mut next = Some(path.to_owned());
        for _ in 0..SMALL_PAGES {
            let Some(path) = next.take() else { return Ok(out) };
            let (v, n) = self.page(&path).await?;
            let list = key.map_or(&v, |k| &v[k]);
            out.extend(list.as_array().into_iter().flatten().cloned());
            next = n;
        }
        if next.is_some() {
            warn!(repo = self.job.repo_id, path = path.split('?').next(), "forge mirror: a list longer than it reads");
        }
        Ok(out)
    }

    /// Run `f` in a transaction on the repo's rows, under the lease.
    fn tx<T>(&self, f: impl FnOnce(&Tx<'_, '_>) -> anyhow::Result<T>) -> Result<T, Stop> {
        self.p.db.mirror(self.job.repo_id, Some(&self.job.lease), f)?.ok_or(Stop::Lost)
    }

    /// Write rows under the lease, counting what was fixed; from a list, as
    /// drift what GitHub changed in the drift window.
    fn write(&mut self, items: &[ItemRow], checks: &[CheckRow], events: &[EventRow], from: Source) -> Result<(), Stop> {
        let now = self.now;
        let wrote = self.tx(|tx| {
            let mut out = Vec::new();
            for it in items {
                out.push((upsert_item(tx, it, from, now)?, it.updated_at, format!("#{}", it.number), "item", false));
            }
            for c in checks {
                out.push((
                    upsert_check(tx, c, from, now)?,
                    c.updated_at,
                    format!("{}:{}", c.head_sha, c.name),
                    "check",
                    // GitHub sends `check_run` for created and completed,
                    // never for one that starts: in progress is the poller's.
                    c.kind == "check_run" && c.status == "in_progress",
                ));
            }
            for e in events {
                out.push((add_event(tx, e)?, e.at, format!("#{}", e.number), "event", false));
            }
            Ok(out)
        })?;
        // A session's PR the mirror has now says where it came from (M259).
        if wrote.iter().any(|(w, _, _, kind, _)| *kind == "item" && matches!(w, Wrote::New)) {
            crate::features::chat().pr_noted(self.p, self.job.repo_id);
        }
        // A session's PR it saw merged ends the session (M265).
        let changed: Vec<&ItemRow> =
            items.iter().zip(&wrote).filter(|(_, (w, ..))| w.fixed()).map(|(it, _)| it).collect();
        self.prs.extend(changed.iter().filter(|it| it.kind == "pr").map(|it| it.number));
        self.issues.extend(changed.iter().filter(|it| it.kind == "issue").map(|it| it.number));
        crate::features::chat().merged_seen(self.p, self.job.repo_id, &changed);
        let before = now.saturating_sub(GRACE_MS);
        for (wrote, at, which, kind, started) in wrote {
            if !wrote.fixed() {
                continue;
            }
            self.fixed += 1;
            // A merge state is no webhook's to write (GitHub works it out
            // when asked, and a base that moves sends nothing), nor a check
            // run's start: never drift.
            let fields = match &wrote {
                Wrote::Changed(f) => f
                    .iter()
                    .filter(|f| **f != "merge_state" && !(started && **f == "status"))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(","),
                _ => "missing".into(),
            };
            if fields.is_empty() {
                continue;
            }
            if from == Source::Pushed && self.drift_from.is_some_and(|since| since <= at && at < before) {
                self.drift += 1;
                // The repo's id, the item or check, and what was wrong; never a title.
                info!(repo = self.job.repo_id, kind, which, fields, "forge mirror: drift fixed");
            }
        }
        Ok(())
    }

    /// The repo itself: its name and privacy, kept current.
    async fn repo(&mut self) -> Result<(), Stop> {
        let id = self.job.repo_id;
        let path = format!("repositories/{id}");
        let v = self.read(&path).await.map_err(|s| match s {
            Stop::Gh { path, error, .. } => Stop::Gh { path, error, repo: true },
            s => s,
        })?;
        let (Some(name), Some(private)) = (v["full_name"].as_str(), v["private"].as_bool()) else {
            return Err(Stop::Failed(anyhow::anyhow!("GitHub didn't say what repo {id} is")));
        };
        self.tx(|tx| mirror::refresh_link(tx, id, name, private))?;
        self.job.full_name = name.to_owned();
        Ok(())
    }

    /// Each PR's reviews, as events.
    async fn reviews(&mut self, numbers: &[u64], from: Source) -> Result<(), Stop> {
        let id = self.job.repo_id;
        for n in numbers {
            let list = self.all(&format!("repositories/{id}/pulls/{n}/reviews?per_page=100"), None).await?;
            let events: Vec<EventRow> = list.iter().filter_map(|r| ingest::review_event(id, *n, r)).collect();
            self.write(&[], &[], &events, from)?;
        }
        Ok(())
    }

    /// A head's check runs and statuses.
    async fn head(&mut self, sha: &str) -> Result<Vec<CheckRow>, Stop> {
        let id = self.job.repo_id;
        let runs = format!("repositories/{id}/commits/{sha}/check-runs?per_page=100");
        let mut rows: Vec<CheckRow> =
            self.all(&runs, Some("check_runs")).await?.iter().filter_map(|r| ingest::run_row(id, r)).collect();
        let statuses = format!("repositories/{id}/commits/{sha}/status?per_page=100");
        let statuses = self.all(&statuses, Some("statuses")).await?;
        rows.extend(statuses.iter().filter_map(|s| ingest::status_row(id, sha, s)));
        Ok(rows)
    }

    /// The checks on every open PR's head, and checks on no head pruned.
    async fn checks(&mut self) -> Result<(), Stop> {
        let id = self.job.repo_id;
        for (n, sha) in self.p.db.open_heads(id)? {
            let rows = match self.head(&sha).await {
                Ok(rows) => rows,
                Err(Stop::Gh { path, error, .. }) if not_there(&error) || refused_one(&error) => {
                    warn!(repo = id, number = n, path, error = %error, "forge mirror: reading a head's checks");
                    continue;
                }
                Err(s) => return Err(s),
            };
            self.write(&[], &rows, &[], Source::Pushed)?;
        }
        let before = self.now.saturating_sub(KEEP_UNHEADED_MS);
        self.tx(|tx| mirror::prune_checks(tx, id, before))?;
        Ok(())
    }

    /// The PRs whose merge state is due a read ([`merge_rechecks`]), read
    /// one by one and written as the poller's own answers (never drift).
    /// One GitHub won't answer is left for the next turn, and so is the
    /// rest once the installation's reserve is reached.
    async fn merges(&mut self) -> Result<(), Stop> {
        let (id, name, now) = (self.job.repo_id, self.job.full_name.clone(), self.now);
        for n in merge_rechecks(&self.p.db.merge_candidates(id)?, now) {
            let v = match self.read(&format!("repositories/{id}/pulls/{n}")).await {
                Ok(v) => v,
                Err(Stop::Budget(_)) => return Ok(()),
                Err(Stop::Gh { path, error, .. }) => {
                    warn!(repo = id, number = n, path, error = %error, "forge mirror: reading a PR's merge state");
                    continue;
                }
                Err(s) => return Err(s),
            };
            let rows: Vec<ItemRow> = ingest::pr_row(id, &name, &v).into_iter().collect();
            self.write(&rows, &[], &[], Source::Fetched)?;
            self.tx(|tx| mirror::merge_checked(tx, id, n, now))?;
        }
        Ok(())
    }

    /// What two answers tied on, or the full pass found open here and not
    /// on GitHub, read again: GitHub's answer now is taken.
    async fn refetch(&mut self) -> Result<(), Stop> {
        let id = self.job.repo_id;
        for (kind, key) in self.p.db.refetches(id)? {
            match kind.as_str() {
                "pr" | "issue" => {
                    if let Ok(n) = key.parse() {
                        self.again(&kind, n, false).await?;
                    }
                }
                "head" => match self.head(&key).await {
                    Ok(rows) => self.write(&[], &rows, &[], Source::Fetched)?,
                    Err(Stop::Gh { path, error, .. }) if not_there(&error) || refused_one(&error) => {
                        warn!(repo = id, path, error = %error, "forge mirror: reading a head's checks again");
                    }
                    Err(s) => return Err(s),
                },
                _ => {}
            }
            self.tx(|tx| mirror::refetched(tx, id, &kind, &key))?;
        }
        Ok(())
    }

    /// Read item `n` (`kind` `pr` or `issue`) again, taking GitHub's answer
    /// now, and with `whole` its text too, for a repo that mirrors it. Gone
    /// from GitHub ([`Poll::gone`]: 404, 410, or an answer that names
    /// another number or repo, as a transfer's redirect does), it goes from
    /// the mirror. An answer that names nothing (an empty or garbled body),
    /// or a refusal of this one item, is logged and left.
    async fn again(&mut self, kind: &str, n: u64, whole: bool) -> Result<(), Stop> {
        let id = self.job.repo_id;
        let mut pr = kind == "pr";
        let mut v = None;
        let mut renamed = false;
        // An issue's read that answers with a PR is read again as one, and
        // one that names the repo otherwise once the repo's name is read
        // again (it may have been renamed since the turn began).
        for _ in 0..3 {
            let path = format!("repositories/{id}/{}/{n}", if pr { "pulls" } else { "issues" });
            let a = match self.read(&path).await {
                Ok(a) => a,
                Err(Stop::Gh { error, .. }) if not_there(&error) => return self.gone(n, "not there").await,
                Err(Stop::Gh { path, error, .. }) if refused_one(&error) || unreadable(&error) => {
                    warn!(repo = id, number = n, path, error = %error, "forge mirror: reading an item again");
                    return Ok(());
                }
                Err(s) => return Err(s),
            };
            match ingest::answer_for(&a, id, &self.job.full_name, n) {
                Answer::Same if !pr && a["pull_request"].is_object() => pr = true,
                Answer::Same => {
                    v = Some(a);
                    break;
                }
                Answer::Other => return self.gone(n, "another repo's or number's item").await,
                Answer::OtherName if !renamed => {
                    renamed = true;
                    self.repo().await?;
                }
                Answer::OtherName => return self.gone(n, "another repo's item").await,
                Answer::Unclear => {
                    warn!(repo = id, number = n, path, "forge mirror: an item's read named no item, left");
                    return Ok(());
                }
            }
        }
        let Some(v) = v else { return Ok(()) };
        let name = self.job.full_name.clone();
        if pr {
            let rows: Vec<ItemRow> = ingest::pr_row(id, &name, &v).into_iter().collect();
            self.write(&rows, &[], &[], Source::Fetched)?;
            self.reviews(&[n], Source::Fetched).await?;
        } else {
            let rows: Vec<ItemRow> = ingest::issue_row(id, &v).into_iter().collect();
            self.write(&rows, &[], &[], Source::Fetched)?;
        }
        if whole {
            self.content(&[n], pr).await?;
        }
        Ok(())
    }

    /// Item `n` is gone from GitHub: from the mirror too, with its text,
    /// and from this machine's read cache. Only once the repo itself still
    /// reads as this one, so a repo GitHub briefly won't show (or one the
    /// App just lost) never loses its rows one by one.
    async fn gone(&mut self, n: u64, why: &str) -> Result<(), Stop> {
        let id = self.job.repo_id;
        match self.read(&format!("repositories/{id}")).await {
            Ok(v) if v["id"].as_u64() == Some(id) => {}
            Ok(_) => {
                warn!(repo = id, number = n, why, "forge mirror: the repo didn't read as itself, the item stays");
                return Ok(());
            }
            Err(Stop::Gh { path, error, .. }) => {
                warn!(repo = id, number = n, why, path, error = %error, "forge mirror: the repo didn't read, the item stays");
                return Ok(());
            }
            Err(s) => return Err(s),
        }
        if self.tx(|tx| mirror::delete_item(tx, id, n))? {
            info!(repo = id, number = n, why, "forge mirror: an item gone from GitHub went");
            self.fixed += 1;
        }
        if let Some(gh) = &self.p.github {
            gh.bodies.forget(id, n);
        }
        Ok(())
    }

    /// The full pass (module docs), after a reconcile that's recorded: each
    /// open row GitHub doesn't list as open is read again at the next turn
    /// (unless that's a large share of them: then none is, and it's
    /// logged), then the stalest rows are read again now, whole.
    async fn full(&mut self) -> Result<(), Stop> {
        let (id, now) = (self.job.repo_id, self.now);
        let mut listed = HashSet::new();
        let mut next = Some(format!("repositories/{id}/issues?state=open&per_page=100"));
        let mut whole = true;
        for _ in 0..FULL_PAGES {
            let Some(path) = next.take() else { break };
            let (page, n) = self.page(&path).await?;
            let Some(items) = page.as_array() else {
                warn!(repo = id, "forge mirror: a full pass's list page wasn't a list, none compared");
                whole = false;
                break;
            };
            listed.extend(items.iter().filter_map(|v| v["number"].as_u64()));
            next = n;
        }
        if next.is_some() {
            warn!(repo = id, pages = FULL_PAGES, "forge mirror: more open items than a full pass lists, none compared");
        } else if whole {
            let open = self.p.db.open_items(id)?;
            let missing: Vec<&(String, u64)> = open.iter().filter(|(_, n)| !listed.contains(n)).collect();
            if missing.len() > FULL_MISSING || (missing.len() > 10 && missing.len() * 4 > open.len()) {
                warn!(
                    repo = id,
                    missing = missing.len(),
                    open = open.len(),
                    "forge mirror: too many open rows GitHub didn't list, none read again"
                );
            } else if !missing.is_empty() {
                info!(repo = id, missing = missing.len(), "forge mirror: open here and not on GitHub, read again");
                self.tx(|tx| {
                    for (kind, n) in &missing {
                        mirror::refetch(tx, id, kind, &n.to_string(), now)?;
                    }
                    Ok(())
                })?;
            }
        }
        for (kind, n) in self.p.db.stalest_items(id, FULL_READS)? {
            self.again(&kind, n, true).await?;
            self.tx(|tx| mirror::merge_checked(tx, id, n, now))?;
        }
        Ok(())
    }

    /// One list, from `path`, down to items updated before `since`: its
    /// rows written, and each PR's reviews. The next page's path when the
    /// turn's pages ran out first.
    async fn list(&mut self, path: &str, since: Option<u64>, prs: bool) -> Result<Option<String>, Stop> {
        let (id, name) = (self.job.repo_id, self.job.full_name.clone());
        let mut next = Some(path.to_owned());
        while let Some(path) = next.take() {
            if self.pages == 0 {
                return Ok(Some(path));
            }
            self.pages -= 1;
            let (page, n) = self.page(&path).await?;
            let mut done = false;
            let mut values = Vec::new();
            for v in page.as_array().into_iter().flatten() {
                if since.is_some_and(|s| ingest::ms(&v["updated_at"]).unwrap_or(0) < s) {
                    done = true;
                    break;
                }
                values.push(v);
            }
            let numbers: Vec<u64> = if prs {
                let rows: Vec<ItemRow> = values.iter().filter_map(|v| ingest::pr_row(id, &name, v)).collect();
                let numbers: Vec<u64> = rows.iter().map(|r| r.number).collect();
                self.write(&rows, &[], &[], Source::Pushed)?;
                self.reviews(&numbers, Source::Pushed).await?;
                numbers
            } else {
                let rows: Vec<ItemRow> = values.iter().filter_map(|v| ingest::issue_row(id, v)).collect();
                self.write(&rows, &[], &[], Source::Pushed)?;
                rows.iter().map(|r| r.number).collect()
            };
            self.content(&numbers, prs).await?;
            next = n.filter(|_| !done);
        }
        Ok(None)
    }

    /// For a repo that mirrors content (M166): each of these items, whole,
    /// in place of what the mirror kept. Through a client of its own, so no
    /// ETag cache keeps the bodies. One that can't be read is left for the
    /// next turn (a read fetches it meanwhile).
    async fn content(&mut self, numbers: &[u64], prs: bool) -> Result<(), Stop> {
        let id = self.job.repo_id;
        if numbers.is_empty() || !self.p.db.linked_repo(id)?.is_some_and(|l| l.mirror_content) {
            return Ok(());
        }
        let Some(gh) = &self.p.github else { return Err(Stop::Lost) };
        let tokens = RepoTokens {
            p: self.p,
            caller: "platform",
            purpose: Purpose::Ingest,
            installation: self.job.installation_id,
            repo_id: id,
        };
        let client = Github::new(&gh.api, self.p.http.clone(), tokens);
        for n in numbers {
            self.may_read()?;
            let fetched = content::fetch(&client, &self.job.full_name, id, *n, prs).await;
            let r = client.rate();
            gh.ingest.stats.heard(self.job.installation_id, r.limit, r.remaining, r.reset);
            match fetched {
                Ok(rows) => self.tx(|tx| {
                    if crate::db::content::mirrors(tx, id)? {
                        crate::db::content::replace_item(tx, id, *n, &rows)?;
                    }
                    Ok(())
                })?,
                // GitHub's message and the path: never a body.
                Err(e) => {
                    warn!(repo = id, number = n, error = format!("{e:#}"), "forge mirror: reading an item's content")
                }
            }
        }
        Ok(())
    }

    /// The lists a backfill or a reconcile reads, in order.
    fn lists(&self, backfill: bool) -> Vec<List> {
        let id = self.job.repo_id;
        let path = |what: &str, q: &str| format!("repositories/{id}/{what}?{q}&per_page=100");
        if backfill {
            let since = Some(self.now.saturating_sub(BACKFILL_DAYS * 86_400_000));
            let recent = "state=closed&sort=updated&direction=desc";
            vec![
                (path("pulls", "state=open"), None, true),
                (path("pulls", recent), since, true),
                (path("issues", "state=open"), None, false),
                (path("issues", recent), since, false),
            ]
        } else {
            let since = Some(self.job.reconciled_at.unwrap_or(0).saturating_sub(OVERLAP_MS));
            let all = "state=all&sort=updated&direction=desc";
            vec![(path("pulls", all), since, true), (path("issues", all), since, false)]
        }
    }

    async fn run(&mut self) -> Result<Finish, Stop> {
        self.repo().await?;
        self.refetch().await?;
        if !self.job.due {
            return Ok(Finish::Refetched);
        }
        let backfill = self.job.backfilled_at.is_none();
        let mut cursor: Option<Cursor> = self.job.cursor.as_deref().and_then(|c| serde_json::from_str(c).ok());
        let lists = self.lists(backfill);
        let start = cursor.as_ref().map_or(0, |c| c.list).min(lists.len());
        for (i, (path, since, prs)) in lists.into_iter().enumerate().skip(start) {
            let first = match cursor.take() {
                Some(c) if c.list == i => c.next,
                _ => path,
            };
            if let Some(next) = self.list(&first, since, prs).await? {
                let c = serde_json::to_string(&Cursor { list: i, next }).context("a poller's cursor")?;
                return Ok(Finish::Cursor(c));
            }
        }
        self.checks().await?;
        self.merges().await?;
        Ok(Finish::Reconciled(backfill))
    }
}

/// The status in an installation token mint's error ("GitHub said 404 ...").
fn mint_status(e: &str) -> Option<u16> {
    let at = e.find("GitHub said ")? + 12;
    e.get(at..at + 3)?.parse().ok()
}

/// A GET as the App itself (its JWT): the status, the JSON, and the next
/// page's URL.
async fn app_get(p: &Platform, gh: &GithubApp, path: &str) -> anyhow::Result<(u16, Value, Option<String>)> {
    let url = if path.starts_with("http") { path.to_owned() } else { format!("{}/{path}", gh.api) };
    let r = p
        .http
        .get(url)
        .bearer_auth(gh.jwt(now_ms() / 1000)?)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugula-control")
        .send()
        .await?;
    let status = r.status().as_u16();
    let next = r.headers().get("link").and_then(|l| l.to_str().ok()).and_then(|l| {
        l.split(',').find_map(|part| {
            let (url, rel) = part.split_once(';')?;
            rel.contains("rel=\"next\"").then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_owned())
        })
    });
    Ok((status, r.json().await.unwrap_or_default(), next))
}

/// What's gone from the App.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gone {
    /// The installation: deleted, or suspended.
    Installation { deleted: bool },
    /// The repo, from the installation (taken out, deleted, blocked).
    Repo,
}

/// Whether `error` means the installation or the repo is gone: GitHub
/// wouldn't mint a token for the repo (404, 422), or its own read said 404
/// or 451. The App's view of the installation decides which; an answer
/// that isn't clear decides nothing (the turn backs off).
async fn gone(p: &Platform, gh: &GithubApp, job: &PollJob, error: &GhError, repo_read: bool) -> Option<Gone> {
    let minted = match error {
        GhError::Login(e) => mint_status(e),
        _ => None,
    };
    let read = repo_read
        && (matches!(error, GhError::NotFound(_)) || matches!(error, GhError::Http(m) if m.starts_with("451")));
    if !matches!(minted, Some(404 | 422)) && !read {
        return None;
    }
    match app_get(p, gh, &format!("app/installations/{}", job.installation_id)).await {
        Ok((404, ..)) => Some(Gone::Installation { deleted: true }),
        Ok((200, v, _)) if !v["suspended_at"].is_null() => Some(Gone::Installation { deleted: false }),
        Ok((200, ..)) => Some(Gone::Repo),
        Ok((status, ..)) => {
            warn!(installation = job.installation_id, status, "forge mirror: asking GitHub about an installation");
            None
        }
        Err(e) => {
            warn!(installation = job.installation_id, error = %e, "forge mirror: asking GitHub about an installation");
            None
        }
    }
}

/// Unlink what's gone, as the installation's events would: how many repos.
fn unlink(p: &Platform, gh: &GithubApp, installation: u64, repo: u64, what: Gone, now: u64) -> anyhow::Result<usize> {
    let n = match what {
        Gone::Installation { deleted } => {
            let i = p.db.installation(installation)?.unwrap_or(Installation {
                id: installation,
                account_id: 0,
                login: String::new(),
                account_type: String::new(),
                permissions: "{}".into(),
            });
            p.db.installation_gone(&i, deleted, now)?
        }
        Gone::Repo => p.db.repos_removed(installation, &[repo], now)?,
    };
    gh.forget();
    info!(installation, repo, ?what, unlinked = n, "forge mirror: gone from the App, unlinked");
    Ok(n)
}

/// One turn: claim a repo whose turn it is at `now`, and backfill or
/// reconcile it. `None` when no repo's turn has come.
pub async fn tick(p: &Platform, now: u64) -> anyhow::Result<Option<Turn>> {
    let Some(gh) = &p.github else { return Ok(None) };
    let Some(job) = p.db.claim_poll(now, gh.ingest.settings.every_ms)? else { return Ok(None) };
    let repo = job.repo_id;
    let tokens = RepoTokens {
        p,
        caller: "platform",
        purpose: Purpose::Ingest,
        installation: job.installation_id,
        repo_id: repo,
    };
    let client = Github::new(&gh.api, p.http.clone(), tokens).with_cache(gh.ingest.etags(job.installation_id));
    // Drift only counts in a reconcile whose last one was since linking (a
    // repo linked again missed its webhooks while it wasn't), from then on.
    let drift_from = job.reconciled_at.filter(|r| job.backfilled_at.is_some() && *r >= job.linked_at);
    let pages = gh.ingest.settings.pages.max(1);
    let mut poll = Poll { p, job, gh: client, drift_from, now, pages, fixed: 0, drift: 0, prs: vec![], issues: vec![] };
    let out = poll.run().await;
    // A reconcile done, and the repo's full pass due: the reconcile is
    // recorded first (and the pass's start, so it's tried once a day), and
    // the pass is best effort: however it stops, the reconcile stands.
    let every = gh.ingest.settings.full_every_ms;
    let last_full = poll.job.full_at.or(poll.job.backfilled_at);
    if matches!(out, Ok(Finish::Reconciled(false))) && full_due(repo, last_full, now, every) {
        match p.db.start_full_pass(&poll.job, now) {
            Ok(true) => match poll.full().await {
                Ok(()) => info!(repo, fixed = poll.fixed, "forge mirror: full pass"),
                Err(Stop::Budget(reset)) => {
                    gh.ingest.stats.backed_off(poll.job.installation_id, reset);
                    info!(repo, reset, "forge mirror: a full pass left the rate reserve be, done for the day");
                }
                Err(Stop::Lost) => info!(repo, "forge mirror: a full pass lost its lease"),
                Err(Stop::Gh { path, error, .. }) => {
                    warn!(repo, path, error = %error, "forge mirror: a full pass stopped, done for the day");
                }
                Err(Stop::Failed(e)) => {
                    warn!(repo, error = format!("{e:#}"), "forge mirror: a full pass stopped, done for the day");
                }
            },
            Ok(false) => {}
            Err(e) => warn!(repo, error = format!("{e:#}"), "forge mirror: recording a full pass's start"),
        }
    }
    // Whatever it wrote before it stopped, as a turn that ran out of pages did.
    crate::boards::auto::auto_attach_prs(p, repo, &poll.prs);
    let bodies = poll.merged_bodies().await;
    super::closes::merged_with(p, repo, &poll.prs, &bodies).await;
    // M205: an exported issue's comments whose webhook never came.
    super::export::polled(p, (repo, poll.job.installation_id, &poll.job.full_name), &poll.issues).await;
    let (job, fixed, drift) = (poll.job, poll.fixed, poll.drift);
    gh.ingest.stats.drift(repo, now, drift);
    match out {
        Ok(how) => {
            p.db.finish_poll(&job, &how, now)?;
            // A backfill's items reach the inbox quietly, as do a turn's that
            // ran out of pages (M171).
            super::inbox::polled(p, repo, !matches!(how, Finish::Reconciled(false) | Finish::Refetched));
            Ok(Some(match how {
                Finish::Reconciled(backfill) => {
                    if backfill || fixed > 0 {
                        info!(repo, backfill, fixed, drift, "forge mirror: poller turn");
                    }
                    Turn::Done { repo, backfill, fixed, drift }
                }
                Finish::Cursor(_) => {
                    info!(repo, fixed, drift, "forge mirror: poller turn ran out of pages, carrying on");
                    Turn::More { repo, fixed }
                }
                Finish::Refetched => Turn::Refetched { repo, fixed },
            }))
        }
        Err(Stop::Budget(reset)) => {
            p.db.release_poll(&job, now_ms() + BACKOFF_MS)?;
            gh.ingest.stats.backed_off(job.installation_id, reset);
            info!(repo, installation = job.installation_id, reset, "forge mirror: poller left the rate reserve be");
            Ok(Some(Turn::BackedOff { repo }))
        }
        Err(Stop::Lost) => {
            // An unlinked repo's purge needn't wait for the lease to run out.
            p.db.release_poll(&job, now_ms())?;
            Ok(Some(Turn::Lost { repo }))
        }
        Err(Stop::Gh { path, error, repo: repo_read }) => {
            if let Some(what) = gone(p, gh, &job, &error, repo_read).await {
                let unlinked = unlink(p, gh, job.installation_id, repo, what, now_ms())?;
                p.db.release_poll(&job, now_ms())?;
                return Ok(Some(Turn::Gone { repo, unlinked }));
            }
            let wait = p.db.fail_poll(&job, BACKOFF_MS, now_ms())?;
            Err(anyhow::anyhow!("{path}: {error}").context(format!("polling repo {repo} (again in {} s)", wait / 1000)))
        }
        Err(Stop::Failed(e)) => {
            let wait = p.db.fail_poll(&job, BACKOFF_MS, now_ms())?;
            Err(e.context(format!("polling repo {repo} (again in {} s)", wait / 1000)))
        }
    }
}

/// Every installation live links go through that GitHub no longer lists
/// (an uninstall whose webhook never came), or lists as suspended: its
/// repos unlinked. How many. Nothing changes unless every page was read.
pub async fn sweep(p: &Platform, now: u64) -> anyhow::Result<usize> {
    let Some(gh) = &p.github else { return Ok(0) };
    let linked = p.db.linked_installations()?;
    if linked.is_empty() {
        return Ok(0);
    }
    let mut listed: HashMap<u64, bool> = HashMap::new();
    let mut next = Some("app/installations?per_page=100".to_owned());
    for _ in 0..100 {
        let Some(path) = next.take() else { break };
        let (status, v, n) = app_get(p, gh, &path).await?;
        if status != 200 {
            bail!("GitHub said {status} listing the App's installations");
        }
        for i in v.as_array().into_iter().flatten() {
            if let Some(id) = i["id"].as_u64() {
                listed.insert(id, !i["suspended_at"].is_null());
            }
        }
        next = n;
    }
    if next.is_some() {
        bail!("too many pages of the App's installations");
    }
    let mut n = 0;
    for id in linked {
        let what = match listed.get(&id) {
            None => Gone::Installation { deleted: true },
            Some(true) => Gone::Installation { deleted: false },
            Some(false) => continue,
        };
        n += unlink(p, gh, id, 0, what, now)?;
    }
    Ok(n)
}

/// The poller on this machine, while there's an App: every [`TICK`], each
/// repo whose turn it is, until none is; the numbers, every
/// [`super::stats::LOG_EVERY`]; and on the leader, the sweep, every
/// [`SWEEP_EVERY`].
pub fn start(p: &Arc<Platform>) {
    if p.github.is_none() {
        return;
    }
    let app = Arc::downgrade(p);
    tokio::spawn(async move {
        loop {
            let Some(p) = app.upgrade() else { return };
            while !p.is_paused() && !p.is_stopping() {
                match tick(&p, now_ms()).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => break,
                    // The repo waits its back-off; the others, the next tick.
                    Err(e) => {
                        warn!(error = format!("{e:#}"), "forge mirror: a poller turn failed");
                        break;
                    }
                }
            }
            drop(p);
            tokio::time::sleep(TICK).await;
        }
    });
    let app = Arc::downgrade(p);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(super::stats::LOG_EVERY).await;
            let Some(p) = app.upgrade() else { return };
            super::stats::log(&p, now_ms());
        }
    });
    let app = Arc::downgrade(p);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SWEEP_EVERY).await;
            let Some(p) = app.upgrade() else { return };
            if p.is_leader() && !p.is_paused() && !p.is_stopping() {
                match sweep(&p, now_ms()).await {
                    Ok(n) if n > 0 => info!(unlinked = n, "forge mirror: swept the App's installations"),
                    Ok(_) => {}
                    Err(e) => warn!(error = format!("{e:#}"), "forge mirror: sweeping the App's installations"),
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{MERGE_CHECKS, MERGE_RECHECK_MS, MergeCandidate, merge_rechecks};
    use crate::db::forge::ForgeCheck;

    fn open(number: u64, state: Option<&str>, synced_at: u64, conclusion: Option<&str>) -> MergeCandidate {
        let checks = conclusion
            .map(|c| ForgeCheck {
                kind: "check_run".into(),
                name: "ci".into(),
                status: "completed".into(),
                conclusion: Some(c.into()),
                url: None,
                updated_at: 1,
            })
            .into_iter()
            .collect();
        MergeCandidate { number, merge_state: state.map(Into::into), synced_at, head_sha: format!("s{number}"), checks }
    }

    /// Unknown first, whatever their checks; then green ones whose rows are
    /// older than the recheck age, stalest first; at most a turn's worth.
    #[test]
    fn which_prs_are_read_again() {
        let now = 10 * MERGE_RECHECK_MS;
        let old = now - MERGE_RECHECK_MS;
        let fresh = now - 1000;
        let prs = [
            open(1, Some("clean"), old - 5, Some("success")),
            open(2, None, fresh, None),
            open(3, Some("dirty"), old - 9, Some("success")),
            open(4, Some("unknown"), fresh - 1, Some("failure")),
            open(5, Some("clean"), fresh, Some("success")),
            open(6, Some("dirty"), old - 20, Some("failure")),
            open(7, Some("blocked"), old, Some("success")),
        ];
        // 5 is fresh, 6 isn't green.
        assert_eq!(merge_rechecks(&prs, now), [4, 2, 3, 1, 7]);
        let many: Vec<MergeCandidate> = (1..=30).map(|n| open(n, None, 30 - n, None)).collect();
        let due = merge_rechecks(&many, now);
        assert_eq!(due.len(), MERGE_CHECKS);
        assert_eq!(due[..3], [30, 29, 28], "stalest first");
    }

    #[test]
    fn mint_statuses() {
        let e = "GitHub said 404 Not Found minting an installation token: Not Found";
        assert_eq!(super::mint_status(e), Some(404));
        assert_eq!(super::mint_status("timed out"), None);
    }
}
