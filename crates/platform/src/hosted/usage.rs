//! Hosted workspaces' usage (M188, #1249): how long each box ran, by its
//! size, per workspace and per project, and what that costs on E2B.
//!
//! - **Metered from the lifecycle.** Each worker checkpoint says whether
//!   the workspace's box runs ([`observed`]), and [`changes`] turns that into
//!   running intervals (`hosted_usage`), in the checkpoint's transaction: a
//!   box runs from its create or a wake to its pause or its delete. A
//!   workspace has at most one open interval, and no interval starts before
//!   the one before it ended, so nothing is counted twice.
//! - **A lost event.** A box that paused itself at its timeout while the
//!   worker was away (or whose pause's checkpoint was lost) is seen paused
//!   by the worker's next status read, and its interval closes at the time
//!   the provider says it paused. One running that the platform thought
//!   wasn't opens one at the provider's start time, never before the last
//!   one ended. A box found gone closes at the time it was found.
//! - **Reconciled with E2B.** E2B has no usage or billing API (its docs
//!   point at the dashboard), but its lifecycle events carry each run as it
//!   billed it: a pause's and a delete's `execution` (start, milliseconds,
//!   vCPUs, MiB). [`reconcile`] reads them for each box the platform
//!   stopped metering ([`crate::compute::Substrate::runs`]) into
//!   `hosted_usage_provider`; loam keeps none. [`report`] puts the two side
//!   by side.
//! - **Cost per active hour** is [`PRICES`] times the seconds kept:
//!   `arugula-platform usage` ([`crate::window`]) prints it, read-only.

use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering::SeqCst},
    time::Duration,
};

use anyhow::Result;

use crate::{Platform, compute::Run, db::hosted::HostedJob};

/// A box's size, what E2B bills by: vCPUs and memory in MiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Class {
    pub cpus: u32,
    pub memory_mb: u32,
}

impl std::fmt::Display for Class {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} vCPU, {} MiB", self.cpus, self.memory_mb)
    }
}

/// What a step learned of a workspace's box from the provider, beside its
/// state (set by the worker, never kept on the row).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// When the provider says the box started running.
    pub started_at: Option<u64>,
    /// When it paused itself, at its timeout.
    pub paused_at: Option<u64>,
    pub class: Option<Class>,
}

/// Whether a workspace's box runs after a step, at `at`: `None` when the
/// step says nothing either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub sandbox: Option<String>,
    pub running: Option<bool>,
    pub seen: Seen,
    pub at: u64,
}

/// What a step that went through says of the box: it runs from the
/// moment it's made (E2B bills from its create) until it's paused, failed
/// or deleted. A delete that's asked but not yet done says nothing: the
/// box may be running or paused.
pub fn observed(job: &HostedJob, at: u64) -> Observed {
    let running = match job.w.state.as_str() {
        _ if job.deleted_at.is_some() => Some(false),
        "deleting" => None,
        "creating" | "enrolling" | "ready" => Some(job.sandbox.is_some()),
        _ => Some(false),
    };
    Observed { sandbox: job.sandbox.clone(), running, seen: job.seen.clone(), at }
}

/// A workspace's open interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub sandbox: String,
    pub started_at: u64,
    pub class: Option<Class>,
}

/// What to write to `hosted_usage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Close {
        started_at: u64,
        ended_at: u64,
    },
    /// An interval that would close as it opened: nothing ran.
    Drop {
        started_at: u64,
    },
    Open {
        sandbox: String,
        started_at: u64,
        class: Option<Class>,
    },
    /// The open interval's box's size, once the provider says it.
    Size {
        started_at: u64,
        class: Class,
    },
}

/// What an observation does to a workspace's intervals: `open` is the one
/// open now, if any, and `last_end` when the latest closed one ended (0
/// for none). A new interval never starts before `last_end`, and one that
/// closes never ends before it started or after `o.at`.
pub fn changes(open: Option<&Open>, last_end: u64, o: &Observed) -> Vec<Change> {
    let mut out = Vec::new();
    let Some(running) = o.running else { return out };
    let mut floor = last_end;
    if let Some(op) = open {
        let same = running && o.sandbox.as_deref() == Some(op.sandbox.as_str());
        if same {
            if let (None, Some(class)) = (op.class, o.seen.class) {
                out.push(Change::Size { started_at: op.started_at, class });
            }
            return out;
        }
        // It stopped (at the provider's pause time, when it said), or
        // another box runs in its place.
        let end = if running { o.at } else { o.seen.paused_at.unwrap_or(o.at).min(o.at) };
        let end = end.max(op.started_at);
        out.push(if end == op.started_at {
            Change::Drop { started_at: op.started_at }
        } else {
            Change::Close { started_at: op.started_at, ended_at: end }
        });
        floor = floor.max(end);
    }
    if let (true, Some(sandbox)) = (running, &o.sandbox) {
        let start = o.seen.started_at.unwrap_or(o.at).min(o.at).max(floor);
        out.push(Change::Open { sandbox: sandbox.clone(), started_at: start, class: o.seen.class });
    }
    out
}

