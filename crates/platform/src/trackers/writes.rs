//! Writing to a tracker (Arugula #359): a person, or an agent, creates,
//! comments on, closes, reopens and edits a Plane work item from Arugula.
//!
//! - **A person's write** (`POST …/trackers/{connection}/writes`, an owner
//!   or editor) lands at once.
//! - **An agent's write** (`POST /api/daemon/projects/{project}/tracker-writes`)
//!   needs the machine's `trackers.write` over the project, asked on every
//!   call and never past its owner's role. Then the connection's
//!   `agent_writes` says: `allow` sends it at once, `approve` (the default)
//!   keeps it as a `tracker_writes` row that waits for a person.
//! - **Approving is a person's.** Approve, edit and drop are routes for an
//!   owner or editor signed in as themselves; no machine route reaches them.
//!   An approved write is sent as that agent's.
//! - **The footer** is added when a write is *sent*, never stored: a body or
//!   comment ends with a blank line and `— via Arugula, by <person>` (for an
//!   agent's, `<person>'s agent on <machine>`), so an edit can't take it away.
//!   Bodies and comments go to Plane as escaped HTML; a title is plain text.
//! - **After a write** the item is read back and written to the mirror
//!   ([`super::connect::ingest_page`]), and the boards that hold it are told.
//! - **The key** is opened for the call into a local `String`; an error says
//!   a plain sentence ([`TrackerError`]), never Plane's answer.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{
    TrackerClose, TrackerPendingWrite, TrackerPendingWrites, TrackerReach, TrackerReachProject, TrackerWrite,
    TrackerWriteDone,
};
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::{info, warn};

use super::{Edit, NewItem, TrackerError, TrackerProvider, connect::ingest_page};
use crate::{
    Platform,
    auth::{Machine, Person},
    chat::name_of,
    db::{
        projects::TRACKERS_WRITE,
        trackers::{Connection, PendingWrite, Target, key_name},
    },
    forge::links::membership,
    grants::{AgentScope, agent_label, agent_may, agent_names, owner_reads},
    keys,
};
use arugula_e2e::team::TeamRole;

/// Longest title Plane keeps.
const TITLE_MAX: usize = 255;
/// Longest body or comment.
const TEXT_MAX: usize = 20_000;
/// Writes a machine's agents make, per hour.
const AGENT_WRITES: (&str, usize) = ("tracker-agent-write", 300);
/// Reads of what a machine reaches, per hour.
const AGENT_READS: (&str, usize) = ("tracker-agent-read", 3_600);

/// Text as HTML for Plane: each paragraph (split at a blank line) in `<p>`,
/// a line break within one as `<br>`, and everything else escaped.
pub fn html(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let esc = p
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;");
            format!("<p>{}</p>", esc.replace('\n', "<br>"))
        })
        .collect()
}

/// `text`, a blank line, and the footer.
fn with_footer(text: &str, footer: &str) -> String {
    let text = text.trim();
    if text.is_empty() { footer.to_owned() } else { format!("{text}\n\n{footer}") }
}

/// Who a write was for.
#[derive(Debug, Clone)]
pub struct Who {
    /// The person's name.
    pub person: String,
    /// Their agent's machine (id, name), for an agent's write.
    pub agent: Option<(String, String)>,
}

impl Who {
    /// `— via Arugula, by Ada` or `— via Arugula, by Ada's agent on geek`.
    fn footer(&self) -> String {
        match &self.agent {
            None => format!("— via Arugula, by {}", self.person),
            Some((id, name)) => format!("— via Arugula, by {}", agent_label(&self.person, id, name)),
        }
    }
}

async fn person_name(app: &Platform, account: &str) -> Result<String, ApiError> {
    let n = name_of(app, account).await?;
    Ok(if n.is_empty() { account.to_owned() } else { n })
}

async fn who_agent(app: &Platform, account: &str, machine: &str) -> Result<Who, ApiError> {
    let names = agent_names(app, &[machine.to_owned()]).await?;
    let name = names.get(machine).map_or_else(|| machine.to_owned(), |d| d.name.clone());
    Ok(Who { person: person_name(app, account).await?, agent: Some((machine.to_owned(), name)) })
}

