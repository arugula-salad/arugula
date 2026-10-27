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
//! - **Webhook to row, by day**: the second again, counted per UTC day in
//!   [`EVENT_BUCKETS`], with this process's `run`. The window starts again
//!   at each deploy, so its p95 just after one is a handful of samples';
//!   the day's counts add up across machines and restarts (each run's
//!   last line), for the gate's day p95.
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

/// The day counts' upper bounds (ms), and one more for anything slower.
pub const EVENT_BUCKETS: [u64; 10] = [500, 1000, 2000, 3000, 4000, 5000, 7500, 10_000, 30_000, 60_000];

/// This process, in the day lines: when it first logged (epoch ms).
static RUN: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

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
    /// Day → event times counted in [`EVENT_BUCKETS`] (and the slower).
    event_days: BTreeMap<u64, [u64; EVENT_BUCKETS.len() + 1]>,
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
        self.landed_at(arugula_e2e::now_ms(), arrival, event);
    }

    fn landed_at(&self, now: u64, arrival: u64, event: Option<u64>) {
        let mut s = self.0.lock().unwrap();
        push(&mut s.arrival, arrival);
        if let Some(e) = event {
            push(&mut s.event, e);
            let day = now / DAY_MS;
            let i = EVENT_BUCKETS.iter().position(|b| e <= *b).unwrap_or(EVENT_BUCKETS.len());
            s.event_days.entry(day).or_default()[i] += 1;
            s.event_days.retain(|d, _| *d + 1 >= day);
        }
    }

    /// Yesterday's and today's event counts, by [`EVENT_BUCKETS`]: the day
    /// (epoch days) and its counts, yesterday's for a final line after
    /// midnight.
    pub fn event_days(&self, now: u64) -> Vec<(u64, [u64; EVENT_BUCKETS.len() + 1])> {
        let day = now / DAY_MS;
        let s = self.0.lock().unwrap();
        s.event_days.iter().filter(|(d, _)| **d + 1 >= day).map(|(d, c)| (*d, *c)).collect()
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
    let run = *RUN.get_or_init(|| now);
    for (day, counts) in s.event_days(now) {
        tracing::info!(
            day = %day_of(day),
            run,
            buckets = %buckets(&counts),
            "forge mirror: webhook to row by day"
        );
    }
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

/// `le_ms:count` for each bucket, the slower ones as `inf`.
fn buckets(counts: &[u64]) -> String {
    let le = EVENT_BUCKETS.iter().map(u64::to_string).chain(["inf".to_owned()]);
    le.zip(counts).map(|(b, n)| format!("{b}:{n}")).collect::<Vec<_>>().join(",")
}

/// `YYYY-MM-DD` of epoch days.
fn day_of(days: u64) -> String {
    // Civil from days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
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
    fn event_times_by_day() {
        let s = Stats::default();
        let day = 20_752; // 2026-10-26
        for e in [400, 500, 501, 4999, 5000, 5001, 61_000] {
            s.landed_at(day * DAY_MS + 5, 3, Some(e));
        }
        s.landed_at(day * DAY_MS + 6, 3, None);
        let got = s.event_days(day * DAY_MS + 10);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, [2, 1, 0, 0, 0, 2, 1, 0, 0, 0, 1]);
        assert_eq!(buckets(&got[0].1), "500:2,1000:1,2000:0,3000:0,4000:0,5000:2,7500:1,10000:0,30000:0,60000:0,inf:1");
        assert_eq!(day_of(day), "2026-10-26");
        // Yesterday's stays for its last line; the day before goes.
        s.landed_at((day + 1) * DAY_MS, 3, Some(100));
        assert_eq!(s.event_days((day + 1) * DAY_MS).iter().map(|(d, _)| *d).collect::<Vec<_>>(), [day, day + 1]);
        s.landed_at((day + 2) * DAY_MS, 3, Some(100));
        assert_eq!(s.event_days((day + 2) * DAY_MS).iter().map(|(d, _)| *d).collect::<Vec<_>>(), [day + 1, day + 2]);
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
