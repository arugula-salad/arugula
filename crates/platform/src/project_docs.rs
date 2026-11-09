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
//! Membership and roles alone say who reads and writes a project's Docs,
//! as for native issues (Arugula #746): a person's chat setting doesn't.
//! Chat still carries their live updates, so a page whose reader has chat
//! off reads a Doc again when it opens it.
//!
//! **Pictures** (#418): members read a Doc's pictures ([`asset_route`]). A
//! machine copies its Docs up to its owner's personal project in
//! [`machine`](crate::project_docs_machine), behind `Gate::Copies`, and
//! they stay there: the route that moved a Doc between projects went with
//! native #673 (only tests called it). A copy moved before then sits where it was put, and
//! the machine's puts keep landing there while its owner may write there.
//!
//! **A copy is read-only** (Arugula #678): the machine's library Doc is the
//! truth, and its next put replaces the copy whole, so a write to a copy
//! here, by a person or a session, is refused naming the machine
//! ([`copy_refused`]), and a copy's view has no writer. The machine's own
//! puts are as they were. A copy goes with its Doc (native #673): the
//! machine takes it down when the Doc is deleted there. And a copy is its
//! project's to delete like any Doc (`deleter`, for the page): the
//! machine's next change makes a new copy.
//!
//! **Share with…** makes a team Doc from a copy ([`share`]): a new,
//! ordinary Doc in a project the copy's owner writes in, from the copy's
//! title, sections and pictures as they are, which the team owns and edits.
//! It records where it came from and at which revs, and **Update copy**
//! ([`update`]) replaces it from the copy again when its owner asks,
//! refusing first if the team edited it since. Only the copy's owner does
//! either, as a person ([`own_copy`]: reading a copy is allowed to whoever
//! reads its project, and that isn't the question); nothing is live, and a
//! machine never writes to a team's project.
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
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse as _, Response},
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
pub(crate) struct Place {
    pub project: String,
    pub role: TeamRole,
}

fn no_project() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such project")
}

pub(crate) fn no_doc() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such Doc")
}

pub(crate) fn bad(msg: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// What lets a machine's copies at a project's Docs, besides its owner
/// being in the project. People and sessions need nothing more
/// ([`Gate::None`], Arugula #746).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Chat on for the machine's owner: its copy that was moved into a
    /// team's project, where the account's word for copies doesn't reach.
    Chat,
    /// The account's own word for its machines' copies (native #673), and
    /// without one, chat (an older daemon's copies): a machine's puts. An
    /// explicit no is refused whatever chat says.
    Copies,
    /// Nothing more: a person or a session, and a machine taking its own
    /// copy down, whatever chat or the word say.
    None,
}

/// The project `asked` names (its id, or [`PERSONAL`]) as `account` sees
/// it: 404 for a stranger; with `write`, the team's roster comes from the
/// hub, not the cache, and a viewer is refused.
pub(crate) async fn place(app: &Platform, account: &str, asked: &str, write: bool) -> Result<Place, ApiError> {
    place_gated(app, account, asked, write, Gate::None).await
}

