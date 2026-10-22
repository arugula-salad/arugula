//! Runs under the project's standing grant (M211, "One standing grant per
//! project starts runs", aru-099): what a hosted machine checks before the
//! platform's start goes ahead, with every owner's device asleep.
//!
//! The platform sends the grant with each start (`runs.start`). The machine
//! checks it with what it pinned at enrollment, not the platform's word
//! ([`check`]): an owner of its project signed it (the team's roster and
//! members' certificates it holds, or its account's chain), for its own
//! project and the delegation it was enrolled under, it covers what's
//! asked, it hasn't expired, and no owner revoked it.
//!
//! Revokes are S37's: signed by an owner's device, relayed by the platform
//! (`runs.revoke`, and with every start), and **kept** here
//! (`<state>/run-grant-revokes.json`), so the platform can't bring a grant
//! back by leaving its revoke out later.
//!
//! The grant gates only what the platform starts. People's own use of the
//! machine (its owners and editors, M180) doesn't go through it.
//!
//! Until M212 a start carries no operation: the check is the whole of it,
//! and M212 starts the run here once it passes. A revoke that says
//! `running stop` has nothing to stop until then either.

use std::{path::Path, sync::Mutex};

use arugula_control_wire::{
    forge::OpAnswer,
    runs::{RunRevoke, RunStart},
};
use arugula_e2e::{
    hosted::{Owner, Owners},
    now_ms,
    project_grant::{self, GrantRefusal, GrantRevoke, ProjectGrant, Running},
};
use tracing::{info, warn};

use crate::control::Saved;

/// The revokes this machine heard, in its state directory.
const FILE: &str = "run-grant-revokes.json";
/// How many it keeps: a project revokes a grant at a time, so a handful.
const KEEP: usize = 256;

/// One writer at a time for [`FILE`].
static KEPT: Mutex<()> = Mutex::new(());

/// Why a start was refused: a code the platform shows, and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    pub code: &'static str,
    pub says: &'static str,
}

impl From<GrantRefusal> for Refused {
    fn from(r: GrantRefusal) -> Self {
        Refused { code: r.code(), says: r.says() }
    }
}

const NOT_HOSTED: Refused = Refused { code: "not_hosted", says: "this machine isn't a hosted machine" };
const WRONG_WORKSPACE: Refused =
    Refused { code: "wrong_workspace", says: "this hosted machine is another workspace's" };

/// Who may sign for the machine's project, from what it pinned.
fn owners(saved: &Saved) -> Option<Owners<'_>> {
    let pin = saved.hosted.as_ref()?;
    match pin.delegation.owner()? {
        Owner::Team(_) => Some(Owners::Team { roster: saved.roster.as_ref()?, certs: &saved.team_certs }),
        Owner::Account(_) => {
            Some(Owners::Account { trust: &saved.trust, certs: &saved.certs, revocations: &saved.revocations })
        }
    }
}

/// Whether the platform may do `s.cap` on this machine under `s.grant` at
/// `now`, with the revokes it sent and the ones `kept` here.
pub fn check(saved: &Saved, s: &RunStart, kept: &[GrantRevoke], now: u64) -> Result<(), Refused> {
    let Some(pin) = &saved.hosted else { return Err(NOT_HOSTED) };
    if s.workspace != pin.cert.workspace {
        return Err(WRONG_WORKSPACE);
    }
    if s.project != pin.cert.project {
        return Err(GrantRefusal::WrongProject.into());
    }
    let owners = owners(saved).ok_or(GrantRefusal::Unsigned)?;
    let revokes: Vec<GrantRevoke> = s.revokes.iter().chain(kept).cloned().collect();
    project_grant::check(&s.grant, &pin.delegation, &revokes, &owners, &s.cap, now)?;
    Ok(())
}

