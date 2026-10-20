//! Control's maintenance windows (M141, #1158): for a few announced
//! minutes the platform's routes (chat, Cloud, grants and billing, every
//! route `routes.toml` gives `process = "platform"`) answer 503 with a
//! `Retry-After` in seconds and `{"error": …, "reason": "maintenance"}`.
//! Terminals and the relay keep working.
//!
//! The hub's gateway answers that 503 before it forwards anything to the
//! platform, so a request refused with [`MAINTENANCE`] wasn't processed:
//! it's safe to send again whatever its method, a `POST` that makes
//! something included. Any other 503 (a proxy's while control restarts,
//! say) may have come after the work was done, so only a safe method
//! (`GET`, `HEAD`) is sent again on one.
//!
//! [`Waits`] is how long a client waits between tries: what `Retry-After`
//! asks, kept between [`Patience::floor`] and [`Patience::per_wait`], or a
//! doubling backoff when it asks nothing, until [`Patience::total`] is spent.
//! The daemon and the CLI share it; the web client has its own copy
//! (`web/src/maintenance.ts`).

use std::time::Duration;

/// The `reason` control's 503 gives during a maintenance window.
pub const MAINTENANCE: &str = "maintenance";

/// What a client says when the window outlasts its patience: calm, since
/// nothing is wrong and it's over soon.
pub const MAINTENANCE_SAID: &str =
    "Arugula control's Chat and Cloud are briefly down for maintenance, back in a minute: try again shortly";

/// How long a client waits out a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    /// The shortest wait, however soon `Retry-After` says (it may say 0).
    pub floor: Duration,
    /// The longest one wait, however long `Retry-After` says.
    pub per_wait: Duration,
    /// All the waits together, after which the refusal is the answer.
    pub total: Duration,
}

impl Patience {
    /// An agent's call (the daemon's `cloud` MCP tools, its chat sync) and
    /// the CLI: agents are patient, and a window is a few minutes.
    pub const AGENT: Patience =
        Patience { floor: Duration::from_secs(1), per_wait: Duration::from_secs(30), total: Duration::from_secs(300) };
}

/// The waits between one request's tries.
#[derive(Debug, Clone)]
pub struct Waits {
    patience: Patience,
    waited: Duration,
    tries: u32,
}

impl Waits {
    pub fn new(patience: Patience) -> Self {
        Self { patience, waited: Duration::ZERO, tries: 0 }
    }

    /// How long to wait before trying again, `asked` being `Retry-After`:
    /// `None` once the patience is spent. The last wait is cut to fit it.
    pub fn next(&mut self, asked: Option<Duration>) -> Option<Duration> {
        let p = self.patience;
        let left = p.total.checked_sub(self.waited).filter(|d| !d.is_zero())?;
        // Nothing asked: 2s, 4s, 8s… up to the longest wait.
        let backoff = || Duration::from_secs(2u64.saturating_pow(self.tries + 1));
        let wait = asked.unwrap_or_else(backoff).clamp(p.floor, p.per_wait.max(p.floor)).min(left);
        self.tries += 1;
        self.waited += wait;
        Some(wait)
    }

    /// All the waits so far.
    pub fn waited(&self) -> Duration {
        self.waited
    }
}

/// `Retry-After` as seconds. An HTTP date (control never sends one) reads
/// as nothing asked.
pub fn retry_after(header: Option<&str>) -> Option<Duration> {
    header?.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Whether an answer is control's maintenance 503: its status and body.
pub fn is_maintenance(status: u16, body: &[u8]) -> bool {
    status == 503
        && serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|v| v["reason"].as_str() == Some(MAINTENANCE))
}

/// Whether an answer may be tried again: control's maintenance 503 for any
/// method (the gateway refused it unprocessed), any other 503 only for a
/// safe one.
pub fn may_retry(method: &str, status: u16, body: &[u8]) -> bool {
    status == 503 && (is_maintenance(status, body) || matches!(method, "GET" | "HEAD"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn waits_honor_retry_after_within_the_patience() {
        let mut w = Waits::new(Patience::AGENT);
        assert_eq!(w.next(Some(S(5))), Some(S(5)), "as asked");
        assert_eq!(w.next(Some(S(0))), Some(S(1)), "never sooner than the floor");
        assert_eq!(w.next(Some(S(600))), Some(S(30)), "never longer than one wait may be");
        // Nothing asked: doubling from 2s, capped.
        let mut b = Waits::new(Patience::AGENT);
        let got: Vec<_> = (0..6).map(|_| b.next(None).unwrap().as_secs()).collect();
        assert_eq!(got, [2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn waits_stop_once_the_patience_is_spent() {
        let mut w = Waits::new(Patience { floor: S(1), per_wait: S(30), total: S(70) });
        assert_eq!(w.next(Some(S(30))), Some(S(30)));
        assert_eq!(w.next(Some(S(30))), Some(S(30)));
        assert_eq!(w.next(Some(S(30))), Some(S(10)), "the last cut to fit");
        assert_eq!(w.waited(), S(70));
        assert_eq!(w.next(Some(S(1))), None);
        // An AGENT's waits add up to five minutes at most.
        let mut a = Waits::new(Patience::AGENT);
        let mut n = 0;
        while a.next(Some(S(30))).is_some() {
            n += 1;
        }
        assert_eq!((n, a.waited()), (10, S(300)));
    }

    #[test]
    fn only_a_maintenance_503_is_retried_for_any_method() {
        let m = br#"{"error":"down for maintenance","reason":"maintenance"}"#;
        let other = br#"{"error":"busy","reason":"full"}"#;
        assert!(is_maintenance(503, m));
        assert!(!is_maintenance(500, m) && !is_maintenance(503, other) && !is_maintenance(503, b"<html>"));
        assert!(may_retry("POST", 503, m) && may_retry("DELETE", 503, m));
        assert!(may_retry("GET", 503, b"") && may_retry("GET", 503, other));
        assert!(!may_retry("POST", 503, other) && !may_retry("POST", 503, b""));
        assert!(!may_retry("GET", 502, m) && !may_retry("GET", 429, b""));
        assert_eq!(retry_after(Some(" 12 ")), Some(S(12)));
        assert_eq!(retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")), None);
        assert_eq!(retry_after(None), None);
    }
}
