//! `arugulad --hosted` (M180, #1241): the daemon in a hosted workspace's
//! box (an E2B sandbox, M176/M177), with nobody at it.
//!
//! - **Headless** ([`headless`]): no tailnet, no update checks (the
//!   template pins the release), no systemd user manager, no Claude Code
//!   IDE relay, no guests' ssh; panes are kept across a daemon restart by
//!   their shims.
//! - **A box of its own.** The template snapshots this daemon already
//!   running (M177), so every box made from it starts with the same memory
//!   and state directory. When a new box's sandbox id shows up
//!   (`/run/e2b/.E2B_SANDBOX_ID`, which envd writes at `/init`), the daemon
//!   removes everything it made that must differ per box ([`PER_BOX`]: its
//!   keys and tokens), notes the id in `<state>/hosted-box`, and starts over
//!   in place (`exec`), making them anew ([`fresh_box`]). A start whose
//!   noted id is the box's own is that box's: it enrolls.
//! - **Enrolls itself** ([`enroll`]) with a ticket in its environment
//!   (`ARUGULA_HOSTED_TICKET`, when it starts after the box is made: loam,
//!   testnet) or else its sandbox id, at `ARUGULA_CONTROL_URL` (or
//!   `--control`), and then dials the relay like any machine.
//! - **Redials at once after a resume** ([`resume`]).
//! - **Says busy or idle** on its relay socket ([`activity`]).
//!
//! It holds nothing else on the platform's behalf.

pub mod activity;
pub mod enroll;
pub mod reseed;
pub mod resume;
pub mod runs;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tracing::{info, warn};

use crate::{args::RunArgs, server::App};

/// The one-time project ticket, for a daemon started after its box was made.
pub const TICKET_ENV: &str = "ARUGULA_HOSTED_TICKET";
/// Control's URL.
pub const CONTROL_ENV: &str = "ARUGULA_CONTROL_URL";
/// The sandbox id this state directory's keys were made for.
const BOX_FILE: &str = "hosted-box";

/// What a daemon makes for itself in its state directory and must not
/// share with another box: its key in control, its id, the local token,
/// web push's key, MCP's key, and the IDE relay's (off here, but an older
/// template may have them).
pub const PER_BOX: &[&str] =
    &["daemon.key", "daemon-id", "local-token", "push/vapid.key", "mcp/key", "ide/token", "ide/port"];

/// Settle a hosted daemon's flags: nothing on the machine to integrate
/// with, and nobody to ask.
pub fn headless(args: &mut RunArgs) {
    args.no_update_check = true;
    args.no_manager_env = true;
    args.no_claude_ide = true;
    args.keep_panes = true;
    args.guest_ssh = "off".into();
}

/// Whether this box is the one the state directory was made for: `None`
/// when it isn't an E2B box (no sandbox id), else whether the id noted is
/// the box's.
pub fn own_box(state_dir: &Path, id_file: &Path) -> Option<bool> {
    let (id, _) = resume::stamp(id_file)?;
    let noted = std::fs::read_to_string(state_dir.join(BOX_FILE)).ok();
    Some(noted.as_deref().map(str::trim) == Some(id.as_str()))
}

/// A new box from the snapshot: forget what this daemon made for the old
/// one, note the box, and start over in place, making them anew.
pub fn fresh_box(state_dir: &Path, id: &str) -> anyhow::Result<std::convert::Infallible> {
    for f in PER_BOX {
        match std::fs::remove_file(state_dir.join(f)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow::anyhow!("removing {f}: {e}")),
        }
    }
    crate::store::write_atomic(&state_dir.join(BOX_FILE), id.as_bytes())?;
    // Before anything here makes a key: the kernel's randomness, which the
    // snapshot shared too.
    reseed::fresh(id);
    info!(sandbox = id, "hosted: a new box; starting over with keys of its own");
    exec_self()
}

#[cfg(unix)]
fn exec_self() -> anyhow::Result<std::convert::Infallible> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec();
    Err(anyhow::anyhow!("can't start again: {err}"))
}

#[cfg(not(unix))]
fn exec_self() -> anyhow::Result<std::convert::Infallible> {
    anyhow::bail!("--hosted runs on Linux")
}

/// What a hosted daemon runs beside the usual: the resume watch, busy and
/// idle, and, until it's in control, waiting for its box and enrolling.
pub fn start(app: &Arc<App>, state_dir: PathBuf, control_url: String) {
    let control = app.control.clone();
    let c = control.clone();
    resume::watch(Arc::new(move || c.resumed()));
    activity::start(app.mux.clone(), control.activity.clone());
    if control.enrolled().is_some() {
        return;
    }
    let id_file = resume::sandbox_id_file();
    let ticket = std::env::var(TICKET_ENV).ok().filter(|t| !t.is_empty());
    let url = std::env::var(CONTROL_ENV).ok().filter(|u| !u.is_empty()).unwrap_or(control_url);
    let url = url.trim_end_matches('/').to_owned();
    tokio::spawn(async move {
        // In the snapshot (or the template's build): wait for a box of our
        // own, then start over in it.
        if ticket.is_none() && own_box(&state_dir, &id_file) == Some(false) {
            let id = enroll::new_box(&id_file).await;
            let Err(e) = fresh_box(&state_dir, &id);
            warn!(error = %format!("{e:#}"), "hosted: can't start over in the new box");
            return;
        }
        let c = control.clone();
        enroll::run(url, ticket, state_dir, id_file, move || c.poke()).await;
    });
}

/// A fresh directory for a test.
#[cfg(test)]
pub(crate) fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("arugula-hosted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_box_the_state_is_for() {
        let state = scratch("state");
        let id_file = scratch("run").join(".E2B_SANDBOX_ID");
        assert_eq!(own_box(&state, &id_file), None);
        // The template's build: no box noted.
        std::fs::write(&id_file, "build1").unwrap();
        assert_eq!(own_box(&state, &id_file), Some(false));
        std::fs::write(state.join(BOX_FILE), "box2").unwrap();
        assert_eq!(own_box(&state, &id_file), Some(false));
        std::fs::write(&id_file, "box2\n").unwrap();
        assert_eq!(own_box(&state, &id_file), Some(true));
    }
}
