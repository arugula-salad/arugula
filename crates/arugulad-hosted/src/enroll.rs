//! Enrolling a hosted box (M180) under its project's delegation (M178).
//!
//! What it enrolls with, its [`Claim`]:
//! - **A one-time ticket** in `ARUGULA_HOSTED_TICKET`, when the daemon was
//!   started after the box was made (loam, testnet, a plain VM): it's in
//!   the process's environment.
//! - **Else a one-time ticket in [`TICKET_FILE`]**, which the platform
//!   writes into an E2B box once it's made, through envd with the box's
//!   access token (the daemon was snapshotted running, so it never sees
//!   the create's environment, below). The file is in a directory only the
//!   box's `user` can read (0700, made by the template), and the daemon
//!   removes it once control has answered it. It waits [`TICKET_WAIT`] for
//!   one before it falls back to its sandbox id.
//! - **Else its E2B sandbox id**, for a platform that writes no ticket
//!   (this release only: it goes once every box is made by a platform that
//!   writes one). The template's daemon was snapshotted
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

use arugula_control_wire::hosted;
use arugula_e2e::{DeviceKeys, hosted::enroll_proof_body, now_ms};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::resume;
// What control sends is checked by the base, which checks it again at each
// refresh.
use crate::hosted::trust::saved_from;

/// What a box enrolls with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Ticket(String),
    Sandbox(String),
}

/// Where the platform writes a box's one-time ticket: in `user`'s home,
/// in a directory the template makes 0700, outside the daemon's state
/// directory (which a box from a workspace's snapshot clears at its first
/// start, perhaps after the ticket came).
pub const TICKET_FILE: &str = "/home/user/.arugula-hosted/ticket";

/// [`TICKET_FILE`], or another for tests (`ARUGULA_HOSTED_TICKET_FILE`).
pub fn ticket_file() -> PathBuf {
    std::env::var_os("ARUGULA_HOSTED_TICKET_FILE").filter(|v| !v.is_empty()).map_or(TICKET_FILE.into(), PathBuf::from)
}

/// How long a box with no ticket in its environment waits for the
/// platform's [`TICKET_FILE`] before it enrolls with its sandbox id: well
/// inside the two minutes control takes a sandbox id for.
pub const TICKET_WAIT: Duration = Duration::from_secs(45);

