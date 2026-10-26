//! Boards over the platform's routes (live status board, step 1): a
//! project's boards ([`db::boards`]), each an ordered list of zones that
//! hold an ordered list of rows. A board is a project-scoped platform
//! object (`brd_`), not a daemon block: it is here, with every machine
//! asleep. It has a thread like an issue (`chat::objects`). The wire is
//! [`arugula_control_wire::boards`], which says how revs work.
//!
//! **Who may do what,** by membership of the board's project, as for
//! issues: a personal project's account does everything; in a team's
//! project owners and editors make boards and change them, viewers read;
//! anyone else gets 404.
//!
//! **Gating:** the `boards` Labs flag ([`FLAG`], resolved as chat's is).
//! With it off for the account, every route here answers 404, as if boards
//! weren't there.
//!
//! **Pointers are stored, not resolved:** a row's `fi_`, `iss_`, `run_`,
//! `hw_` id or agent block is kept as given, after a check of its shape.
//! Later steps read the live state it points at.
//!
//! [`db::boards`]: crate::db::boards

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::boards as wire;
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    Platform,
    auth::Person,
    chat::flag_on,
    db::boards::{self as db, Put, Refused},
    issues::PERSONAL,
};

/// The Labs flag boards are behind.
pub const FLAG: &str = "boards";
/// The reason a refusal gives for a write made on a stale rev.
pub const STALE_REV: &str = "stale_rev";

type J<T> = Result<Json<T>, ApiError>;

fn nothing() -> ApiError {
    err(StatusCode::NOT_FOUND, "not found")
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_board() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such board")
}

fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// 404 unless boards are on for `account`.
async fn gate(app: &Platform, account: &str) -> Result<(), ApiError> {
    if flag_on(app, account, FLAG).await? { Ok(()) } else { Err(nothing()) }
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `account` sees
/// it: 404 for a stranger or when boards are off; with `write`, 403 for a
/// viewer, and the team's roster comes from the hub, not the cache.
async fn place(app: &Platform, account: &str, asked: &str, write: bool) -> Result<String, ApiError> {
    gate(app, account).await?;
    let id = if asked == PERSONAL {
        crate::projects::personal_project(app, account).await?.ok_or_else(no_project)?
    } else {
        asked.to_owned()
    };
    let project = app.db.issue_project(&id)?.ok_or_else(no_project)?;
    let role = match (&project.account, &project.team) {
        (Some(a), _) if a == account => TeamRole::Owner,
        (None, Some(t)) => {
            let t = if write { app.cache.team_now(&*app.hub, t).await? } else { app.cache.team(&*app.hub, t).await? };
            t.ok_or_else(no_project)?.role(account).ok_or_else(no_project)?
        }
        _ => return Err(no_project()),
    };
    if write && role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads boards, and doesn't change them"));
    }
    Ok(project.id)
}

/// A board by its id as `account` sees it: 404 unless they may read it
/// (or, with `write`, change it).
async fn board_place(app: &Platform, account: &str, id: &str, write: bool) -> Result<wire::Board, ApiError> {
    gate(app, account).await?;
    let board = app.db.board(id)?.ok_or_else(no_board)?;
    match place(app, account, &board.project, write).await {
        Err(e) if e.0 == StatusCode::NOT_FOUND => Err(no_board()),
        r => r.map(|_| board),
    }
}

fn refused(r: Refused) -> ApiError {
    match r {
        Refused::Missing(what) => err(StatusCode::NOT_FOUND, what),
        Refused::Stale { rev } => {
            refusal(StatusCode::CONFLICT, STALE_REV, &format!("this entry is at rev {rev} now: read the board again"))
        }
        Refused::Bad(why) => err(StatusCode::BAD_REQUEST, &why),
    }
}

fn one_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

