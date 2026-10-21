//! The forge mirror's numbers (M164), on each machine, in memory: what
//! #1204's gate is read from. Each is logged every [`LOG_EVERY`]
//! ([`log`]), with a message starting `forge mirror:` (docs/control-ops.md,
//! "The forge mirror").
//!
//! - **Webhook to row**: for each delivery that wrote a row, the time from
//!   its arrival to the row's commit, and from GitHub's own time for the
//!   change (the item's `updated_at`, a check's, a comment's) to the
//!   commit. The newest [`WINDOW`] of each, for p50 and p95. GitHub's
//!   times are whole seconds, so the second is the fairer gate number,
//!   and a little pessimistic.
//! - **Drift**: rows the poller found missing or wrong that a webhook
//!   should have written, per repo per day (UTC), for drift per thousand
//!   items a day.
//! - **Rate budget**: each installation's GitHub rate limit as last heard
//!   (`x-ratelimit-*`), and whether the poller has backed off it.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Mutex,
    time::Duration,
};

/// How many webhook-to-row times are kept, the newest.
pub const WINDOW: usize = 1000;

/// How often the numbers are logged.
pub const LOG_EVERY: Duration = Duration::from_secs(300);

const DAY_MS: u64 = 86_400_000;

/// An installation's GitHub rate limit, as last heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Budget {
    pub limit: u64,
    pub remaining: u64,
    /// When the window resets (epoch seconds).
    pub reset: u64,
}

impl Budget {
    /// The share of the limit used, in percent.
    pub fn used_pct(&self) -> u64 {
        ((self.limit - self.remaining.min(self.limit)) * 100).checked_div(self.limit).unwrap_or(0)
    }
}

/// p50 and p95 of the window, in ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Latency {
    pub samples: usize,
    pub arrival_p50: u64,
    pub arrival_p95: u64,
    /// From GitHub's time for the change, where the payload had one.
    pub event_samples: usize,
    pub event_p50: u64,
    pub event_p95: u64,
}

#[derive(Default)]
struct Inner {
    arrival: VecDeque<u64>,
    event: VecDeque<u64>,
    /// (day, repo) → rows fixed.
    drift: BTreeMap<(u64, u64), u64>,
    budgets: HashMap<u64, Budget>,
    /// Installations the poller backed off, and until when (epoch s).
    backed_off: HashMap<u64, u64>,
}

#[derive(Default)]
pub struct Stats(Mutex<Inner>);

fn push(q: &mut VecDeque<u64>, v: u64) {
    if q.len() >= WINDOW {
        q.pop_front();
    }
    q.push_back(v);
}

/// The `p`th percentile (nearest rank) of `q`.
fn percentile(q: &VecDeque<u64>, p: u64) -> u64 {
    if q.is_empty() {
        return 0;
    }
    let mut v: Vec<u64> = q.iter().copied().collect();
    v.sort_unstable();
    let rank = (v.len() as u64 * p).div_ceil(100).max(1) as usize;
    v[rank - 1]
}

impl Stats {
    /// A delivery's rows landed: `arrival` ms after it came, and `event` ms
    /// after GitHub's time for the change, if it said one.
    pub fn landed(&self, arrival: u64, event: Option<u64>) {
        let mut s = self.0.lock().unwrap();
        push(&mut s.arrival, arrival);
        if let Some(e) = event {
            push(&mut s.event, e);
        }
    }

    pub fn latency(&self) -> Latency {
        let s = self.0.lock().unwrap();
        Latency {
            samples: s.arrival.len(),
            arrival_p50: percentile(&s.arrival, 50),
            arrival_p95: percentile(&s.arrival, 95),
            event_samples: s.event.len(),
            event_p50: percentile(&s.event, 50),
            event_p95: percentile(&s.event, 95),
        }
    }

    /// The poller fixed `n` rows of `repo` that webhooks should have
    /// written. Yesterday's count is kept for the day's last log line.
    pub fn drift(&self, repo: u64, now: u64, n: u64) {
        if n == 0 {
            return;
        }
        let day = now / DAY_MS;
        let mut s = self.0.lock().unwrap();
        *s.drift.entry((day, repo)).or_default() += n;
        s.drift.retain(|(d, _), _| *d + 1 >= day);
    }

    /// Each repo's drift on the day `now` is in.
    pub fn drift_today(&self, now: u64) -> BTreeMap<u64, u64> {
        let day = now / DAY_MS;
        let s = self.0.lock().unwrap();
        s.drift.iter().filter(|((d, _), _)| *d == day).map(|((_, r), n)| (*r, *n)).collect()
    }

