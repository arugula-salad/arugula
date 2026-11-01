//! The standing project grants' rows (M211), made by migration `0019`:
//! what an owner signed, kept as the bytes signed and read back from them
//! ([`ProjectGrant::parse`]), so nothing the platform holds beside them can
//! change what a grant says. [`crate::run_grant`] has the rules.
//!
//! Since DK6 (#387) a grant is v2 and a revoke v3, each signed by a passkey
//! assertion over its bytes rather than a device: its row keeps the
//! assertion as JSON in the signature's column (`sig`, `revoke_sig`), and
//! `by_device` is empty. The first line of the bytes says which it is, so
//! v1 rows (and device revokes) read as they always did.
//!
//! A project has at most one live grant: not revoked, not superseded, and
//! not expired. A new one supersedes the live row (renewing it, or one that
//! expired or names another delegation: the caller checked the new one
//! names the project's live delegation). A revoke takes back every grant of
//! the project under its delegation made by its time, v1 or v2, and each of
//! those rows keeps it.

use anyhow::{Result, bail};
use arugula_e2e::project_grant::{GrantRevoke, ProjectGrant, REVOKE_DEVICE, Running, V1};

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
    /// A newer grant took its place: it was renewed, it had expired, or its
    /// delegation wasn't the project's live one any more.
    pub superseded_at: Option<u64>,
}

/// [`Db::live_run_grant`] in the caller's transaction (M214: a run a
/// trigger makes names it).
pub fn live_in(c: &impl Sql, project: &str, now: u64) -> Result<Option<StoredGrant>> {
    c.query_row(
        &format!("SELECT {COLUMNS} FROM run_grants WHERE project_id = ?1 AND {LIVE}"),
        params![project, now],
        row,
    )
    .optional()
}

const COLUMNS: &str = "body, sig, posted_by, created_at, revoke_body, revoke_sig, revoked_at, superseded_at";

/// What makes a row the project's live grant at `?2` (ms).
const LIVE: &str = "revoked_at IS NULL AND superseded_at IS NULL AND (expires_at IS NULL OR expires_at > ?2)";

/// A grant from its row's bytes and signature: a v1's device signature,
/// or a v2's passkey assertion (JSON).
fn grant_of(body: &str, sig: &str) -> Result<ProjectGrant> {
    if let Some(g) = ProjectGrant::parse(body, sig) {
        return Ok(g);
    }
    let Some(mut g) = ProjectGrant::parse(body, "") else { bail!("a run grant's stored bytes aren't a grant") };
    g.passkey = Some(serde_json::from_str(sig)?);
    Ok(g)
}

/// A revoke from its bytes and signature, as [`grant_of`].
fn revoke_of(body: &str, sig: &str) -> Result<GrantRevoke> {
    if let Some(r) = GrantRevoke::parse(body, sig) {
        return Ok(r);
    }
    let Some(mut r) = GrantRevoke::parse(body, "") else { bail!("a run grant's revoke isn't a revoke") };
    r.passkey = Some(serde_json::from_str(sig)?);
    Ok(r)
}

/// What a grant's row keeps as its signature: a v1's device signature, a
/// v2's passkey assertion as JSON.
fn grant_sig(g: &ProjectGrant) -> Result<String> {
    Ok(if g.v == V1 { g.sig.clone() } else { serde_json::to_string(&g.passkey)? })
}

/// [`grant_sig`] for a revoke.
fn revoke_sig(r: &GrantRevoke) -> Result<String> {
    Ok(if r.v == REVOKE_DEVICE { r.sig.clone() } else { serde_json::to_string(&r.passkey)? })
}

