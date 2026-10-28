//! A hosted workspace's box (M180, #1241): `arugulad --hosted` in an E2B
//! sandbox (M176/M177), with nobody at it.
//!
//! This is the crate `arugulad-hosted` (#232): the binary calls
//! [`register`], which installs the base's `hosted::Hosted`, and the daemon
//! (`arugulad-base`) reaches it only through that, `features` and
//! `subcommands`. The base keeps what its runs and agent blocks need
//! (`hosted`: the flags, the runs, the archive, the enrollment's check).
//!
//! - **A box of its own.** The template snapshots this daemon already
//!   running (M177), so every box made from it starts with the same memory
//!   and state directory. When a new box's sandbox id shows up
//!   (`/run/e2b/.E2B_SANDBOX_ID`, which envd writes at `/init`), the daemon
//!   removes everything it made that must differ per box ([`PER_BOX`]: its
//!   keys, its tokens and what its setup did), notes the id in
//!   `<state>/hosted-box`, and starts over in place (`exec`), making them
//!   anew ([`fresh_box`]). A start whose noted id is the box's own is that
//!   box's: it enrolls.
//! - **Enrolls itself** ([`enroll`]) with a ticket in its environment
//!   (`ARUGULA_HOSTED_TICKET`, when it starts after the box is made: loam,
//!   testnet) or else its sandbox id, at `ARUGULA_CONTROL_URL` (or
//!   `--control`), and then dials the relay like any machine.
//! - **Redials at once after a resume** ([`resume`]).
//! - **Ends `/etc/resolv.conf` with a newline** ([`resolv`], Arugula #263),
//!   at its start and after each resume, as envd leaves none and Zig
//!   can't read it then.
//! - **Says busy or idle** on its relay socket ([`activity`]).
//! - **Sets its box up** on its first boot, in a pane anyone on the project
//!   can watch ([`setup`], M183), and says how that went ([`ops`]).
//! - **Snapshots** (M263, [`snapshot`]): a builder (enrolled for a
//!   workspace's snapshot) wipes itself after its setup; a box from the
//!   snapshot clears what's left of a daemon's state at its first start,
//!   takes its sandbox id as its own at once, and is set up already.
//! - **Is supervised** by the template (M258, Arugula #124): `arugula-hosted-start`
//!   or `arugulad.service` (packaging/hosted) starts it again when it exits
//!   and gives it an `oom_score_adj` of -1000, which it logs at start
//!   ([`oom_protected`]). Its panes and agents give that up (their shim's
//!   `unprotect_oom`). The platform marks a box whose daemon doesn't dial
//!   in after a wake as needing a rebuild; nothing reaches in to restart it.
//! - **Its own programs**, as root: `arugulad _reseed` ([`reseed`]) and
//!   `arugulad _seal` ([`snapshot`]'s wipe), through `subcommands`.

pub mod activity;
pub mod enroll;
pub mod ops;
pub mod reseed;
pub mod resolv;
pub mod resume;
pub mod setup;
pub mod snapshot;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::op::ops::HostedSetupGet;
use axum::{Router, routing::get};
use futures_util::future::BoxFuture;
use tracing::{info, warn};

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::{mux, pane, store};
use arugulad_base::{api, control, features, hosted, server, subcommands, task, work};

use crate::server::App;

/// The one-time project ticket, for a daemon started after its box was made.
pub const TICKET_ENV: &str = "ARUGULA_HOSTED_TICKET";
/// Control's URL.
pub const CONTROL_ENV: &str = "ARUGULA_CONTROL_URL";
/// The sandbox id this state directory's keys were made for.
const BOX_FILE: &str = "hosted-box";

/// What a daemon makes for itself in its state directory and must not
/// share with another box: its key in control, its id, the local token,
/// web push's key, MCP's key, the IDE relay's (off here, but an older
/// template may have them), and what its setup did.
pub const PER_BOX: &[&str] = &[
    "daemon.key",
    "daemon-id",
    "local-token",
    "push/vapid.key",
    "mcp/key",
    "ide/token",
    "ide/port",
    setup::STATE_FILE,
];

/// Whose the hosted box's operations are (`impl Handle<HostedBox> for
/// HostedSetupGet`), and what registers `hosted::Hosted`.
pub struct HostedBox;

