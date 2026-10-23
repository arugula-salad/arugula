//! Pausing only between turns (M181, #1242; "Hosted machines pause only
//! between turns", aru-094).
//!
//! A box pauses itself (memory and disk kept) when its timeout passes:
//! [`super::Config::idle_timeout`] after it was made, woken or last held.
//! The platform's leader looks every [`TICK`] at what each running box's
//! daemon last said on its relay socket (M180, `HubPort::activity`):
//!
//! - **Busy:** the box is held ([`Substrate::hold`]): its timeout is set to
//!   the idle timeout from now, at once when it turns busy and then every
//!   third of the idle timeout, so a hold or two can fail without it
//!   lapsing.
//! - **Idle, or silent** (off the relay, or the hub didn't answer): it isn't
//!   held, so it pauses itself within the idle timeout of its last hold.
//!   Nothing holds a box forever.
//!
//! The platform never pauses a box whose daemon says busy: the worker's
//! pauses check first ([`super::worker`]). What's left is a race at the
//! timeout: a turn that starts in the last few seconds before it may find
//! its box pausing. Its daemon said busy at once, so the next tick's hold
//! finds the box just paused, and wakes it ([`woke_mid_turn`]): the turn
//! goes on, maybe with its network connections dropped.
//!
//! **The provider's cap on continuous running** ([`E2B_PRO_RUNTIME`]): a
//! hold never moves the timeout past it, so at the cap the box pauses
//! whatever was said. In the last stretch before it ([`cycle_window`]) the
//! worker pauses and resumes the box at the next idle moment, which
//! restarts the clock; a single turn that runs to the cap is woken at once
//! and said on the workspace (`error`), in the platform's log, and in a push
//! to the workspace's maker, again each cap it runs on ([`super::worker`]).

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::Ordering::SeqCst},
    time::Duration,
};

use anyhow::Result;
use tracing::{info, warn};

use crate::{
    Platform,
    compute::{self, State, Substrate},
};

/// E2B Pro's limit on how long a box runs without a pause: "A sandbox can
/// remain running (without being paused) for: 24 hours on the Pro tier, 1
/// hour on the Hobby tier. After a sandbox is paused and resumed, the
/// continuous runtime limit is reset"
/// (<https://e2b.dev/docs/sandbox/persistence>, checked 2026-10-21). A
/// timeout past it is cut to it: the box pauses at its start plus the cap
/// (seen on the API, 2026-10-21). `ARUGULA_HOSTED_RUNTIME_CAP` sets another.
pub const E2B_PRO_RUNTIME: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the leader looks at what running boxes' daemons said.
pub const TICK: Duration = Duration::from_secs(5);

/// A running box the leader may hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub workspace: String,
    /// The hosted machine its box enrolled as.
    pub machine: String,
    pub sandbox: String,
}

/// How often a busy box is held: a third of the idle timeout, so a hold
/// that fails is tried twice more before the box would pause.
pub fn every(idle_timeout: Duration) -> u64 {
    (idle_timeout.as_millis() as u64 / 3).max(TICK.as_millis() as u64)
}

/// The last stretch before the cap in which the worker pauses and resumes
/// a box at its next idle moment: a twelfth of it (two hours on Pro, five
/// minutes on Hobby).
pub fn cycle_window(cap: Duration) -> u64 {
    cap.as_millis() as u64 / 12
}

/// Whether a box that ran from `started` to `paused_at` (ms) paused at
/// the provider's cap: within a minute of it, for clocks and rounding.
pub fn ran_to_cap(started: u64, paused_at: u64, cap: Duration) -> bool {
    paused_at.saturating_sub(started) + 60_000 >= cap.as_millis() as u64
}

/// When each busy box was last held, on this machine. A new leader starts
/// with none, so it holds every busy box on its first tick.
#[derive(Debug, Default)]
pub struct Holds(HashMap<String, u64>);

impl Holds {
    /// Which of `boxes` to hold at `now`: each whose machine is in `busy`
    /// and that wasn't held in the last `every` ms. The others are
    /// forgotten, so a box that turns busy again is held at once.
    pub fn due(&mut self, boxes: &[Running], busy: &HashSet<String>, every: u64, now: u64) -> Vec<Running> {
        let working: HashSet<&str> =
            boxes.iter().filter(|b| busy.contains(&b.machine)).map(|b| b.workspace.as_str()).collect();
        self.0.retain(|w, _| working.contains(w.as_str()));
        boxes
            .iter()
            .filter(|b| working.contains(b.workspace.as_str()))
            .filter(|b| self.0.get(&b.workspace).is_none_or(|at| now >= at + every))
            .cloned()
            .collect()
    }

