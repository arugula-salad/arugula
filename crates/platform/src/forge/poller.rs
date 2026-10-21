//! One leased poller per linked repo (M164, #1210), in `cloud::worker`'s
//! way: each machine claims a repo whose turn it is (`Db::claim_poll`; on
//! Postgres `FOR UPDATE SKIP LOCKED`, so two machines never poll the same
//! one), renews the lease between reads, and lets it go when it's done.
//!
//! - **Backfill**, on the first turn after linking: open PRs and issues,
//!   and everything updated in the last [`BACKFILL_DAYS`] days; the checks
//!   on open PRs' heads; each PR's reviews, as events.
//! - **Reconcile**, every few minutes (`Settings::every_ms`): what GitHub
//!   lists as updated since the last reconcile (less [`OVERLAP_MS`]), each
//!   such PR's reviews, and the checks on every open PR's head, compared
//!   with the rows. A row that was missing or wrong is **drift**: written
//!   now, counted (per repo per day, [`super::stats`]) and logged. A change
//!   younger than [`GRACE_MS`] isn't counted: its webhook may be on its way.
//! - **Reads** go through `arugula-forge-github`'s client, with
//!   installation tokens for the one repo, read only, minted as
//!   `Purpose::Ingest` (M167's helper, logged). Conditional GETs: an
//!   unchanged answer costs no rate limit.
//! - **Rate budget**: before each read, if less than `Settings::reserve`
//!   percent of the installation's limit is left, the turn stops, gives
//!   its lease up and the repo waits [`BACKOFF_MS`]: the rest is for
//!   reads on demand and sends.
//! - **Unlinking** stops a turn at its next read or write (`Db::mirror`
//!   and `Db::renew_poll` check the link), and M163's purge waits for the
//!   lease to run out.

use std::{sync::Arc, time::Duration};

use arugula_e2e::now_ms;
use arugula_forge_github::{Github, Purpose as GhPurpose, Token, TokenSource};
use futures_util::future::BoxFuture;
use serde_json::Value;
use tracing::{info, warn};

use super::{Caller, Purpose, Repo, ingest};
use crate::{
    Platform,
    db::mirror::{CheckRow, EventRow, ItemRow, PollJob, Wrote, add_event, upsert_check, upsert_item},
};

/// How far back a backfill reads closed items.
pub const BACKFILL_DAYS: u64 = 90;
/// How far before the last reconcile the next one reads from.
pub const OVERLAP_MS: u64 = 120_000;
/// A change GitHub made this recently isn't drift yet.
pub const GRACE_MS: u64 = 60_000;
/// How long a repo waits when its installation's budget is reserved.
pub const BACKOFF_MS: u64 = 60_000;
/// How often each machine looks for a repo whose turn it is.
const TICK: Duration = Duration::from_secs(5);
/// Most pages of one list a turn reads.
const PAGES: usize = 50;

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
    /// Left the installation's reserve be.
    BackedOff { repo: u64 },
    /// Lost its lease, or the repo was unlinked.
    Lost { repo: u64 },
}