/// [`place`], behind `gate`.
pub(crate) async fn place_gated(
    app: &Platform,
    account: &str,
    asked: &str,
    write: bool,
    gate: Gate,
) -> Result<Place, ApiError> {
    let open = match gate {
        Gate::None => true,
        Gate::Chat => chat_on(app, account).await?,
        // The account's own word; without one, chat (an older daemon's
        // copies). An explicit no wins over chat.
        Gate::Copies => match crate::chat::copies_word(app, account).await? {
            Some(true) => true,
            Some(false) => {
                return Err(err(
                    StatusCode::FORBIDDEN,
                    "this account turned copying its machines' Docs to the platform off (Your Docs on the platform, \
                     under Teams)",
                ));
            }
            None => chat_on(app, account).await?,
        },
    };
    if !open {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            CHAT_OFF,
            "this machine copies its Docs to the platform once its account turns that on (Your Docs on the platform, \
             under Teams), or while chat is on for it",
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

/// Why a copy isn't written here: the machine (by name, or its id once it
/// left) holds the Doc.
fn copy_refused(doc: &db::Stored, names: &Names) -> ApiError {
    let machine = doc.machine.as_deref().unwrap_or_default();
    let name = names.get(machine).filter(|n| !n.is_empty()).map_or(machine, String::as_str);
    err(
        StatusCode::FORBIDDEN,
        &format!(
            "this Doc is a copy of one in {name}'s library, and the copy is read-only: change it on {name}, and its \
             next copy brings the change"
        ),
    )
}

pub(crate) fn refused(r: Refused) -> ApiError {
    match r {
        Refused::Missing(what) => err(StatusCode::NOT_FOUND, what),
        Refused::Bad(why) => bad(&why),
        Refused::Edited => {
            err(StatusCode::CONFLICT, "the team edited this Doc since; Update copy replaces their edits")
        }
        Refused::Made(_) => err(StatusCode::CONFLICT, "a Doc was made from this copy in that project already"),
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

pub(crate) fn title_ok(t: &str) -> Result<String, ApiError> {
    one_line(t, "title", wire::TITLE_MAX, false)
}

pub(crate) fn heading_ok(h: &str) -> Result<String, ApiError> {
    one_line(h, "heading", wire::HEADING_MAX, false)
}

pub(crate) fn intent_ok(i: &str) -> Result<String, ApiError> {
    one_line(i, "intent", wire::INTENT_MAX, true)
}

/// A Doc's id: `pdc_` and 32 lower-case hex digits.
fn id_ok(id: &str) -> Result<(), ApiError> {
    match id.strip_prefix("pdc_") {
        Some(h) if h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => Ok(()),
        _ => Err(no_doc()),
    }
}

/// Machines' names by id, for the copies among some Docs.
type Names = std::collections::HashMap<String, String>;

/// The names of the machines these Docs are copies from (none for one
/// control doesn't know any more).
async fn machine_names<'a>(app: &Platform, docs: impl Iterator<Item = &'a db::Stored>) -> anyhow::Result<Names> {
    let ids: Vec<String> =
        docs.filter_map(|d| d.machine.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    if ids.is_empty() {
        return Ok(Names::new());
    }
    Ok(crate::grants::agent_names(app, &ids).await?.into_iter().map(|(id, d)| (id, d.name)).collect())
}

pub(crate) fn doc_wire(d: &db::Stored, names: &Names) -> wire::Doc {
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
        machine: d.machine.clone(),
        machine_name: d.machine.as_ref().and_then(|m| names.get(m)).filter(|n| !n.is_empty()).cloned(),
        made_from: d.made_from.as_ref().map(|m| wire::MadeFrom {
            project: m.project.clone(),
            id: m.doc.clone(),
            machine_name: None,
        }),
    }
}

async fn view(app: &Platform, by: By<'_>, place: &Place, d: &db::Stored) -> Result<wire::DocView, ApiError> {
    let names = machine_names(app, std::iter::once(d)).await?;
    let sections = app
        .db
        .project_doc_sections(&d.id)?
        .into_iter()
        .map(|s| wire::Section { id: s.id, heading: s.heading, intent: s.intent, body: s.body, rev: s.rev })
        .collect();
    let assets = app
        .db
        .project_doc_assets(&d.id)?
        .into_iter()
        .map(|a| wire::Asset { name: a.name, mime: a.mime, size: a.size, rev: a.rev })
        .collect();
    // A copy is nobody's to write here (#678), and anyone's who writes in
    // its project to delete (native #673); a session never deletes.
    let writer = d.machine.is_none() && may_write(by, place, d).is_ok();
    let deleter = matches!(by, By::Person(_)) && place.role != TeamRole::Viewer;
    let mut doc = doc_wire(d, &names);
    // A team Doc says which machine its copy was from: the one name the
    // platform has, so the page looks nothing up.
    if let (Some(from), Some(m)) = (&d.made_from, &mut doc.made_from)
        && let Some(src) = app.db.project_doc(&from.doc)?
    {
        let names = machine_names(app, std::iter::once(&src)).await?;
        m.machine_name = src.machine.as_ref().and_then(|m| names.get(m)).filter(|n| !n.is_empty()).cloned();
    }
    Ok(wire::DocView { doc, sections, assets, writer, deleter })
}

/// The Doc `id`, in `project`, changed: pages showing it read it again,
/// through its thread's readers (as a board's).
pub fn changed(app: &Platform, project: &str, id: &str) {
    app.chat.object_changed(&app.db, &format!("project:{project}"), id);
    // The boards that list it (#517) show its title and sections.
    match app.db.boards_on_doc(id) {
        Ok(boards) => crate::boards::boards_changed(app, &boards),
        Err(e) => tracing::warn!(error = %e, "project docs: finding the boards that list a Doc"),
    }
}

/// The Doc's page on the platform's page.
pub fn link(app: &Platform, project: &str, id: &str) -> String {
    format!("{}/p/{project}/docs/{id}", app.cfg.page())
}

// ---------------------------------------------------------------- the seam

/// A project's Docs as `by` may read them, the latest changed first.
pub async fn list(app: &Platform, by: By<'_>, project: &str) -> Result<wire::DocList, ApiError> {
    let place = place(app, by.account(), project, false).await?;
    let docs = app.db.project_docs(&place.project)?;
    let names = machine_names(app, docs.iter()).await?;
    let docs = docs.iter().map(|d| doc_wire(d, &names)).collect();
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
    view(app, by, &place, &doc).await
}

/// A Doc with its sections, as `by` may read it.
pub async fn get(app: &Platform, by: By<'_>, project: &str, id: &str) -> Result<wire::DocView, ApiError> {
    id_ok(id)?;
    let (place, doc) = doc_in(app, by, project, id, false).await?;
    view(app, by, &place, &doc).await
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
    if doc.machine.is_some() {
        return Err(copy_refused(&doc, &machine_names(app, std::iter::once(&doc)).await?));
    }
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
    view(app, by, &place, &doc).await
}

/// A picture of a Doc, its type and bytes, as `by` may read the Doc: 404
/// for a stranger, as for the Doc.
pub async fn asset(
    app: &Platform,
    by: By<'_>,
    project: &str,
    id: &str,
    name: &str,
) -> Result<(String, Vec<u8>), ApiError> {
    id_ok(id)?;
    let (_, doc) = doc_in(app, by, project, id, false).await?;
    app.db.project_doc_asset(&doc.id, name)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such picture"))
}

/// Delete a Doc: owners and editors, never a session. A copy too: the
/// machine's next change to the Doc makes a new copy, and the page says so
/// (native #673).
pub async fn delete(app: &Platform, account: &str, project: &str, id: &str) -> Result<(), ApiError> {
    id_ok(id)?;
    let by = By::Person(account);
    let (place, _) = doc_in(app, by, project, id, true).await?;
    app.db.delete_project_doc(id)?;
    changed(app, &place.project, id);
    Ok(())
}

// ------------------------------------------------------- Share with…

/// The copy `id` in `project`, for its owner to share: a copy (a Doc a
/// machine put) in `account`'s own personal project. Anyone else, whatever
/// they read or write elsewhere, is refused: 404 for what they can't see,
/// 403 for a copy that sits in a team's project (one moved there before
/// native #673) or isn't a copy's to share.
async fn own_copy(app: &Platform, account: &str, project: &str, id: &str) -> Result<db::Stored, ApiError> {
    id_ok(id)?;
    let (place, doc) = doc_in(app, By::Person(account), project, id, false).await?;
    if doc.machine.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "this Doc isn't a copy from a machine"));
    }
    let mine = app
        .db
        .issue_project(&place.project)?
        .is_some_and(|p| p.team.is_none() && p.account.as_deref() == Some(account));
    if !mine {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the person whose machine made a copy shares it, from their own project",
        ));
    }
    Ok(doc)
}