fn bad(why: &str) -> ApiError {
    err(StatusCode::UNPROCESSABLE_ENTITY, why)
}

/// A title is one line.
fn title_ok(t: &str) -> Result<String, ApiError> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > TITLE_MAX || t.chars().any(char::is_control) {
        return Err(bad("a title is one line of 1 to 255 characters"));
    }
    Ok(t.to_owned())
}

fn text_ok(what: &str, t: &str, may_be_empty: bool) -> Result<String, ApiError> {
    let t = t.trim();
    if (t.is_empty() && !may_be_empty) || t.len() > TEXT_MAX {
        return Err(bad(&format!("{what} is 1 to {TEXT_MAX} characters")));
    }
    Ok(t.to_owned())
}

fn tracker_error(e: TrackerError) -> ApiError {
    match e {
        TrackerError::Unauthorized => err(
            StatusCode::BAD_GATEWAY,
            "Plane refused this connection's API key: connect the project again with a new one",
        ),
        TrackerError::NotFound => err(StatusCode::NOT_FOUND, "Plane has no such work item or project any more"),
        e => err(StatusCode::BAD_GATEWAY, &format!("couldn't write to Plane: {e}")),
    }
}

/// The write as it will be kept and sent: texts checked, an item named by
/// its `fi_` id, and the live connection it goes to (which, when
/// `connection` is given, must be that one).
async fn resolve(
    app: &Platform,
    project: &str,
    connection: Option<&str>,
    w: &TrackerWrite,
) -> Result<(Connection, TrackerWrite), ApiError> {
    let live = |id: &str| -> Result<Connection, ApiError> {
        app.db
            .tracker_connection(project, id)?
            .filter(|c| c.unlinked_at.is_none())
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project"))
    };
    let target = |item: &str| -> Result<Target, ApiError> {
        let t = app
            .db
            .tracker_target(project, item.trim())?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such Plane work item in this project"))?;
        if connection.is_some_and(|c| c != t.connection_id) {
            return Err(bad("that work item is in another Plane connection"));
        }
        Ok(t)
    };
    Ok(match w {
        TrackerWrite::Create { connection: own, title, body, state } => {
            let id = match (connection, own.as_deref()) {
                (Some(a), Some(b)) if a != b => return Err(bad("connection: that isn't the one in the address")),
                (Some(a), _) | (None, Some(a)) => a,
                (None, None) => return Err(bad("a new item needs the connection to create it in")),
            };
            let state = match state.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None => None,
                Some(s @ ("backlog" | "unstarted" | "started")) => Some(s.to_owned()),
                Some(_) => return Err(bad("state: backlog, unstarted or started")),
            };
            let body = body.as_deref().map(|b| text_ok("a body", b, true)).transpose()?.filter(|b| !b.is_empty());
            let c = live(id)?;
            (c.clone(), TrackerWrite::Create { connection: Some(c.id), title: title_ok(title)?, body, state })
        }
        TrackerWrite::Comment { item, text } => {
            let t = target(item)?;
            (live(&t.connection_id)?, TrackerWrite::Comment { item: t.id, text: text_ok("a comment", text, false)? })
        }
        TrackerWrite::Close { item, close_as } => {
            let t = target(item)?;
            (live(&t.connection_id)?, TrackerWrite::Close { item: t.id, close_as: *close_as })
        }
        TrackerWrite::Reopen { item } => {
            let t = target(item)?;
            (live(&t.connection_id)?, TrackerWrite::Reopen { item: t.id })
        }
        TrackerWrite::Edit { item, title, body } => {
            if title.is_none() && body.is_none() {
                return Err(bad("an edit needs a title or a body"));
            }
            let t = target(item)?;
            let title = title.as_deref().map(title_ok).transpose()?;
            let body = body.as_deref().map(|b| text_ok("a body", b, true)).transpose()?;
            (live(&t.connection_id)?, TrackerWrite::Edit { item: t.id, title, body })
        }
    })
}

fn kind(w: &TrackerWrite) -> &'static str {
    match w {
        TrackerWrite::Create { .. } => "create",
        TrackerWrite::Comment { .. } => "comment",
        TrackerWrite::Close { .. } => "close",
        TrackerWrite::Reopen { .. } => "reopen",
        TrackerWrite::Edit { .. } => "edit",
    }
}

