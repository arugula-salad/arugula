//! Hosted machines (M178): project delegations, the one-time enrollments
//! the platform records as it makes a box, and the machines enrolled.
//! Signed statements and public keys only: what checks a delegation is the
//! owners' devices' signature on it, and clients check it again.
//!
//! Hosted machines aren't in `daemons`, on purpose: nothing that reads a
//! daemon's account (its owner's machines, moves, shares, grants, Labs
//! flags) ever sees one. What they take part in (the relay, routing, the
//! directory) asks for them here.

use arugula_e2e::hosted::{Delegation, DelegationRevoke, HostedCert};
use rusqlite::{OptionalExtension, Transaction, params};

use super::Db;

pub(super) const SCHEMA: &str = "
-- An owner's device's signed delegation (JSON), and its revocation.
-- `owner` is `team:<id>` or `account:<id>`; `by_account` signed it.
-- `revoked` is when it ended, set once, for good; `revoke` is the first
-- revoke, and `more_revokes` (added since, a JSON list) the others, one per
-- device up to `MAX_REVOKES`. Another delegation's revoke can end this one
-- too: `revoked` set, and no revoke of its own.
CREATE TABLE IF NOT EXISTS project_delegations (
    id TEXT PRIMARY KEY,
    project TEXT NOT NULL,
    owner TEXT NOT NULL,
    body TEXT NOT NULL,
    by_account TEXT NOT NULL,
    created INTEGER NOT NULL,
    revoke TEXT,
    revoked INTEGER
);
CREATE INDEX IF NOT EXISTS project_delegations_owner ON project_delegations (owner);
-- One enrollment, by the SHA-256 of its ticket or sandbox id (`via`):
-- once, until `expires`. A used one stays until it would have expired,
-- so a second try is refused, not taken for a new one; `device` is the
-- machine that used it, and `contested` says a second try came (fail
-- closed: that machine is removed, and the platform deletes the box).
CREATE TABLE IF NOT EXISTS hosted_enrollments (
    hash TEXT PRIMARY KEY,
    via TEXT NOT NULL,
    project TEXT NOT NULL,
    owner TEXT NOT NULL,
    workspace TEXT NOT NULL,
    name TEXT NOT NULL,
    delegation TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    used INTEGER,
    device TEXT,
    contested INTEGER
);
CREATE INDEX IF NOT EXISTS hosted_enrollments_workspace ON hosted_enrollments (workspace);
-- An enrolled hosted machine: its certificate (JSON), signed with
-- control's hosted key. One live machine per workspace. `urls` is never
-- read: a hosted machine is reached only through the relay (the column
-- stays, as a shipped migration can't change).
CREATE TABLE IF NOT EXISTS hosted_machines (
    id TEXT PRIMARY KEY,
    project TEXT NOT NULL,
    owner TEXT NOT NULL,
    workspace TEXT NOT NULL,
    delegation TEXT NOT NULL,
    cert TEXT NOT NULL,
    urls TEXT NOT NULL DEFAULT '[]',
    created INTEGER NOT NULL,
    last_seen INTEGER,
    removed INTEGER
);
CREATE INDEX IF NOT EXISTS hosted_machines_owner ON hosted_machines (owner);
CREATE INDEX IF NOT EXISTS hosted_machines_workspace ON hosted_machines (workspace);
";

#[derive(Debug, Clone)]
pub struct DelegationRow {
    pub delegation: Delegation,
    pub by_account: String,
    /// The revokes kept, first first, one per device. Each was checked as it
    /// came, against who was an owner then.
    pub revokes: Vec<DelegationRevoke>,
    /// When it ended, by control's clock. Once set it's over.
    pub revoked: Option<u64>,
}

impl DelegationRow {
    /// The first revoke, which ended it.
    pub fn revoke(&self) -> Option<&DelegationRevoke> {
        self.revokes.first()
    }

    /// When it ended: `revoked`, or the earliest revoke's own time if that's
    /// before it (within the slack control allows a revoke's time), so a
    /// client's answer is the same whichever of them it can check. `None`
    /// while it stands.
    pub fn ended(&self) -> Option<u64> {
        self.revoked.map(|at| self.revokes.iter().map(|r| r.at).fold(at, u64::min))
    }
}

/// What became of a revoke control was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeTaken {
    /// It's the one that ended the delegation.
    pub first: bool,
    /// It's kept. Not when this device's revoke of it was already, or
    /// [`MAX_REVOKES`] are.
    pub kept: bool,
}