fn row(r: &Row) -> Result<StoredGrant> {
    let (body, sig): (String, String) = (r.get(0)?, r.get(1)?);
    let grant = grant_of(&body, &sig)?;
    let revoke = match (r.get::<Option<String>>(4)?, r.get::<Option<String>>(5)?) {
        (Some(b), Some(s)) => Some(revoke_of(&b, &s)?),
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
    /// Another grant was kept for the project at the same moment (the
    /// unique index on its live one).
    LiveAlready,
    /// A revoke of the project's grants under its delegation was signed at
    /// or after it was made: it was revoked before it got here.
    Revoked,
    /// A grant with this id is here already.
    Seen,
}

impl Db {
    /// Keep a grant for its project at `now`, unless a revoke covers it.
    /// The project's live one is superseded (DK6: renewing is signing a new
    /// one, with one passkey, not a revoke and a grant). The caller checked
    /// it's well formed, signed, and names the project's live delegation.
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
        let seen: u64 = tx.query_row("SELECT count(*) FROM run_grants WHERE id = ?1", params![g.id], |r| r.get(0))?;
        if seen > 0 {
            return Ok(Added::Seen);
        }
        tx.execute(
            "UPDATE run_grants SET superseded_at = ?2
             WHERE project_id = ?1 AND revoked_at IS NULL AND superseded_at IS NULL",
            params![g.project, now],
        )?;
        let made = tx.execute(
            "INSERT INTO run_grants (id, project_id, delegation, body, sig, by_device, made_at, expires_at, posted_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![g.id, g.project, g.delegation, g.body(), grant_sig(g)?, g.by, g.made, g.expires, posted_by, now],
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
        live_in(&self.c(), project, now)
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
            params![r.body(), revoke_sig(r)?, r.at, now, r.running == Running::Stop, r.project, r.delegation],
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
pub(crate) mod tests {
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

    pub(crate) fn grant(project: &str, keys: &DeviceKeys, made: u64) -> ProjectGrant {
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
        assert_eq!(db.add_run_grant(&g, "a1", 101).unwrap(), Added::Seen);
        let live = db.live_run_grant("prj_1", 101).unwrap().unwrap();
        assert_eq!((live.grant.clone(), live.posted_by.as_deref(), live.created_at), (g.clone(), Some("a1"), 100));
        // Another (renewing it, DK6) takes its place: one live a project.
        let g2 = grant("prj_1", &k, 20);
        assert_eq!(db.add_run_grant(&g2, "a1", 102).unwrap(), Added::Kept);
        assert_eq!(db.live_run_grant("prj_1", 103).unwrap().unwrap().grant, g2);
        assert_eq!(db.run_grant("prj_1", &g.id).unwrap().unwrap().superseded_at, Some(102));
        assert!(db.live_run_grant("prj_2", 103).unwrap().is_none());
        // Revoked (it names no grant: every one made by its time, replaced
        // or not): the next one is kept, and all are listed.
        let r = revoke(&g, 150, &k);
        assert_eq!(db.revoke_run_grants(&r, 160).unwrap(), 2);
        assert_eq!(db.revoke_run_grants(&r, 161).unwrap(), 0, "once");
        assert!(db.live_run_grant("prj_1", 162).unwrap().is_none());
        assert_eq!(db.add_run_grant(&g, "a1", 170).unwrap(), Added::Revoked);
        assert_eq!(db.add_run_grant(&g2, "a1", 170).unwrap(), Added::Revoked, "made before the revoke");
        let g3 = grant("prj_1", &k, 151);
        assert_eq!(db.add_run_grant(&g3, "a1", 170).unwrap(), Added::Kept);
        let all = db.run_grants("prj_1", 10).unwrap();
        assert_eq!(
            all.iter().map(|s| s.grant.id.clone()).collect::<Vec<_>>(),
            [g3.id.clone(), g2.id.clone(), g.id.clone()]
        );
        assert_eq!((all[2].revoke.as_ref(), all[2].revoked_at), (Some(&r), Some(160)));
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
        assert_eq!(db.add_run_grant(&g2, "a1", 600).unwrap(), Added::Kept);
        assert_eq!(db.run_grant("prj_1", &g.id).unwrap().unwrap().superseded_at, Some(600));
        // The project's delegation changed: the next grant names the new one.
        let d2 = delegation("prj_1", &k, 2);
        let g3 = grant_under(&d2, &k, 700, Some(5000));
        assert_eq!(db.add_run_grant(&g3, "a1", 700).unwrap(), Added::Kept);
        assert_eq!(db.live_run_grant("prj_1", 701).unwrap().unwrap().grant, g3);
        assert_eq!(db.run_grant("prj_1", &g2.id).unwrap().unwrap().superseded_at, Some(700));
    }

    /// DK6: a v2 grant and a v3 revoke are kept with their passkey
    /// assertions and read back whole, beside a v1 row and a device's
    /// revoke, which read as they always did.
    #[test]
    fn passkey_grants_and_revokes_read_back() {
        let db = Db::test();
        let k = DeviceKeys::generate();
        let d = delegation("prj_1", &k, 1);
        let v1 = grant_under(&d, &k, 10, Some(5000));
        assert_eq!(db.add_run_grant(&v1, "a1", 100).unwrap(), Added::Kept);
        let pk = arugula_e2e::passkey::Assertion {
            id: "pk".into(),
            client_data_json: "cd".into(),
            authenticator_data: "ad".into(),
            signature: "sig".into(),
        };
        let mut v2 = ProjectGrant::v2(&d, "a1", 20, 5000);
        v2.passkey = Some(pk.clone());
        assert_eq!(db.add_run_grant(&v2, "a1", 110).unwrap(), Added::Kept);
        assert_eq!(db.live_run_grant("prj_1", 120).unwrap().unwrap().grant, v2);
        assert_eq!(db.run_grant("prj_1", &v1.id).unwrap().unwrap().grant, v1);
        let mut r = GrantRevoke::v3("prj_1", &d.id(), Running::Finish, 150, "a1");
        r.passkey = Some(pk);
        assert_eq!(db.revoke_run_grants(&r, 160).unwrap(), 2, "the v1 and the v2");
        assert_eq!(db.run_grant("prj_1", &v1.id).unwrap().unwrap().revoke, Some(r.clone()));
        assert_eq!(db.run_grant_revokes("prj_1").unwrap(), [r]);
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
