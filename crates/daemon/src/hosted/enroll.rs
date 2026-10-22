//! Enrolling a hosted box (M180) under its project's delegation (M178).
//!
//! What it enrolls with, its [`Claim`]:
//! - **A one-time ticket** in `ARUGULA_HOSTED_TICKET`, when the daemon was
//!   started after the box was made (loam, testnet, a plain VM): it's in
//!   the process's environment.
//! - **Else its E2B sandbox id.** The template's daemon was snapshotted
//!   running, so it never sees the environment a box is created with: E2B's
//!   envd keeps that in memory and serves it (`GET :49983/envs`) only with
//!   the box's access token, which only its creator has. What the daemon
//!   can see is the sandbox id envd writes at `/init`
//!   (`/run/e2b/.E2B_SANDBOX_ID`): when it differs from the one the
//!   snapshot was taken in, this is a new box, and it enrolls with that.
//!   Control takes a sandbox id once, for about two minutes after the
//!   platform made the box, and refuses a second try; a box whose id was
//!   taken by someone else is deleted (Jake, 2026-10-21: fail closed).
//!
//! The key is made here, after the box is: every box made from one
//! snapshot starts with the same memory and files, so a key from before
//! the snapshot would be the same key in all of them.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, ensure};
use arugula_control_wire::hosted;
use arugula_e2e::{
    Cert, DeviceKeys, Kind, Trust,
    cert::verify_hex,
    hosted::{Owner, Owners, enroll_proof_body},
    now_ms,
};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::resume;
use crate::control::{HostedPin, Saved};

/// What a box enrolls with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Ticket(String),
    Sandbox(String),
}

/// Why an enrollment didn't happen.
#[derive(Debug)]
pub enum NotEnrolled {
    /// Control said no: the claim is spent or was never good. Never tried
    /// again.
    Refused(String),
    /// Control couldn't be asked, or failed: try again shortly.
    Failed(anyhow::Error),
}