fn read_kept(dir: &Path) -> Vec<GrantRevoke> {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Keep the revokes among `revokes` that take back `g` here (an owner of
/// this machine's project signed them): whether any was new.
fn keep(dir: &Path, saved: &Saved, g: &ProjectGrant, revokes: &[GrantRevoke]) -> anyhow::Result<bool> {
    let Some(owners) = owners(saved) else { return Ok(false) };
    if saved.hosted.as_ref().is_none_or(|p| p.cert.project != g.project) {
        return Ok(false);
    }
    let _held = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let mut kept = read_kept(dir);
    let mut new = false;
    for r in revokes.iter().filter(|r| r.revokes(g, &owners)) {
        if !kept.iter().any(|k| k.grant == r.grant) {
            kept.push(r.clone());
            new = true;
        }
    }
    if new {
        let over = kept.len().saturating_sub(KEEP);
        kept.drain(..over);
        crate::store::write_atomic(&dir.join(FILE), &serde_json::to_vec_pretty(&kept)?)?;
    }
    Ok(new)
}

fn saved(app: &crate::server::App) -> Option<Saved> {
    app.control.enrolled().map(|e| e.saved.clone())
}

/// `runs.start`: check the grant, and answer. Nothing runs yet (M212).
pub fn start(app: &crate::server::App, s: &RunStart) -> OpAnswer {
    let Some(saved) = saved(app) else { return refused(NOT_HOSTED) };
    let dir = app.control.state_dir();
    if let Err(e) = keep(dir, &saved, &s.grant, &s.revokes) {
        warn!(error = %format!("{e:#}"), "hosted: can't keep a standing grant's revoke");
    }
    match check(&saved, s, &read_kept(dir), now_ms()) {
        Ok(()) => {
            info!(
                grant = s.grant.id,
                cap = s.cap,
                run = s.run.as_deref().unwrap_or("-"),
                "hosted: a start under the project's standing grant checks out"
            );
            OpAnswer::Done { url: None }
        }
        Err(r) => {
            info!(grant = s.grant.id, code = r.code, "hosted: refused a start under the project's standing grant");
            refused(r)
        }
    }
}

/// `runs.revoke`: keep it, if an owner of this machine's project signed it.
pub fn revoke(app: &crate::server::App, r: &RunRevoke) -> OpAnswer {
    let Some(saved) = saved(app) else { return refused(NOT_HOSTED) };
    match keep(app.control.state_dir(), &saved, &r.grant, std::slice::from_ref(&r.revoke)) {
        Ok(new) => {
            let stop = r.revoke.running == Running::Stop;
            info!(grant = r.grant.id, new, stop, "hosted: the project's standing grant was revoked");
            if owners(&saved).is_some_and(|o| r.revoke.revokes(&r.grant, &o)) {
                OpAnswer::Done { url: None }
            } else {
                refused(GrantRefusal::Unsigned.into())
            }
        }
        Err(e) => OpAnswer::Failed { error: format!("{e:#}") },
    }
}

fn refused(r: Refused) -> OpAnswer {
    OpAnswer::Cant { why: format!("{}: {}", r.code, r.says) }
}

#[cfg(test)]
mod tests {
    use arugula_control_wire::hosted;
    use arugula_e2e::{
        Cert, DeviceKeys, Kind, Trust,
        hosted::{Delegation, HostedCert, HostedKey},
    };

    use super::*;

    /// A hosted machine of an account's project, enrolled under a
    /// delegation from the account's root device.
    fn machine() -> (Saved, Delegation, DeviceKeys) {
        let root = DeviceKeys::generate();
        let mut root_cert = Cert::new(&root, "acct1", Kind::Browser, "laptop");
        root_cert.sign_with(&root);
        let key = HostedKey::generate();
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&root);
        let keys = DeviceKeys::generate();
        let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
        let mut cert = HostedCert::new(&d, "hw_1", "box", &noise, &sign, 2);
        key.sign(&mut cert);
        let got = hosted::Enrollment {
            url: "http://control".into(),
            cert,
            delegation: d.clone(),
            revocations: vec![],
            team: None,
            roster: None,
            certs: Default::default(),
            trust: Some(Trust { account: "acct1".into(), root: root.id() }),
            account_certs: vec![root_cert],
            account_revocations: vec![],
        };
        (super::super::enroll::saved_from(got, &keys, None).unwrap(), d, root)
    }

    fn start(g: &ProjectGrant) -> RunStart {
        RunStart {
            project: "prj_1".into(),
            workspace: "hw_1".into(),
            run: None,
            cap: "runs.start".into(),
            grant: g.clone(),
            revokes: vec![],
        }
    }

    #[test]
    fn a_start_checks_the_grant_against_the_enrollment() {
        let (saved, d, root) = machine();
        let mut g = ProjectGrant::new(&d, 10, None);
        g.sign_with(&root);
        assert_eq!(check(&saved, &start(&g), &[], 20), Ok(()));
        // Another workspace's start, another project's, a grant signed by
        // someone else, one for another delegation.
        let s = RunStart { workspace: "hw_2".into(), ..start(&g) };
        assert_eq!(check(&saved, &s, &[], 20), Err(WRONG_WORKSPACE));
        let s = RunStart { project: "prj_2".into(), ..start(&g) };
        assert_eq!(check(&saved, &s, &[], 20).unwrap_err().code, "grant_wrong_project");
        let mut forged = g.clone();
        forged.sign_with(&DeviceKeys::generate());
        assert_eq!(check(&saved, &start(&forged), &[], 20).unwrap_err().code, "grant_unsigned");
        let other = Delegation { at: 5, ..d.clone() };
        let mut moved = ProjectGrant::new(&other, 10, None);
        moved.sign_with(&root);
        assert_eq!(check(&saved, &start(&moved), &[], 20).unwrap_err().code, "grant_wrong_delegation");
        // A machine that isn't hosted.
        let plain = Saved { hosted: None, ..saved.clone() };
        assert_eq!(check(&plain, &start(&g), &[], 20), Err(NOT_HOSTED));
    }

    #[test]
    fn a_revoke_heard_once_is_kept() {
        let (saved, d, root) = machine();
        let dir = super::super::scratch("run-grant");
        let mut g = ProjectGrant::new(&d, 10, None);
        g.sign_with(&root);
        let r = GrantRevoke::new(&g, Running::Finish, 30, &root);
        // A revoke nobody in the project signed isn't kept.
        let spite = GrantRevoke::new(&g, Running::Stop, 30, &DeviceKeys::generate());
        assert!(!keep(&dir, &saved, &g, &[spite]).unwrap());
        assert!(read_kept(&dir).is_empty());
        assert!(keep(&dir, &saved, &g, std::slice::from_ref(&r)).unwrap());
        assert!(!keep(&dir, &saved, &g, std::slice::from_ref(&r)).unwrap(), "once");
        // The platform leaves it out of the next start: still revoked.
        assert_eq!(check(&saved, &start(&g), &read_kept(&dir), 40).unwrap_err().code, "grant_revoked");
        // A new grant isn't.
        let mut g2 = ProjectGrant::new(&d, 50, None);
        g2.sign_with(&root);
        assert_eq!(check(&saved, &start(&g2), &read_kept(&dir), 60), Ok(()));
    }
}