/// A turn on one repo.
struct Poll<'p> {
    p: &'p Platform,
    job: PollJob,
    gh: Github<IngestTokens<'p>>,
    /// Count drift (a reconcile after one since linking), and before when.
    drift_before: Option<u64>,
    now: u64,
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

    async fn read(&self, path: &str) -> Result<Value, Stop> {
        self.may_read()?;
        let r = self.gh.read(&self.job.full_name, path).await;
        self.heard();
        r.map_err(|e| Stop::Failed(anyhow::anyhow!("{path}: {e}")))
    }

    /// A list, newest first or not, down to items updated before `since`.
    async fn list(&self, path: &str, since: Option<u64>) -> Result<Vec<Value>, Stop> {
        let mut out = Vec::new();
        let mut next = Some(path.to_owned());
        for _ in 0..PAGES {
            let Some(path) = next.take() else { break };
            self.may_read()?;
            let r = self.gh.page(&self.job.full_name, &path).await;
            self.heard();
            let (page, n) =
                r.map_err(|e| Stop::Failed(anyhow::anyhow!("{}: {e}", path.split('?').next().unwrap_or(""))))?;
            let mut done = false;
            for v in page {
                if since.is_some_and(|s| ingest::ms(&v["updated_at"]).unwrap_or(0) < s) {
                    done = true;
                    break;
                }
                out.push(v);
            }
            next = n.filter(|_| !done);
        }
        Ok(out)
    }

    /// Write rows under the lease, counting what was fixed, and as drift
    /// what GitHub changed before `drift_before`, by its time.
    fn write(&mut self, items: &[ItemRow], checks: &[CheckRow], events: &[EventRow]) -> Result<(), Stop> {
        let now = self.now;
        let wrote = self
            .p
            .db
            .mirror(self.job.repo_id, Some(&self.job.lease), |tx| {
                let mut out = Vec::new();
                for it in items {
                    out.push((upsert_item(tx, it, now)?, it.updated_at, format!("#{}", it.number), "item"));
                }
                for c in checks {
                    out.push((upsert_check(tx, c)?, c.updated_at, format!("{}:{}", c.head_sha, c.name), "check"));
                }
                for e in events {
                    out.push((add_event(tx, e)?, e.at, format!("#{}", e.number), "event"));
                }
                Ok(out)
            })?
            .ok_or(Stop::Lost)?;
        for (wrote, at, which, kind) in wrote {
            if !wrote.fixed() {
                continue;
            }
            self.fixed += 1;
            if self.drift_before.is_some_and(|before| at < before) {
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

    /// Each PR's reviews, as events.
    async fn reviews(&mut self, numbers: &[u64]) -> Result<(), Stop> {
        let id = self.job.repo_id;
        for n in numbers {
            let list = self.read(&format!("repositories/{id}/pulls/{n}/reviews?per_page=100")).await?;
            let events: Vec<EventRow> =
                list.as_array().into_iter().flatten().filter_map(|r| ingest::review_event(id, *n, r)).collect();
            self.write(&[], &[], &events)?;
        }
        Ok(())
    }

    /// The checks on every open PR's head.
    async fn checks(&mut self) -> Result<(), Stop> {
        let id = self.job.repo_id;
        for (_, sha) in self.p.db.open_heads(id)? {
            let runs = self.read(&format!("repositories/{id}/commits/{sha}/check-runs?per_page=100")).await?;
            let status = self.read(&format!("repositories/{id}/commits/{sha}/status?per_page=100")).await?;
            let mut rows: Vec<CheckRow> =
                runs["check_runs"].as_array().into_iter().flatten().filter_map(|r| ingest::run_row(id, r)).collect();
            rows.extend(
                status["statuses"].as_array().into_iter().flatten().filter_map(|s| ingest::status_row(id, &sha, s)),
            );
            self.write(&[], &rows, &[])?;
        }
        Ok(())
    }

    /// PRs and issues from these lists: written, and the PRs' numbers.
    async fn items(&mut self, prs: &[(&str, Option<u64>)], issues: &[(&str, Option<u64>)]) -> Result<Vec<u64>, Stop> {
        let (id, name) = (self.job.repo_id, self.job.full_name.clone());
        let mut numbers = Vec::new();
        for (q, since) in prs {
            let list = self.list(&format!("repositories/{id}/pulls?{q}&per_page=100"), *since).await?;
            let rows: Vec<ItemRow> = list.iter().filter_map(|v| ingest::pr_row(id, &name, v)).collect();
            numbers.extend(rows.iter().map(|r| r.number));
            self.write(&rows, &[], &[])?;
        }
        for (q, since) in issues {
            let list = self.list(&format!("repositories/{id}/issues?{q}&per_page=100"), *since).await?;
            let rows: Vec<ItemRow> = list.iter().filter_map(|v| ingest::issue_row(id, v)).collect();
            self.write(&rows, &[], &[])?;
        }
        numbers.sort_unstable();
        numbers.dedup();
        Ok(numbers)
    }

    async fn run(&mut self) -> Result<bool, Stop> {
        let backfill = self.job.backfilled_at.is_none();
        let numbers = if backfill {
            let since = Some(self.now.saturating_sub(BACKFILL_DAYS * 86_400_000));
            let recent = "state=closed&sort=updated&direction=desc";
            self.items(&[("state=open", None), (recent, since)], &[("state=open", None), (recent, since)]).await?
        } else {
            let since = Some(self.job.reconciled_at.unwrap_or(0).saturating_sub(OVERLAP_MS));
            let all = "state=all&sort=updated&direction=desc";
            self.items(&[(all, since)], &[(all, since)]).await?
        };
        self.reviews(&numbers).await?;
        self.checks().await?;
        Ok(backfill)
    }
}

/// One turn: claim a repo whose turn it is at `now`, and backfill or
/// reconcile it. `None` when no repo's turn has come.
pub async fn tick(p: &Platform, now: u64) -> anyhow::Result<Option<Turn>> {
    let Some(gh) = &p.github else { return Ok(None) };
    let Some(job) = p.db.claim_poll(now, gh.ingest.settings.every_ms)? else { return Ok(None) };
    let repo = job.repo_id;
    let tokens = IngestTokens { p, installation: job.installation_id, repo_id: repo };
    let client = Github::new(&gh.api, p.http.clone(), tokens).with_cache(gh.ingest.etags(job.installation_id));
    // Drift only counts from a reconcile whose last one was since linking:
    // a repo linked again missed its webhooks while it wasn't.
    let drift_before = job
        .reconciled_at
        .filter(|r| job.backfilled_at.is_some() && *r >= job.linked_at)
        .map(|_| now.saturating_sub(GRACE_MS));
    let mut poll = Poll { p, job, gh: client, drift_before, now, fixed: 0, drift: 0 };
    let out = poll.run().await;
    let (job, fixed, drift) = (poll.job, poll.fixed, poll.drift);
    gh.ingest.stats.drift(repo, now, drift);
    match out {
        Ok(backfill) => {
            p.db.finish_poll(&job, backfill, now)?;
            if backfill || fixed > 0 {
                info!(repo, backfill, fixed, drift, "forge mirror: poller turn");
            }
            Ok(Some(Turn::Done { repo, backfill, fixed, drift }))
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
        Err(Stop::Failed(e)) => {
            // Its lease runs out and a later turn tries again.
            p.db.release_poll(&job, now_ms() + BACKOFF_MS)?;
            Err(e.context(format!("polling repo {repo}")))
        }
    }
}

/// The poller on this machine, while there's an App: every [`TICK`], each
/// repo whose turn it is, until none is; and the numbers, every
/// [`super::stats::LOG_EVERY`].
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
}