/// Wait for a new box: a sandbox id in `file` other than the one there
/// now (the snapshot's, or the template build's), or one at all.
pub async fn new_box(file: &Path) -> String {
    let built = resume::stamp(file).map(|(id, _)| id);
    info!(snapshot = built.as_deref().unwrap_or("none"), "hosted: waiting for a box of its own");
    loop {
        if let Some((id, _)) = resume::stamp(file)
            && !id.is_empty()
            && built.as_ref() != Some(&id)
        {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What this box enrolls with: the ticket in its environment, else the
/// sandbox id in `file`.
pub fn claim(ticket: Option<String>, file: &Path) -> Option<Claim> {
    if let Some(t) = ticket.filter(|t| !t.is_empty()) {
        return Some(Claim::Ticket(t));
    }
    resume::stamp(file).map(|(id, _)| id).filter(|id| !id.is_empty()).map(Claim::Sandbox)
}

/// Enroll this machine in control at `url` with `claim` (M178's
/// `POST /api/hosted/enroll`): new keys, a proof that it holds them for
/// this claim, and what control answers kept in `<state>/control.json`,
/// which the running daemon picks up. Every answer of control's but a
/// server error is final (fail closed).
pub async fn enroll(url: &str, claim: &Claim, state_dir: &Path) -> Result<(), NotEnrolled> {
    // A key from before this box (the snapshot's) would be every such box's.
    let key = state_dir.join(crate::control::KEY_FILE);
    if key.exists() && crate::control::joined_in(state_dir).is_none() {
        std::fs::remove_file(&key).map_err(|e| NotEnrolled::Failed(e.into()))?;
    }
    let keys = DeviceKeys::load_or_create(&key).map_err(NotEnrolled::Failed)?;
    let req = request(claim, &keys, now_ms());
    let http =
        crate::roots::http().timeout(Duration::from_secs(20)).build().map_err(|e| NotEnrolled::Failed(e.into()))?;
    let res = http
        .post(format!("{url}{}", hosted::ENROLL))
        .json(&req)
        .send()
        .await
        .map_err(|e| NotEnrolled::Failed(anyhow::Error::new(e).context(format!("can't reach control at {url}"))))?;
    let status = res.status();
    if status.is_server_error() {
        return Err(NotEnrolled::Failed(anyhow::anyhow!("control answered {status}")));
    }
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(NotEnrolled::Refused(format!("{status}: {}", body.trim())));
    }
    let got: hosted::Enrollment =
        res.json().await.map_err(|e| NotEnrolled::Refused(format!("control's answer: {e}")))?;
    // What control signed must check out, or nothing is kept.
    let saved = saved_from(got, &keys, None).map_err(|e| NotEnrolled::Refused(format!("{e:#}")))?;
    crate::control::write_saved(state_dir, &saved).map_err(NotEnrolled::Failed)?;
    Ok(())
}

/// The request for `claim`, signed by `keys` at `at`.
pub fn request(claim: &Claim, keys: &DeviceKeys, at: u64) -> hosted::EnrollRequest {
    let (ticket, sandbox, credential) = match claim {
        Claim::Ticket(t) => (Some(t.clone()), None, t),
        Claim::Sandbox(s) => (None, Some(s.clone()), s),
    };
    let credential = hex::encode(Sha256::digest(credential.as_bytes()));
    let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
    let proof = hex::encode(keys.signature(enroll_proof_body(&credential, &noise, &sign, at).as_bytes()));
    hosted::EnrollRequest { ticket, sandbox, noise, sign, at, proof }
}

/// What a hosted machine keeps of control's [`hosted::Enrollment`], checked:
/// its certificate is for its own keys and signed with the key the
/// delegation names; the delegation is signed by an owner of the project as
/// it's described. At enrollment that description is control's word (the
/// operator can read the box anyway: "Hosted: the operator can read"); after
/// it, a team's roster must follow the one it holds, as a team machine's
/// does, and the project, team and account root can't change.
///
/// Who gets in is the project's people: a team's members by their roles
/// (owners as the owner), or an account's own devices.
pub fn saved_from(got: hosted::Enrollment, keys: &DeviceKeys, prev: Option<&Saved>) -> anyhow::Result<Saved> {
    let hosted::Enrollment {
        url,
        cert,
        delegation,
        revocations,
        team,
        roster,
        certs,
        trust,
        account_certs,
        account_revocations,
    } = got;
    ensure!(cert.device == keys.id(), "control sent a certificate for another key");
    cert.check_form()?;
    ensure!(
        delegation.well_formed()
            && cert.project == delegation.project
            && cert.owner == delegation.owner
            && cert.delegation == delegation.id(),
        "the certificate and the delegation don't name each other"
    );
    ensure!(
        verify_hex(&delegation.key, cert.body().as_bytes(), &cert.sig),
        "the certificate isn't signed with the delegation's key"
    );
    if let Some(p) = prev.and_then(|p| p.hosted.as_ref()) {
        ensure!(p.cert == cert, "control sent another certificate for this machine");
    }
    let (account, trust, own_certs, own_revocations, team, roster, team_certs) = match delegation.owner() {
        Some(Owner::Team(id)) => {
            let pin = team.context("a team's project came without its team")?;
            ensure!(pin.team == id, "the project's team isn't the delegation's");
            let mut r = roster.context("a team's project came without its roster")?;
            if let Some(p) = prev {
                ensure!(p.team.as_ref() == Some(&pin), "control changed this machine's team");
                if let Some(last) = &p.roster
                    && r != *last
                    && !r.follows(Some(last), &pin, &certs)
                {
                    warn!(
                        version = r.version,
                        "hosted: a team roster from control doesn't check out; keeping the last"
                    );
                    r = last.clone();
                }
            }
            ensure!(r.team == pin.team && r.well_formed(), "the team's roster isn't well formed");
            ensure!(
                delegation.signed_for(&Owners::Team { roster: &r, certs: &certs }),
                "the delegation isn't signed by an owner of the team"
            );
            let mut certs = certs;
            certs.retain(|a, _| r.member(a).is_some());
            let trust = Trust { account: pin.founder.clone(), root: pin.founder_root.clone() };
            (pin.founder.clone(), trust, vec![], vec![], Some(pin), Some(r), certs)
        }
        Some(Owner::Account(id)) => {
            let t = trust.context("an account's project came without its account")?;
            ensure!(t.account == id, "the project's account isn't the delegation's");
            if let Some(p) = prev {
                ensure!(p.trust.root == t.root, "control changed the account's root");
            }
            ensure!(
                delegation.signed_for(&Owners::Account {
                    trust: &t,
                    certs: &account_certs,
                    revocations: &account_revocations
                }),
                "the delegation isn't signed by the account's device"
            );
            (id, t, account_certs, account_revocations, None, None, Default::default())
        }
        None => anyhow::bail!("the delegation names no owner"),
    };
    let own = Cert {
        v: 1,
        account,
        device: cert.device.clone(),
        kind: Kind::Daemon,
        name: cert.name.clone(),
        noise: cert.noise.clone(),
        sign: cert.sign.clone(),
        created: cert.created,
        approver: String::new(),
        sig: cert.sig.clone(),
    };
    Ok(Saved {
        url: url.trim_end_matches('/').to_owned(),
        trust,
        cert: own,
        certs: own_certs,
        revocations: own_revocations,
        team,
        roster,
        team_certs,
        team_names: Default::default(),
        locked: false,
        peers: Default::default(),
        shared_teams: Default::default(),
        login: String::new(),
        moved_at: 0,
        hosted: Some(HostedPin { cert, delegation, revocations }),
    })
}

/// Enroll with this box's claim, trying again while control can't be
/// reached; `enrolled` once in. A refused claim is the end: a ticket is
/// spent, and a box whose sandbox id was taken is deleted by the platform
/// (fail closed).
pub async fn run(url: String, ticket: Option<String>, state_dir: PathBuf, file: PathBuf, enrolled: impl Fn()) {
    let Some(claim) = claim(ticket, &file) else {
        warn!("hosted: no ticket in {} and no sandbox id: can't enroll", super::TICKET_ENV);
        return;
    };
    let mut backoff = Duration::from_millis(250);
    loop {
        match enroll(&url, &claim, &state_dir).await {
            Ok(()) => {
                info!(control = url, "hosted: enrolled");
                enrolled();
                return;
            }
            Err(NotEnrolled::Failed(e)) => {
                warn!(error = %format!("{e:#}"), "hosted: can't enroll yet");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(10));
            }
            Err(NotEnrolled::Refused(why)) => {
                warn!(%why, "hosted: control refused this box's enrollment");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use arugula_e2e::hosted::{Delegation, HostedCert, HostedKey};

    use super::*;

    /// An account's project with a delegation from its root device, and
    /// control's answer for the machine with `keys`.
    fn enrollment(keys: &DeviceKeys) -> (hosted::Enrollment, DeviceKeys) {
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
        let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
        let mut cert = HostedCert::new(&d, "hw_1", "box", &noise, &sign, 2);
        key.sign(&mut cert);
        let got = hosted::Enrollment {
            url: "http://control/".into(),
            cert,
            delegation: d,
            revocations: vec![],
            team: None,
            roster: None,
            certs: Default::default(),
            trust: Some(Trust { account: "acct1".into(), root: root.id() }),
            account_certs: vec![root_cert],
            account_revocations: vec![],
        };
        (got, root)
    }

    #[test]
    fn what_a_hosted_machine_keeps_checks_out() {
        let keys = DeviceKeys::generate();
        let (got, root) = enrollment(&keys);
        let saved = saved_from(got.clone(), &keys, None).unwrap();
        assert_eq!((saved.url.as_str(), saved.cert.device.as_str()), ("http://control", keys.id().as_str()));
        assert_eq!(saved.cert.account, "acct1");
        assert!(saved.hosted.is_some());
        // The account's own devices get in, as the owner.
        assert!(saved.trust.evaluate(&saved.certs, &saved.revocations).get(&root.id()).is_some());
        // And again on a refresh.
        assert!(saved_from(got.clone(), &keys, Some(&saved)).is_ok());
        // Another key's certificate, one not signed by the delegation's key,
        // a delegation nobody in the project signed, a root that changed.
        assert!(saved_from(got.clone(), &DeviceKeys::generate(), None).is_err());
        let mut bad = got.clone();
        bad.cert.sig = "00".repeat(64);
        assert!(saved_from(bad, &keys, None).is_err());
        let mut bad = got.clone();
        bad.delegation.sign_with(&DeviceKeys::generate());
        assert!(saved_from(bad, &keys, None).is_err());
        let (other, _) = enrollment(&keys);
        assert!(saved_from(other, &keys, Some(&saved)).is_err());
    }

    #[test]
    fn the_request_proves_the_key_for_this_claim_only() {
        let keys = DeviceKeys::generate();
        let r = request(&Claim::Sandbox("sbx1".into()), &keys, 5);
        assert_eq!((r.ticket.as_deref(), r.sandbox.as_deref(), r.at), (None, Some("sbx1"), 5));
        let credential = hex::encode(Sha256::digest(b"sbx1"));
        let body = enroll_proof_body(&credential, &r.noise, &r.sign, 5);
        assert!(verify_hex(&r.sign, body.as_bytes(), &r.proof));
        let t = request(&Claim::Ticket("t1".into()), &keys, 5);
        assert_eq!((t.ticket.as_deref(), t.sandbox.as_deref()), (Some("t1"), None));
        assert!(!verify_hex(&t.sign, body.as_bytes(), &t.proof));
    }

    #[tokio::test]
    async fn a_ticket_first_else_the_boxs_own_sandbox_id() {
        let file = super::super::scratch("enroll").join(".E2B_SANDBOX_ID");
        assert_eq!(claim(None, &file), None);
        assert_eq!(claim(Some("t1".into()), &file), Some(Claim::Ticket("t1".into())));
        std::fs::write(&file, "box1\n").unwrap();
        assert_eq!(claim(Some(String::new()), &file), Some(Claim::Sandbox("box1".into())));
        // The snapshot's id isn't a new box: only another is.
        let waiting = tokio::spawn({
            let file = file.clone();
            async move { new_box(&file).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiting.is_finished());
        std::fs::write(&file, "box2").unwrap();
        assert_eq!(waiting.await.unwrap(), "box2");
    }
}
