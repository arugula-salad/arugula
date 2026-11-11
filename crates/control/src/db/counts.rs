//! What the hub's database holds, counted for `/metrics` (part of
//! Arugula #532): accounts, machines joined, and machines on the relay
//! within a day.

use rusqlite::params;

use super::Db;

const DAY_MS: u64 = 86_400_000;

/// The counts `/metrics` says.
#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub accounts: u64,
    pub machines: u64,
    pub machines_seen_day: u64,
}

impl Db {
    pub fn counts(&self, now: u64) -> anyhow::Result<Counts> {
        Ok(self.c().query_row(
            "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM daemons),
                    (SELECT count(*) FROM daemons WHERE last_seen >= ?1)",
            params![now.saturating_sub(DAY_MS) as i64],
            |r| {
                Ok(Counts {
                    accounts: r.get::<_, i64>(0)? as u64,
                    machines: r.get::<_, i64>(1)? as u64,
                    machines_seen_day: r.get::<_, i64>(2)? as u64,
                })
            },
        )?)
    }
}
