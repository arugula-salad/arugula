//! Waking a workspace's box for the hub (M182, #1243): someone opened its
//! machine (a terminal, a block, the phone, the CLI or MCP) and its daemon
//! isn't connected ("A hosted machine is arugulad in a rented sandbox":
//! the hub wakes it on connect). The hub asks over `PlatformPort::hosted_wake`
//! and waits here until the box runs or didn't wake.
//!
//! - **One resume.** The worker does the resume, as it does any wake
//!   ([`super::worker`]): this asks for it ([`super::Ask::Wake`], which the
//!   database takes once while it's asked, whichever platform machine asks)
//!   and starts a worker tick at once rather than at its next second. Opens
//!   on this machine at the same time share one wait ([`Wakes`]).
//! - **A box that may have paused itself.** A box pauses at its timeout
//!   before the worker sees it, so a `ready` workspace is woken too: a
//!   running box only has its timeout moved.
//! - **Why not.** A box that's gone, a provider that says no or doesn't
//!   answer, a workspace being made or one that failed earlier: each is a
//!   [`WakeCode`] and a sentence, and the page offers rebuild.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use arugula_control_wire::hosted::{WakeAnswer, WakeCode};
use tokio::sync::{Notify, watch};

use super::{Ask, worker};
use crate::Platform;

/// How long a wake is waited on before it's "unavailable". The worker goes
/// on trying after that.
pub const WAIT: Duration = Duration::from_secs(20);
/// How often the row is read while a wake is waited on, besides when this
/// machine's worker takes a step (another machine's may do it).
const POLL: Duration = Duration::from_millis(100);

/// Wakes waited on by this platform machine, and its worker's signals.
#[derive(Default)]
pub struct Wakes {
    /// By workspace: the wake being waited on, and its answer once there.
    waiting: Mutex<HashMap<String, watch::Receiver<Option<WakeAnswer>>>>,
    /// Start a worker tick now.
    kick: Notify,
    /// A worker here took a step.
    step: Notify,
}

impl Wakes {
    /// For the worker: a wake was asked, so tick now.
    pub async fn kicked(&self) {
        self.kick.notified().await
    }

    /// For the worker: it took a step, so whoever waits looks again.
    pub fn stepped(&self) {
        self.step.notify_waiters();
    }
}

fn failed(code: WakeCode, why: &str) -> WakeAnswer {
    WakeAnswer::Failed { code, why: why.into() }
}

/// Wake `workspace` (in `project`): once its box runs, or why it didn't.
/// Opens at the same time share one wait, and one resume.
pub async fn wake(app: &Platform, project: &str, workspace: &str) -> anyhow::Result<WakeAnswer> {
    loop {
        let (tx, waiting) = {
            let mut all = app.wakes.waiting.lock().unwrap();
            match all.get(workspace) {
                Some(rx) => (None, Some(rx.clone())),
                None => {
                    let (tx, rx) = watch::channel(None);
                    all.insert(workspace.to_owned(), rx);
                    (Some(tx), None)
                }
            }
        };
        if let Some(mut rx) = waiting {
            // Another open's wait: its answer, or (it ended without one)
            // a wait of this one's own.
            while rx.borrow().is_none() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
            if let Some(a) = rx.borrow().clone() {
                return Ok(a);
            }
            continue;
        }
        let tx = tx.expect("this open waits");
        // However it ends (an answer, an error, the hub hanging up), the
        // next open starts a wait of its own.
        struct Done<'a>(&'a Wakes, &'a str);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.waiting.lock().unwrap().remove(self.1);
            }
        }
        let _done = Done(&app.wakes, workspace);
        let a = wait(app, project, workspace).await?;
        tracing::info!(%workspace, answer = ?a, "a hosted workspace's wake answered");
        let _ = tx.send(Some(a.clone()));
        return Ok(a);
    }
}

/// Ask for the wake and wait for the worker to do it, reading the row.
async fn wait(app: &Platform, project: &str, workspace: &str) -> anyhow::Result<WakeAnswer> {
    let until = tokio::time::Instant::now() + WAIT;
    // Whether a wake was asked since this began (by this or another open).
    let mut asked = false;
    loop {
        let stepped = app.wakes.step.notified();
        tokio::pin!(stepped);
        stepped.as_mut().enable();
        let Some(w) = app.db.hosted_workspace(project, workspace)? else {
            return Ok(failed(WakeCode::Gone, "This workspace was deleted."));
        };
        let error = w.error.as_deref();
        match (w.state.as_str(), w.asked.as_deref()) {
            ("deleting" | "deleted", _) | (_, Some("delete")) => {
                return Ok(failed(WakeCode::Gone, "This workspace is being deleted."));
            }
            ("creating" | "enrolling", _) | (_, Some("rebuild")) => {
                return Ok(failed(WakeCode::Starting, "This workspace's box is being made. It opens when it's ready."));
            }
            ("failed", None) => {
                let why = error.unwrap_or("This workspace failed. Rebuild it to make its box again.");
                let code = if why == worker::GONE { WakeCode::Gone } else { WakeCode::Failed };
                return Ok(failed(code, why));
            }
            (_, Some("wake")) => {
                asked = true;
                if error == Some(worker::UNAVAILABLE) {
                    return Ok(failed(WakeCode::Unavailable, worker::UNAVAILABLE));
                }
            }
            ("ready" | "paused", None) if asked => {
                return Ok(match (w.state.as_str(), error) {
                    ("ready", None) => WakeAnswer::Awake,
                    (_, Some(e)) => failed(WakeCode::Refused, e),
                    // Paused again with nothing said: as the provider not
                    // answering, rather than a wake asked twice.
                    _ => failed(WakeCode::Unavailable, worker::UNAVAILABLE),
                });
            }
            ("ready" | "paused", None) if super::ask(app, &w.id, Ask::Wake)? => {
                asked = true;
                app.wakes.kick.notify_one();
                continue;
            }
            // A pause under way, or something new: wait for it.
            _ => {}
        }
        if tokio::time::Instant::now() >= until {
            return Ok(failed(
                WakeCode::Unavailable,
                "The box didn't wake in time. Arugula keeps trying: open it again in a moment.",
            ));
        }
        tokio::select! {
            _ = stepped => {}
            _ = tokio::time::sleep(POLL) => {}
        }
    }
}