/// Puts the hosted box in the daemon. Call it before `arugulad_base::main`.
pub fn register() {
    crate::hosted::install(Box::new(HostedBox));
    crate::features::register(Box::new(HostedBox));
    // A hosted box's reseed (M180), as root.
    crate::subcommands::register("_reseed", reseed_cli);
    // A snapshot builder's wipe (M263), as root, what to look for on its
    // stdin.
    crate::subcommands::register("_seal", seal_cli);
}

fn reseed_cli(argv: &[String]) -> ! {
    reseed::cli(argv.first().map_or("", String::as_str))
}

fn seal_cli(_: &[String]) -> ! {
    snapshot::cli()
}

impl crate::features::Feature for HostedBox {
    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        use arugulad_base::ops::OpRoutes;
        r.op_of::<HostedSetupGet, HostedBox>().route("/api/hosted/setup/archive", get(ops::setup_archive))
    }
}

impl crate::hosted::Hosted for HostedBox {
    fn first_boot(&self, state_dir: &Path) {
        snapshot::first_boot(state_dir, &resume::sandbox_id_file(), &snapshot::dir());
    }

    fn start(&self, app: &Arc<App>, state_dir: PathBuf, control_url: String) {
        start(app, state_dir, control_url);
    }

    fn clones(&self, state_dir: &Path) -> Vec<(String, PathBuf)> {
        setup::Saved::load(state_dir).repos.into_iter().map(|c| (c.url, c.dir)).collect()
    }

    fn fix_resolv(&self) -> BoxFuture<'static, ()> {
        Box::pin(resolv::fix())
    }
}

/// The sandbox id this state directory's keys were made for, if noted.
pub fn noted_box(state_dir: &Path) -> Option<String> {
    std::fs::read_to_string(state_dir.join(BOX_FILE)).ok().map(|s| s.trim().to_owned())
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

/// What the box runs beside the daemon's (`hosted::start`): the resume
/// watch, busy and idle, its setup once it's in control, and, until then,
/// waiting for its box and enrolling.
pub fn start(app: &Arc<App>, state_dir: PathBuf, control_url: String) {
    match oom_protected() {
        Some(true) => info!("hosted: protected from the OOM killer (oom_score_adj -1000)"),
        Some(false) => warn!("hosted: not protected from the OOM killer: is the template's supervisor running it?"),
        None => {}
    }
    let control = app.control.clone();
    let c = control.clone();
    resume::watch(Arc::new(move || {
        c.resumed();
        resolv::after_resume();
    }));
    // Arugula #263: the newline Zig needs at the end of /etc/resolv.conf.
    crate::task::spawn(resolv::fix());
    activity::start(app.mux.clone(), control.activity.clone());
    setup::start(app.clone(), state_dir.clone());
    if control.enrolled().is_some() {
        return;
    }
    let id_file = resume::sandbox_id_file();
    let ticket = std::env::var(TICKET_ENV).ok().filter(|t| !t.is_empty());
    let url = std::env::var(CONTROL_ENV).ok().filter(|u| !u.is_empty()).unwrap_or(control_url);
    let url = url.trim_end_matches('/').to_owned();
    crate::task::spawn(async move {
        // In the snapshot (or the template's build): wait for a box of our
        // own, then start over in it.
        if ticket.is_none() && own_box(&state_dir, &id_file) == Some(false) {
            let of = snapshot::snapshot_of(&snapshot::dir());
            let id = enroll::new_box(&id_file, of.as_deref()).await;
            let Err(e) = fresh_box(&state_dir, &id);
            warn!(error = %format!("{e:#}"), "hosted: can't start over in the new box");
            return;
        }
        let c = control.clone();
        enroll::run(url, ticket, state_dir, id_file, move || c.poke()).await;
    });
}

/// Whether this daemon's `oom_score_adj` is the supervisor's -1000:
/// `None` where it can't be read.
pub fn oom_protected() -> Option<bool> {
    let score = std::fs::read_to_string("/proc/self/oom_score_adj").ok()?;
    Some(score.trim().parse::<i32>().ok()? <= -1000)
}

/// A fresh directory for a test.
#[cfg(test)]
pub(crate) fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("arugula-hosted-box-{name}-{}", std::process::id()));
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
