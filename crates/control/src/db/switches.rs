//! The launch switches' rows (#531, `switches.rs`): what the operator set
//! with `arugula-control switches`, which the running hub reads every
//! second, so a change takes effect without a restart and holds across one.
//!
//! - `switches`: one row per global switch that's set (`signups`,
//!   `hosted`, a cap's default). A switch with no row is at its default.
//! - `account_switches`: one row per account and switch (`admitted`,
//!   `suspended`, `waiting`, a cap of its own). Deleting the account
//!   deletes its rows.

use rusqlite::{Connection, params};

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS switches (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS account_switches (
    account TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (account, name)
);
";

/// Every switch that's set: the global ones, and each account's.
pub type All = (Vec<Row>, Vec<(String, Row)>);

/// A switch's row: its name, value and when it was set (ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub value: String,
    pub at: u64,
}

/// Every global switch that's set.
pub fn global(c: &Connection) -> rusqlite::Result<Vec<Row>> {
    let mut q = c.prepare("SELECT name, value, at FROM switches ORDER BY name")?;
    q.query_map([], |r| Ok(Row { name: r.get(0)?, value: r.get(1)?, at: r.get(2)? }))?.collect()
}

/// Every account's switches that are set, as (account, row).
pub fn accounts(c: &Connection) -> rusqlite::Result<Vec<(String, Row)>> {
    let mut q = c.prepare("SELECT account, name, value, at FROM account_switches ORDER BY account, name")?;
    q.query_map([], |r| Ok((r.get(0)?, Row { name: r.get(1)?, value: r.get(2)?, at: r.get(3)? })))?.collect()
}

/// Set a global switch, or put it back to its default with `None`.
pub fn set(c: &Connection, name: &str, value: Option<&str>, now: u64) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    match value {
        Some(v) => c.execute(
            "INSERT INTO switches (name, value, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (name) DO UPDATE SET value = ?2, at = ?3",
            params![name, v, now],
        )?,
        None => c.execute("DELETE FROM switches WHERE name = ?1", params![name])?,
    };
    Ok(())
}

/// Set one of an account's switches, or clear it with `None`.
pub fn set_account(c: &Connection, account: &str, name: &str, value: Option<&str>, now: u64) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    match value {
        Some(v) => c.execute(
            "INSERT INTO account_switches (account, name, value, at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (account, name) DO UPDATE SET value = ?3, at = ?4",
            params![account, name, v, now],
        )?,
        None => c.execute("DELETE FROM account_switches WHERE account = ?1 AND name = ?2", params![account, name])?,
    };
    Ok(())
}

/// Put an account on the waitlist, unless it's there already (it keeps
/// its place).
pub fn wait(c: &Connection, account: &str, now: u64) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO account_switches (account, name, value, at) VALUES (?1, 'waiting', '', ?2)
         ON CONFLICT (account, name) DO NOTHING",
        params![account, now],
    )?;
    Ok(())
}

impl super::Db {
    /// [`global`] and [`accounts`], read together.
    pub fn switches(&self) -> anyhow::Result<All> {
        let c = self.c();
        Ok((global(&c)?, accounts(&c)?))
    }

    pub fn set_switch(&self, name: &str, value: Option<&str>, now: u64) -> anyhow::Result<()> {
        Ok(set(&self.c(), name, value, now)?)
    }

    pub fn set_account_switch(&self, account: &str, name: &str, value: Option<&str>, now: u64) -> anyhow::Result<()> {
        Ok(set_account(&self.c(), account, name, value, now)?)
    }

    /// [`wait`].
    pub fn wait_for_box(&self, account: &str, now: u64) -> anyhow::Result<()> {
        Ok(wait(&self.c(), account, now)?)
    }

    /// The account an identity signs in as, without making one.
    pub fn identity_account(&self, provider: &str, subject: &str) -> anyhow::Result<Option<String>> {
        use rusqlite::OptionalExtension as _;
        Ok(self
            .c()
            .query_row(
                "SELECT account FROM identities WHERE provider = ?1 AND subject = ?2",
                params![provider, subject],
                |r| r.get(0),
            )
            .optional()?)
    }
}