fn title_ok(t: &str) -> Result<&str, ApiError> {
    let t = t.trim();
    if t.is_empty() {
        return Err(bad("a title can't be empty"));
    }
    if !one_line(t) {
        return Err(bad("a title is one line, without control characters"));
    }
    if t.chars().count() > wire::TITLE_MAX {
        return Err(bad(&format!("a title is at most {} characters", wire::TITLE_MAX)));
    }
    Ok(t)
}

fn note_ok(n: &str) -> Result<&str, ApiError> {
    let n = n.trim();
    if !one_line(n) {
        return Err(bad("a note is one line, without control characters"));
    }
    if n.chars().count() > wire::NOTE_MAX {
        return Err(bad(&format!("a note is at most {} characters", wire::NOTE_MAX)));
    }
    Ok(n)
}

/// `id` is `prefix_` and a few letters and digits: a platform id's shape.
fn id_ok(id: &str, prefix: &str) -> Result<(), ApiError> {
    let rest = id.strip_prefix(prefix).and_then(|r| r.strip_prefix('_'));
    match rest {
        Some(r) if !r.is_empty() && r.len() <= 64 && r.chars().all(|c| c.is_ascii_alphanumeric()) => Ok(()),
        _ => Err(bad(&format!("expected an id that starts {prefix}_"))),
    }
}

fn name_ok(s: &str, what: &str) -> Result<(), ApiError> {
    if s.is_empty() || s.chars().count() > 128 || !one_line(s) {
        return Err(bad(&format!("a {what} is 1 to 128 characters, on one line")));
    }
    Ok(())
}

fn pointer_ok(p: &wire::Pointer) -> Result<(), ApiError> {
    match p {
        wire::Pointer::None => Ok(()),
        wire::Pointer::ForgeItem { id } => id_ok(id, "fi"),
        wire::Pointer::Issue { id } => id_ok(id, "iss"),
        wire::Pointer::Run { id } => id_ok(id, "run"),
        wire::Pointer::Workspace { id } => id_ok(id, "hw"),
        wire::Pointer::Block { machine, pane } => {
            name_ok(machine, "machine")?;
            name_ok(pane, "pane")
        }
    }
}

fn view(s: db::Stored) -> wire::BoardView {
    let mut zones: Vec<wire::Zone> = vec![];
    for e in s.entries {
        match e.kind {
            wire::EntryKind::Zone => {
                zones.push(wire::Zone { id: e.id, rev: e.rev, title: e.title, kind: e.zone_kind, rows: vec![] })
            }
            wire::EntryKind::Row => {
                if let Some(z) = zones.last_mut() {
                    z.rows.push(wire::Row {
                        id: e.id,
                        rev: e.rev,
                        pointer: e.pointer,
                        note: e.note,
                        status: e.status,
                        created_at: e.created_at,
                        updated_at: e.updated_at,
                    });
                }
            }
        }
    }
    wire::BoardView { board: s.board, zones }
}

fn read(app: &Platform, id: &str) -> Result<wire::BoardView, ApiError> {
    Ok(view(app.db.board_stored(id)?.ok_or_else(no_board)?))
}

// ---------------------------------------------------------------- the seam

/// A project's boards as `account` may read them.
pub async fn list(app: &Platform, account: &str, project: &str) -> Result<wire::BoardList, ApiError> {
    let id = place(app, account, project, false).await?;
    Ok(wire::BoardList { boards: app.db.boards(&id)?, project: id })
}

/// Make a board in `project`.
pub async fn create(
    app: &Platform,
    account: &str,
    project: &str,
    new: &wire::NewBoard,
) -> Result<wire::BoardView, ApiError> {
    let id = place(app, account, project, true).await?;
    let title = title_ok(&new.title)?;
    let board = app
        .db
        .create_board(&id, title, account, now_ms(), wire::BOARDS_MAX)?
        .ok_or_else(|| bad(&format!("a project has at most {} boards", wire::BOARDS_MAX)))?;
    read(app, &board.id)
}

