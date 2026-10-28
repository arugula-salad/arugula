//! Move to… (Arugula #214): a thread's session sent to another of its
//! project's workspaces, from its card.
//!
//! - **Where:** the project's other live workspaces that have the
//!   session's repo, as the pool ranks them ([`places`],
//!   [`super::pool::ranked`]), each with how many sessions are going there
//!   (queued, or a turn under way: done and parked ones only wait, and
//!   one left from before its box was rebuilt isn't its box's, Arugula
//!   #266).
//! - **When:** while nothing runs: `queued` (it just goes, for the other
//!   box to take) or `done` (queued again there). Not mid-turn, and not
//!   once it ended. A Try 3's sibling or a review stays on its box, beside
//!   the session it's of. Not "after this turn" either (Arugula #266): a
//!   turn's work is in its box's checkout until it pushes, and the other
//!   box starts from the thread, so the move waits for the turn's end,
//!   when the card offers it; and the pool spreading new sessions over
//!   idle boxes ([`super::pool`]) is what moving a working one was for.
//! - **Who:** its asker, or an owner or editor of its project; not a
//!   viewer.
//! - **How** (`db::sessions::moved_in`): as a rebuilt box's session moves
//!   (M258). A done one starts its next turn there from the thread
//!   hydrated again, and from its conversation, when its old box archived
//!   it: if the old box is up, it's asked for that one block's transcript
//!   first (`hosted.archive_blocks`). Not `hosted.archive`: that archives
//!   every open block, and the platform keeps one transcript per block,
//!   so the box's other sessions would keep a transcript of now rather
//!   than of how they end. A box from before the op answers that it
//!   doesn't know it, and the session moves without its conversation, as
//!   its card says.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::{
    forge::{DaemonOp, OpAnswer},
    sessions::{SessionMove, SessionPlace, SessionView},
    transcripts::ArchiveBlocks,
};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State as AxState},
    http::StatusCode,
};
use tracing::{info, warn};

use super::pool;
use crate::{
    Platform,
    auth::Person,
    db::{
        hosted::HostedWorkspace,
        sessions::{NewPrompt, Role, Session, Source, State},
    },
};

/// Whether `s` may move now: its own thread's, queued or done.
pub fn movable(s: &Session) -> bool {
    s.role == Role::Thread && matches!(s.state, State::Queued | State::Done)
}

/// Where `s` may move: the project's other workspaces with its repo,
/// best first.
pub async fn places(app: &Platform, s: &Session) -> anyhow::Result<Vec<SessionPlace>> {
    if !movable(s) {
        return Ok(vec![]);
    }
    let others: Vec<HostedWorkspace> =
        pool::candidates_all(app, &s.project)?.into_iter().filter(|w| w.id != s.workspace).collect();
    let others = pool::with_repo(others, s.repo.as_deref());
    Ok(pool::ranked(app, &s.project, others)
        .await?
        .into_iter()
        .map(|(w, l)| SessionPlace { workspace: w.id, name: w.name, awake: l.awake, busy: l.live })
        .collect())
}

/// `POST /api/sessions/{ses}/move` [`SessionMove`]: Move to… (module docs).
pub async fn route(
    AxState(app): AxState<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Json(b): Json<SessionMove>,
) -> Result<Json<SessionView>, ApiError> {
    let (s, may) = crate::session_live::readable(&app, &account, &id, true).await?;
    if !may && (s.asker.is_empty() || s.asker != account) {
        return Err(err(StatusCode::FORBIDDEN, "its asker, or an owner or editor of its project, moves it"));
    }
    if s.role != Role::Thread {
        return Err(err(StatusCode::CONFLICT, "a try or a review stays on its box, beside the session it's of"));
    }
    match s.state {
        State::Queued | State::Done => {}
        State::Working | State::NeedsInput | State::Unresponsive => {
            return Err(err(StatusCode::CONFLICT, "a turn is under way: it moves once the turn is done"));
        }
        _ => return Err(crate::session_live::refused(&crate::db::sessions::Refused::Moved { now: s.state })),
    }
    let Some(to) = places(&app, &s).await?.into_iter().find(|p| p.workspace == b.workspace) else {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't another of the project's workspaces with its repo"));
    };
    let (text, lost) = match s.state {
        State::Done => {
            archive_first(&app, &s).await;
            let r = super::hydrate::after_move(&app, &s).await.map_err(ApiError::from)?;
            (r.text, r.lost)
        }
        _ => (String::new(), false),
    };
    let prompt = NewPrompt { author: &s.asker, source: Source::Member, association: None, text: &text };
    let now = now_ms();
    match app.db.move_session_to(&s.id, s.state, &to.workspace, &prompt, lost, (&account, &to.name), now)? {
        Ok(after) => {
            info!(session = s.id, from = s.workspace, to = to.workspace, by = account, "a session moved (Move to…)");
            crate::session_live::told(&app, std::slice::from_ref(&s.id))?;
            app.db.nudge_session(&s.id, now)?;
            Ok(Json(crate::session_live::view(&app, &after, may, &account).await?))
        }
        Err(r) => Err(crate::session_live::refused(&r)),
    }
}

/// Before a done session leaves its box: ask the box, when it's up, to
/// archive the session's own block's transcript now (module docs). The
/// move goes ahead whatever it says.
async fn archive_first(app: &Platform, s: &Session) {
    let Some(block) = s.block.as_deref().and_then(|b| b.parse::<u64>().ok()) else { return };
    let Some(machine) = s.machine.as_deref() else { return };
    let up = app
        .db
        .hosted_any(&s.project, &s.workspace)
        .ok()
        .flatten()
        .is_some_and(|w| w.state == "ready" && w.machine.as_deref() == Some(machine));
    if !up {
        return;
    }
    let op = DaemonOp::HostedArchiveBlocks(ArchiveBlocks { blocks: vec![block] });
    match tokio::time::timeout(app.hosted.archive_wait(), app.hub.daemon_op(machine, &op)).await {
        Ok(Ok(OpAnswer::Done { .. })) => {
            info!(session = s.id, machine, "a session's box archived its transcript before it moved")
        }
        Ok(Ok(OpAnswer::Offline | OpAnswer::Unknown)) => {}
        Ok(Ok(said)) => {
            warn!(session = s.id, machine, ?said, "a session's box didn't archive its transcript before it moved")
        }
        Ok(Err(e)) => {
            warn!(session = s.id, machine, error = %e, "the hub didn't ask a session's box to archive its transcript")
        }
        Err(_) => warn!(session = s.id, machine, "a session's box didn't archive its transcript in time: moving it"),
    }
}
