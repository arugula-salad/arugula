//! Try 3 (M264, thread = session, Arugula #129): a session tried three
//! ways at once from its card, then one kept. The routes are
//! `arugula_control_wire::sessions`' `SESSION_TRY` and `SESSION_PICK`; the
//! rows are `db::sessions`' (`start_tries_in`, `pick_in`).
//!
//! - **Who:** an owner or editor of the session's project who may start
//!   work on its hosted workspaces (the rule `@arugula` asks,
//!   `triggers::may_start`).
//! - **The three:** the session itself, which keeps its turn, and two
//!   siblings that re-run its first prompt, each in a slot of its own on
//!   the same box, on its own branch. Their harnesses are the asker's
//!   choice on the button: `claude` and `codex` unless they say (Codex where
//!   the box has it; a box without it runs Claude Code and says so), or
//!   `claude` twice. Each sibling's turns are the session's asker's, as
//!   the session's are (aru-115), whoever clicked.
//! - **The cap:** a box holds three sessions, done ones too
//!   (`MAX_SESSIONS_PER_BOX`). A sibling with no slot waits queued, and the
//!   card counts it as waiting for one.
//! - **Pick** keeps one: the others that haven't ended stop, "not picked"
//!   (`db::sessions::NOT_PICKED`), and their boxes clean their slots. What
//!   they pushed stays on GitHub; what they didn't push goes with their
//!   checkouts. A picked sibling gets its own card in the thread and is
//!   the thread's session from then on: replies prompt it, and its asker
//!   sends its PR from it as from any session's.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::sessions::{SessionPick, SessionTryAsk, SessionView};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::info;

use crate::{
    Platform,
    auth::Person,
    db::sessions::{Refused, Role},
    session_live::{actor, refused, told, view},
    triggers::{Cannot, may_start},
};

/// The siblings' harnesses when the button doesn't say.
pub const DEFAULT: [&str; 2] = ["claude", "codex"];

/// `POST /api/sessions/{ses}/try`: two siblings beside it (module docs).
pub async fn start(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Json(b): Json<SessionTryAsk>,
) -> Result<Json<SessionView>, ApiError> {
    let s = actor(&app, &account, &id).await?;
    if let Some(why) = may_start(&app, &s.project, &account).await? {
        let said = match why {
            Cannot::Viewer => "a viewer can't try a session three ways",
            Cannot::Unlisted => {
                "Try 3 runs on hosted workspaces, which are in a limited beta: you aren't on its list yet"
            }
            Cannot::Outside => "only the project's members try its sessions",
        };
        return Err(err(StatusCode::FORBIDDEN, said));
    }
    let asked: Vec<String> = b.harnesses.unwrap_or_else(|| DEFAULT.map(String::from).to_vec());
    let harnesses: Vec<&str> = asked.iter().map(String::as_str).collect();
    match app.db.start_tries(&s.id, &harnesses, now_ms())? {
        Ok(siblings) => {
            info!(session = s.id, by = account, ?harnesses, "a session is tried three ways");
            for sib in &siblings {
                app.chat.session(&app.db, sib);
            }
            let after = app.db.session(&s.id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such session"))?;
            Ok(Json(view(&app, &after, true, &account).await?))
        }
        Err(Refused::Detail(why)) if why.starts_with("a Try 3 runs") => Err(err(StatusCode::BAD_REQUEST, why)),
        Err(r) => Err(refused(&r)),
    }
}

/// `POST /api/sessions/{ses}/pick`: keep one of its Try 3's sessions, from
/// its card or a sibling's (module docs).
pub async fn pick(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Json(b): Json<SessionPick>,
) -> Result<Json<SessionView>, ApiError> {
    let s = actor(&app, &account, &id).await?;
    let parent = match (s.role, &s.parent_session) {
        (Role::Try, Some(p)) => p.clone(),
        _ => s.id.clone(),
    };
    let (after, stopped) = match app.db.pick_try(&parent, &b.session, &account, now_ms())? {
        Ok(done) => done,
        Err(r) => return Err(refused(&r)),
    };
    info!(session = parent, picked = b.session, by = account, stopped = stopped.len(), "a try was kept");
    told(&app, &stopped)?;
    app.chat.session(&app.db, &after);
    if b.session != parent
        && let Some(kept) = app.db.session(&b.session)?
    {
        // The thread's session now: its card goes in the thread.
        crate::chat::session_card::post_card(&app, &kept)?;
        app.chat.session(&app.db, &kept);
    }
    let shown = app.db.session(&id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such session"))?;
    Ok(Json(view(&app, &shown, true, &account).await?))
}
