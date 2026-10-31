//! An account's email address, for security notices only (#1609,
//! docs/design/control-email.md): where it came from (GitHub's primary
//! verified address, or one the person typed and verified by a link), the
//! links that verify a typed one, and the notices sent, for limits and for
//! "did it go out". Never a notice's body.

use rusqlite::{OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS account_emails (
    account TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    source TEXT NOT NULL,
    verified_at INTEGER,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS email_tokens (
    token_hash TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    email TEXT NOT NULL,
    expires INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS email_tokens_account ON email_tokens (account);
CREATE TABLE IF NOT EXISTS email_sends (
    account TEXT NOT NULL,
    kind TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS email_sends_account ON email_sends (account, at);
";

/// How long a send is kept.
pub const SENDS_KEEP_MS: u64 = 90 * 24 * 3600 * 1000;

/// An account's address as control keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    pub email: String,
    pub source: String,
    pub verified_at: Option<u64>,
}

impl Db {
    /// The account's address, verified or not.
    pub fn email(&self, account: &str) -> anyhow::Result<Option<Email>> {
        Ok(self
            .c()
            .query_row(
                "SELECT email, source, verified_at FROM account_emails WHERE account = ?1",
                params![account],
                |r| {
                    Ok(Email {
                        email: r.get(0)?,
                        source: r.get(1)?,
                        verified_at: r.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                    })
                },
            )
            .optional()?)
    }

    /// The account's address, only once verified: where notices go.
    pub fn verified_email(&self, account: &str) -> anyhow::Result<Option<String>> {
        Ok(self.email(account)?.filter(|e| e.verified_at.is_some()).map(|e| e.email))
    }

    /// GitHub's primary verified address, at a sign-in: kept when the
    /// account has none, never over one the person typed. Whether it
    /// changed: the same address again leaves the row, and when it last
    /// changed, as they were (DK10 counts from that).
    pub fn email_from_github(&self, account: &str, email: &str, now: u64) -> anyhow::Result<bool> {
        let n = self.c().execute(
            "INSERT INTO account_emails (account, email, source, verified_at, updated_at)
             VALUES (?1, ?2, 'github', ?3, ?3)
             ON CONFLICT (account) DO UPDATE SET email = excluded.email, verified_at = excluded.verified_at,
                 updated_at = excluded.updated_at
             WHERE account_emails.source = 'github' AND account_emails.email != excluded.email",
            params![account, email, now as i64],
        )?;
        Ok(n > 0)
    }

    /// When the account's address was last set or changed (`updated_at`),
    /// if it has one (DK10: no passkey reset within two days of a change).
    pub fn email_changed_at(&self, account: &str) -> anyhow::Result<Option<u64>> {
        Ok(self
            .c()
            .query_row("SELECT updated_at FROM account_emails WHERE account = ?1", params![account], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .map(|t| t.max(0) as u64))
    }

    /// A typed address waiting for its link: the link's token (by its hash)
    /// replaces any the account had, and expires at `expires`.
    pub fn add_email_token(&self, account: &str, email: &str, token_hash: &str, expires: u64) -> anyhow::Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM email_tokens WHERE account = ?1", params![account])?;
        tx.execute(
            "INSERT INTO email_tokens (token_hash, account, email, expires) VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, account, email, expires as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The address a live verify link was sent to, if one was.
    pub fn pending_email(&self, account: &str, now: u64) -> anyhow::Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT email FROM email_tokens WHERE account = ?1 AND expires > ?2",
                params![account, now as i64],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// A verify link used: its address becomes the account's, verified, and
    /// the link is gone. The account, the address and the verified one it
    /// replaced (when another: told, DK10), or none for a link that
    /// expired, was used, or never was.
    #[allow(clippy::type_complexity)]
    pub fn verify_email(&self, token_hash: &str, now: u64) -> anyhow::Result<Option<(String, String, Option<String>)>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let found: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT account, email, expires FROM email_tokens WHERE token_hash = ?1",
                params![token_hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        tx.execute("DELETE FROM email_tokens WHERE token_hash = ?1", params![token_hash])?;
        let Some((account, email, _)) = found.filter(|f| f.2 as u64 > now) else {
            tx.commit()?;
            return Ok(None);
        };
        let old: Option<String> = tx
            .query_row(
                "SELECT email FROM account_emails WHERE account = ?1 AND verified_at IS NOT NULL",
                params![account],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "INSERT INTO account_emails (account, email, source, verified_at, updated_at) VALUES (?1, ?2, 'typed', ?3, ?3)
             ON CONFLICT (account) DO UPDATE SET email = excluded.email, source = 'typed',
                 verified_at = excluded.verified_at, updated_at = excluded.updated_at",
            params![account, email, now as i64],
        )?;
        tx.commit()?;
        let old = old.filter(|o| *o != email);
        Ok(Some((account, email, old)))
    }

    /// The account's address removed (Account settings).
    pub fn remove_email(&self, account: &str) -> anyhow::Result<bool> {
        let c = self.c();
        c.execute("DELETE FROM email_tokens WHERE account = ?1", params![account])?;
        Ok(c.execute("DELETE FROM account_emails WHERE account = ?1", params![account])? > 0)
    }

    /// A notice sent (its kind, never its body). `limited`: it counts
    /// toward the daily limit; a reset's, or a changed address's, doesn't
    /// (DK10).
    pub fn record_send(&self, account: &str, kind: &str, now: u64, limited: bool) -> anyhow::Result<()> {
        let c = self.c();
        c.execute(
            "INSERT INTO email_sends (account, kind, at, limited) VALUES (?1, ?2, ?3, ?4)",
            params![account, kind, now as i64, limited],
        )?;
        c.execute("DELETE FROM email_sends WHERE at < ?1", params![now.saturating_sub(SENDS_KEEP_MS) as i64])?;
        Ok(())
    }

    /// How many notices the account was sent since `since` that count
    /// toward the daily limit.
    pub fn sends_since(&self, account: &str, since: u64) -> anyhow::Result<u64> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) FROM email_sends WHERE account = ?1 AND at >= ?2 AND limited = 1",
            params![account, since as i64],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }
}