/// The `fi_` id a write names, if it names one.
fn item_of(w: &TrackerWrite) -> Option<&str> {
    match w {
        TrackerWrite::Create { .. } => None,
        TrackerWrite::Comment { item, .. }
        | TrackerWrite::Close { item, .. }
        | TrackerWrite::Reopen { item }
        | TrackerWrite::Edit { item, .. } => Some(item),
    }
}

/// Send `w` (already resolved) to the tracker as `who`, with the footer;
/// then read the item back into the mirror. The item's `fi_` id and its
/// `IDENT-N`.
pub async fn send(
    app: &Platform,
    project: &str,
    c: &Connection,
    w: &TrackerWrite,
    who: &Who,
) -> Result<(String, String), ApiError> {
    let master = app
        .secret_key
        .as_deref()
        .ok_or_else(|| err(StatusCode::NOT_IMPLEMENTED, "this control can't keep a tracker's key"))?;
    let key = keys::open(&app.db, master, project, &key_name(&c.id))?
        .ok_or_else(|| err(StatusCode::CONFLICT, "this connection's API key isn't kept: connect the project again"))?;
    let provider: Arc<dyn TrackerProvider> = app.trackers.plane(&key);
    drop(key);
    let (ws, pid) = (c.workspace_slug.as_str(), c.source_project_id.as_str());
    let footer = who.footer();
    let source = |fi: &str| -> Result<String, ApiError> {
        let t =
            app.db.tracker_target(project, fi)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such Plane work item"))?;
        Ok(t.source_item_id)
    };
    let done = match w {
        TrackerWrite::Create { title, body, state, .. } => {
            let new = NewItem {
                title: title.clone(),
                body: Some(html(&with_footer(body.as_deref().unwrap_or_default(), &footer))),
                state_group: state.clone(),
                ..Default::default()
            };
            provider.create(ws, pid, &new).await
        }
        TrackerWrite::Comment { item, text } => {
            let id = source(item)?;
            provider.comment(ws, pid, &id, &html(&with_footer(text, &footer))).await.map_err(tracker_error)?;
            provider.get(ws, pid, &id).await
        }
        TrackerWrite::Close { item, close_as } => {
            let group = match close_as {
                TrackerClose::Completed => "completed",
                TrackerClose::Cancelled => "cancelled",
            };
            provider.set_state(ws, pid, &source(item)?, group).await
        }
        TrackerWrite::Reopen { item } => provider.set_state(ws, pid, &source(item)?, "unstarted").await,
        TrackerWrite::Edit { item, title, body } => {
            let edit = Edit { title: title.clone(), body: body.as_deref().map(|b| html(&with_footer(b, &footer))) };
            provider.edit(ws, pid, &source(item)?, &edit).await
        }
    }
    .map_err(tracker_error)?;
    let changed = ingest_page(app, &c.id, c.mirror_id, None, std::slice::from_ref(&done), now_ms())?;
    if let Some(ids) = &changed {
        crate::boards::items_changed(app, ids);
    }
    let fi = app
        .db
        .tracker_item_id(&c.id, &done.id)?
        .ok_or_else(|| err(StatusCode::CONFLICT, "the connection was disconnected as the write was sent"))?;
    Ok((fi, format!("{}-{}", c.identifier, done.sequence_id)))
}

fn sent_text(w: &TrackerWrite, reference: &str) -> String {
    match w {
        TrackerWrite::Create { .. } => format!("created {reference} in Plane"),
        TrackerWrite::Comment { .. } => format!("commented on {reference} in Plane"),
        TrackerWrite::Close { .. } => format!("closed {reference} in Plane"),
        TrackerWrite::Reopen { .. } => format!("reopened {reference} in Plane"),
        TrackerWrite::Edit { .. } => format!("edited {reference} in Plane"),
    }
}

