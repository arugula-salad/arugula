//! The standing project grants' rows (M211), made by migration `0025`:
//! what an owner's device signed, kept as the bytes it signed and read back
//! from them ([`ProjectGrant::parse`]), so nothing the platform holds beside
//! them can change what a grant says. [`crate::run_grant`] has the rules.

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
}

const COLUMNS: &str = "body, sig, posted_by, created_at, revoke_body, revoke_sig, revoked_at";

fn row(r: &Row) -> Result<StoredGrant> {
    let (body, sig): (String, String) = (r.get(0)?, r.get(1)?);
    let Some(grant) = ProjectGrant::parse(&body, &sig) else { bail!("a run grant's stored bytes aren't a grant") };
    let revoke = match (r.get::<Option<String>>(4)?, r.get::<Option<String>>(5)?) {
        (Some(b), Some(s)) => {
            Some(GrantRevoke::parse(&b, &s).ok_or_else(|| anyhow::anyhow!("a run grant's revoke isn't a revoke"))?)
        }
        _ => None,
    };
    Ok(StoredGrant { grant, posted_by: r.get(2)?, created_at: r.get(3)?, revoke, revoked_at: r.get(6)? })
}

/// What [`Db::add_run_grant`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Kept,
    /// The project has a live grant: it's revoked before another is kept.
    LiveAlready,
    /// A grant with this id is here already.
    Seen,
}

impl Db {
    /// Keep a grant for its project, unless the project has a live one.
    /// The caller checked it's well formed.
    pub fn add_run_grant(&self, g: &ProjectGrant, posted_by: &str, now: u64) -> Result<Added> {
        if self.live_run_grant(&g.project)?.is_some() {
            return Ok(Added::LiveAlready);
        }
        let made = self.c().execute(
            "INSERT INTO run_grants (id, project_id, delegation, body, sig, by_device, made_at, expires_at, posted_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![g.id, g.project, g.delegation, g.body(), g.sig, g.by, g.made, g.expires, posted_by, now],
        );
        match made.map_err(taken) {
            // Its id, or a live grant that came in since (the unique index
            // on the project's live one).
            Err(e) if e.is::<Taken>() => {
                let seen: u64 =
                    self.c().query_row("SELECT count(*) FROM run_grants WHERE id = ?1", params![g.id], |r| r.get(0))?;
                Ok(if seen > 0 { Added::Seen } else { Added::LiveAlready })
            }
            r => r.map(|_| Added::Kept),
        }
    }

    /// The project's live grant, if it has one.
    pub fn live_run_grant(&self, project: &str) -> Result<Option<StoredGrant>> {
        self.c()
            .query_row(
                &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND revoked_at IS NULL"),
                params![project],
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

    /// Every revoke of the project's grants, oldest first: what goes with
    /// each start, so a machine that missed one hears it.
    pub fn run_grant_revokes(&self, project: &str) -> Result<Vec<GrantRevoke>> {
        Ok(self
            .c()
            .query_map(
                &format!(
                    "SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND revoked_at IS NOT NULL
                     ORDER BY revoked_at, id"
                ),
                params![project],
                row,
            )?
            .into_iter()
            .filter_map(|g| g.revoke)
            .collect())
    }

    /// Revoke the grant `r` names, if it's the project's and live: whether
    /// it was. The caller checked the revoke is for it.
    pub fn revoke_run_grant(&self, r: &GrantRevoke, now: u64) -> Result<bool> {
        Ok(self.c().execute(
            "UPDATE run_grants SET revoke_body = ?1, revoke_sig = ?2, revoked_at = ?3, stop_running = ?4
             WHERE id = ?5 AND project_id = ?6 AND revoked_at IS NULL",
            params![r.body(), r.sig, now, r.running == Running::Stop, r.grant, r.project],
        )? > 0)
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

    fn grant(project: &str, keys: &DeviceKeys, made: u64) -> ProjectGrant {
        let d = Delegation {
            project: project.into(),
            owner: "account:owner-1".into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at: 1,
            by: keys.id(),
            sig: String::new(),
        };
        let mut g = ProjectGrant::new(&d, made, None);
        g.sign_with(keys);
        g
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
        let live = db.live_run_grant("prj_1").unwrap().unwrap();
        assert_eq!((live.grant.clone(), live.posted_by.as_deref(), live.created_at), (g.clone(), Some("a1"), 100));
        assert!(db.live_run_grant("prj_2").unwrap().is_none());
        // Revoked: the next one is kept, and both are listed.
        let r = GrantRevoke::new(&g, Running::Stop, 150, &k);
        assert!(db.revoke_run_grant(&r, 160).unwrap());
        assert!(!db.revoke_run_grant(&r, 161).unwrap(), "once");
        assert!(db.live_run_grant("prj_1").unwrap().is_none());
        assert_eq!(db.add_run_grant(&g, "a1", 170).unwrap(), Added::Seen);
        assert_eq!(db.add_run_grant(&g2, "a1", 170).unwrap(), Added::Kept);
        let all = db.run_grants("prj_1", 10).unwrap();
        assert_eq!(all.iter().map(|s| s.grant.id.clone()).collect::<Vec<_>>(), [g2.id.clone(), g.id.clone()]);
        assert_eq!((all[1].revoke.as_ref(), all[1].revoked_at), (Some(&r), Some(160)));
        assert_eq!(db.run_grant_revokes("prj_1").unwrap(), std::slice::from_ref(&r));
        assert_eq!(db.run_grant("prj_1", &g.id).unwrap().unwrap().revoke, Some(r.clone()));
        assert!(db.run_grant("prj_2", &g.id).unwrap().is_none());
        // A revoke for another project's grant id changes nothing.
        let elsewhere = GrantRevoke { project: "prj_2".into(), grant: g2.id.clone(), ..r };
        assert!(!db.revoke_run_grant(&elsewhere, 180).unwrap());
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
        assert!(db.live_run_grant("prj_gone").unwrap().is_none());
        assert_eq!(db.live_run_grant("prj_1").unwrap().unwrap().posted_by, None);
        assert!(db.mentions("poster-1").is_empty(), "{:?}", db.mentions("poster-1"));
    }
}