/// The part of an interval inside `[from, to)`, in ms; an open one runs to
/// `to`.
pub fn active_ms(started_at: u64, ended_at: Option<u64>, from: u64, to: u64) -> u64 {
    let end = ended_at.unwrap_or(to).min(to);
    end.saturating_sub(started_at.max(from))
}

/// E2B's usage prices.
#[derive(Debug, Clone, Copy)]
pub struct Prices {
    pub source: &'static str,
    pub checked: &'static str,
    pub usd_per_cpu_second: f64,
    pub usd_per_gib_second: f64,
    /// What a box whose size the provider never said is costed as: E2B's
    /// default sandbox.
    pub default: Class,
}

/// From E2B's pricing page, checked 2026-10-21: "$0.000014 per
/// vCPU-second" and "$0.0000045 per GiB-second", storage free, and "the
/// default sandbox has 2 vCPUs and 4 GiB of RAM". A paused box costs
/// nothing: "once a sandbox is paused, killed or times out, billing stops
/// immediately" (<https://docs.e2b.dev/billing>). Pro's $150 a month is on
/// top, whatever the use.
pub const PRICES: Prices = Prices {
    source: "https://e2b.dev/pricing",
    checked: "2026-10-21",
    usd_per_cpu_second: 0.000014,
    usd_per_gib_second: 0.0000045,
    default: Class { cpus: 2, memory_mb: 4096 },
};

impl Prices {
    /// What `ms` of a box of `class` costs, in dollars.
    pub fn cost(&self, class: Class, ms: u64) -> f64 {
        let seconds = ms as f64 / 1000.0;
        seconds
            * (f64::from(class.cpus) * self.usd_per_cpu_second
                + f64::from(class.memory_mb) / 1024.0 * self.usd_per_gib_second)
    }

    /// An hour of a box of `class`, in dollars.
    pub fn per_hour(&self, class: Class) -> f64 {
        self.cost(class, 3_600_000)
    }
}

/// An interval as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub workspace: String,
    pub project: String,
    pub class: Option<Class>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Its workspace's purpose: `workspace`, or `deploy` for a box made for
    /// one deploy's run (M219), which the report counts apart too.
    pub purpose: String,
}

/// A box the provider has a record of, beside the platform's: both in ms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub workspace: String,
    pub provider_ms: u64,
    /// What the provider billed it, in vCPU-ms and MiB-ms.
    pub provider_cpu_ms: u64,
    pub provider_mib_ms: u64,
    pub metered_ms: u64,
}

/// A project's (or every project's) running time by size, in a window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub ms: BTreeMap<Option<Class>, u64>,
    pub workspaces: std::collections::BTreeSet<String>,
}

impl Totals {
    pub fn total_ms(&self) -> u64 {
        self.ms.values().sum()
    }

    /// What it cost, at [`PRICES`] (a box of no known size at its default).
    pub fn cost(&self) -> f64 {
        self.ms.iter().map(|(c, ms)| PRICES.cost(c.unwrap_or(PRICES.default), *ms)).sum()
    }

    /// Dollars per hour a box ran, across every size: `None` when none ran.
    pub fn per_active_hour(&self) -> Option<f64> {
        let ms = self.total_ms();
        (ms > 0).then(|| self.cost() / (ms as f64 / 3_600_000.0))
    }
}

/// Usage in `[from, to)`: every project's, each project's, and the
/// provider's record beside the platform's.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub from: u64,
    pub to: u64,
    pub all: Totals,
    pub projects: BTreeMap<String, Totals>,
    /// Deploys' boxes (M219), which are in `all` and `projects` too.
    pub deploys: Totals,
    pub checked: Vec<Checked>,
}

/// [`Report`] from the intervals that overlap the window and the boxes
/// the provider has a record of.
pub fn report(intervals: &[Interval], checked: Vec<Checked>, from: u64, to: u64) -> Report {
    let mut r = Report { from, to, checked, ..Default::default() };
    for i in intervals {
        let ms = active_ms(i.started_at, i.ended_at, from, to);
        if ms == 0 {
            continue;
        }
        for t in [&mut r.all, r.projects.entry(i.project.clone()).or_default()] {
            *t.ms.entry(i.class).or_default() += ms;
            t.workspaces.insert(i.workspace.clone());
        }
        if i.purpose == crate::db::hosted::DEPLOY {
            *r.deploys.ms.entry(i.class).or_default() += ms;
            r.deploys.workspaces.insert(i.workspace.clone());
        }
    }
    r
}

fn hours(ms: u64) -> f64 {
    ms as f64 / 3_600_000.0
}