/// How many revokes of one delegation are kept. The first is what ends it, so
/// the rest only tell clients from before more than they need.
pub const MAX_REVOKES: usize = 32;

/// A live hosted machine. It has no direct URLs: it's reached only
/// through the relay, whatever its row holds.
#[derive(Debug, Clone)]
pub struct HostedRow {
    pub cert: HostedCert,
    pub last_seen: Option<u64>,
    /// `deploy` for a box the platform made for one deploy's run (M219):
    /// nobody is routed to it, and it isn't listed. `None`: a people's
    /// workspace.
    pub purpose: Option<String>,
}

/// What recording an enrollment did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    New,
    /// The same one, recorded before: until this time.
    Again(u64),
    /// That credential is another workspace's.
    Taken,
}

/// What became of an enrollment someone tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// Theirs, now: the project, owner, workspace, name and delegation it
    /// was for.
    Fresh { project: String, owner: String, workspace: String, name: String, delegation: String },
    /// Used already: fail closed. The machine that used it, removed now.
    Used(Option<String>),
    /// No such enrollment, or it ran out.
    Missing,
}

const DELEGATION_COLUMNS: &str = "body, by_account, revoke, more_revokes, revoked";

fn delegation_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<DelegationRow> {
    let json = |e: serde_json::Error| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
    let mut revokes: Vec<DelegationRevoke> = r
        .get::<_, Option<String>>(2)?
        .map(|f| serde_json::from_str(&f))
        .transpose()
        .map_err(json)?
        .into_iter()
        .collect();
    if let Some(more) = r.get::<_, Option<String>>(3)? {
        revokes.extend(serde_json::from_str::<Vec<DelegationRevoke>>(&more).map_err(json)?);
    }
    Ok(DelegationRow {
        delegation: serde_json::from_str(&r.get::<_, String>(0)?).map_err(json)?,
        by_account: r.get(1)?,
        revokes,
        revoked: r.get(4)?,
    })
}

fn hosted_of(cert: String, last_seen: Option<u64>, purpose: Option<String>) -> rusqlite::Result<HostedRow> {
    let json = |e: serde_json::Error| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
    Ok(HostedRow { cert: serde_json::from_str(&cert).map_err(json)?, last_seen, purpose })
}

