//! The standing project grants' rows (M211), made by migration `0019`:
//! what an owner's device signed, kept as the bytes it signed and read back
//! from them ([`ProjectGrant::parse`]), so nothing the platform holds beside
//! them can change what a grant says. [`crate::run_grant`] has the rules.
//!
//! A project has at most one live grant: not revoked, not superseded, and
//! not expired. A new one supersedes the live row when that one expired or
//! names another delegation (the caller checked the new one names the
//! project's live delegation). A revoke takes back every grant of the
//! project under its delegation made by its time, and each of those rows
//! keeps it.

use anyhow::{Result, bail};
use arugula_e2e::project_grant::{GrantRevoke, ProjectGrant, Running};

use super::{
    Db, Taken,
    sql::{Optional as _, Row, Sql, params},
    taken,
};

/// A grant as the platform keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrant {
    pub grant: ProjectGrant,
    /// The account that sent it; `None` once that account is deleted.
    pub posted_by: Option<String>,
    pub created_at: u64,
    pub revoke: Option<GrantRevoke>,
    pub revoked_at: Option<u64>,
    /// A newer grant took its place: it had expired, or its delegation
    /// wasn't the project's live one any more.
    pub superseded_at: Option<u64>,
}

const COLUMNS: &str = "body, sig, posted_by, created_at, revoke_body, revoke_sig, revoked_at, superseded_at";

/// What makes a row the project's live grant at `?2` (ms).
const LIVE: &str = "revoked_at IS NULL AND superseded_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)";

fn row(r: &Row) -> Result<StoredGrant> {
    let (body, sig): (String, String) = (r.get(0)?, r.get(1)?);
    let Some(grant) = ProjectGrant::parse(&body, &sig) else { bail!("a run grant's stored bytes aren't a grant") };
    let revoke = match (r.get::<Option<String>>(4)?, r.get::<Option<String>>(5)?) {
        (Some(b), Some(s)) => {
            Some(GrantRevoke::parse(&b, &s).ok_or_else(|| anyhow::anyhow!("a run grant's revoke isn't a revoke"))?)
        }
        _ => None,
    };
    Ok(StoredGrant {
        grant,
        posted_by: r.get(2)?,
        created_at: r.get(3)?,
        revoke,
        revoked_at: r.get(6)?,
        superseded_at: r.get(7)?,
    })
}

/// What [`Db::add_run_grant`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Kept,
    /// The project has a live grant under the same delegation: it's revoked
    /// before another is kept.
    LiveAlready,
    /// A revoke of the project's grants under its delegation was signed at
    /// or after it was made: it was revoked before it got here.
    Revoked,
    /// A grant with this id is here already.
    Seen,
}

