//! A board's Docs (Arugula #517): a strip of Docs under the goal, and Docs
//! on one row (`board_docs`, `row_id`). A Doc is a project Doc of the board's own project (`pdc_…`), or a Doc
//! block on a machine, which an agent of that machine links
//! (`MachineDoc`: the title is a snapshot, since the machine may be off).
//! A machine Doc whose copy (#418) sits in the board's project is read
//! from the copy instead (native #673): the title and sections as the
//! machine last put them, and its `pdc_` id (`copy`), so the chip opens it
//! while the machine is away. A copy anywhere else lends nothing: the
//! board's readers may not read that project, so the chip keeps the title
//! it was linked with. With the move route gone, a copy sits in its
//! owner's personal project, so that is the owner's own boards.
//!
//! **Who may:** whoever writes the board (owners and editors, and an
//! agent with `boards.write`, who takes off only the Docs its own machine
//! linked and links only its own machine's Docs). A person links a
//! machine's Doc only from a machine control routes them to (their own, a
//! team's, one shared with them: the hub's `may_reach`). A Doc of another
//! project is no Doc: 404, checked here and not in the page. A Doc linked
//! already answers the board as it is. A closed board's Docs stay
//! editable, as its rows do.
//!
//! **Whose machine it names:** a machine Doc an agent linked names its
//! machine while that machine holds `boards.write` over the project, as
//! its blocks' states show while it does. One a person linked names the
//! machine only to a reader who may reach it. To anyone else the Doc is on
//! "a machine", with the machine's id blanked too, so a link made before
//! the check, or by someone the project has since lost, names nothing to
//! those it doesn't concern. That is a label, not a check: it waits on the
//! hub [`super::LABEL_WAIT`] at most, and on an error says "a machine" and goes on.
//!
//! **What a Doc is not:** a row's Doc is no attachment: nothing that works
//! out a lane, the ready set, progress or a proof ever reads it.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

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
    db::{boards_docs as db, issues::Actor},
};

/// 404: no such Doc in this project.
fn no_doc() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such Doc")
}