/// The ticket in `file`, if there's one there.
pub fn read_ticket(file: &Path) -> Option<String> {
    let t = std::fs::read_to_string(file).ok()?;
    let t = t.trim();
    (!t.is_empty() && t.len() <= 256).then(|| t.to_owned())
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
/// now (the snapshot's, or the template build's), or one at all. A box
/// from a workspace's snapshot (M263) boots from disk with its own id
/// there already: `snapshot_of` (the builder's id) is then the one that
/// isn't new, and any other is this box's at once.
pub async fn new_box(file: &Path, snapshot_of: Option<&str>) -> String {
    let built = match snapshot_of {
        Some(of) => Some(of.to_owned()),
        None => resume::stamp(file).map(|(id, _)| id),
    };
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
/// one in `ticket_file`, else the sandbox id in `id_file`.
pub fn claim(ticket: Option<String>, ticket_file: &Path, id_file: &Path) -> Option<Claim> {
    if let Some(t) = ticket.filter(|t| !t.is_empty()) {
        return Some(Claim::Ticket(t));
    }
    if let Some(t) = read_ticket(ticket_file) {
        return Some(Claim::Ticket(t));
    }
    resume::stamp(id_file).map(|(id, _)| id).filter(|id| !id.is_empty()).map(Claim::Sandbox)
}

/// [`claim`], once there's a ticket: in the environment at once, else in
/// `ticket_file` within `wait`, else whatever [`claim`] finds then.
pub async fn wait_for_claim(
    ticket: Option<String>,
    ticket_file: &Path,
    id_file: &Path,
    wait: Duration,
) -> Option<Claim> {
    if ticket.as_deref().is_some_and(|t| !t.is_empty()) {
        return claim(ticket, ticket_file, id_file);
    }
    let until = tokio::time::Instant::now() + wait;
    while tokio::time::Instant::now() < until {
        if let Some(t) = read_ticket(ticket_file) {
            return Some(Claim::Ticket(t));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    claim(None, ticket_file, id_file)
}

/// Enroll this machine in control at `url` with `claim` (M178's
/// `POST /api/hosted/enroll`): new keys, a proof that it holds them for
/// this claim, and what control answers kept in `<state>/control.json`,
/// which the running daemon picks up. Every answer of control's but a
/// server error is final (fail closed).
pub async fn enroll(url: &str, claim: &Claim, state_dir: &Path) -> Result<(), NotEnrolled> {
    // An enrollment that's there but doesn't load (#672) is kept as it
    // is, key and all: enrolling over it would drop what it pinned, and
    // this box's claim is spent either way. The daemon runs as not
    // joined; the workspace is rebuilt.
    let joined = match crate::control::read_saved(state_dir) {
        Ok(s) => s.is_some(),
        Err(e) => {
            return Err(NotEnrolled::Refused(format!(
                "control.json is there but doesn't load ({e:#}); keeping it and this box's key, enrolling nothing"
            )));
        }
    };
    // A key from before this box (the snapshot's) would be every such box's.
    let key = state_dir.join(crate::control::KEY_FILE);
    if key.exists() && !joined {
        std::fs::remove_file(&key).map_err(|e| NotEnrolled::Failed(e.into()))?;
    }
    let keys = DeviceKeys::load_or_create(&key).map_err(NotEnrolled::Failed)?;
    let req = request(claim, &keys, now_ms());
    let http = crate::control::control_http()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| NotEnrolled::Failed(e.into()))?;
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

/// Where a box finds what it enrolls with.
pub struct Sources {
    /// `ARUGULA_HOSTED_TICKET`'s.
    pub ticket: Option<String>,
    /// [`ticket_file`].
    pub ticket_file: PathBuf,
    /// The sandbox id's file (`resume::sandbox_id_file`).
    pub id_file: PathBuf,
    /// [`TICKET_WAIT`], or less for tests.
    pub wait: Duration,
}

/// Enroll with this box's claim, trying again while control can't be
/// reached; `enrolled` once in. A refused claim is the end (a ticket is
/// spent, and a box whose sandbox id was taken is deleted by the platform:
/// fail closed), unless the ticket file holds another ticket by then: the
/// platform's, written after this box read one left from before, or after
/// it gave up waiting. The ticket file goes once control has answered.
pub async fn run(url: String, from: Sources, state_dir: PathBuf, enrolled: impl Fn()) {
    let Some(mut claim) = wait_for_claim(from.ticket.clone(), &from.ticket_file, &from.id_file, from.wait).await else {
        warn!(
            "hosted: no ticket in {} or {} and no sandbox id: can't enroll",
            super::TICKET_ENV,
            from.ticket_file.display()
        );
        return;
    };
    let forget = || match std::fs::remove_file(&from.ticket_file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(error = %e, "hosted: can't remove the ticket file"),
    };
    let mut backoff = Duration::from_millis(250);
    loop {
        match enroll(&url, &claim, &state_dir).await {
            Ok(()) => {
                info!(control = url, "hosted: enrolled");
                forget();
                enrolled();
                return;
            }
            Err(NotEnrolled::Failed(e)) => {
                warn!(error = %format!("{e:#}"), "hosted: can't enroll yet");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(10));
            }
            Err(NotEnrolled::Refused(why)) => {
                if from.ticket.is_none()
                    && let Some(t) = read_ticket(&from.ticket_file)
                    && claim != Claim::Ticket(t.clone())
                {
                    info!("hosted: control refused this box's claim; trying the ticket the platform wrote since");
                    claim = Claim::Ticket(t);
                    continue;
                }
                warn!(%why, "hosted: control refused this box's enrollment");
                forget();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use arugula_e2e::cert::verify_hex;

    use super::*;

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

    /// #672: a `control.json` that's there but doesn't load stops the
    /// enrollment before anything is touched: the key stays, the file
    /// stays as it was, and control isn't asked.
    #[tokio::test]
    async fn an_enrollment_that_doesnt_load_is_kept_and_nothing_enrolls() {
        let dir = super::super::scratch("enroll-unloadable");
        let key = dir.join(crate::control::KEY_FILE);
        let before = DeviceKeys::load_or_create(&key).unwrap().id();
        let file = dir.join(crate::control::FILE);
        let text = r#"{"url": "http://control", "locked": true}"#;
        std::fs::write(&file, text).unwrap();
        // Nothing listens at this URL: a request would fail, not be refused.
        let got = enroll("http://127.0.0.1:9", &Claim::Ticket("t1".into()), &dir).await;
        match got {
            Err(NotEnrolled::Refused(why)) => assert!(why.contains("doesn't load"), "{why}"),
            Err(NotEnrolled::Failed(e)) => panic!("asked control: {e:#}"),
            Ok(()) => panic!("enrolled"),
        }
        assert_eq!(DeviceKeys::load(&key).unwrap().id(), before, "the key stays");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "the file stays");
    }

    #[tokio::test]
    async fn a_ticket_first_else_the_boxs_own_sandbox_id() {
        let dir = super::super::scratch("enroll");
        let (file, tf) = (dir.join(".E2B_SANDBOX_ID"), dir.join("ticket"));
        assert_eq!(claim(None, &tf, &file), None);
        assert_eq!(claim(Some("t1".into()), &tf, &file), Some(Claim::Ticket("t1".into())));
        std::fs::write(&file, "box1\n").unwrap();
        assert_eq!(claim(Some(String::new()), &tf, &file), Some(Claim::Sandbox("box1".into())));
        // The platform's ticket file, before the sandbox id; the
        // environment's before both.
        std::fs::write(&tf, "t2\n").unwrap();
        assert_eq!(claim(None, &tf, &file), Some(Claim::Ticket("t2".into())));
        assert_eq!(claim(Some("t1".into()), &tf, &file), Some(Claim::Ticket("t1".into())));
        std::fs::write(&tf, "  \n").unwrap();
        assert_eq!(claim(None, &tf, &file), Some(Claim::Sandbox("box1".into())), "an empty file is no ticket");
        std::fs::write(&tf, "x".repeat(300)).unwrap();
        assert_eq!(claim(None, &tf, &file), Some(Claim::Sandbox("box1".into())), "nor is one too long");
        std::fs::remove_file(&tf).unwrap();
        // The snapshot's id isn't a new box: only another is.
        let waiting = crate::task::spawn({
            let file = file.clone();
            async move { new_box(&file, None).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiting.is_finished());
        std::fs::write(&file, "box2").unwrap();
        assert_eq!(waiting.await.unwrap(), "box2");
    }

    /// A box waits for the platform's ticket file, and falls back to its
    /// sandbox id when none comes in time.
    #[tokio::test]
    async fn a_box_waits_for_the_ticket_file_then_takes_its_sandbox_id() {
        let dir = super::super::scratch("enroll-wait");
        let (file, tf) = (dir.join(".E2B_SANDBOX_ID"), dir.join("ticket"));
        std::fs::write(&file, "box1").unwrap();
        let waiting = crate::task::spawn({
            let (file, tf) = (file.clone(), tf.clone());
            async move { wait_for_claim(None, &tf, &file, Duration::from_secs(10)).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!waiting.is_finished(), "it waits for the ticket");
        std::fs::write(&tf, "t1").unwrap();
        assert_eq!(waiting.await.unwrap(), Some(Claim::Ticket("t1".into())));
        std::fs::remove_file(&tf).unwrap();
        let none = wait_for_claim(None, &tf, &file, Duration::from_millis(300)).await;
        assert_eq!(none, Some(Claim::Sandbox("box1".into())), "none came: its sandbox id");
        let env = wait_for_claim(Some("t0".into()), &tf, &file, Duration::from_secs(10)).await;
        assert_eq!(env, Some(Claim::Ticket("t0".into())), "the environment's, at once");
    }
}
