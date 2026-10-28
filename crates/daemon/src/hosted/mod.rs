//! `arugulad --hosted` (M180, #1241): the daemon in a hosted workspace's
//! box (an E2B sandbox, M176/M177), with nobody at it.
//!
//! The box itself (enrolling, its setup, snapshots, resumes, saying busy
//! or idle) is the hosted crate's (`hosted_box`, #232), which registers
//! [`Hosted`]; with none registered, `--hosted` only settles the flags and
//! takes runs. What the rest of the daemon runs on stays here:
//!
//! - **Headless** ([`headless`]): no tailnet, no update checks (the
//!   template pins the release), no systemd user manager, no Claude Code
//!   IDE relay, no guests' ssh; panes are kept across a daemon restart by
//!   their shims.
//! - **What it keeps of control's enrollment, checked** ([`trust`]), at
//!   enrollment and at each refresh (`control.rs`).
//! - **Takes runs** from the platform under the project's standing grant
//!   ([`runs`]) and runs each at most once (`crate::work`, M212).
//! - **Archives closed agent blocks' transcripts** to its project
//!   ([`archive`], M184).
//!
//! It holds nothing else on the platform's behalf.

pub mod archive;
pub mod runs;
pub mod trust;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use futures_util::future::BoxFuture;

use crate::{args::RunArgs, server::App};

/// The hosted box, as the daemon calls it.
pub trait Hosted: Send + Sync + 'static {
    /// Before anything reads the state directory: a box from a workspace's
    /// snapshot (M263) keeps nothing of the builder's daemon.
    fn first_boot(&self, state_dir: &Path);
    /// What a hosted daemon runs beside the usual and its runs: the resume
    /// watch, busy and idle, its box's setup once it's in control, and,
    /// until then, waiting for its box and enrolling.
    fn start(&self, app: &Arc<App>, state_dir: PathBuf, control_url: String);
    /// The repos its setup cloned: each one's URL and directory.
    fn clones(&self, state_dir: &Path) -> Vec<(String, PathBuf)>;
    /// Ends `/etc/resolv.conf` with a newline (Arugula #263), on a box with
    /// sessions.
    fn fix_resolv(&self) -> BoxFuture<'static, ()>;
}

static HOSTED: OnceLock<Box<dyn Hosted>> = OnceLock::new();

/// Puts the hosted box in this daemon (once, before `main`).
pub fn install(h: Box<dyn Hosted>) {
    if HOSTED.set(h).is_err() {
        tracing::warn!("the hosted box was registered twice");
    }
}

fn hosted() -> Option<&'static dyn Hosted> {
    HOSTED.get().map(|h| h.as_ref())
}

/// Settle a hosted daemon's flags: nothing on the machine to integrate
/// with, and nobody to ask.
pub(crate) fn headless(args: &mut RunArgs) {
    args.no_update_check = true;
    args.no_manager_env = true;
    args.no_claude_ide = true;
    args.keep_panes = true;
    args.guest_ssh = "off".into();
}

/// Before anything reads the state directory ([`Hosted::first_boot`]).
pub fn first_boot(state_dir: &Path) {
    if let Some(h) = hosted() {
        h.first_boot(state_dir);
    }
}

/// What a hosted daemon runs beside the usual: its runs (M212, once it's in
/// control), what a session's agent has of Arugula's MCP (its project's
/// issues), closed agent blocks' archive, and the box's own
/// ([`Hosted::start`]).
pub fn start(app: &Arc<App>, state_dir: PathBuf, control_url: String) {
    crate::work::start(app.clone());
    crate::mcp::session::start(app.clone());
    archive::start(app.control.clone());
    match hosted() {
        Some(h) => h.start(app, state_dir, control_url),
        None => tracing::warn!("hosted: this build has no hosted box: it won't enroll or set itself up"),
    }
}

/// The repos this box's setup cloned: each one's URL and directory. None
/// without the hosted box.
pub fn clones(state_dir: &Path) -> Vec<(String, PathBuf)> {
    hosted().map(|h| h.clones(state_dir)).unwrap_or_default()
}

/// Ends `/etc/resolv.conf` with a newline (Arugula #263) where it's missing.
pub async fn fix_resolv() {
    if let Some(h) = hosted() {
        h.fix_resolv().await;
    }
}

/// A fresh directory for a test.
#[cfg(test)]
pub(crate) fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("arugula-hosted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