async fn land(
    app: &Platform,
    project: &str,
    c: &Connection,
    w: &TrackerWrite,
    who: &Who,
) -> Result<TrackerWriteDone, ApiError> {
    let (fi, reference) = send(app, project, c, w, who).await?;
    Ok(TrackerWriteDone {
        state: "sent".into(),
        write: None,
        text: sent_text(w, &reference),
        item: Some(fi),
        reference: Some(reference),
    })
}

/// A member who may write: not a team's viewer.
async fn writer(app: &Platform, account: &str, asked: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, asked, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't write to a project's connected trackers"));
    }
    Ok(project)
}

/// `POST /api/projects/{project}/trackers/{connection}/writes`: a person's
/// write, which lands at once.
pub async fn person(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, connection)): Path<(String, String)>,
    Json(w): Json<TrackerWrite>,
) -> Result<Json<TrackerWriteDone>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = writer(&app, &account, &project).await?;
    let (c, w) = resolve(&app, &project, Some(&connection), &w).await?;
    let who = Who { person: person_name(&app, &account).await?, agent: None };
    info!(%account, %project, connection = %c.id, kind = kind(&w), "a person wrote to Plane");
    Ok(Json(land(&app, &project, &c, &w, &who).await?))
}

/// `POST /api/daemon/projects/{project}/tracker-writes`: a machine's agent
/// writes (module docs).
pub async fn agent(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(project): Path<String>,
    Json(w): Json<TrackerWrite>,
) -> Result<Json<TrackerWriteDone>, ApiError> {
    app.limits.check_daemon(AGENT_WRITES, &d.daemon)?;
    crate::boards::gate(&app, &d.account).await?;
    let (project, _) = membership(&app, &d.account, &project, false).await?;
    agent_may(&app, &d, TRACKERS_WRITE, &AgentScope::Project(project.clone())).await?;
    // Never past its owner: one who may write there by hand.
    writer(&app, &d.account, &project).await?;
    let (c, w) = resolve(&app, &project, None, &w).await?;
    let who = who_agent(&app, &d.account, &d.daemon).await?;
    if c.agent_writes == "allow" {
        info!(account = %d.account, %project, connection = %c.id, kind = kind(&w), "an agent wrote to Plane");
        return Ok(Json(land(&app, &project, &c, &w, &who).await?));
    }
    let id = format!("tw_{}", hex::encode(arugula_e2e::random::<16>()));
    app.db.add_tracker_write(&PendingWrite {
        id: id.clone(),
        project_id: project.clone(),
        connection_id: c.id.clone(),
        kind: kind(&w).into(),
        request: serde_json::to_string(&w).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?,
        agent_machine: d.daemon.clone(),
        for_account: d.account.clone(),
        state: "waiting".into(),
        error: None,
        item_id: None,
        created_at: now_ms(),
    })?;
    info!(account = %d.account, %project, connection = %c.id, write = %id, "an agent's Plane write waits for a person");
    Ok(Json(TrackerWriteDone {
        state: "waiting".into(),
        text: format!("waiting for a person to approve it ({id})"),
        write: Some(id),
        item: None,
        reference: None,
    }))
}

/// `GET /api/daemon/trackers/reach`: the projects this machine holds
/// `trackers.write` over, as its owner reads them now.
pub async fn reach(State(app): State<Arc<Platform>>, d: Machine) -> Result<Json<TrackerReach>, ApiError> {
    app.limits.check_daemon(AGENT_READS, &d.daemon)?;
    crate::boards::gate(&app, &d.account).await?;
    let mut projects = vec![];
    for (id, owner) in app.db.machine_write_projects(&d.daemon, TRACKERS_WRITE)? {
        let scope = AgentScope::Project(id.clone());
        if owner == d.account
            && agent_may(&app, &d, TRACKERS_WRITE, &scope).await.is_ok()
            && let Some(name) = owner_reads(&app, &d.account, &scope, false).await?
        {
            projects.push(TrackerReachProject { id, name });
        }
    }
    projects.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(Json(TrackerReach { projects }))
}

// ---- waiting writes: a person's

