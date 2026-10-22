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

use tracing::{info, warn};

use super::resume;

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

/// Enroll this machine in control at `url` with `claim`, and save it in
/// `<state>/control.json`, which the running daemon picks up.
///
/// **The interface M178 defines (#1239, branch `m178-delegation`).** Control
/// enrolls a hosted box under the project's delegation with a ticket or a
/// sandbox id and the box's new keys, and answers with its project-scoped
/// certificate. Until that call lands, a ticket goes through the ticket
/// join M20's sandboxes use (`POST /api/join` with `ticket`), and a sandbox
/// id is refused here.
pub async fn enroll(url: &str, name: &str, claim: &Claim, state_dir: &Path) -> Result<(), NotEnrolled> {
    // A key from before this box (the snapshot's) is every such box's key.
    let key = state_dir.join(crate::control::KEY_FILE);
    if key.exists() && crate::control::joined_in(state_dir).is_none() {
        std::fs::remove_file(&key).map_err(|e| NotEnrolled::Failed(e.into()))?;
    }
    let ticket = match claim {
        Claim::Ticket(t) => t,
        Claim::Sandbox(_) => {
            return Err(NotEnrolled::Refused("this build can't enroll by sandbox id yet (M178)".into()));
        }
    };
    let p = crate::control::join_start(url, name, None, Some(ticket), state_dir, "`arugulad --hosted`")
        .await
        .map_err(refused_or_failed)?;
    let approved = crate::control::join_finish(p).await.map_err(refused_or_failed)?;
    approved.save(state_dir).map_err(NotEnrolled::Failed)?;
    Ok(())
}

/// Control's answer ends it; not reaching control doesn't.
fn refused_or_failed(e: anyhow::Error) -> NotEnrolled {
    let reached = e.chain().all(|c| !c.is::<reqwest::Error>());
    if reached { NotEnrolled::Refused(format!("{e:#}")) } else { NotEnrolled::Failed(e) }
}

/// Enroll with this box's claim, trying again while control can't be
/// reached; `enrolled` once in. A refused claim is the end: a ticket is
/// spent, and a box whose sandbox id was taken is deleted by the platform
/// (fail closed).
pub async fn run(
    url: String,
    name: String,
    ticket: Option<String>,
    state_dir: PathBuf,
    file: PathBuf,
    enrolled: impl Fn(),
) {
    let Some(claim) = claim(ticket, &file) else {
        warn!("hosted: no ticket in {} and no sandbox id: can't enroll", super::TICKET_ENV);
        return;
    };
    let mut backoff = Duration::from_millis(250);
    loop {
        match enroll(&url, &name, &claim, &state_dir).await {
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
    use super::*;

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
