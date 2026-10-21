//! One leased poller per linked repo (M164, #1210), in `cloud::worker`'s
//! way: each machine claims a repo whose turn it is (`Db::claim_poll`; on
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
//!   history (reviews of a PR updated again) is written, not counted.
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
//! - **Unlinking** stops a turn at its next read or write (`Db::mirror`
//!   and `Db::renew_poll` check the link), and M163's purge waits for the
//!   lease to run out.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context as _, bail};
use arugula_e2e::now_ms;
use arugula_forge_github::{Error as GhError, Github, Purpose as GhPurpose, Token, TokenSource};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use super::{Caller, GithubApp, Purpose, Repo, ingest};
use crate::{
    Platform,
    db::{
        forge::Installation,
        mirror::{
            self, CheckRow, EventRow, Finish, ItemRow, PollJob, Source, Wrote, add_event, upsert_check, upsert_item,
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

/// Installation tokens for the one repo, read only (`Purpose::Ingest`).
struct IngestTokens<'p> {
    p: &'p Platform,
    installation: u64,
    repo_id: u64,
}

impl TokenSource for IngestTokens<'_> {
    fn token<'a>(
        &'a self,
        _repo: &'a str,
        purpose: GhPurpose,
        _fresh: bool,
    ) -> BoxFuture<'a, Result<Option<Token>, String>> {
        Box::pin(async move {
            // The poller only reads.
            if purpose != GhPurpose::Read {
                return Ok(None);
            }
            let gh = self.p.github.as_ref().ok_or(super::NO_APP)?;
            let caller = Caller::of(self.p, "platform");
            let t = gh
                .read_token(&caller, self.installation, Repo::Id(self.repo_id), Purpose::Ingest)
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
    gh: Github<IngestTokens<'p>>,
    /// Count drift from GitHub changes since then (the last reconcile),
    /// when this is a reconcile after one since linking.
    drift_from: Option<u64>,
    now: u64,
    pages: usize,
    fixed: usize,
    drift: u64,
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
                out.push((upsert_item(tx, it, from, now)?, it.updated_at, format!("#{}", it.number), "item"));
            }
            for c in checks {
                out.push((
                    upsert_check(tx, c, from, now)?,
                    c.updated_at,
                    format!("{}:{}", c.head_sha, c.name),
                    "check",
                ));
            }
            for e in events {
                out.push((add_event(tx, e)?, e.at, format!("#{}", e.number), "event"));
            }
            Ok(out)
        })?;
        let before = now.saturating_sub(GRACE_MS);
        for (wrote, at, which, kind) in wrote {
            if !wrote.fixed() {
                continue;
            }
            self.fixed += 1;
            if from == Source::Pushed && self.drift_from.is_some_and(|since| since <= at && at < before) {
                self.drift += 1;
                let fields = match &wrote {
                    Wrote::Changed(f) => f.join(","),
                    _ => "missing".into(),
                };
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
        for (_, sha) in self.p.db.open_heads(id)? {
            let rows = self.head(&sha).await?;
            self.write(&[], &rows, &[], Source::Pushed)?;
        }
        let before = self.now.saturating_sub(KEEP_UNHEADED_MS);
        self.tx(|tx| mirror::prune_checks(tx, id, before))?;
        Ok(())
    }

    /// What two answers tied on, read again: GitHub's answer now is taken.
    async fn refetch(&mut self) -> Result<(), Stop> {
        let (id, name) = (self.job.repo_id, self.job.full_name.clone());
        for (kind, key) in self.p.db.refetches(id)? {
            match kind.as_str() {
                "pr" => {
                    let v = self.read(&format!("repositories/{id}/pulls/{key}")).await?;
                    let rows: Vec<ItemRow> = ingest::pr_row(id, &name, &v).into_iter().collect();
                    let numbers: Vec<u64> = rows.iter().map(|r| r.number).collect();
                    self.write(&rows, &[], &[], Source::Fetched)?;
                    self.reviews(&numbers, Source::Fetched).await?;
                }
                "issue" => match self.read(&format!("repositories/{id}/issues/{key}")).await {
                    Ok(v) => {
                        let rows: Vec<ItemRow> = ingest::issue_row(id, &v).into_iter().collect();
                        self.write(&rows, &[], &[], Source::Fetched)?;
                    }
                    // Deleted (410), or moved to another repo (404).
                    Err(Stop::Gh { error, .. })
                        if matches!(&error, GhError::NotFound(_))
                            || matches!(&error, GhError::Http(m) if m.starts_with("410")) =>
                    {
                        let n: u64 = key.parse().unwrap_or(0);
                        self.tx(|tx| mirror::delete_item(tx, id, n))?;
                    }
                    Err(s) => return Err(s),
                },
                "head" => {
                    let rows = self.head(&key).await?;
                    self.write(&[], &rows, &[], Source::Fetched)?;
                }
                _ => {}
            }
            self.tx(|tx| mirror::refetched(tx, id, &kind, &key))?;
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
            if prs {
                let rows: Vec<ItemRow> = values.iter().filter_map(|v| ingest::pr_row(id, &name, v)).collect();
                let numbers: Vec<u64> = rows.iter().map(|r| r.number).collect();
                self.write(&rows, &[], &[], Source::Pushed)?;
                self.reviews(&numbers, Source::Pushed).await?;
            } else {
                let rows: Vec<ItemRow> = values.iter().filter_map(|v| ingest::issue_row(id, v)).collect();
                self.write(&rows, &[], &[], Source::Pushed)?;
            }
            next = n.filter(|_| !done);
        }
        Ok(None)
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
    let tokens = IngestTokens { p, installation: job.installation_id, repo_id: repo };
    let client = Github::new(&gh.api, p.http.clone(), tokens).with_cache(gh.ingest.etags(job.installation_id));
    // Drift only counts in a reconcile whose last one was since linking (a
    // repo linked again missed its webhooks while it wasn't), from then on.
    let drift_from = job.reconciled_at.filter(|r| job.backfilled_at.is_some() && *r >= job.linked_at);
    let pages = gh.ingest.settings.pages.max(1);
    let mut poll = Poll { p, job, gh: client, drift_from, now, pages, fixed: 0, drift: 0 };
    let out = poll.run().await;
    let (job, fixed, drift) = (poll.job, poll.fixed, poll.drift);
    gh.ingest.stats.drift(repo, now, drift);
    match out {
        Ok(how) => {
            p.db.finish_poll(&job, &how, now)?;
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
    #[test]
    fn mint_statuses() {
        let e = "GitHub said 404 Not Found minting an installation token: Not Found";
        assert_eq!(super::mint_status(e), Some(404));
        assert_eq!(super::mint_status("timed out"), None);
    }
}
