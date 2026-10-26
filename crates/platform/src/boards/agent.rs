//! Boards for a machine's agents (live status board, step 3): the routes
//! MCP's board tools call, signed by the machine. Each is one of
//! [`super`]'s functions with the agent as the [`Actor`], so who may do
//! what is checked in one place ([`super::place`] and the rules beside it):
//! the owner's role in the project is the ceiling, and the machine needs
//! its grant over the project too (`boards.read` to read, `boards.write` to
//! change), asked again on every call. What an agent writes is its owner's,
//! and a row it adds says which machine's agent added it.
//!
//! There is no route to make a board, and a zone can't be made, changed,
//! moved or deleted from here (see [`super`]).

use std::sync::Arc;

use arugula_control_wire::boards as wire;
use axum::{
    Json,
    extract::{Path, State},
};

use super::J;
use crate::{Platform, auth::Machine, db::issues::Actor};

/// Reads a machine's agents make, per hour (a list, a board).
const AGENT_BOARD_READS: (&str, usize) = ("board-agent-read", 3_600);
/// Writes a machine's agents make, per hour: each lands at once and shows
/// on everyone's board, so fewer than reads.
const AGENT_BOARD_WRITES: (&str, usize) = ("board-agent-write", 600);

fn actor(d: &Machine) -> Actor<'_> {
    Actor { account: &d.account, agent: Some(&d.daemon) }
}

/// `GET /api/daemon/projects/{project}/boards`.
pub async fn list(State(app): State<Arc<Platform>>, d: Machine, Path(project): Path<String>) -> J<wire::BoardList> {
    app.limits.check_daemon(AGENT_BOARD_READS, &d.daemon)?;
    Ok(Json(super::list(&app, actor(&d), &project).await?))
}

/// `GET /api/daemon/boards/{id}`.
pub async fn get(State(app): State<Arc<Platform>>, d: Machine, Path(id): Path<String>) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_READS, &d.daemon)?;
    Ok(Json(super::get(&app, actor(&d), &id).await?))
}

/// `PUT /api/daemon/boards/{id}/entries`: add a row, or set a row's note
/// or status; 409 `stale_rev` when the row moved on.
pub async fn put(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(id): Path<String>,
    Json(p): Json<wire::PutEntry>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::put(&app, actor(&d), &id, &p).await?))
}

/// `POST /api/daemon/boards/{id}/entries/{entry}/move`: move a row.
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
/// agents added.
pub async fn delete(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((id, entry)): Path<(String, String)>,
) -> J<wire::BoardView> {
    app.limits.check_daemon(AGENT_BOARD_WRITES, &d.daemon)?;
    Ok(Json(super::delete(&app, actor(&d), &id, &entry).await?))
}
