//! A session's job (M256, the `session` kind on `platform_jobs`): it
//! follows its session while it lives, as a run's dispatch follows its run
//! ([`crate::work::Dispatch`]). The box pulls the session itself
//! (`Db::session_work`, `takes=session`); the job sees that it can.
//!
//! Each step, for a session that hasn't ended:
//! - **Its workspace gone** (deleted, or being deleted): the session
//!   stops, saying so.
//! - **Waiting for its box** (`queued`, or `done` with a prompt queued,
//!   or `unresponsive`): the box is woken through M182's `hosted::wake`
//!   when it's paused or hasn't been heard from in [`work::QUIET_MS`], at
//!   most once every [`work::WAKE_EVERY_MS`]. A box that runs a turn says
//!   it's busy, which holds it (M181).
//! - **Not taken:** a `queued` session no box took in [`work::TAKE_MS`]
//!   while its box had room fails, saying so. One waiting for room (the
//!   box holds [`MAX_SESSIONS_PER_BOX`] sessions, done ones too) waits,
//!   and its wait is counted from when room was made.
//! - **Gone quiet:** a turn under way whose box hasn't reported for
//!   [`QUIET_MS`] is marked `unresponsive` (`Db::mark_unresponsive`'s
//!   rule, for this one session).
//! - **Idle:** a `done` session with nothing queued and no prompt for
//!   [`idle_end_ms`] (a day, `ARUGULA_SESSION_IDLE_END` seconds) is
//!   `stopped`, saying so ([`IDLE`]): a done session holds a slot on its
//!   box, which this frees. `@arugula` again starts a new one.
//!
//! It looks again in [`work::WAITING_MS`] while the session waits for its
//! box, [`work::RUNNING_MS`] while a turn is under way, and [`IDLE_MS`]
//! while it's `done` with nothing queued; a prompt queued in its thread
//! makes it look at once (`Db::nudge_session`). It's done when the session
//! ends.

use std::sync::Arc;

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        Db,
        jobs::{JobKind, LeasedJob},
        session_threads::WORKSPACE_GONE,
        sessions::{MAX_SESSIONS_PER_BOX, Move, PromptState, QUIET_MS, State},
    },
    jobs::{Clock, Handler, Step},
    work,
};

/// How often a `done` session with nothing queued is looked at: a prompt
/// queued nudges it sooner.
pub const IDLE_MS: u64 = 10 * 60_000;

/// How long a `done` session may go without a prompt before it ends: a day.
pub const IDLE_END_MS: u64 = 24 * 60 * 60_000;

/// What an idle session's `stopped` says, first the code (the card knows
/// it), then the words.
pub const IDLE: &str = "idle";

/// [`IDLE_END_MS`], unless `ARUGULA_SESSION_IDLE_END` says how many
/// seconds.
pub fn idle_end_ms() -> u64 {
    std::env::var("ARUGULA_SESSION_IDLE_END")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map_or(IDLE_END_MS, |s| s.saturating_mul(1000))
}

/// The reason an idle session stops with: "idle: ended after a day idle; …".
pub fn idle_reason(ms: u64) -> String {
    let h = ms / 3_600_000;
    let long = match (h, ms / 60_000) {
        (24, _) => "a day".to_owned(),
        (h, _) if h >= 2 => format!("{h} hours"),
        (1, _) => "an hour".to_owned(),
        (_, m) if m >= 2 => format!("{m} minutes"),
        _ => format!("{} seconds", ms / 1000),
    };
    format!("{IDLE}: ended after {long} idle; @arugula again to start a new one")
}

/// What a session's job keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Checkpoint {
    /// When the job last asked for its box's wake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woke_at: Option<u64>,
    /// When the job last saw a queued session's box with no room for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_at: Option<u64>,
}

/// The `session` kind's handler.
pub struct Follow {
    pub app: Arc<Platform>,
}

/// Control's move, told to the session's card (`session_live::record`).
fn control(app: &Platform, id: &str, from: State, to: State, detail: serde_json::Value, now: u64) -> Result<bool> {
    let detail = detail.to_string();
    Ok(crate::session_live::record(app, id, &Move::Control { from, to, at: now, detail: &detail })?.is_ok())
}