fn class_name(c: Option<Class>) -> String {
    c.map_or_else(|| format!("size not seen (as {})", PRICES.default), |c| c.to_string())
}

impl Report {
    /// As `arugula-platform usage` prints it.
    pub fn render(&self, at: &str, days: u64) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(s, "hosted workspaces' usage in {at}, the last {days} days (read-only)");
        let _ = writeln!(
            s,
            "prices: ${}/vCPU-s, ${}/GiB-s ({}, checked {}); paused boxes cost nothing\n",
            PRICES.usd_per_cpu_second, PRICES.usd_per_gib_second, PRICES.source, PRICES.checked
        );
        let _ = writeln!(s, "{:<36} {:>10} {:>10} {:>10}", "size", "hours", "$", "$/hour");
        for (c, ms) in &self.all.ms {
            let class = c.unwrap_or(PRICES.default);
            let _ = writeln!(
                s,
                "{:<36} {:>10.2} {:>10.2} {:>10.4}",
                class_name(*c),
                hours(*ms),
                PRICES.cost(class, *ms),
                PRICES.per_hour(class)
            );
        }
        let _ = writeln!(
            s,
            "\n{} workspaces ran {:.2} hours: ${:.2}, ${} per active hour",
            self.all.workspaces.len(),
            hours(self.all.total_ms()),
            self.all.cost(),
            self.all.per_active_hour().map_or_else(|| "-".into(), |c| format!("{c:.4}"))
        );
        if !self.deploys.workspaces.is_empty() {
            let _ = writeln!(
                s,
                "of which {} deploys' boxes (M219) ran {:.2} hours: ${:.2}",
                self.deploys.workspaces.len(),
                hours(self.deploys.total_ms()),
                self.deploys.cost()
            );
        }
        if !self.projects.is_empty() {
            let _ = writeln!(s, "\n{:<40} {:>10} {:>10} {:>10}", "project", "workspaces", "hours", "$");
            for (p, t) in &self.projects {
                let _ =
                    writeln!(s, "{p:<40} {:>10} {:>10.2} {:>10.2}", t.workspaces.len(), hours(t.total_ms()), t.cost());
            }
        }
        let provider: u64 = self.checked.iter().map(|c| c.provider_ms).sum();
        let metered: u64 = self.checked.iter().map(|c| c.metered_ms).sum();
        if self.checked.is_empty() {
            let _ = writeln!(s, "\nno box here has a provider record yet (E2B's lifecycle events; loam keeps none)");
        } else {
            let billed: f64 = self
                .checked
                .iter()
                .map(|c| {
                    c.provider_cpu_ms as f64 / 1000.0 * PRICES.usd_per_cpu_second
                        + c.provider_mib_ms as f64 / 1000.0 / 1024.0 * PRICES.usd_per_gib_second
                })
                .sum();
            let diff = if provider == 0 { 0.0 } else { (metered as f64 - provider as f64) / provider as f64 * 100.0 };
            let _ = writeln!(
                s,
                "\n{} boxes checked against E2B's record, whole lives: E2B {:.2} hours (${billed:.2}), metered {:.2} \
                 hours ({diff:+.1}%)",
                self.checked.len(),
                hours(provider),
                hours(metered),
            );
        }
        s
    }
}

/// How long after a box stopped its provider record is read: E2B's events
/// lag its lifecycle by seconds.
pub const SETTLE_MS: u64 = 10 * 60 * 1000;
/// How often the leader reconciles, and how many boxes at a time.
const RECONCILE_EVERY: Duration = Duration::from_secs(10 * 60);
const RECONCILE_BATCH: usize = 20;

/// Read the provider's record of each box that stopped running a while
/// ago and wasn't read since: how many were.
pub async fn reconcile(app: &Platform, now: u64) -> Result<usize> {
    let Some(sub) = app.compute.clone() else { return Ok(0) };
    let due = app.db.hosted_usage_due(now, SETTLE_MS, RECONCILE_BATCH)?;
    let mut n = 0;
    for (sandbox, workspace, project) in due {
        let runs: Option<Vec<Run>> = sub.runs(&sandbox).await?;
        app.db.set_hosted_usage_provider(&sandbox, &workspace, &project, runs.as_deref(), now)?;
        n += 1;
    }
    Ok(n)
}

/// The leader's reconcile, every few minutes.
pub fn start(app: Arc<Platform>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RECONCILE_EVERY).await;
            if app.is_paused() || app.is_stopping() || !app.is_leader() {
                continue;
            }
            app.busy.fetch_add(1, SeqCst);
            if let Err(error) = reconcile(&app, arugula_e2e::now_ms()).await {
                tracing::warn!(%error, "reconciling hosted usage with the provider failed");
            }
            app.busy.fetch_sub(1, SeqCst);
        }
    });
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
