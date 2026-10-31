//! Project Docs over the platform's routes (aru-118): a project's own
//! Docs ([`db::project_docs`]), each a title and an ordered list of
//! sections in Markdown, the shape of a machine's Doc. A Doc (`pdc_…`) is
//! a project-scoped platform object, here with every machine asleep, read
//! on the platform page at `/p/<project>/docs/<id>`. The wire is
//! [`arugula_control_wire::project_docs`].
//!
//! **Who may do what,** by membership of the Doc's project, as for issues:
//! - a personal project's account does everything; in a team's project,
//!   owners and editors make, write and delete Docs, viewers read them;
//! - an agent session ([`By::Session`], aru-115) acts with its asker's
//!   rights: it makes Docs when they may, and writes only the Docs it made
//!   (people who may write may write those too); it never deletes;
//! - anyone else gets 404, as if the project or Doc weren't there;
//! - nothing leaves the project: a Doc is named with its project in every
//!   path, and one in another project is no Doc at all.
//!
//! Native Docs come with chat, as native issues do (`chat_on`): their live
//! updates are chat's.
//!
//! **Live:** a write pushes an object change on the Doc's thread
//! ([`changed`]), as a board's does, so pages showing it read it again.
//!
//! [`db::project_docs`]: crate::db::project_docs

use std::sync::Arc;

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::{chat::ChatAgent, project_docs as wire};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::Person,
    chat::{CHAT_OFF, chat_on},
    db::project_docs::{self as db, Refused},
    issues::PERSONAL,
};

type J<T> = Result<Json<T>, ApiError>;

/// Who acts on a Doc.
#[derive(Debug, Clone, Copy)]
pub enum By<'a> {
    /// A person, by their account.
    Person(&'a str),
    /// An agent session, with its asker's rights, as itself.
    Session { asker: &'a str, session: &'a str },
}

impl By<'_> {
    /// Whose rights are checked.
    fn account(&self) -> &str {
        match self {
            By::Person(a) | By::Session { asker: a, .. } => a,
        }
    }
}

/// A project as someone acting on its Docs sees it.
struct Place {
    project: String,
    role: TeamRole,
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

fn no_doc() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such Doc")
}

fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `account` sees
/// it: 404 for a stranger; with `write`, the team's roster comes from the
/// hub, not the cache, and a viewer is refused.
async fn place(app: &Platform, account: &str, asked: &str, write: bool) -> Result<Place, ApiError> {
    if !chat_on(app, account).await? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            CHAT_OFF,
            "native Docs come with chat, which isn't on for you: a team's owners turn it on in control (Teams)",
        ));
    }
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
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads Docs, and doesn't make or write them"));
    }
    Ok(Place { project: project.id, role })
}

/// The Doc `id` in the project `asked` names, as `by` sees it: 404 unless
/// it's that project's and they may read it.
async fn doc_in(
    app: &Platform,
    by: By<'_>,
    asked: &str,
    id: &str,
    write: bool,
) -> Result<(Place, db::Stored), ApiError> {
    let place = place(app, by.account(), asked, write).await?;
    let doc = app.db.project_doc(id)?.filter(|d| d.project == place.project).ok_or_else(no_doc)?;
    Ok((place, doc))
}

/// Whether `by` may write `doc`: one who may make Docs, and a session only
/// its own.
fn may_write(by: By<'_>, place: &Place, doc: &db::Stored) -> Result<(), ApiError> {
    if place.role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer reads Docs, and doesn't make or write them"));
    }
    if let By::Session { session, .. } = by
        && doc.made_by_session.as_deref() != Some(session)
    {
        return Err(err(
            StatusCode::FORBIDDEN,
            "an agent session writes only the Docs it made: make a new one, or ask a person to change this one",
        ));
    }
    Ok(())
}

fn refused(r: Refused) -> ApiError {
    match r {
        Refused::Missing(what) => err(StatusCode::NOT_FOUND, what),
        Refused::Bad(why) => bad(&why),
    }
}

fn one_line(s: &str, what: &str, max: usize, empty: bool) -> Result<String, ApiError> {
    let s = s.trim();
    if !empty && s.is_empty() {
        return Err(bad(&format!("a {what} can't be empty")));
    }
    if s.chars().any(char::is_control) {
        return Err(bad(&format!("a {what} is one line, without control characters")));
    }
    if s.chars().count() > max {
        return Err(bad(&format!("a {what} is at most {max} characters")));
    }
    Ok(s.to_owned())
}

fn title_ok(t: &str) -> Result<String, ApiError> {
    one_line(t, "title", wire::TITLE_MAX, false)
}

fn heading_ok(h: &str) -> Result<String, ApiError> {
    one_line(h, "heading", wire::HEADING_MAX, false)
}

fn intent_ok(i: &str) -> Result<String, ApiError> {
    one_line(i, "intent", wire::INTENT_MAX, true)
}

/// A Doc's id: `pdc_` and 32 lower-case hex digits.
fn id_ok(id: &str) -> Result<(), ApiError> {
    match id.strip_prefix("pdc_") {
        Some(h) if h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => Ok(()),
        _ => Err(no_doc()),
    }
}

fn doc_wire(d: &db::Stored) -> wire::Doc {
    wire::Doc {
        id: d.id.clone(),
        project: d.project.clone(),
        title: d.title.clone(),
        author: d.made_by.clone(),
        agent: d.made_by_session.as_ref().map(|s| ChatAgent {
            machine: s.clone(),
            machine_name: crate::issues::session_name(s),
            session: None,
        }),
        sections: d.sections,
        written: d.written,
        rev: d.rev,
        created_at: d.created_at,
        updated_at: d.updated_at,
    }
}

