//! Not finished (Arugula #841): a person or an agent marks the issue of a
//! row, a native issue or a Plane item, with what is left, so a merged PR
//! doesn't close it. Taking the mark off closes it if a PR that would have
//! has merged ([`crate::forge::closes::released`]). The mark is on the
//! ticket ([`crate::db::item_holds`]), so every board with the ticket shows
//! it; the board's lanes and marks read it ([`super::lanes::not_finished`]).
//!
//! Owners and editors, or an agent with `boards.write`, as for a note. A
//! board keeps no timeline of it: the mark says who set it and when, and a
//! release leaves nothing behind but the close, if one follows.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::boards as wire;
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{J, bad, board_place, changed, issue_changed, items_changed, one_line, read};
use crate::{Platform, auth::Person, db::issues::Actor, forge::closes};

/// The ticket of `row` of board `id` in `project`: its issue's id, when the
/// row's item is a native issue of the project or a Plane item.
fn ticket(app: &Platform, project: &str, id: &str, row: &str) -> Result<String, ApiError> {
    super::id_ok(row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    let entry = app.db.board_entry(id, row)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such row"))?;
    let item = match entry.pointer {
        wire::Pointer::Issue { id } if app.db.issue(&id)?.is_some_and(|i| i.project == project) => Some(id),
        wire::Pointer::ForgeItem { id } if app.db.tracker_target(project, &id)?.is_some() => Some(id),
        _ => None,
    };
    item.ok_or_else(|| {
        err(StatusCode::UNPROCESSABLE_ENTITY, "only a native issue or a Plane item can be marked not finished")
    })
}

/// `reason` trimmed, 1 to [`wire::HOLD_MAX`] characters, on one line.
fn reason_ok(reason: &str) -> Result<&str, ApiError> {
    let r = reason.trim();
    if r.is_empty() || !one_line(r) || r.chars().count() > wire::HOLD_MAX {
        return Err(bad(&format!("a reason is 1 to {} characters, on one line", wire::HOLD_MAX)));
    }
    Ok(r)
}

/// Every board with the ticket was changed.
fn shown(app: &Platform, project: &str, board: &str, item: &str) {
    changed(app, project, board);
    if item.starts_with("iss_") {
        issue_changed(app, item);
    } else {
        items_changed(app, &[item.to_owned()]);
    }
}

/// Mark the ticket of `row` of board `id` Not finished with `reason`, as
/// `by`; marked already, it takes the new reason.
pub async fn hold(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    row: &str,
    h: &wire::HoldRow,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    let reason = reason_ok(&h.reason)?;
    let item = ticket(app, &board.project, id, row)?;
    app.db.hold_set(&board.project, &item, reason, by, now_ms())?;
    shown(app, &board.project, id, &item);
    read(app, by.account, id).await
}

/// Take the mark off the ticket of `row`: it finishes if a PR that would
/// have closed it merged. Not marked: nothing changes.
pub async fn release(app: &Platform, by: Actor<'_>, id: &str, row: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    let item = ticket(app, &board.project, id, row)?;
    let held = app.db.hold_of(&board.project, &item)?;
    if held.is_some() && app.db.hold_clear(&board.project, &item)? {
        closes::released(app, &board.project, &item, held.and_then(|h| h.skipped)).await;
        shown(app, &board.project, id, &item);
    }
    read(app, by.account, id).await
}

/// `PUT /api/boards/{id}/rows/{row}/hold`.
pub async fn hold_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, row)): Path<(String, String)>,
    Json(h): Json<wire::HoldRow>,
) -> J<wire::BoardView> {
    Ok(Json(hold(&app, Actor::person(&s.account), &id, &row, &h).await?))
}

/// `DELETE /api/boards/{id}/rows/{row}/hold`.
pub async fn release_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, row)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(release(&app, Actor::person(&s.account), &id, &row).await?))
}
