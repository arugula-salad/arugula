//! A board's Docs (Arugula #517): a strip of Docs under the goal
//! (`board_docs`), and a Doc on one row as a `reference` attachment.
//! A Doc is a project Doc of the board's own project (`pdc_…`), or a Doc
//! block on a machine, which an agent of that machine links
//! (`MachineDoc`: the title is a snapshot, since the machine may be off).
//!
//! **Who may:** whoever writes the board (owners and editors, and an
//! agent with `boards.write`, who takes off only the Docs its own machine
//! linked and links only its own machine's Docs). A Doc of another
//! project is no Doc: 404, checked here and not in the page. A Doc linked
//! already answers the board as it is. A closed board's Docs stay
//! editable, as its rows do.
//!
//! **What a Doc is not:** a `reference` is read by nothing that works out
//! a lane, the ready set, progress or a proof ([`super::assess`] takes
//! only `work` and `proof`).

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::boards as wire;
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{J, bad, board_place, changed, id_ok, name_ok, place, read, refused, title_ok};
use crate::{
    Platform,
    auth::Person,
    chat::flag_on,
    db::{boards_docs as db, issues::Actor},
};

/// 404: no such Doc in this project.
fn no_doc() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such Doc")
}

/// A Doc pointer as it is kept: shaped, with a project Doc checked to be in
/// `project` and a machine Doc's title trimmed. A machine's agent links
/// only a Doc of its own machine.
pub(super) fn doc_ok(
    app: &Platform,
    by: Actor<'_>,
    project: &str,
    p: &wire::Pointer,
) -> Result<wire::Pointer, ApiError> {
    match p {
        wire::Pointer::Doc { id } => {
            id_ok(id, "pdc").map_err(|_| no_doc())?;
            app.db.project_doc(id)?.filter(|d| d.project == project).ok_or_else(no_doc)?;
            Ok(p.clone())
        }
        wire::Pointer::MachineDoc { machine, doc, title } => {
            name_ok(machine, "machine")?;
            name_ok(doc, "Doc")?;
            if let Some(own) = by.agent
                && own != machine
            {
                return Err(err(StatusCode::FORBIDDEN, "an agent links only Docs on its own machine"));
            }
            Ok(wire::Pointer::MachineDoc {
                machine: machine.clone(),
                doc: doc.clone(),
                title: title_ok(title)?.to_owned(),
            })
        }
        _ => Err(bad("a Doc is a project Doc (pdc_…) or a Doc on a machine")),
    }
}

/// What a project Doc's chip says, from its head; a Doc that is gone says so.
pub(super) fn project_chip(head: Option<&db::Head>) -> wire::DocChip {
    match head {
        Some(h) => wire::DocChip {
            title: h.title.clone(),
            sections: Some(h.sections),
            written: Some(h.written),
            missing: false,
            machine_name: None,
        },
        None => wire::DocChip {
            title: wire::DOC_GONE.to_owned(),
            sections: None,
            written: None,
            missing: true,
            machine_name: None,
        },
    }
}

/// The strip as a read answers it. `names` are the machines' names by id.
pub(super) fn view(strip: Vec<db::Strip>, names: &HashMap<String, String>) -> Vec<wire::BoardDoc> {
    let name = |m: &str| names.get(m).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| "a machine".to_owned());
    strip
        .into_iter()
        .map(|s| {
            let (chip, machine_name) = match &s.pointer {
                wire::Pointer::MachineDoc { machine, title, .. } => (
                    wire::DocChip {
                        title: title.clone(),
                        sections: None,
                        written: None,
                        missing: false,
                        machine_name: None,
                    },
                    Some(name(machine)),
                ),
                _ => (project_chip(s.head.as_ref()), None),
            };
            wire::BoardDoc {
                id: s.id,
                pointer: s.pointer,
                title: chip.title,
                sections: chip.sections,
                written: chip.written,
                missing: chip.missing,
                machine_name,
                added_by: s.added_by,
                agent: s.agent.map(|m| arugula_control_wire::chat::ChatAgent {
                    machine_name: names.get(&m).cloned().unwrap_or_default(),
                    machine: m,
                    session: None,
                }),
                created_at: s.created_at,
            }
        })
        .collect()
}