fn view(app: &Platform, by: By<'_>, place: &Place, d: &db::Stored) -> Result<wire::DocView, ApiError> {
    let sections = app
        .db
        .project_doc_sections(&d.id)?
        .into_iter()
        .map(|s| wire::Section { id: s.id, heading: s.heading, intent: s.intent, body: s.body, rev: s.rev })
        .collect();
    Ok(wire::DocView { doc: doc_wire(d), sections, writer: may_write(by, place, d).is_ok() })
}

/// The Doc `id`, in `project`, changed: pages showing it read it again,
/// through its thread's readers (as a board's).
pub fn changed(app: &Platform, project: &str, id: &str) {
    app.chat.object_changed(&app.db, &format!("project:{project}"), id);
}

/// The Doc's page on the platform's page.
pub fn link(app: &Platform, project: &str, id: &str) -> String {
    format!("{}/p/{project}/docs/{id}", app.cfg.page())
}

// ---------------------------------------------------------------- the seam

/// A project's Docs as `by` may read them, the latest changed first.
pub async fn list(app: &Platform, by: By<'_>, project: &str) -> Result<wire::DocList, ApiError> {
    let place = place(app, by.account(), project, false).await?;
    let docs = app.db.project_docs(&place.project)?.iter().map(doc_wire).collect();
    Ok(wire::DocList { writer: place.role != TeamRole::Viewer, project: place.project, docs })
}

/// Make a Doc in `project` as `by`.
pub async fn create(app: &Platform, by: By<'_>, project: &str, new: &wire::NewDoc) -> Result<wire::DocView, ApiError> {
    let place = place(app, by.account(), project, true).await?;
    let title = title_ok(&new.title)?;
    let mut kept = Vec::with_capacity(new.sections.len());
    for s in &new.sections {
        kept.push((heading_ok(&s.heading)?, intent_ok(&s.intent)?, s.markdown.as_deref()));
    }
    let sections: Vec<db::NewSection> =
        kept.iter().map(|(h, i, b)| db::NewSection { heading: h, intent: i, body: *b }).collect();
    let (made_by, made_by_session) = match by {
        By::Person(a) => (a, None),
        By::Session { session, .. } => ("", Some(session)),
    };
    let n = db::NewDoc { project: &place.project, title: &title, made_by, made_by_session, sections: &sections };
    let doc = app.db.create_project_doc(&n, now_ms(), wire::DOCS_MAX)?.map_err(refused)?;
    view(app, by, &place, &doc)
}

/// A Doc with its sections, as `by` may read it.
pub async fn get(app: &Platform, by: By<'_>, project: &str, id: &str) -> Result<wire::DocView, ApiError> {
    id_ok(id)?;
    let (place, doc) = doc_in(app, by, project, id, false).await?;
    view(app, by, &place, &doc)
}

/// Change a Doc as `by` (see [`wire::DocWrite`]).
pub async fn write(
    app: &Platform,
    by: By<'_>,
    project: &str,
    id: &str,
    w: &wire::DocWrite,
) -> Result<wire::DocView, ApiError> {
    id_ok(id)?;
    let (place, doc) = doc_in(app, by, project, id, true).await?;
    may_write(by, &place, &doc)?;
    let title = w.title.as_deref().map(title_ok).transpose()?;
    let heading = w.heading.as_deref().map(heading_ok).transpose()?;
    let intent = w.intent.as_deref().map(intent_ok).transpose()?;
    let write = db::Write {
        title: title.as_deref(),
        section: w.section.as_deref(),
        heading: heading.as_deref(),
        intent: intent.as_deref(),
        body: w.markdown.as_deref(),
    };
    let doc = app.db.write_project_doc(id, &write, now_ms())?.map_err(refused)?;
    changed(app, &place.project, id);
    view(app, by, &place, &doc)
}

/// Delete a Doc: owners and editors, never a session.
pub async fn delete(app: &Platform, account: &str, project: &str, id: &str) -> Result<(), ApiError> {
    id_ok(id)?;
    let by = By::Person(account);
    let (place, _) = doc_in(app, by, project, id, true).await?;
    app.db.delete_project_doc(id)?;
    changed(app, &place.project, id);
    Ok(())
}

// ---------------------------------------------------------------- routes

/// `GET /api/projects/{project}/docs`: a project's Docs, the latest changed
/// first.
pub async fn list_route(State(app): State<Arc<Platform>>, s: Person, Path(project): Path<String>) -> J<wire::DocList> {
    Ok(Json(list(&app, By::Person(&s.account), &project).await?))
}

/// `POST /api/projects/{project}/docs`: make one (owners and editors).
pub async fn create_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(project): Path<String>,
    Json(new): Json<wire::NewDoc>,
) -> J<wire::DocView> {
    Ok(Json(create(&app, By::Person(&s.account), &project, &new).await?))
}

/// `GET /api/projects/{project}/docs/{id}`: the Doc and its sections.
pub async fn get_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
) -> J<wire::DocView> {
    Ok(Json(get(&app, By::Person(&s.account), &project, &id).await?))
}

/// `PUT /api/projects/{project}/docs/{id}`: rename it, or write or add a
/// section (owners and editors).
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
    Json(w): Json<wire::DocWrite>,
) -> J<wire::DocView> {
    Ok(Json(write(&app, By::Person(&s.account), &project, &id, &w).await?))
}

/// `DELETE /api/projects/{project}/docs/{id}` (owners and editors).
pub async fn delete_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
) -> J<Value> {
    delete(&app, &s.account, &project, &id).await?;
    Ok(Json(json!({ "deleted": id })))
}

#[cfg(test)]
#[path = "project_docs_tests.rs"]
mod tests;