/// A board with its zones and rows in order, as `account` may read it.
pub async fn get(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    board_place(app, account, id, false).await?;
    read(app, id)
}

/// Make or change a zone or a row (see [`wire::PutEntry`]).
pub async fn put(app: &Platform, account: &str, id: &str, p: &wire::PutEntry) -> Result<wire::BoardView, ApiError> {
    board_place(app, account, id, true).await?;
    if let Some(i) = &p.id
        && id_ok(i, "row").is_err()
        && id_ok(i, "zon").is_err()
    {
        return Err(bad("expected an id that starts zon_ or row_"));
    }
    if let Some(z) = &p.zone {
        id_ok(z, "zon")?;
    }
    let title = p.title.as_deref().map(title_ok).transpose()?;
    let note = p.note.as_deref().map(note_ok).transpose()?;
    if let Some(ptr) = &p.pointer {
        pointer_ok(ptr)?;
    }
    let put = Put {
        id: p.id.as_deref(),
        rev: p.rev,
        entry: p.entry,
        zone: p.zone.as_deref(),
        title,
        kind: p.kind,
        pointer: p.pointer.as_ref(),
        note,
        status: p.status,
    };
    app.db.put_entry(id, &put, now_ms(), wire::ZONES_MAX, wire::ROWS_MAX)?.map_err(refused)?;
    read(app, id)
}

/// Reorder a row or a zone.
pub async fn move_entry(
    app: &Platform,
    account: &str,
    id: &str,
    entry: &str,
    m: &wire::MoveEntry,
) -> Result<wire::BoardView, ApiError> {
    board_place(app, account, id, true).await?;
    app.db
        .move_entry(
            id,
            entry,
            &db::Move { zone: m.zone.as_deref(), before: m.before.as_deref(), rev: m.rev },
            now_ms(),
            wire::ROWS_MAX,
        )?
        .map_err(refused)?;
    read(app, id)
}

/// Delete a row, or a zone and its rows.
pub async fn delete(app: &Platform, account: &str, id: &str, entry: &str) -> Result<wire::BoardView, ApiError> {
    board_place(app, account, id, true).await?;
    app.db.delete_entry(id, entry, now_ms())?.map_err(refused)?;
    read(app, id)
}

// ---------------------------------------------------------------- routes

/// `GET /api/projects/{project}/boards`: a project's boards, oldest first.
pub async fn list_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
) -> J<wire::BoardList> {
    Ok(Json(list(&app, &s.account, &project).await?))
}

/// `POST /api/projects/{project}/boards`: make one (owners and editors).
pub async fn create_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(new): Json<wire::NewBoard>,
) -> J<wire::BoardView> {
    Ok(Json(create(&app, &s.account, &project, &new).await?))
}

/// `GET /api/boards/{id}`: the board with its zones and rows in order.
pub async fn get_route(State(app): State<Arc<Platform>>, s: Person, Path(id): Path<String>) -> J<wire::BoardView> {
    Ok(Json(get(&app, &s.account, &id).await?))
}

/// `PUT /api/boards/{id}/entries`: make or change a zone or a row (owners
/// and editors); 409 `stale_rev` when the entry moved on.
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(p): Json<wire::PutEntry>,
) -> J<wire::BoardView> {
    Ok(Json(put(&app, &s.account, &id, &p).await?))
}

/// `POST /api/boards/{id}/entries/{entry}/move`: reorder within or between
/// zones (owners and editors).
pub async fn move_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
    Json(m): Json<wire::MoveEntry>,
) -> J<wire::BoardView> {
    Ok(Json(move_entry(&app, &s.account, &id, &entry, &m).await?))
}

/// `DELETE /api/boards/{id}/entries/{entry}`: a row, or a zone and its rows
/// (owners and editors).
pub async fn delete_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, entry)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(delete(&app, &s.account, &id, &entry).await?))
}

#[cfg(test)]
#[path = "boards_tests.rs"]
mod tests;
