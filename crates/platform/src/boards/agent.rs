//! Boards for a machine's agents (boards, step 3): the routes
//! MCP's board tools call, signed by the machine. Each is one of
//! [`super`]'s functions with the agent as the [`Actor`], so who may do
//! what is checked in one place ([`super::place`] and the rules beside it):
//! the owner's role in the project is the ceiling, and the machine needs
//! its grant over the project too (`boards.read` to read, `boards.write` to
//! change), asked again on every call. What an agent writes is its owner's,
//! and a row it adds says which machine's agent added it.
//!
//! A machine also reports the state of its own agent blocks
//! ([`put_block_state`]), for the rows that point at them: the machine is
//! the one that signed the request, and it needs `boards.write` over at
//! least one project of its owner's.
//!
//! An agent makes and runs boards too (aru-123): it makes one, sets its title
//! and goal, closes and reopens it, and works its zones and rows, and the
//! board says it was an agent's. What it may not do is destroy or rewrite
//! another hand's work (see [`super`]).

use std::sync::Arc;

use arugula_control_wire::boards as wire;
use axum::{
    Json,
    extract::{Path, State},
};

use super::J;
use crate::{
    Platform,
    auth::Machine,
    db::{issues::Actor, projects::BOARDS_WRITE},
    grants::{AGENT_UNGRANTED, AgentScope, agent_may},
};
use arugula_control_common::{ApiError, err, refusal};
use axum::http::StatusCode;

/// Reads a machine's agents make, per hour (a list, a board).
const AGENT_BOARD_READS: (&str, usize) = ("board-agent-read", 3_600);
/// Writes a machine's agents make, per hour: each lands at once and shows
/// on everyone's board, so fewer than reads.
const AGENT_BOARD_WRITES: (&str, usize) = ("board-agent-write", 600);
/// Block reports a machine makes, per hour: its daemon sends a block's
/// changes a few seconds apart at most.
const AGENT_BLOCK_REPORTS: (&str, usize) = ("board-block-state", 2_400);

fn actor(d: &Machine) -> Actor<'_> {
    Actor { account: &d.account, agent: Some(&d.daemon) }
}

/// `GET /api/daemon/projects/{project}/boards`.
pub async fn list(State(app): State<Arc<Platform>>, d: Machine, Path(project): Path<String>) -> J<wire::BoardList> {
    app.limits.check_daemon(AGENT_BOARD_READS, &d.daemon)?;
    Ok(Json(super::list(&app, actor(&d), &project).await?))
}

/// `POST /api/daemon/projects/{project}/boards`: make a board.
pub async fn create(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Json(new): Json<wire::NewBoard>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::create(&app, actor(&d), &project, &new).await?))
}

/// `GET /api/daemon/boards/{id}`.
pub async fn get(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_READS, &d.daemon)?;
    Ok(Json(super::get(&app, actor(&d), &id).await?))
}

/// `PATCH /api/daemon/boards/{id}`: set the title or the goal.
pub async fn edit(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(e): Json<wire::EditBoard>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::edit(&app, actor(&d), &id, &e).await?))
}

/// `POST /api/daemon/boards/{id}/close`: close it with a done note.
pub async fn close(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(c): Json<wire::CloseBoard>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::close(&app, actor(&d), &id, &c).await?))
}

/// `POST /api/daemon/boards/{id}/reopen`: open it again.
pub async fn reopen(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::reopen(&app, actor(&d), &id).await?))
}

/// `PUT /api/daemon/boards/{id}/entries`: make or change a zone, add a row,
/// or set a row's note, status or (its own machine's) pointer; 409
/// `stale_rev` when the entry moved on.
pub async fn put(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(p): Json<wire::PutEntry>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::put(&app, actor(&d), &id, &p).await?))
}

/// `POST /api/daemon/boards/{id}/entries/{entry}/move`: move a row or a zone.
pub async fn move_entry(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, entry)): Path<(String, String)>,
    Json(m): Json<wire::MoveEntry>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::move_entry(&app, actor(&d), &id, &entry, &m).await?))
}

/// `DELETE /api/daemon/boards/{id}/entries/{entry}`: a row this machine's
/// agents added, or a zone with no row of another hand's in it.
pub async fn delete(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, entry)): Path<(String, String)>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::delete(&app, actor(&d), &id, &entry).await?))
}

/// `PUT /api/daemon/block-state`: the state of one of the signing machine's
/// own agent blocks. The machine is the signer's, never the body's, so one
/// machine can't set another's block. Refused (403) unless the machine holds
/// `boards.write` over a project of its owner's; 404 with the `boards` flag
/// off for the owner.
pub async fn put_block_state(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Json(p): Json<wire::PutBlockState>,
) -> J<serde_json::Value> {
    app.limits.check_daemon(AGENT_BLOCK_REPORTS, &d.daemon)?;
    super::gate(&app, &d.account).await?;
    may_report(&app, &d).await?;
    super::name_ok(&p.pane, "pane")?;
    state_ok(&p.block)?;
    app.db.put_block_state(&d.daemon, &d.account, &p.pane, &p.block, arugula_e2e::now_ms())?;
    match app.db.boards_on_block(&d.daemon, &p.pane) {
        Ok(boards) => boards.iter().for_each(|(b, project)| super::changed(&app, project, b)),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on a block"),
    }
    Ok(Json(serde_json::json!({})))
}

/// The machine holds `boards.write` over at least one project, as
/// [`agent_may`] reads it now.
async fn may_report(app: &Platform, d: &Machine) -> Result<(), ApiError> {
    for (project, owner) in app.db.machine_write_projects(&d.daemon, BOARDS_WRITE)? {
        if owner == d.account && agent_may(app, d, BOARDS_WRITE, &AgentScope::Project(project)).await.is_ok() {
            return Ok(());
        }
    }
    Err(refusal(
        StatusCode::FORBIDDEN,
        AGENT_UNGRANTED,
        "this machine has no boards.write grant over a project, so it may not report its agent blocks: its owner \
         gives one with `arugula grants add <machine> boards.write --project <name>`",
    ))
}

/// A report is short facts: a branch on one line, an abbreviated HEAD.
fn state_ok(b: &wire::BlockState) -> Result<(), ApiError> {
    let line = |s: &str| !s.is_empty() && s.chars().count() <= 200 && !s.chars().any(char::is_control);
    if b.branch.as_deref().is_some_and(|s| !line(s)) {
        return Err(err(StatusCode::BAD_REQUEST, "a branch is 1 to 200 characters, on one line"));
    }
    if b.head.as_deref().is_some_and(|h| h.is_empty() || h.len() > 40 || !h.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return Err(err(StatusCode::BAD_REQUEST, "a head is an abbreviated commit id"));
    }
    Ok(())
}
