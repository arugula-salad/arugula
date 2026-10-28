//! M11: views for checking an agent's work, from anywhere and especially
//! the phone. Both are read-only blocks on a host (this one, or the VM a
//! tab or pane runs on), and both are the crate `arugulad-review`'s (#232):
//!
//! - a **diff block** (`diff.rs`): what changed in a git repository, as a
//!   file list with +/− first, each file opening to its unified hunks;
//! - a **file block** (`file.rs`): one file, followed live, at a line.
//!
//! This module is what they share, which the forge and Labs use too.
//!
//! A diff block opened from a chant workspace member's card also says which
//! decision and run made each hunk (#619), through Labs' chant workspaces.
//!
//! A view of a Managoat conversation's sandbox (`fountain` in its config,
//! #942's F3) went with daemon-side Managoat: a new one is refused, and a
//! saved one is dropped when the daemon restores it ([`SANDBOX_GONE`]).
//!
//! What they share, beyond the block contract:
//!
//! - **Live only while drawn.** They poll their host (a file's size and
//!   time, the repository's diff) only while some client draws them
//!   ([`crate::block::Block::drawn`]), because polling a VM keeps it awake.
//!   A block nobody looks at holds what it last read, and reads again when
//!   someone does.
//! - **What's drawn is in the state.** A shared session's viewers can't
//!   call methods (they're the editor role), so the state carries the file
//!   list, the open files' hunks and the file's text, capped; methods only
//!   change what's drawn.
//! - **The log is only what they were pointed at.** The file and the repo
//!   are the source of truth.
//! - **`git` runs on the host:** `sh -c` here, through
//!   [`Provider::run`](crate::provider::Provider::run) on a machine, with
//!   `GIT_OPTIONAL_LOCKS=0` so a poll never takes a lock an agent's own
//!   `git` wants.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::Value;

use crate::{block::BlockCtx, provider::Provider};

/// Why a view of a Managoat sandbox is refused.
pub const SANDBOX_GONE: &str =
    "a view of a Managoat sandbox went with daemon-side Managoat (cloud agents run through your Arugula account)";

/// How long one run of a host's command may take.
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a view's commands run: this host, or a machine through its
/// provider.
#[derive(Clone)]
pub enum Runner {
    Local {
        env: Vec<(String, String)>,
        home: PathBuf,
    },
    /// `env`: set before `sh` there (the user's shell environment, #74).
    Machine {
        provider: Arc<dyn Provider>,
        sprite: String,
        env: Vec<(String, String)>,
    },
    /// On this host as a hosted session's user (M255): the daemon's git in
    /// a session's checkout runs as the session, never as `user` (who can't
    /// get in) or root (whose git would run the agent's repo config).
    As(crate::agent::hosted::AsUser),
}

impl Runner {
    pub fn of(ctx: &BlockCtx) -> Result<Self, String> {
        match (&ctx.sprite, &ctx.provider) {
            (None, _) => Ok(Runner::Local { env: ctx.env.clone(), home: ctx.home.clone() }),
            (Some(sprite), Some(p)) => Ok(Runner::Machine { provider: p.clone(), sprite: sprite.clone(), env: vec![] }),
            (Some(_), None) => Err("this block's machine can't be reached: VM panes aren't set up".into()),
        }
    }

    /// As [`Runner::of`], with the user's shell environment over the
    /// daemon's (#74): for blocks that run the user's tools (node from
    /// mise or nvm, pyenv), which a pane would find. Waits for it the
    /// first time; without it, the daemon's.
    pub async fn user(ctx: &BlockCtx) -> Result<Self, String> {
        Ok(match Runner::of(ctx)? {
            Runner::Local { env, home } => {
                let shell = ctx.shell_env.local().await;
                Runner::Local { env: crate::shellenv::merge(&env, &shell, ctx.launch.exe.parent()), home }
            }
            Runner::Machine { provider, sprite, .. } => {
                let env = ctx.shell_env.machine(&provider, &sprite).await.vars.clone();
                Runner::Machine { provider, sprite, env }
            }
            r @ Runner::As(_) => r,
        })
    }