impl Follow {
    async fn step(&self, db: &Db, job: &mut LeasedJob, clock: &Clock) -> Result<Step> {
        let now = clock.now();
        // `cancel` is "look again" for this kind (`Db::nudge_session`).
        if job.cancelled() {
            db.uncancel_job(job, now)?;
        }
        let Some(s) = db.session(&job.id)? else { return Ok(Step::Done) };
        if s.state.terminal() {
            return Ok(Step::Done);
        }
        let w = db
            .hosted_any(&s.project, &s.workspace)?
            .filter(|w| !matches!(w.state.as_str(), "deleting" | "deleted") && w.asked.as_deref() != Some("delete"));
        let Some(w) = w else {
            let stopped = db.stop_workspace_sessions(&s.workspace, now)?;
            crate::session_live::told(&self.app, &stopped)?;
            info!(session = s.id, reason = WORKSPACE_GONE, "a session stopped");
            return Ok(Step::Done);
        };
        // Gone quiet mid-turn.
        let mut state = s.state;
        if state.watched() && s.heard_at.unwrap_or(s.updated_at).saturating_add(QUIET_MS) <= now {
            let heard = json!({ "heard_at": s.heard_at });
            if control(&self.app, &s.id, state, State::Unresponsive, heard, now)? {
                info!(session = s.id, "a session's box went quiet");
                state = State::Unresponsive;
            }
        }
        let prompts = db.session_prompts(&s.id)?;
        let queued = prompts.iter().any(|p| p.state == PromptState::Queued);
        // Idle: done, nothing queued, and no prompt for a day. It ends, and
        // its slot on the box frees.
        let idle_end = idle_end_ms();
        let idle_since = prompts.iter().map(|p| p.created_at).max().unwrap_or(0).max(s.updated_at);
        if state == State::Done && !queued && now.saturating_sub(idle_since) >= idle_end {
            let why = json!({ "reason": idle_reason(idle_end) });
            if control(&self.app, &s.id, State::Done, State::Stopped, why, now)? {
                info!(session = s.id, "an idle session ended");
                return Ok(Step::Done);
            }
        }
        let waiting = match state {
            State::Queued | State::Unresponsive => true,
            State::Done => queued,
            _ => false,
        };
        let busy = db
            .session_work(&s.workspace)?
            .iter()
            .filter(|x| x.session != s.id && x.state != State::Queued.as_str())
            .count();
        let mut cp = job
            .checkpoint
            .as_ref()
            .and_then(|c| serde_json::from_value::<Checkpoint>(c.clone()).ok())
            .unwrap_or_default();
        // Not taken while its box had room: counted from its making, or
        // from when the job last saw the box full.
        if state == State::Queued {
            if busy >= MAX_SESSIONS_PER_BOX {
                cp.full_at = Some(now);
                job.checkpoint = Some(serde_json::to_value(&cp)?);
                db.checkpoint_job(job, now)?;
            } else if now.saturating_sub(cp.full_at.unwrap_or(0).max(s.updated_at)) > work::TAKE_MS {
                let why = json!({ "reason": "not_taken: its hosted workspace didn't take it in 15 minutes" });
                if control(&self.app, &s.id, State::Queued, State::Failed, why, now)? {
                    info!(session = s.id, "a session no box took failed");
                }
                return Ok(Step::Done);
            }
        }
        let quiet = w.state == "paused" || s.heard_at.is_none_or(|h| now.saturating_sub(h) > work::QUIET_MS);
        if waiting && quiet && cp.woke_at.is_none_or(|t| now.saturating_sub(t) >= work::WAKE_EVERY_MS) {
            cp.woke_at = Some(now);
            job.checkpoint = Some(serde_json::to_value(&cp)?);
            db.checkpoint_job(job, now)?;
            let (app, project, ws, id) = (self.app.clone(), s.project.clone(), w.id.clone(), s.id.clone());
            tokio::spawn(async move {
                match crate::hosted::wake::wake(&app, &project, &ws).await {
                    Ok(a) => {
                        info!(session = id, workspace = ws, answer = ?a, "woke a hosted workspace for its session")
                    }
                    Err(e) => {
                        warn!(session = id, workspace = ws, error = %format!("{e:#}"), "can't wake a session's workspace")
                    }
                }
            });
        }
        let next = match state {
            _ if waiting => work::WAITING_MS,
            State::Done => IDLE_MS.min(idle_since.saturating_add(idle_end).saturating_sub(now).max(1)),
            _ => work::RUNNING_MS,
        };
        Ok(Step::Again(now + next))
    }
}

impl Handler for Follow {
    fn kind(&self) -> JobKind {
        JobKind::Session
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(Follow::step(self, db, job, clock))
    }
}
