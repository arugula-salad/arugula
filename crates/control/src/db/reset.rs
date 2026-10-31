//! Resets of an account's devices that a person asked for themselves
//! (aru-119, #1582, docs/design/self-service-reset.md): one row each,
//! pending until it's due (then done, by the same step as support's
//! `Db::reset_account_devices`) or cancelled.

use rusqlite::{OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS reset_requests (
    account TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    due INTEGER NOT NULL,
    session_hash TEXT NOT NULL,
    cancelled_at INTEGER,
    cancelled_by TEXT,
    done_at INTEGER
);
CREATE INDEX IF NOT EXISTS reset_requests_account ON reset_requests (account, started_at);
";

/// A reset waiting for its time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub started_at: u64,
    pub due: u64,
    /// The sign-in that started it (its token's hash): it may cancel
    /// without a device's signature.
    pub session_hash: String,
}

impl Db {
    /// The account's pending reset, if one is.
    pub fn pending_reset(&self, account: &str) -> anyhow::Result<Option<Pending>> {
        Ok(self
            .c()
            .query_row(
                "SELECT started_at, due, session_hash FROM reset_requests
                 WHERE account = ?1 AND cancelled_at IS NULL AND done_at IS NULL",
                params![account],
                |r| {
                    Ok(Pending {
                        started_at: r.get::<_, i64>(0)? as u64,
                        due: r.get::<_, i64>(1)? as u64,
                        session_hash: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// How many resets the account started since `since`.
    pub fn resets_started_since(&self, account: &str, since: u64) -> anyhow::Result<u64> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) FROM reset_requests WHERE account = ?1 AND started_at >= ?2",
            params![account, since as i64],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// A reset started, due at `due`. False if one is pending already.
    pub fn start_reset(&self, account: &str, session_hash: &str, now: u64, due: u64) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let pending: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM reset_requests WHERE account = ?1 AND cancelled_at IS NULL AND done_at IS NULL)",
            params![account],
            |r| r.get(0),
        )?;
        if pending {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO reset_requests (account, started_at, due, session_hash) VALUES (?1, ?2, ?3, ?4)",
            params![account, now as i64, due as i64, session_hash],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The pending reset cancelled, by `by` (a device id, or `session`).
    /// False if none was pending.
    pub fn cancel_reset(&self, account: &str, by: &str, now: u64) -> anyhow::Result<bool> {
        Ok(self.c().execute(
            "UPDATE reset_requests SET cancelled_at = ?2, cancelled_by = ?3
             WHERE account = ?1 AND cancelled_at IS NULL AND done_at IS NULL",
            params![account, now as i64, by],
        )? > 0)
    }

    /// The pending reset, once it's due: done (its devices reset, as
    /// support's is, `by` `self-service`), and its passkeys dropped
    /// ([`Db::drop_passkeys_by_reset`]). Whether it was.
    pub fn finish_due_reset(&self, account: &str, now: u64) -> anyhow::Result<bool> {
        let Some(p) = self.pending_reset(account)? else { return Ok(false) };
        if p.due > now {
            return Ok(false);
        }
        let marked = self.c().execute(
            "UPDATE reset_requests SET done_at = ?2 WHERE account = ?1 AND started_at = ?3 AND done_at IS NULL AND cancelled_at IS NULL",
            params![account, now as i64, p.started_at as i64],
        )? > 0;
        if !marked {
            return Ok(false);
        }
        self.reset_account_devices(account, "self-service", "asked for on the approval screen, after the wait", now)?;
        // Its passkeys too (DK10, #393, aru-119 as amended): the one way to
        // drop them without another passkey, so one can be added again.
        self.drop_passkeys_by_reset(account, now)?;
        Ok(true)
    }
}