async fn view(app: &Platform, p: &PendingWrite) -> Result<TrackerPendingWrite, ApiError> {
    let write: TrackerWrite = serde_json::from_str(&p.request)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "a kept write is unreadable"))?;
    let target = match item_of(&write) {
        Some(fi) => app.db.tracker_target(&p.project_id, fi)?.map(|t| format!("{}-{}", t.identifier, t.number)),
        None => None,
    };
    let names = agent_names(app, std::slice::from_ref(&p.agent_machine)).await?;
    Ok(TrackerPendingWrite {
        id: p.id.clone(),
        connection: p.connection_id.clone(),
        state: p.state.clone(),
        write,
        target,
        person: person_name(app, &p.for_account).await?,
        machine: p.agent_machine.clone(),
        machine_name: names.get(&p.agent_machine).map_or_else(|| p.agent_machine.clone(), |d| d.name.clone()),
        error: p.error.clone(),
        created_at: p.created_at,
    })
}

/// `GET /api/projects/{project}/tracker-writes`: those waiting.
pub async fn waiting(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<TrackerPendingWrites>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = writer(&app, &account, &project).await?;
    let mut writes = vec![];
    for p in app.db.tracker_writes_waiting(&project)? {
        writes.push(view(&app, &p).await?);
    }
    Ok(Json(TrackerPendingWrites { project, writes }))
}

fn none_waiting() -> ApiError {
    err(StatusCode::CONFLICT, "that write isn't waiting any more: someone approved or dropped it")
}

/// `PATCH /api/projects/{project}/tracker-writes/{write}`: its new text, a
/// write of the same kind to the same item or connection.
pub async fn edit(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, id)): Path<(String, String)>,
    Json(w): Json<TrackerWrite>,
) -> Result<Json<TrackerPendingWrite>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = writer(&app, &account, &project).await?;
    let p = app.db.tracker_write(&project, &id)?.filter(|p| p.state == "waiting").ok_or_else(none_waiting)?;
    let old: TrackerWrite = serde_json::from_str(&p.request)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "a kept write is unreadable"))?;
    let (c, new) = resolve(&app, &project, Some(&p.connection_id), &w).await?;
    if kind(&new) != kind(&old) || item_of(&new) != item_of(&old) || c.id != p.connection_id {
        return Err(bad("an edit changes a waiting write's text, not what it is or where it goes"));
    }
    let json = serde_json::to_string(&new).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if !app.db.set_tracker_write_request(&project, &id, &json)? {
        return Err(none_waiting());
    }
    let p = app.db.tracker_write(&project, &id)?.ok_or_else(none_waiting)?;
    Ok(Json(view(&app, &p).await?))
}

/// `POST /api/projects/{project}/tracker-writes/{write}/approve`: send it,
/// as that agent's.
pub async fn approve(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<TrackerWriteDone>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = writer(&app, &account, &project).await?;
    let p = app.db.take_tracker_write(&project, &id, "sent", &account, now_ms())?.ok_or_else(none_waiting)?;
    let sent = async {
        let w: TrackerWrite = serde_json::from_str(&p.request)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "a kept write is unreadable"))?;
        let c = app
            .db
            .tracker_connection(&project, &p.connection_id)?
            .filter(|c| c.unlinked_at.is_none())
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "that Plane project isn't connected to this project"))?;
        let who = who_agent(&app, &p.for_account, &p.agent_machine).await?;
        land(&app, &project, &c, &w, &who).await
    }
    .await;
    match sent {
        Ok(done) => {
            app.db.finish_tracker_write(&id, done.item.as_deref(), None)?;
            info!(%account, %project, write = %id, "approved an agent's Plane write");
            Ok(Json(done))
        }
        Err(e) => {
            warn!(%project, write = %id, status = e.0.as_u16(), "an approved Plane write failed");
            app.db.finish_tracker_write(&id, None, Some(&e.1))?;
            Err(e)
        }
    }
}

/// `POST /api/projects/{project}/tracker-writes/{write}/drop`.
pub async fn drop_write(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<TrackerPendingWrite>, ApiError> {
    crate::boards::gate(&app, &account).await?;
    let project = writer(&app, &account, &project).await?;
    let p = app.db.take_tracker_write(&project, &id, "dropped", &account, now_ms())?.ok_or_else(none_waiting)?;
    info!(%account, %project, write = %id, "dropped an agent's Plane write");
    Ok(Json(view(&app, &p).await?))
}