/// A Doc pointer as it is kept: shaped, with a project Doc checked to be in
/// `project` and a machine Doc's title trimmed. A machine's agent links
/// only a Doc of its own machine; a person, only one on a machine they may
/// reach (a "no" is never kept, so a machine just joined or shared is asked
/// of the hub again).
pub(super) async fn doc_ok(
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
            match by.agent {
                Some(own) if own != machine => {
                    return Err(err(StatusCode::FORBIDDEN, "an agent links only Docs on its own machine"));
                }
                Some(_) => {}
                None => {
                    let asked = std::slice::from_ref(machine);
                    if !app.cache.may_reach(&*app.hub, by.account, asked).await?.contains(machine) {
                        return Err(err(StatusCode::FORBIDDEN, "a person links only Docs on a machine they can reach"));
                    }
                }
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

/// Which machine Docs a read names to its reader (the module's top says
/// which): the machines the reader may reach, and the machines whose agent
/// writes the project now.
#[derive(Default)]
pub(super) struct Named {
    reach: HashSet<String>,
    writing: HashSet<String>,
}

impl Named {
    fn shows(&self, machine: &str, by_agent: bool) -> bool {
        self.reach.contains(machine) || (by_agent && self.writing.contains(machine))
    }
}

/// What `account` is told of the machines of `docs` (the Docs of a board,
/// strip and rows): one hub call for the reach, kept a tick ("no" too),
/// within [`super::LABEL_WAIT`]; an error or a wait past it names nothing, with a
/// warning, rather than failing the read.
pub(super) async fn named(app: &Platform, account: &str, project: &str, docs: &[db::Strip]) -> Named {
    let (mut linked, mut by_agent) = (vec![], vec![]);
    for d in docs {
        if let wire::Pointer::MachineDoc { machine, .. } = &d.pointer {
            if d.agent.is_some() { &mut by_agent } else { &mut linked }.push(machine.clone());
        }
    }
    linked.sort();
    linked.dedup();
    by_agent.sort();
    by_agent.dedup();
    let mut named =
        Named { reach: super::reach_label(app, account, &linked, "a Doc's machine").await, writing: HashSet::new() };
    for m in by_agent {
        if matches!(super::machine_writes(app, &m, project).await, Ok(true)) {
            named.writing.insert(m);
        }
    }
    named
}

/// The machine Docs' copies in the board's project, by machine and the
/// machine's Doc id, for [`view`].
pub(super) type Copies = HashMap<(String, String), crate::db::project_docs::Stored>;

/// One read of the project's Docs, kept by key: a board's Docs are looked
/// up in it, however many there are.
pub(super) fn copies_in(app: &Platform, project: &str) -> Result<Copies, ApiError> {
    Ok(app
        .db
        .project_docs(project)?
        .into_iter()
        .filter_map(|d| Some(((d.machine.clone()?, d.machine_doc.clone()?), d)))
        .collect())
}

/// The Docs of a strip or a row as a read answers them: a project Doc by its
/// title and sections now (one that is gone says so); a machine Doc by its
/// copy in this project (`copies`) when it has one, else the title it had,
/// and by its machine's name, or on "a machine" with its id blanked when the
/// reader isn't told (`named`). The copy is looked up before the id is
/// blanked: a reader not told the machine still reads a copy in the board's
/// project, where they may read. `names` are the machines' names by id.
pub(super) fn view(
    docs: Vec<db::Strip>,
    names: &HashMap<String, String>,
    named: &Named,
    copies: &Copies,
) -> Vec<wire::BoardDoc> {
    let name = |m: &str| names.get(m).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| "a machine".to_owned());
    docs.into_iter()
        .map(|s| {
            let mut d = wire::BoardDoc {
                id: s.id,
                title: s.title,
                sections: None,
                written: None,
                missing: false,
                machine_name: None,
                copy: None,
                added_by: s.added_by,
                agent: s.agent.map(|m| arugula_control_wire::chat::ChatAgent {
                    machine_name: names.get(&m).cloned().unwrap_or_default(),
                    machine: m,
                    session: None,
                }),
                created_at: s.created_at,
                pointer: s.pointer,
            };
            if let wire::Pointer::MachineDoc { machine, doc, .. } = &mut d.pointer {
                // The copy first: the machine's id may be blanked below.
                if let Some(c) = copies.get(&(machine.clone(), doc.clone())) {
                    (d.title, d.sections, d.written) = (c.title.clone(), Some(c.sections), Some(c.written));
                    d.copy = Some(c.id.clone());
                }
                if named.shows(machine, d.agent.is_some()) {
                    d.machine_name = Some(name(machine));
                } else {
                    d.machine_name = Some("a machine".to_owned());
                    machine.clear();
                    if let Some(a) = &mut d.agent {
                        a.machine.clear();
                        a.machine_name.clear();
                    }
                }
            } else {
                match s.head {
                    Some(h) => (d.title, d.sections, d.written) = (h.title, Some(h.sections), Some(h.written)),
                    None => (d.title, d.missing) = (wire::DOC_GONE.to_owned(), true),
                }
            }
            d
        })
        .collect()
}

/// Link a Doc to the board's strip, or with `a.row` to that row, last. One
/// linked there already is not linked again.
pub async fn add(app: &Platform, by: Actor<'_>, id: &str, a: &wire::AddDoc) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    if let Some(row) = &a.row {
        id_ok(row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    }
    let pointer = doc_ok(app, by, &board.project, &a.pointer).await?;
    let said = match app.db.add_board_doc(id, a.row.as_deref(), &pointer, by.account, by.agent, now_ms())? {
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
/// them.
pub async fn on_boards(app: &Platform, account: &str, project: &str, doc: &str) -> Result<wire::DocBoards, ApiError> {
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