    pub fn local(&self) -> bool {
        matches!(self, Runner::Local { .. } | Runner::As(_))
    }

    /// `sh -c SCRIPT sh ARGS…` on the host: its stdout and exit code
    /// (stderr is dropped; a script says what went wrong on stdout).
    pub async fn sh(&self, script: &str, args: &[String]) -> Result<(Vec<u8>, Option<i32>), String> {
        let run = async {
            match self {
                Runner::Local { env, home } => {
                    let out = tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg(script)
                        .arg("sh")
                        .args(args)
                        .envs(env.iter().map(|(k, v)| (k, v)))
                        .env("GIT_OPTIONAL_LOCKS", "0")
                        .current_dir(home)
                        .stdin(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .output()
                        .await
                        .map_err(|e| format!("can't run sh: {e}"))?;
                    Ok((out.stdout, out.status.code()))
                }
                Runner::As(u) => {
                    let mut argv = vec!["sh".to_owned(), "-c".into(), script.into(), "sh".into()];
                    argv.extend(args.iter().cloned());
                    let out = u
                        .command(&[("GIT_OPTIONAL_LOCKS".into(), "0".into())], &argv)
                        .stdin(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .output()
                        .await
                        .map_err(|e| format!("can't run sh as {}: {e}", u.user))?;
                    Ok((out.stdout, out.status.code()))
                }
                Runner::Machine { provider, sprite, env } => {
                    let env: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    let mut argv = vec!["env"];
                    argv.extend(env.iter().map(String::as_str));
                    argv.extend(["GIT_OPTIONAL_LOCKS=0", "sh", "-c", script, "sh"]);
                    argv.extend(args.iter().map(String::as_str));
                    provider.run(sprite, &argv).await.map_err(|e| format!("{e:#}"))
                }
            }
        };
        tokio::time::timeout(RUN_TIMEOUT, run).await.map_err(|_| "the host took too long to answer".to_owned())?
    }
}

/// Polling only while drawn (S8's gap 4): each time a client starts to draw
/// the block a new round begins; it ends when none does, or the block
/// closes.
#[derive(Default)]
pub struct Live {
    drawn: AtomicBool,
    round: AtomicU64,
    closed: AtomicBool,
}

impl Live {
    /// Drawn or not. A new round's number when one should start.
    pub fn set(&self, on: bool) -> Option<u64> {
        let was = self.drawn.swap(on, Ordering::SeqCst);
        let round = self.round.fetch_add(1, Ordering::SeqCst) + 1;
        (on && !was && !self.closed.load(Ordering::SeqCst)).then_some(round)
    }

    /// Whether round `round` should go on.
    pub fn on(&self, round: u64) -> bool {
        !self.closed.load(Ordering::SeqCst)
            && self.drawn.load(Ordering::SeqCst)
            && self.round.load(Ordering::SeqCst) == round
    }

    pub fn drawn(&self) -> bool {
        self.drawn.load(Ordering::SeqCst)
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.round.fetch_add(1, Ordering::SeqCst);
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// How often a view polls its host while drawn.
pub fn every(local: bool, here: Duration) -> Duration {
    if local { here } else { Duration::from_secs(3) }
}

/// One line for a block's log.
pub fn log(ctx: &BlockCtx, event: &Value) {
    if let Ok(mut log) = ctx.log() {
        let mut line = event.to_string().into_bytes();
        line.push(b'\n');
        let _ = log.append(&line);
    }
}

/// The repository a local directory is in, for summaries (a machine's
/// isn't looked at: that would wake it).
pub fn project(dir: &str, local: bool) -> Option<arugula_proto::Project> {
    local.then(|| crate::classify::project(dir)).flatten()
}