impl Db {
    /// Keep a delegation an owner's device signed (again: nothing changes),
    /// unless another of its project and owner `stands`: one at a time. In
    /// one transaction, so two at once get one answer each. Whether it's kept.
    pub fn add_delegation(
        &self,
        d: &Delegation,
        by_account: &str,
        now: u64,
        stands: impl Fn(&DelegationRow) -> bool,
    ) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let id = d.id();
        let other = {
            let mut q = tx.prepare(&format!(
                "SELECT {DELEGATION_COLUMNS} FROM project_delegations WHERE project = ?1 AND owner = ?2 AND id != ?3"
            ))?;
            let rows = q.query_map(params![d.project, d.owner, id], delegation_of)?;
            let mut found = false;
            for row in rows {
                found |= stands(&row?);
            }
            found
        };
        if other {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO project_delegations (id, project, owner, body, by_account, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, d.project, d.owner, serde_json::to_string(d)?, by_account, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn delegation(&self, id: &str) -> anyhow::Result<Option<DelegationRow>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {DELEGATION_COLUMNS} FROM project_delegations WHERE id = ?1"),
                params![id],
                delegation_of,
            )
            .optional()?)
    }

    /// Every delegation of these owners' projects, newest first.
    pub fn delegations_of(&self, owners: &[String]) -> anyhow::Result<Vec<DelegationRow>> {
        let c = self.c();
        let mut q = c.prepare(&format!(
            "SELECT {DELEGATION_COLUMNS} FROM project_delegations WHERE owner = ?1 ORDER BY created DESC, id"
        ))?;
        let mut out = Vec::new();
        for o in owners {
            for row in q.query_map(params![o], delegation_of)? {
                out.push(row?);
            }
        }
        Ok(out)
    }

    /// Take a delegation back. The first revoke ends it, and `revoked` is
    /// set then, once. Later ones are kept beside it, one per device, up to
    /// [`MAX_REVOKES`]. It ends any other of its project and owner too, so a
    /// project that holds two from before the rule of one has none after.
    pub fn revoke_delegation(&self, r: &DelegationRevoke, now: u64) -> anyhow::Result<RevokeTaken> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let row = tx
            .query_row(
                &format!("SELECT {DELEGATION_COLUMNS} FROM project_delegations WHERE id = ?1"),
                params![r.delegation],
                delegation_of,
            )
            .optional()?;
        let taken = match row {
            None => RevokeTaken { first: false, kept: false },
            Some(row) if row.revoked.is_none() => {
                tx.execute(
                    "UPDATE project_delegations SET revoke = ?2, revoked = ?3 WHERE id = ?1",
                    params![r.delegation, serde_json::to_string(r)?, now],
                )?;
                tx.execute(
                    "UPDATE project_delegations SET revoked = ?1
                     WHERE project = ?2 AND owner = ?3 AND id != ?4 AND revoked IS NULL",
                    params![now, row.delegation.project, row.delegation.owner, r.delegation],
                )?;
                RevokeTaken { first: true, kept: true }
            }
            // Ended by another delegation's revoke: this is the first of its own.
            Some(row) if row.revokes.is_empty() => {
                tx.execute(
                    "UPDATE project_delegations SET revoke = ?2 WHERE id = ?1",
                    params![r.delegation, serde_json::to_string(r)?],
                )?;
                RevokeTaken { first: false, kept: true }
            }
            Some(row) if row.revokes.len() >= MAX_REVOKES || row.revokes.iter().any(|h| h.by == r.by) => {
                RevokeTaken { first: false, kept: false }
            }
            Some(row) => {
                let more: Vec<&DelegationRevoke> = row.revokes.iter().skip(1).chain([r]).collect();
                tx.execute(
                    "UPDATE project_delegations SET more_revokes = ?2 WHERE id = ?1",
                    params![r.delegation, serde_json::to_string(&more)?],
                )?;
                RevokeTaken { first: false, kept: true }
            }
        };
        tx.commit()?;
        Ok(taken)
    }

    /// Record a one-time enrollment. A sandbox id is enrolled once, ever
    /// (until its row is swept, long after the box is gone): asked again
    /// for the same workspace it's the same enrollment (a lost answer, sent
    /// again), and for another it's taken.
    #[allow(clippy::too_many_arguments)]
    pub fn add_enrollment(
        &self,
        hash: &str,
        via: &str,
        project: &str,
        owner: &str,
        workspace: &str,
        name: &str,
        delegation: &str,
        now: u64,
        expires: u64,
    ) -> anyhow::Result<Recorded> {
        let mut c = self.c();
        let tx = c.transaction()?;
        // Swept a day after they'd have expired.
        tx.execute("DELETE FROM hosted_enrollments WHERE expires < ?1", params![now.saturating_sub(86_400_000)])?;
        let was: Option<(String, String, u64)> = tx
            .query_row(
                "SELECT project, workspace, expires FROM hosted_enrollments WHERE hash = ?1",
                params![hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let out = match was {
            Some((p, w, expires)) if p == project && w == workspace => Recorded::Again(expires),
            Some(_) => Recorded::Taken,
            None => {
                tx.execute(
                    "INSERT INTO hosted_enrollments
                     (hash, via, project, owner, workspace, name, delegation, created, expires)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![hash, via, project, owner, workspace, name, delegation, now, expires],
                )?;
                Recorded::New
            }
        };
        tx.commit()?;
        Ok(out)
    }

    /// What an enrollment's box is for (M219): `deploy`, set as it's
    /// recorded.
    pub fn set_enrollment_purpose(&self, hash: &str, purpose: &str) -> anyhow::Result<()> {
        self.c().execute("UPDATE hosted_enrollments SET purpose = ?2 WHERE hash = ?1", params![hash, purpose])?;
        Ok(())
    }

    /// What an enrollment's box is for, if it says.
    pub fn enrollment_purpose(&self, hash: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .c()
            .query_row("SELECT purpose FROM hosted_enrollments WHERE hash = ?1", params![hash], |r| r.get(0))
            .optional()?
            .flatten())
    }

    /// What a hosted machine is for, from its enrollment (M219).
    pub fn set_hosted_purpose(&self, id: &str, purpose: &str) -> anyhow::Result<()> {
        self.c().execute("UPDATE hosted_machines SET purpose = ?2 WHERE id = ?1", params![id, purpose])?;
        Ok(())
    }

    /// The machine that used an enrollment.
    pub fn enrolled_as(&self, hash: &str, device: &str) -> anyhow::Result<()> {
        self.c().execute("UPDATE hosted_enrollments SET device = ?2 WHERE hash = ?1", params![hash, device])?;
        Ok(())
    }

    /// Of these workspaces, which have a live machine, and which had their
    /// enrollment contested (the platform deletes those boxes).
    pub fn hosted_enrolled(&self, workspaces: &[String]) -> anyhow::Result<Vec<(String, Option<String>, bool)>> {
        let c = self.c();
        let mut live = c.prepare("SELECT id FROM hosted_machines WHERE workspace = ?1 AND removed IS NULL")?;
        let mut contested = c.prepare(
            "SELECT contested IS NOT NULL FROM hosted_enrollments WHERE workspace = ?1 ORDER BY created DESC LIMIT 1",
        )?;
        let mut out = Vec::new();
        for w in workspaces {
            let machine: Option<String> = live.query_row(params![w], |r| r.get(0)).optional()?;
            let taken: bool = contested.query_row(params![w], |r| r.get(0)).optional()?.unwrap_or(false);
            if machine.is_some() || taken {
                out.push((w.clone(), machine, taken));
            }
        }
        Ok(out)
    }

    /// These workspaces' boxes are gone: their machines are removed, and
    /// enrollments nobody used are dropped (used ones stay, so a sandbox id
    /// never enrolls twice). The machines' ids, to hang up on.
    pub fn hosted_gone(&self, workspaces: &[String], now: u64) -> anyhow::Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let mut gone = Vec::new();
        for w in workspaces {
            gone.extend(remove_where(&tx, "workspace = ?1", w, now)?);
            tx.execute("DELETE FROM hosted_enrollments WHERE workspace = ?1 AND used IS NULL", params![w])?;
        }
        tx.commit()?;
        Ok(gone)
    }

    /// Use an enrollment, once: in one statement, so two tries at once get
    /// one answer each.
    pub fn take_enrollment(&self, hash: &str, now: u64) -> anyhow::Result<Taken> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let row: Option<(Taken, u64, bool)> = tx
            .query_row(
                "SELECT project, owner, workspace, name, delegation, expires, used FROM hosted_enrollments
                 WHERE hash = ?1",
                params![hash],
                |r| {
                    let fresh = Taken::Fresh {
                        project: r.get(0)?,
                        owner: r.get(1)?,
                        workspace: r.get(2)?,
                        name: r.get(3)?,
                        delegation: r.get(4)?,
                    };
                    Ok((fresh, r.get(5)?, r.get::<_, Option<u64>>(6)?.is_some()))
                },
            )
            .optional()?;
        let taken = match row {
            None => Taken::Missing,
            Some((_, _, true)) => {
                // Fail closed: a second try means someone else has the
                // credential. The machine that used it goes too.
                tx.execute("UPDATE hosted_enrollments SET contested = ?2 WHERE hash = ?1", params![hash, now])?;
                let device: Option<String> =
                    tx.query_row("SELECT device FROM hosted_enrollments WHERE hash = ?1", params![hash], |r| r.get(0))?;
                if let Some(d) = &device {
                    remove_where(&tx, "id = ?1", d, now)?;
                }
                Taken::Used(device)
            }
            Some((_, expires, false)) if expires <= now => Taken::Missing,
            Some((fresh, ..)) => {
                tx.execute("UPDATE hosted_enrollments SET used = ?2 WHERE hash = ?1", params![hash, now])?;
                fresh
            }
        };
        tx.commit()?;
        Ok(taken)
    }

    /// Keep an enrolled machine. The workspace's machine before it, if
    /// any, is removed: their ids, to hang up on.
    pub fn add_hosted(&self, cert: &HostedCert, now: u64) -> anyhow::Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let old = remove_where(&tx, "workspace = ?1", &cert.workspace, now)?;
        tx.execute(
            "INSERT INTO hosted_machines (id, project, owner, workspace, delegation, cert, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cert.device,
                cert.project,
                cert.owner,
                cert.workspace,
                cert.delegation,
                serde_json::to_string(cert)?,
                now
            ],
        )?;
        tx.commit()?;
        Ok(old)
    }

    /// A live hosted machine, by its device id.
    pub fn hosted(&self, id: &str) -> anyhow::Result<Option<HostedRow>> {
        Ok(self
            .c()
            .query_row(
                "SELECT cert, last_seen, purpose FROM hosted_machines WHERE id = ?1 AND removed IS NULL",
                params![id],
                |r| hosted_of(r.get(0)?, r.get(1)?, r.get(2)?),
            )
            .optional()?)
    }

    /// Whether `id` was a hosted machine that's been removed.
    pub fn hosted_removed(&self, id: &str) -> anyhow::Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT 1 FROM hosted_machines WHERE id = ?1 AND removed IS NOT NULL", params![id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// The live hosted machines of these owners' projects.
    pub fn hosted_of(&self, owners: &[String]) -> anyhow::Result<Vec<HostedRow>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT cert, last_seen, purpose FROM hosted_machines WHERE owner = ?1 AND removed IS NULL ORDER BY created",
        )?;
        let mut out = Vec::new();
        for o in owners {
            for row in q.query_map(params![o], |r| hosted_of(r.get(0)?, r.get(1)?, r.get(2)?))? {
                out.push(row?);
            }
        }
        Ok(out)
    }

    /// A hosted machine was heard from on the relay. Nothing else is
    /// kept of how it connected: it's reached only through the relay.
    pub fn hosted_seen(&self, id: &str, now: u64) -> anyhow::Result<()> {
        self.c().execute("UPDATE hosted_machines SET last_seen = ?2 WHERE id = ?1", params![id, now])?;
        Ok(())
    }

    /// A row as an older control left it, with direct URLs in it.
    #[cfg(test)]
    pub(crate) fn put_hosted_urls(&self, id: &str, urls: &[String]) -> anyhow::Result<()> {
        self.c()
            .execute("UPDATE hosted_machines SET urls = ?2 WHERE id = ?1", params![id, serde_json::to_string(urls)?])?;
        Ok(())
    }

    /// Remove a hosted machine: whether it was live.
    pub fn remove_hosted(&self, id: &str, now: u64) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let gone = remove_where(&tx, "id = ?1", id, now)?;
        tx.commit()?;
        Ok(!gone.is_empty())
    }
}

