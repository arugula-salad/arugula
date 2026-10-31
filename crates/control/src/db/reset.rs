//! Resets a person asked for themselves (aru-119, #1582,
//! docs/design/self-service-reset.md): one row each, pending until it's
//! due (then done) or cancelled. Two kinds ([`Kind`]):
//! - `devices`: every device, recovery code and passkey, by the same step
//!   as support's `Db::reset_account_devices`;
//! - `passkeys` (DK10, #393, aru-119 as amended): the account's passkeys
//!   only, for a person whose passkeys are lost while their devices are in
//!   use. Devices, machines and recovery codes stay.

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

/// What a reset replaces. A row from before kinds is `devices`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Devices,
    Passkeys,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Devices => "devices",
            Kind::Passkeys => "passkeys",
        }
    }
}

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
    /// The account's pending device reset, if one is.
    pub fn pending_reset(&self, account: &str) -> anyhow::Result<Option<Pending>> {
        self.pending_reset_of(account, Kind::Devices)
    }

    /// The account's pending reset of `kind`, if one is.
    pub fn pending_reset_of(&self, account: &str, kind: Kind) -> anyhow::Result<Option<Pending>> {
        Ok(self
            .c()
            .query_row(
                "SELECT started_at, due, session_hash FROM reset_requests
                 WHERE account = ?1 AND kind = ?2 AND cancelled_at IS NULL AND done_at IS NULL",
                params![account, kind.as_str()],
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

    /// How many device resets the account started since `since`.
    pub fn resets_started_since(&self, account: &str, since: u64) -> anyhow::Result<u64> {
        self.resets_started_since_of(account, Kind::Devices, since)
    }

    /// How many resets of `kind` the account started since `since`.
    pub fn resets_started_since_of(&self, account: &str, kind: Kind, since: u64) -> anyhow::Result<u64> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) FROM reset_requests WHERE account = ?1 AND kind = ?2 AND started_at >= ?3",
            params![account, kind.as_str(), since as i64],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// When the account's last reset of `kind` was done, if one was.
    pub fn last_reset_done_of(&self, account: &str, kind: Kind) -> anyhow::Result<Option<u64>> {
        Ok(self
            .c()
            .query_row(
                "SELECT MAX(done_at) FROM reset_requests WHERE account = ?1 AND kind = ?2",
                params![account, kind.as_str()],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .map(|t| t.max(0) as u64))
    }

    /// A device reset started, due at `due`. False if one is pending already.
    pub fn start_reset(&self, account: &str, session_hash: &str, now: u64, due: u64) -> anyhow::Result<bool> {
        self.start_reset_of(account, Kind::Devices, session_hash, now, due)
    }

    /// A reset of `kind` started, due at `due`. False if one of that kind
    /// is pending already.
    pub fn start_reset_of(
        &self,
        account: &str,
        kind: Kind,
        session_hash: &str,
        now: u64,
        due: u64,
    ) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let pending: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM reset_requests
             WHERE account = ?1 AND kind = ?2 AND cancelled_at IS NULL AND done_at IS NULL)",
            params![account, kind.as_str()],
            |r| r.get(0),
        )?;
        if pending {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO reset_requests (account, kind, started_at, due, session_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account, kind.as_str(), now as i64, due as i64, session_hash],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The pending device reset cancelled, by `by` (a device id, or
    /// `session`). False if none was pending.
    pub fn cancel_reset(&self, account: &str, by: &str, now: u64) -> anyhow::Result<bool> {
        self.cancel_reset_of(account, Kind::Devices, by, now)
    }

    /// The pending reset of `kind` cancelled, by `by` (a device id, a
    /// passkey's `passkey <id>`, or `session`). False if none was pending.
    pub fn cancel_reset_of(&self, account: &str, kind: Kind, by: &str, now: u64) -> anyhow::Result<bool> {
        Ok(self.c().execute(
            "UPDATE reset_requests SET cancelled_at = ?3, cancelled_by = ?4
             WHERE account = ?1 AND kind = ?2 AND cancelled_at IS NULL AND done_at IS NULL",
            params![account, kind.as_str(), now as i64, by],
        )? > 0)
    }

    /// The pending reset of `kind`, if it's due, marked done. Whether it was.
    fn mark_due_done(&self, account: &str, kind: Kind, now: u64) -> anyhow::Result<bool> {
        let Some(p) = self.pending_reset_of(account, kind)? else { return Ok(false) };
        if p.due > now {
            return Ok(false);
        }
        Ok(self.c().execute(
            "UPDATE reset_requests SET done_at = ?3
             WHERE account = ?1 AND kind = ?2 AND started_at = ?4 AND done_at IS NULL AND cancelled_at IS NULL",
            params![account, kind.as_str(), now as i64, p.started_at as i64],
        )? > 0)
    }

    /// The pending device reset, once it's due: done (its devices reset, as
    /// support's is, `by` `self-service`), and its passkeys dropped
    /// ([`Db::drop_passkeys_by_reset`]). Whether it was.
    pub fn finish_due_reset(&self, account: &str, now: u64) -> anyhow::Result<bool> {
        if !self.mark_due_done(account, Kind::Devices, now)? {
            return Ok(false);
        }
        self.reset_account_devices(account, "self-service", "asked for on the approval screen, after the wait", now)?;
        // Its passkeys too (DK10, #393, aru-119 as amended): one way to
        // drop them without another passkey, so one can be added again.
        self.drop_passkeys_by_reset(account, now)?;
        Ok(true)
    }

    /// The pending passkey reset, once it's due: done, every passkey
    /// dropped `by reset` (DK10, #393). Devices, machines and recovery codes
    /// stay. Whether it was.
    pub fn finish_due_passkey_reset(&self, account: &str, now: u64) -> anyhow::Result<bool> {
        if !self.mark_due_done(account, Kind::Passkeys, now)? {
            return Ok(false);
        }
        self.drop_passkeys_by_reset(account, now)?;
        Ok(true)
    }

    /// When the account last had a reset done that lets a first passkey in
    /// again: a device reset or a passkey reset, whichever came last.
    pub fn last_reset_for_passkeys(&self, account: &str) -> anyhow::Result<Option<u64>> {
        let devices = self.last_reset(account)?.map(|(at, _)| at);
        Ok(devices.max(self.last_reset_done_of(account, Kind::Passkeys)?))
    }
}
