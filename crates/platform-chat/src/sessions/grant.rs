//! Agent sessions under the project's standing grant ("One standing grant
//! per project starts runs", aru-099, and "A thread's agent session runs on
//! the project's hosted box", aru-115, both as amended 2026-11-04). The
//! grant's own rules are [`crate::run_grant`]; a box checks it for itself
//! (`crates/arugulad-work/src/sessions.rs`). What's here is what the platform
//! does about it:
//!
//! - **Nothing starts with no grant** ([`barred`]): `@arugula` and its
//!   which-repo answer ([`crate::chat::sessions`]), `session_start`, Agent on
//!   this, Try 3, Move to… and `@codex review` each say why, and how an owner
//!   gives one.
//! - **A session going takes no follow-up** without one, or once it was
//!   revoked ([`barred_for`]), and isn't otherwise touched: its turn finishes
//!   and it stays, for a grant given later. Its thread is told once, and what
//!   was queued is dropped ([`crate::sessions::job`]). Only a queued one
//!   fails.
//! - **A revoke ends what's live** ([`revoked`], from `POST …/runs/grant/revoke`):
//!   `running stop` every session at once; `running finish` every one but a
//!   turn under way, whose job ends it when the turn does ([`ended_by_revoke`]).
//!   A grant signed afterwards doesn't bring those back.

use std::collections::BTreeSet;

use anyhow::Result;
use arugula_control_common::{ApiError, refusal};
use arugula_e2e::project_grant::Running;
use axum::http::StatusCode;
use serde_json::json;
use tracing::info;

use crate::{
    Platform,
    chat::sessions::say,
    db::{
        Db,
        sessions::{AgentSession, Move, PromptState, State},
    },
    run_grant::{self, NO_GRANT, how_to_grant, no_grant_because},
};

/// What a session a revoke ended says it ended for: as a run's.
pub const REVOKED: &str = "stopped: the project's standing grant was revoked";

/// [`run_grant::sessions_barred`]: the line, when agent sessions can't run
/// on `project` now.
pub async fn barred(app: &Platform, project: &str) -> Result<Option<String>> {
    run_grant::sessions_barred(app, project).await
}

/// Refused with the line, when agent sessions can't run on `project` now, for
/// the routes: `409`, [`NO_GRANT`].
pub async fn must_run(app: &Platform, project: &str) -> Result<(), ApiError> {
    match barred(app, project).await? {
        Some(line) => Err(refusal(StatusCode::CONFLICT, NO_GRANT, &line)),
        None => Ok(()),
    }
}

/// Whether the grant in force was revoked since session `s` was made: it was
/// live then, so nothing signed after brings it back. Revoking a grant another
/// had replaced takes nothing from it (`Db::run_grant_revoked_since`).
pub fn ended_by_revoke(db: &Db, s: &AgentSession) -> Result<bool> {
    db.run_grant_revoked_since(&s.project, s.created_at)
}

/// [`barred`], of a session going: the line, when it can't take a follow-up
/// now.
pub async fn barred_for(app: &Platform, s: &AgentSession) -> Result<Option<String>> {
    if ended_by_revoke(&app.db, s)? {
        return Ok(Some(ended_line(app)));
    }
    barred(app, &s.project).await
}

/// What a live session's thread is told when the project has no grant, once.
pub fn live_line(app: &Platform, s: &AgentSession, now: u64) -> Result<String> {
    let because = no_grant_because(&app.db, &s.project, now)?;
    Ok(format!(
        "Agent sessions run under this project's standing grant, and {because}. A turn under way finishes, and this \
         session takes no follow-ups until there's one. {}.",
        how_to_grant(app)
    ))
}

/// What the thread of a session a revoke ended is told.
fn ended_line(app: &Platform) -> String {
    format!(
        "An owner revoked this project's standing grant, so this agent session ended. {}, then ask again.",
        how_to_grant(app)
    )
}

/// End session `s` for a revoke, as control, saying so in its thread: whether
/// it moved. It may have moved already; then what it says stands.
pub fn end(app: &Platform, s: &AgentSession, now: u64) -> Result<bool> {
    let detail = json!({ "reason": REVOKED }).to_string();
    let m = Move::Control { from: s.state, to: State::Stopped, at: now, detail: &detail };
    let moved = crate::session_live::record(app, &s.id, &m)?.is_ok();
    if moved {
        info!(session = s.id, "a session ended: its standing grant was revoked");
    }
    Ok(moved)
}

/// Fail a queued session that has no grant to run under, saying how: whether
/// it moved.
pub fn fail_queued(app: &Platform, s: &AgentSession, now: u64) -> Result<bool> {
    let because = no_grant_because(&app.db, &s.project, now)?;
    let detail = json!({ "reason": format!("{NO_GRANT}: this project's standing grant: {because}") }).to_string();
    let m = Move::Control { from: State::Queued, to: State::Failed, at: now, detail: &detail };
    let moved = crate::session_live::record(app, &s.id, &m)?.is_ok();
    if moved {
        info!(session = s.id, "a queued session failed: no standing grant");
        if let Some(room) = app.db.chat_room(&s.room)? {
            let line = format!(
                "This agent session didn't start: it runs under this project's standing grant, and {because}. {}, then \
                 ask again.",
                how_to_grant(app)
            );
            say(app, &room, s.parent_seq, &line)?;
        }
    }
    Ok(moved)
}

/// Say in `s`'s thread that a revoke ended it.
pub fn say_ended(app: &Platform, s: &AgentSession) -> Result<()> {
    if let Some(room) = app.db.chat_room(&s.room)? {
        say(app, &room, s.parent_seq, &ended_line(app))?;
    }
    Ok(())
}

/// The project's standing grant was just revoked (module docs): how many of
/// its sessions ended.
pub async fn revoked(app: &Platform, project: &str, running: Running) -> Result<u64> {
    let now = arugula_e2e::now_ms();
    let (mut ended, mut told) = (0, BTreeSet::new());
    for s in app.db.live_sessions_of(project)? {
        if running == Running::Finish && s.state == State::Working {
            // Its turn finishes, and nothing queued goes after it.
            let queued = app.db.session_prompts(&s.id)?.into_iter().filter(|p| p.state == PromptState::Queued);
            for p in queued {
                app.db.drop_prompt(&p.id, now)?;
            }
            continue;
        }
        if end(app, &s, now)? {
            ended += 1;
            // One line a thread: a Try 3's siblings share one.
            if told.insert((s.room.clone(), s.parent_seq)) {
                say_ended(app, &s)?;
            }
        }
    }
    Ok(ended)
}