fn remove_where(tx: &Transaction, cond: &str, v: &str, now: u64) -> rusqlite::Result<Vec<String>> {
    let ids: Vec<String> = {
        let mut q = tx.prepare(&format!("SELECT id FROM hosted_machines WHERE {cond} AND removed IS NULL"))?;
        q.query_map(params![v], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    tx.execute(&format!("UPDATE hosted_machines SET removed = ?2 WHERE {cond} AND removed IS NULL"), params![v, now])?;
    Ok(ids)
}

/// What deleting an account takes of this (inside its transaction): the
/// delegations, enrollments and machines of its own project and of the
/// teams it disbands, and every delegation it signed (it can't sign for
/// anyone now). The machines' ids, to hang up on.
pub(super) fn erase(tx: &Transaction, account: &str, disband: &[String]) -> rusqlite::Result<Vec<String>> {
    let mut owners = vec![format!("account:{account}")];
    owners.extend(disband.iter().map(|t| format!("team:{t}")));
    let mut ids = Vec::new();
    for o in &owners {
        let mut q = tx.prepare("SELECT id FROM hosted_machines WHERE owner = ?1")?;
        ids.extend(q.query_map(params![o], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
        for sql in [
            "DELETE FROM hosted_machines WHERE owner = ?1",
            "DELETE FROM hosted_enrollments WHERE owner = ?1",
            "DELETE FROM project_delegations WHERE owner = ?1",
        ] {
            tx.execute(sql, params![o])?;
        }
    }
    // Machines it delegated elsewhere go with the delegation: clients
    // would refuse them anyway, its signer gone.
    let signed = "SELECT id FROM project_delegations WHERE by_account = ?1";
    {
        let mut q = tx.prepare(&format!("SELECT id FROM hosted_machines WHERE delegation IN ({signed})"))?;
        ids.extend(q.query_map(params![account], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
    }
    tx.execute(&format!("DELETE FROM hosted_machines WHERE delegation IN ({signed})"), params![account])?;
    tx.execute(&format!("DELETE FROM hosted_enrollments WHERE delegation IN ({signed})"), params![account])?;
    tx.execute("DELETE FROM project_delegations WHERE by_account = ?1", params![account])?;
    Ok(ids)
}