impl Db {
    /// Keep a grant for its project at `now`, unless the project has a
    /// live one under the same delegation, or a revoke covers it. A live one
    /// that expired, or names another delegation, is superseded. The caller
    /// checked it's well formed and names the project's live delegation.
    pub fn add_run_grant(&self, g: &ProjectGrant, posted_by: &str, now: u64) -> Result<Added> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let revoked: u64 = tx.query_row(
            "SELECT count(*) FROM run_grants WHERE project_id = ?1 AND delegation = ?2 AND revoke_at >= ?3",
            params![g.project, g.delegation, g.made],
            |r| r.get(0),
        )?;
        if revoked > 0 {
            return Ok(Added::Revoked);
        }
        tx.execute(
            "UPDATE run_grants SET superseded_at = ?2
             WHERE project_id = ?1 AND revoked_at IS NULL AND superseded_at IS NULL
               AND ((expires_at IS NOT NULL AND expires_at <= ?2) OR delegation != ?3)",
            params![g.project, now, g.delegation],
        )?;
        let live: u64 = tx.query_row(
            "SELECT count(*) FROM run_grants WHERE project_id = ?1 AND revoked_at IS NULL AND superseded_at IS NULL",
            params![g.project],
            |r| r.get(0),
        )?;
        if live > 0 {
            return Ok(Added::LiveAlready);
        }
        let made = tx.execute(
            "INSERT INTO run_grants (id, project_id, delegation, body, sig, by_device, made_at, expires_at, posted_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![g.id, g.project, g.delegation, g.body(), g.sig, g.by, g.made, g.expires, posted_by, now],
        );
        match made.map_err(taken) {
            // Its id, or a live grant that came in since (the unique index
            // on the project's live one): nothing of this is kept.
            Err(e) if e.is::<Taken>() => {
                drop(tx);
                let seen: u64 =
                    self.c().query_row("SELECT count(*) FROM run_grants WHERE id = ?1", params![g.id], |r| r.get(0))?;
                Ok(if seen > 0 { Added::Seen } else { Added::LiveAlready })
            }
            r => {
                r?;
                tx.commit()?;
                Ok(Added::Kept)
            }
        }
    }

    /// The project's live grant at `now`, if it has one.
    pub fn live_run_grant(&self, project: &str, now: u64) -> Result<Option<StoredGrant>> {
        self.c()
            .query_row(
                &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND {LIVE}"),
                params![project, now],
                row,
            )
            .optional()
    }

    /// A grant of the project's, by its id.
    pub fn run_grant(&self, project: &str, id: &str) -> Result<Option<StoredGrant>> {
        self.c()
            .query_row(
                &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND id = ?2"),
                params![project, id],
                row,
            )
            .optional()
    }

    /// The project's grants, newest first, at most `limit`.
    pub fn run_grants(&self, project: &str, limit: u64) -> Result<Vec<StoredGrant>> {
        self.c().query_map(
            &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 ORDER BY created_at DESC, id LIMIT ?2"),
            params![project, limit],
            row,
        )
    }

    /// The newest revoke of the project's grants under each delegation:
    /// what goes with each start, so a machine that missed one hears it (a
    /// newer revoke covers what an older one did).
    pub fn run_grant_revokes(&self, project: &str) -> Result<Vec<GrantRevoke>> {
        let mut newest: Vec<GrantRevoke> = Vec::new();
        let all = self.c().query_map(
            &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND revoked_at IS NOT NULL"),
            params![project],
            row,
        )?;
        for r in all.into_iter().filter_map(|g| g.revoke) {
            match newest.iter_mut().find(|n| n.delegation == r.delegation) {
                Some(n) if n.at >= r.at => {}
                Some(n) => *n = r,
                None => newest.push(r),
            }
        }
        newest.sort_by(|a, b| a.delegation.cmp(&b.delegation));
        Ok(newest)
    }

    /// Revoke every grant of `r`'s project under its delegation made at or
    /// before its time that isn't revoked yet: how many. The caller checked
    /// the revoke is well formed and for this project.
    pub fn revoke_run_grants(&self, r: &GrantRevoke, now: u64) -> Result<u64> {
        let n = self.c().execute(
            "UPDATE run_grants SET revoke_body = ?1, revoke_sig = ?2, revoke_at = ?3, revoked_at = ?4, stop_running = ?5
             WHERE project_id = ?6 AND delegation = ?7 AND made_at <= ?3 AND revoked_at IS NULL",
            params![r.body(), r.sig, r.at, now, r.running == Running::Stop, r.project, r.delegation],
        )?;
        Ok(n as u64)
    }
}