    /// What GitHub last said of an installation's limit.
    pub fn heard(&self, installation: u64, limit: Option<u64>, remaining: Option<u64>, reset: Option<u64>) {
        let (Some(limit), Some(remaining)) = (limit, remaining) else { return };
        let mut s = self.0.lock().unwrap();
        s.budgets.insert(installation, Budget { limit, remaining, reset: reset.unwrap_or(0) });
    }

    pub fn budget(&self, installation: u64) -> Option<Budget> {
        self.0.lock().unwrap().budgets.get(&installation).copied()
    }

    /// Whether the poller must leave `installation`'s limit be: less than
    /// `reserve` percent of it is left and it hasn't reset yet. When it
    /// resets (epoch s), if so.
    pub fn reserved(&self, installation: u64, reserve: u8, now_s: u64) -> Option<u64> {
        let b = self.budget(installation)?;
        let keep = b.limit * u64::from(reserve.min(100)) / 100;
        (b.remaining < keep.max(1) && b.reset > now_s).then_some(b.reset)
    }

    /// The poller backed off an installation until `until` (epoch s).
    pub fn backed_off(&self, installation: u64, until: u64) {
        self.0.lock().unwrap().backed_off.insert(installation, until);
    }

    /// Every installation's budget, and whether the poller is backing off
    /// it now.
    pub fn budgets(&self, now_s: u64) -> Vec<(u64, Budget, bool)> {
        let mut s = self.0.lock().unwrap();
        s.backed_off.retain(|_, until| *until > now_s);
        let mut out: Vec<_> = s.budgets.iter().map(|(i, b)| (*i, *b, s.backed_off.contains_key(i))).collect();
        out.sort_by_key(|(i, ..)| *i);
        out
    }
}

/// Log the numbers: webhook to row, each linked repo's drift today (per
/// thousand of its items), and each installation's rate budget.
pub fn log(p: &crate::Platform, now: u64) {
    let Some(gh) = &p.github else { return };
    let s = &gh.ingest.stats;
    let l = s.latency();
    tracing::info!(
        samples = l.samples,
        arrival_p50_ms = l.arrival_p50,
        arrival_p95_ms = l.arrival_p95,
        event_samples = l.event_samples,
        event_p50_ms = l.event_p50,
        event_p95_ms = l.event_p95,
        "forge mirror: webhook to row"
    );
    let drift = s.drift_today(now);
    match p.db.mirror_sizes() {
        Ok(sizes) => {
            for (repo, items) in sizes {
                let n = drift.get(&repo).copied().unwrap_or(0);
                let per_thousand = if items == 0 { 0.0 } else { n as f64 * 1000.0 / items as f64 };
                tracing::info!(repo, items, drift = n, per_thousand, "forge mirror: drift today");
            }
        }
        Err(e) => tracing::warn!(error = %e, "counting the forge mirror's items"),
    }
    for (installation, b, backing_off) in s.budgets(now / 1000) {
        tracing::info!(
            installation,
            limit = b.limit,
            remaining = b.remaining,
            used_pct = b.used_pct(),
            reserve_pct = gh.ingest.settings.reserve,
            backing_off,
            "forge mirror: rate budget"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_the_window() {
        let s = Stats::default();
        assert_eq!(s.latency(), Latency::default());
        for v in 1..=100 {
            s.landed(v, (v % 2 == 0).then_some(v * 10));
        }
        let l = s.latency();
        assert_eq!((l.samples, l.arrival_p50, l.arrival_p95), (100, 50, 95));
        assert_eq!((l.event_samples, l.event_p95), (50, 960));
        for _ in 0..WINDOW {
            s.landed(7, None);
        }
        assert_eq!(s.latency().arrival_p95, 7, "only the newest");
    }

    #[test]
    fn drift_by_day_and_the_reserve() {
        let s = Stats::default();
        s.drift(1, DAY_MS * 3 + 5, 2);
        s.drift(1, DAY_MS * 3 + 9, 1);
        s.drift(2, DAY_MS * 2, 4);
        assert_eq!(s.drift_today(DAY_MS * 3 + 10), BTreeMap::from([(1, 3)]));
        s.drift(1, DAY_MS * 5, 1);
        assert_eq!(s.drift_today(DAY_MS * 3), BTreeMap::new(), "two days on, gone");

        assert_eq!(s.reserved(9, 30, 100), None, "nothing heard yet");
        s.heard(9, Some(5000), Some(1600), Some(200));
        assert_eq!(s.reserved(9, 30, 100), None);
        s.heard(9, Some(5000), Some(1499), Some(200));
        assert_eq!(s.reserved(9, 30, 100), Some(200));
        assert_eq!(s.reserved(9, 30, 200), None, "it reset");
        assert_eq!(s.budget(9).unwrap().used_pct(), 70);
    }
}
