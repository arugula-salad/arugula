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
//! **Pointers are stored, and their state is read:** a row's `fi_`, `iss_`,
//! `run_`, `hw_` id or agent block is kept as given, after a check of its
//! shape. A read adds each `fi_` and `iss_` row's state now as `live`
//! ([`resolve`]): one query for the mirror's items, one for their checks
//! and one for the issues, through the reader's own forge access. `status`
//! is the row's own and never changes with it.
//!
//! **Live:** a change to a board, to an issue a row points at, or (a
//! webhook) to a mirrored item it points at pushes an object change
//! ([`changed`]), so clients following the board read it again.
//!
//! [`db::boards`]: crate::db::boards

use std::{collections::HashMap, sync::Arc};

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
    db::{
        boards::{self as db, Put, Refused},
        forge::{CheckState, ForgeItem},
    },
    forge::viewer::{self, Viewer},
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
                        live: None,
                        created_at: e.created_at,
                        updated_at: e.updated_at,
                    });
                }
            }
        }
    }
    wire::BoardView { board: s.board, zones }
}

/// A forge item's state now, from the checks on its head.
fn live_item(i: &ForgeItem, repo: &str, checks: &[&CheckState]) -> wire::Live {
    use wire::LiveState::*;
    let state = match (i.kind.as_str(), i.state.as_str()) {
        (_, "merged") => Merged,
        (_, "closed") => Closed,
        ("pr", _) => {
            let red = |c: &&CheckState| {
                c.status == "completed"
                    && matches!(
                        c.conclusion.as_deref(),
                        Some("failure" | "error" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
                    )
            };
            if checks.iter().any(red) {
                ChecksFailed
            } else if checks.iter().any(|c| c.status != "completed") {
                ChecksRunning
            } else {
                Open
            }
        }
        _ => Open,
    };
    wire::Live { state, kind: i.kind.clone(), repo: Some(repo.to_owned()), number: i.number, title: i.title.clone() }
}

/// Add each `fi_` and `iss_` row's live state to `v`, as `account` may see
/// it: items of the project's linked repos they may read, and its issues.
async fn resolve(app: &Platform, account: &str, v: &mut wire::BoardView) -> Result<(), ApiError> {
    let (items, issues) = app.db.board_targets(&v.board.id)?;
    let mut live: HashMap<String, wire::Live> = HashMap::new();
    if !items.is_empty() {
        let repos = app.db.linked_repos(&v.board.project)?;
        let checked = viewer::check(app, &Viewer::of(app, account).await?, &repos).await?;
        let ids: Vec<&str> = items.iter().map(String::as_str).collect();
        let found = app.db.forge_items_among(&checked.readable, &ids)?;
        let heads: Vec<&str> = found.iter().filter_map(|i| i.head_sha.as_deref()).collect();
        let checks = app.db.forge_checks_on_heads(&checked.readable, &heads)?;
        for i in &found {
            let repo = repos.iter().find(|r| r.repo_id == i.repo_id).map_or("", |r| r.full_name.as_str());
            let mine: Vec<_> = checks
                .iter()
                .filter(|c| c.repo_id == i.repo_id && Some(c.head_sha.as_str()) == i.head_sha.as_deref())
                .collect();
            live.insert(i.id.clone(), live_item(i, repo, &mine));
        }
    }
    if !issues.is_empty() {
        let ids: Vec<&str> = issues.iter().map(String::as_str).collect();
        for (id, number, title, closed) in app.db.issue_states(&v.board.project, &ids)? {
            let state = if closed { wire::LiveState::Closed } else { wire::LiveState::Open };
            live.insert(id, wire::Live { state, kind: "native".to_owned(), repo: None, number, title });
        }
    }
    for row in v.zones.iter_mut().flat_map(|z| z.rows.iter_mut()) {
        if let wire::Pointer::ForgeItem { id } | wire::Pointer::Issue { id } = &row.pointer {
            row.live = live.get(id).cloned();
        }
    }
    Ok(())
}

async fn read(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    let mut v = view(app.db.board_stored(id)?.ok_or_else(no_board)?);
    resolve(app, account, &mut v).await?;
    Ok(v)
}

/// The board `id`, in `project`, changed: pages showing it read it again
/// (as for an issue, through its thread's readers).
pub fn changed(app: &Platform, project: &str, id: &str) {
    app.chat.object_changed(&app.db, &format!("project:{project}"), id);
}

/// A native issue changed: the boards that point at it.
pub fn issue_changed(app: &Platform, issue: &str) {
    match app.db.boards_on_issue(issue) {
        Ok(boards) => boards.iter().for_each(|(b, p)| changed(app, p, b)),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on an issue"),
    }
}

/// Mirror rows of `repo` were written (a webhook): the boards that point
/// at the items they changed, or at items whose head a check landed on.
pub fn mirror_changed(app: &Platform, repo: u64, numbers: &[u64], shas: &[String]) {
    match app.db.boards_on_items(repo, numbers, shas) {
        Ok(boards) => boards.iter().for_each(|(b, p)| changed(app, p, b)),
        Err(e) => tracing::warn!(error = %e, "boards: finding those on a mirrored item"),
    }
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
    read(app, account, &board.id).await
}

/// A board with its zones and rows in order, as `account` may read it.
pub async fn get(app: &Platform, account: &str, id: &str) -> Result<wire::BoardView, ApiError> {
    board_place(app, account, id, false).await?;
    read(app, account, id).await
}

/// Make or change a zone or a row (see [`wire::PutEntry`]).
pub async fn put(app: &Platform, account: &str, id: &str, p: &wire::PutEntry) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, account, id, true).await?;
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
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// Reorder a row or a zone.
pub async fn move_entry(
    app: &Platform,
    account: &str,
    id: &str,
    entry: &str,
    m: &wire::MoveEntry,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, account, id, true).await?;
    app.db
        .move_entry(
            id,
            entry,
            &db::Move { zone: m.zone.as_deref(), before: m.before.as_deref(), rev: m.rev },
            now_ms(),
            wire::ROWS_MAX,
        )?
        .map_err(refused)?;
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// Delete a row, or a zone and its rows.
pub async fn delete(app: &Platform, account: &str, id: &str, entry: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, account, id, true).await?;
    app.db.delete_entry(id, entry, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, account, id).await
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
