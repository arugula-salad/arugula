//! What Arugula control's two processes both need ("Control is two
//! processes: a hub and a platform", #1150): the hub (`arugula-control`)
//! and the platform (`arugula-platform`) open their SQLite the same way,
//! answer errors in the same JSON, count rate limits and send product
//! analytics the same way, and agree on who is asking (`Principal`, and the
//! signed header the hub forwards it in once the platform is its own
//! process, M140), and how the two sign the calls between them
//! (`internal`).
//!
//! Nothing here knows about either side's state: no `App`, no tables.

pub mod analytics;
pub mod error;
pub mod internal;
pub mod limit;
pub mod principal;
pub mod roots;
pub mod sqlite;

pub use error::{ApiError, err, refusal, reply};
pub use principal::Principal;

/// `YYYY-MM-DD` (UTC) for a time in ms, for daily meters.
pub fn day(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
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
    #[test]
    fn days() {
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(1_790_000_000_000), "2026-09-21");
        assert_eq!(super::day(951_782_400_000), "2000-02-29");
    }
}