/// A team Doc made from `src`, as its owner sees it.
fn copy_of(app: &Platform, src: &db::Stored, d: &db::Stored, made: &db::MadeFrom) -> Result<wire::DocCopy, ApiError> {
    let name = app.db.issue_project(&d.project)?.map(|p| p.name).unwrap_or_default();
    Ok(wire::DocCopy {
        project: d.project.clone(),
        project_name: name,
        id: d.id.clone(),
        link: link(app, &d.project, &d.id),
        made: d.created_at,
        updated: d.updated_at,
        stale: src.rev > made.rev,
        edited: d.rev > made.made_rev,
    })
}

/// Share with…: `account` makes a team Doc in `to` from their copy. `Err`
/// of the id when one was made there already.
pub async fn share(
    app: &Platform,
    account: &str,
    project: &str,
    id: &str,
    to: &str,
) -> Result<Result<wire::DocView, String>, ApiError> {
    let src = own_copy(app, account, project, id).await?;
    let by = By::Person(account);
    let place = place(app, account, to, true).await?;
    if place.project == src.project {
        return Err(bad("a copy is in your personal project already: share it with a team's"));
    }
    let made = app.db.snapshot_project_doc(&src.id, &place.project, account, now_ms(), wire::DOCS_MAX)?;
    let doc = match made {
        Ok(d) => d,
        Err(db::Refused::Made(id)) => return Ok(Err(id)),
        Err(r) => return Err(refused(r)),
    };
    changed(app, &place.project, &doc.id);
    Ok(Ok(view(app, by, &place, &doc).await?))
}