/// What deleting an account takes from here, in the platform's
/// `account_deleted` transaction (after the projects that go are gone):
/// the grants of projects that went, and its name on the others.
pub(crate) fn erase_account(c: &impl Sql, account: &str) -> Result<()> {
    c.execute("DELETE FROM run_grants WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE run_grants SET posted_by = NULL WHERE posted_by = ?1", params![account])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{
        DeviceKeys,
        hosted::{Delegation, HostedKey},
        project_grant::{GrantRevoke, ProjectGrant, Running},
    };

    use super::*;

    fn delegation(project: &str, keys: &DeviceKeys, at: u64) -> Delegation {
        Delegation {
            project: project.into(),
            owner: "account:owner-1".into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at,
            by: keys.id(),
            sig: String::new(),
        }
    }

    fn grant(project: &str, keys: &DeviceKeys, made: u64) -> ProjectGrant {
        grant_under(&delegation(project, keys, 1), keys, made, None)
    }

    fn grant_under(d: &Delegation, keys: &DeviceKeys, made: u64, expires: Option<u64>) -> ProjectGrant {
        let mut g = ProjectGrant::new(d, made, expires);
        g.sign_with(keys);
        g
    }

    fn revoke(g: &ProjectGrant, at: u64, keys: &DeviceKeys) -> GrantRevoke {
        GrantRevoke::new(&g.project, &g.delegation, Running::Stop, at, keys)
    }

    #[test]
    fn one_live_grant_a_project_kept_as_signed() {
        let db = Db::test();
        let k = DeviceKeys::generate();
        let g = grant("prj_1", &k, 10);
        assert_eq!(db.add_run_grant(&g, "a1", 100).unwrap(), Added::Kept);
        assert_eq!(db.add_run_grant(&g, "a1", 101).unwrap(), Added::LiveAlready);
        let g2 = grant("prj_1", &k, 20);
        assert_eq!(db.add_run_grant(&g2, "a1", 102).unwrap(), Added::LiveAlready);
        let live = db.live_run_grant("prj_1", 103).unwrap().unwrap();
        assert_eq!((live.grant.clone(), live.posted_by.as_deref(), live.created_at), (g.clone(), Some("a1"), 100));
        assert!(db.live_run_grant("prj_2", 103).unwrap().is_none());
        // Revoked (it names no grant: every one made by its time): the
        // next one is kept, and both are listed.
        let r = revoke(&g, 150, &k);
        assert_eq!(db.revoke_run_grants(&r, 160).unwrap(), 1);
        assert_eq!(db.revoke_run_grants(&r, 161).unwrap(), 0, "once");
        assert!(db.live_run_grant("prj_1", 162).unwrap().is_none());
        assert_eq!(db.add_run_grant(&g, "a1", 170).unwrap(), Added::Revoked);
        assert_eq!(db.add_run_grant(&g2, "a1", 170).unwrap(), Added::Revoked, "made before the revoke");
        let g3 = grant("prj_1", &k, 151);
        assert_eq!(db.add_run_grant(&g3, "a1", 170).unwrap(), Added::Kept);
        let all = db.run_grants("prj_1", 10).unwrap();
        assert_eq!(all.iter().map(|s| s.grant.id.clone()).collect::<Vec<_>>(), [g3.id.clone(), g.id.clone()]);
        assert_eq!((all[1].revoke.as_ref(), all[1].revoked_at), (Some(&r), Some(160)));
        assert_eq!(db.run_grant_revokes("prj_1").unwrap(), std::slice::from_ref(&r));
        assert_eq!(db.run_grant("prj_1", &g.id).unwrap().unwrap().revoke, Some(r.clone()));
        assert!(db.run_grant("prj_2", &g.id).unwrap().is_none());
        // A revoke for another project changes nothing; a newer one is the
        // one sent.
        let elsewhere = GrantRevoke { project: "prj_2".into(), ..r.clone() };
        assert_eq!(db.revoke_run_grants(&elsewhere, 180).unwrap(), 0);
        let r2 = revoke(&g3, 200, &k);
        assert_eq!(db.revoke_run_grants(&r2, 210).unwrap(), 1);
        assert_eq!(db.run_grant_revokes("prj_1").unwrap(), [r2]);
    }

    /// An expired grant isn't live, and a new one takes its place; so does
    /// one under the project's new delegation.
    #[test]
    fn expired_and_old_delegations_are_superseded() {
        let db = Db::test();
        let k = DeviceKeys::generate();
        let d = delegation("prj_1", &k, 1);
        let g = grant_under(&d, &k, 10, Some(500));
        assert_eq!(db.add_run_grant(&g, "a1", 100).unwrap(), Added::Kept);
        assert!(db.live_run_grant("prj_1", 499).unwrap().is_some());
        assert!(db.live_run_grant("prj_1", 500).unwrap().is_none(), "expired");
        let g2 = grant_under(&d, &k, 450, Some(5000));
        assert_eq!(db.add_run_grant(&g2, "a1", 400).unwrap(), Added::LiveAlready, "not expired yet");
        assert_eq!(db.add_run_grant(&g2, "a1", 600).unwrap(), Added::Kept);
        assert_eq!(db.run_grant("prj_1", &g.id).unwrap().unwrap().superseded_at, Some(600));
        // The project's delegation changed: the next grant names the new one.
        let d2 = delegation("prj_1", &k, 2);
        let g3 = grant_under(&d2, &k, 700, Some(5000));
        assert_eq!(db.add_run_grant(&g3, "a1", 700).unwrap(), Added::Kept);
        assert_eq!(db.live_run_grant("prj_1", 701).unwrap().unwrap().grant, g3);
        assert_eq!(db.run_grant("prj_1", &g2.id).unwrap().unwrap().superseded_at, Some(700));
    }

    #[test]
    fn an_accounts_deletion() {
        let db = Db::test();
        let k = DeviceKeys::generate();
        db.c()
            .execute(
                "INSERT INTO projects (id, account_id, name, is_default, created_at) VALUES ('prj_1', 'owner-1', 'P', ?1, 1)",
                params![true],
            )
            .unwrap();
        db.add_run_grant(&grant("prj_1", &k, 10), "poster-1", 100).unwrap();
        db.add_run_grant(&grant("prj_gone", &k, 10), "poster-1", 100).unwrap();
        erase_account(&db.c(), "poster-1").unwrap();
        assert!(db.live_run_grant("prj_gone", 101).unwrap().is_none());
        assert_eq!(db.live_run_grant("prj_1", 101).unwrap().unwrap().posted_by, None);
        assert!(db.mentions("poster-1").is_empty(), "{:?}", db.mentions("poster-1"));
    }
}