/// Link a Doc to the board's strip, last. One linked already is not linked
/// again.
pub async fn add(app: &Platform, by: Actor<'_>, id: &str, a: &wire::AddDoc) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    let pointer = doc_ok(app, by, &board.project, &a.pointer)?;
    let said = match app.db.add_board_doc(id, &pointer, by.account, by.agent, now_ms())? {
        Ok(db::Added::New(_)) => None,
        Ok(db::Added::Already(_)) => {
            Some("that Doc is linked to this board already: it was not linked again".to_owned())
        }
        Err(r) => return Err(refused(r)),
    };
    if said.is_none() {
        changed(app, &board.project, id);
    }
    let mut v = read(app, by.account, id).await?;
    v.said = said;
    Ok(v)
}

/// Take a Doc off the strip. An agent takes off only those its own machine
/// linked.
pub async fn remove(app: &Platform, by: Actor<'_>, id: &str, doc: &str) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    id_ok(doc, "bdc").map_err(|_| bad("expected an id that starts bdc_"))?;
    if let (Some(machine), Some(d)) = (by.agent, app.db.board_doc(id, doc)?)
        && d.agent.as_deref() != Some(machine)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent takes off only the Docs its own machine linked: this one is someone else's",
        ));
    }
    app.db.remove_board_doc(id, doc, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Move a Doc along the strip. That is planning, so people and agents alike
/// may.
pub async fn move_doc(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    doc: &str,
    m: &wire::MoveDoc,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    id_ok(doc, "bdc").map_err(|_| bad("expected an id that starts bdc_"))?;
    if let Some(b) = &m.before {
        id_ok(b, "bdc").map_err(|_| bad("expected an id that starts bdc_"))?;
    }
    app.db.move_board_doc(id, doc, m.before.as_deref(), now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// The boards of `project` that list its Doc `doc`, as `account` may read
/// them: none when boards are off for them.
pub async fn on_boards(app: &Platform, account: &str, project: &str, doc: &str) -> Result<wire::DocBoards, ApiError> {
    if !flag_on(app, account, super::FLAG).await? {
        return Ok(wire::DocBoards::default());
    }
    let project = place(app, Actor::person(account), project, false).await?;
    id_ok(doc, "pdc").map_err(|_| no_doc())?;
    app.db.project_doc(doc)?.filter(|d| d.project == project).ok_or_else(no_doc)?;
    let boards = app
        .db
        .doc_listings(&project, doc)?
        .into_iter()
        .map(|l| wire::DocOnBoard { board: l.board, title: l.title, strip: l.strip, rows: l.rows })
        .collect();
    Ok(wire::DocBoards { boards })
}

// ---------------------------------------------------------------- routes

/// `POST /api/boards/{id}/docs`: link a Doc to the strip (owners and
/// editors).
pub async fn add_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Json(a): Json<wire::AddDoc>,
) -> J<wire::BoardView> {
    Ok(Json(add(&app, Actor::person(&s.account), &id, &a).await?))
}

/// `DELETE /api/boards/{id}/docs/{doc}`: take a Doc off the strip (owners
/// and editors).
pub async fn remove_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, doc)): Path<(String, String)>,
) -> J<wire::BoardView> {
    Ok(Json(remove(&app, Actor::person(&s.account), &id, &doc).await?))
}

/// `POST /api/boards/{id}/docs/{doc}/move`: move a Doc along the strip
/// (owners and editors).
pub async fn move_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, doc)): Path<(String, String)>,
    Json(m): Json<wire::MoveDoc>,
) -> J<wire::BoardView> {
    Ok(Json(move_doc(&app, Actor::person(&s.account), &id, &doc, &m).await?))
}

/// `GET /api/projects/{project}/docs/{doc}/boards`: the boards that list a
/// project Doc (members).
pub async fn on_boards_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, doc)): Path<(String, String)>,
) -> J<wire::DocBoards> {
    Ok(Json(on_boards(&app, &s.account, &project, &doc).await?))
}