/// The team Docs made from the copy `id`, in the projects `account` still
/// reads.
pub async fn copies(app: &Platform, account: &str, project: &str, id: &str) -> Result<wire::DocCopies, ApiError> {
    let src = own_copy(app, account, project, id).await?;
    let mut copies = Vec::new();
    for d in app.db.project_doc_snapshots(&src.id)? {
        let Some(made) = &d.made_from else { continue };
        match place(app, account, &d.project, false).await {
            Ok(_) => copies.push(copy_of(app, &src, &d, made)?),
            // A project they left isn't theirs to hear of.
            Err(e) if e.0 == StatusCode::NOT_FOUND => {}
            Err(e) => return Err(e),
        }
    }
    Ok(wire::DocCopies { copies })
}

/// Update copy: the team Doc in `to` made from the copy `id` takes the
/// copy's title, sections and pictures as they are now. `Err` when the team
/// edited it since and `replace` isn't set.
pub async fn update(
    app: &Platform,
    account: &str,
    project: &str,
    id: &str,
    to: &str,
    replace: bool,
) -> Result<Result<wire::DocCopy, ()>, ApiError> {
    let src = own_copy(app, account, project, id).await?;
    // Still writing there, from the hub's roster: someone removed from
    // the project can't change its Docs by way of one they made.
    let place = place(app, account, to, true).await?;
    let team = app.db.project_doc_snapshot(&src.id, &place.project)?.ok_or_else(no_doc)?;
    team.made_from.as_ref().ok_or_else(no_doc)?;
    // Whether the team edited it is read again under the Doc's lock.
    let team = match app.db.refresh_project_doc_snapshot(&team.id, now_ms(), replace)? {
        Ok(t) => t,
        Err(db::Refused::Edited) => return Ok(Err(())),
        Err(r) => return Err(refused(r)),
    };
    changed(app, &place.project, &team.id);
    let src = app.db.project_doc(&src.id)?.ok_or_else(no_doc)?;
    let made = team.made_from.as_ref().ok_or_else(no_doc)?;
    Ok(Ok(copy_of(app, &src, &team, made)?))
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

/// `GET /api/projects/{project}/docs/{id}/assets/{name}`: a picture, shown
/// inline and never run (`nosniff`, a CSP that allows nothing).
pub async fn asset_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id, name)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let (mime, bytes) = asset(&app, By::Person(&s.account), &project, &id, &name).await?;
    let mut r = bytes.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).map_err(|_| no_doc())?);
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    h.insert("cross-origin-resource-policy", HeaderValue::from_static("same-origin"));
    Ok(r)
}

/// `POST /api/projects/{project}/docs/{id}/copies`: Share with… (the copy's
/// owner); 409 with the team Doc's id when one is there already.
pub async fn share_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
    Json(body): Json<wire::ShareCopy>,
) -> Result<Response, ApiError> {
    Ok(match share(&app, &s.account, &project, &id, &body.to).await? {
        Ok(v) => Json(v).into_response(),
        Err(id) => {
            let error = "a Doc was made from this copy in that project already; Update copy refreshes it".into();
            (StatusCode::CONFLICT, Json(wire::Shared { error, id })).into_response()
        }
    })
}

/// `GET /api/projects/{project}/docs/{id}/copies`: the team Docs made from
/// the copy (its owner).
pub async fn copies_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id)): Path<(String, String)>,
) -> J<wire::DocCopies> {
    Ok(Json(copies(&app, &s.account, &project, &id).await?))
}

/// `PUT /api/projects/{project}/docs/{id}/copies/{to}`: Update copy (the
/// copy's owner, who still writes in `to`); 409 `edited` when the team
/// edited it since and `replace` isn't set.
pub async fn update_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((project, id, to)): Path<(String, String, String)>,
    Json(body): Json<wire::UpdateCopy>,
) -> Result<Response, ApiError> {
    Ok(match update(&app, &s.account, &project, &id, &to, body.replace).await? {
        Ok(c) => Json(c).into_response(),
        Err(()) => {
            let error = "the team edited its copy since; Update copy replaces their edits".into();
            (StatusCode::CONFLICT, Json(wire::Edited { error, edited: true })).into_response()
        }
    })
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