    /// The box was held (or woken) at `now`.
    pub fn held(&mut self, workspace: &str, now: u64) {
        self.0.insert(workspace.to_owned(), now);
    }
}

/// One look: hold each busy box that's due. How many were held.
pub async fn tick_at(app: &Platform, holds: &mut Holds, now: u64) -> Result<usize> {
    let Some(sub) = app.compute.clone() else { return Ok(0) };
    let boxes = app.db.hosted_running()?;
    if boxes.is_empty() {
        holds.0.clear();
        return Ok(0);
    }
    let machines: Vec<String> = boxes.iter().map(|b| b.machine.clone()).collect();
    let busy: HashSet<String> = match app.hub.activity(&machines).await {
        Ok(said) => said.into_iter().filter(|a| a.busy).map(|a| a.machine).collect(),
        // Silent counts as idle: nothing is held while the hub can't say.
        Err(error) => {
            warn!(%error, "the hub didn't say which hosted machines are working: holding none");
            HashSet::new()
        }
    };
    let timeout = app.hosted.idle_timeout();
    let mut n = 0;
    for b in holds.due(&boxes, &busy, every(timeout), now) {
        match sub.hold(&b.sandbox, timeout).await {
            Ok(()) => {
                holds.held(&b.workspace, now);
                n += 1;
            }
            Err(compute::Error::NotFound) => {
                if woke_mid_turn(&*sub, &b, timeout, app.hosted.runtime_cap(), now).await {
                    holds.held(&b.workspace, now);
                }
            }
            Err(error) => warn!(workspace = %b.workspace, %error, "holding a busy hosted box failed: tried again"),
        }
    }
    Ok(n)
}

/// How recently a box found paused under a busy daemon has to have paused
/// to be woken: a box that paused a while ago did so between turns, and
/// what the hub has of it is stale.
const JUST_PAUSED_MS: u64 = 60_000;

/// A hold found the box paused although its daemon says busy: the race at
/// its timeout. If it paused just now, wake it, so the turn goes on.
/// Whether it was woken. A box that's gone is the worker's to find, and so
/// is one that ran to the provider's `cap`: the worker wakes it and says so.
async fn woke_mid_turn(sub: &dyn Substrate, b: &Running, timeout: Duration, cap: Duration, now: u64) -> bool {
    let Ok(Some(s)) = sub.status(&b.sandbox).await else { return false };
    let ms = |t: &str| arugula_forge_github::util::time(t).and_then(|t| u64::try_from(t).ok());
    let (Some(started), Some(paused_at)) = (ms(&s.started_at), ms(&s.end_at)) else { return false };
    if s.state != State::Paused || now > paused_at + JUST_PAUSED_MS || ran_to_cap(started, paused_at, cap) {
        return false;
    }
    match sub.resume(&b.sandbox, timeout).await {
        Ok(_) => {
            warn!(workspace = %b.workspace, "a hosted box paused as a turn started: woke it");
            true
        }
        Err(error) => {
            warn!(workspace = %b.workspace, %error, "a hosted box paused as a turn started, and didn't wake");
            false
        }
    }
}

/// The leader's holds, every [`TICK`].
pub fn start(app: Arc<Platform>) {
    tokio::spawn(async move {
        let mut holds = Holds::default();
        let mut was_leader = false;
        loop {
            tokio::time::sleep(TICK).await;
            if app.is_paused() || app.is_stopping() || !app.is_leader() {
                was_leader = false;
                holds = Holds::default();
                continue;
            }
            if !was_leader {
                info!(idle_timeout = ?app.hosted.idle_timeout(), "holding busy hosted boxes");
                was_leader = true;
            }
            app.busy.fetch_add(1, SeqCst);
            if let Err(error) = tick_at(&app, &mut holds, arugula_e2e::now_ms()).await {
                warn!(%error, "holding busy hosted boxes failed");
            }
            app.busy.fetch_sub(1, SeqCst);
        }
    });
}

#[cfg(test)]
#[path = "hold_tests.rs"]
mod tests;
