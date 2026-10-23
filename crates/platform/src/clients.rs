//! studio becomes a client (M236, Arugula #34; "studio is a full client:
//! Sign in with Arugula, apps by project"): what a registered client (the
//! lobby, by the credential M231's [`broker::clients`] checks) may ask the
//! platform besides Claude, and what people and project owners let it.
//!
//! **The client's own routes** (`/api/client/...`, `Authorization: Bearer
//! <its credential>`; the gateway lets a bearer through to these only,
//! `gateway::bearer_route`):
//! - For an account it names (`arugula-account`), **only once that account
//!   let it see its projects** ([`Scope::Projects`], a switch beside the
//!   product under Settings, Linked products; off until then, and
//!   unlinking ends it):
//!   - `GET /api/client/projects`: the account's projects, its role in
//!     each and their members, as the platform page shows them to it;
//!   - `GET /api/client/apps`: the client's apps in those projects, with
//!     the account's role in each, so an app's owners are its project's
//!     members;
//!   - `GET /api/client/apps/{app}`: one of them, with the account's role,
//!     read fresh (a member who left can't open it a moment later);
//!   - `PUT /api/client/apps/{app}` `{project, title}`: keep the app in a
//!     project the account edits or owns (`personal` is its own), and when
//!     it moves, one it edits or owns there too.
//! - With no account:
//!   - `DELETE /api/client/apps/{app}`: the client forgets its app (the
//!     lobby retired it);
//!   - `GET /api/client/apps/{app}/secrets/{name}`: the value of the app's
//!     project's own secret `name` (`project/<NAME>`, M230), **only when a
//!     project owner let this client read that secret** (a grant, by name,
//!     on the project's settings), each read counted and kept as an event
//!     for the project's owners to see. The lobby's egress broker uses it
//!     server side, never a browser or a box.
//!
//! **The platform page's** (a session; the switches are the page's only:
//! trust, never a CLI's or an agent's):
//! - the account's switch: `PUT`/`DELETE /api/linked-products/{client}/projects`;
//! - a project's: its own secrets set and cleared by its owners
//!   (`PUT`/`DELETE /api/projects/{project}/secrets/{name}`), the clients'
//!   apps in it and, for owners, the grants and their reads
//!   (`GET /api/projects/{project}/clients`), and a grant made or ended
//!   (`PUT`/`DELETE /api/projects/{project}/clients/{client}/secrets/{name}`).
//!
//! A link (M229) is never authorization: nothing here is granted because a
//! product's user is linked to an account. A client acts for an account
//! only with that account's own switch, and reads a secret only by its
//! project owner's grant.
//!
//! [`broker::clients`]: crate::hosted::broker::clients

use std::{sync::Arc, time::Duration};

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;

pub use crate::db::clients::Scope;
use crate::{
    Platform,
    agent_mcp::{MAX_PROJECT_SECRETS, MAX_SECRET, project_secret},
    auth::Person,
    db::clients::{ClientApp, SecretEvent, SecretGrant},
    forge::links::membership,
    hosted::broker::{ACCOUNT, bearer, header_of},
};

/// The client's routes.
pub const PROJECTS: &str = "/api/client/projects";
pub const APPS: &str = "/api/client/apps";
pub const APP: &str = "/api/client/apps/{app}";
pub const APP_SECRET: &str = "/api/client/apps/{app}/secrets/{name}";
/// The page's.
pub const CONSENT: &str = "/api/linked-products/{client}/projects";
pub const PROJECT_CLIENTS: &str = "/api/projects/{project}/clients";
pub const PROJECT_GRANT: &str = "/api/projects/{project}/clients/{client}/secrets/{name}";
pub const PROJECT_SECRET: &str = "/api/projects/{project}/secrets/{name}";

/// A client's secret reads in an hour, before they're refused (the lobby
/// keeps a value a minute, so this is many apps' worth).
pub const SECRET_READS: (&str, usize) = ("client secret reads", 6000);
/// The longest title an app is kept under.
const MAX_TITLE: usize = 100;
/// The events a project's owners see.
const EVENTS: u32 = 20;

/// A refusal to a client: `{error, reason}`, the reason for its code to
/// read (`client`, `account`, `consent`, `no_app`, `not_member`, `role`,
/// `no_grant`, `no_secret`, ...).
#[derive(Debug)]
pub struct Nope {
    status: StatusCode,
    reason: &'static str,
    message: String,
}

fn nope(status: StatusCode, reason: &'static str, message: impl Into<String>) -> Nope {
    Nope { status, reason, message: message.into() }
}

impl IntoResponse for Nope {
    fn into_response(self) -> Response {
        let body = json!({ "error": self.message, "reason": self.reason });
        (self.status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
    }
}

impl From<ApiError> for Nope {
    fn from(e: ApiError) -> Nope {
        let reason = match e.0 {
            StatusCode::NOT_FOUND => "no_project",
            StatusCode::TOO_MANY_REQUESTS => "busy",
            _ => "refused",
        };
        nope(e.0, reason, e.1)
    }
}

impl From<anyhow::Error> for Nope {
    fn from(e: anyhow::Error) -> Nope {
        tracing::warn!(error = %format!("{e:#}"), "a client's call failed");
        nope(StatusCode::INTERNAL_SERVER_ERROR, "error", "The platform couldn't answer.")
    }
}

type Answer<T> = Result<Json<T>, Nope>;

/// The registered client whose credential the request carries.
async fn client_of(app: &Platform, headers: &HeaderMap) -> Result<String, Nope> {
    let refused = || nope(StatusCode::UNAUTHORIZED, "client", "That isn't a registered client's credential.");
    let credential = bearer(headers).ok_or_else(refused)?;
    let (client, _) = app.hosted.broker.clients.who(credential).await?.ok_or_else(refused)?;
    Ok(client)
}

/// The client, and the account it names, which let it see its projects.
async fn acting(app: &Platform, headers: &HeaderMap) -> Result<(String, String), Nope> {
    let client = client_of(app, headers).await?;
    let account = header_of(headers, ACCOUNT)
        .map(str::to_owned)
        .ok_or_else(|| nope(StatusCode::BAD_REQUEST, "account", format!("Name the account in {ACCOUNT}.")))?;
    if !app.db.client_consented(&account, &client, Scope::Projects)? {
        return Err(nope(
            StatusCode::FORBIDDEN,
            "consent",
            format!(
                "{account} hasn't let {client} see their projects. They turn it on in Arugula's settings, under Linked \
                 products."
            ),
        ));
    }
    Ok((client, account))
}

/// One of an account's projects, as a client sees it.
#[derive(Debug, Clone, Serialize)]
pub struct ClientProject {
    pub id: String,
    pub name: String,
    /// The account's own (its personal project).
    pub personal: bool,
    /// The account's role in it: `owner`, `editor` or `viewer`.
    pub role: String,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub account: String,
    pub name: String,
    pub role: String,
}

/// `account`'s projects, as the platform page lists them (its personal one
/// first, then its teams' by name), with its role and the members in each.
async fn projects_of(app: &Platform, account: &str) -> Result<Vec<ClientProject>, Nope> {
    let mut out = vec![];
    if let Some(id) = crate::projects::personal_project(app, account).await.map_err(ApiError::from)? {
        let me = app.cache.accounts(&*app.hub, &[account.to_owned()]).await.map_err(ApiError::from)?.remove(account);
        let name = me.map(|a| a.name).unwrap_or_default();
        out.push(ClientProject {
            id,
            name: "Personal".into(),
            personal: true,
            role: TeamRole::Owner.as_str().into(),
            members: vec![Member { account: account.to_owned(), name, role: TeamRole::Owner.as_str().into() }],
        });
    }
    let ids = app.cache.teams_of(&*app.hub, account).await.map_err(ApiError::from)?;
    let mut teams: Vec<_> = app.cache.teams(&*app.hub, &ids).await.map_err(ApiError::from)?.into_values().collect();
    teams.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    for t in teams {
        let Some(role) = t.role(account) else { continue };
        let id = app.db.team_project(&t.id, &t.name, now_ms())?;
        let members = t
            .members
            .iter()
            .map(|m| Member { account: m.account.clone(), name: m.name.clone(), role: m.role.as_str().into() })
            .collect();
        out.push(ClientProject { id, name: t.name.clone(), personal: false, role: role.as_str().into(), members });
    }
    Ok(out)
}

/// `GET /api/client/projects`.
pub async fn projects(State(app): State<Arc<Platform>>, headers: HeaderMap) -> Answer<Value> {
    let (_, account) = acting(&app, &headers).await?;
    let projects = projects_of(&app, &account).await?;
    Ok(Json(json!({ "account": account, "projects": projects })))
}

/// One of a client's apps, as an account it acts for sees it.
#[derive(Debug, Clone, Serialize)]
pub struct SeenApp {
    pub app: String,
    pub project: String,
    pub project_name: String,
    pub personal: bool,
    pub title: String,
    /// The account's role in the app's project.
    pub role: String,
    pub added_by: Option<String>,
    pub added_at: u64,
}

fn seen(a: ClientApp, p: &ClientProject) -> SeenApp {
    SeenApp {
        app: a.app,
        project: a.project,
        project_name: p.name.clone(),
        personal: p.personal,
        title: a.title,
        role: p.role.clone(),
        added_by: a.added_by,
        added_at: a.added_at,
    }
}

/// `GET /api/client/apps`: the client's apps in the account's projects.
pub async fn apps(State(app): State<Arc<Platform>>, headers: HeaderMap) -> Answer<Value> {
    let (client, account) = acting(&app, &headers).await?;
    let projects = projects_of(&app, &account).await?;
    let ids: Vec<String> = projects.iter().map(|p| p.id.clone()).collect();
    let apps: Vec<SeenApp> = app
        .db
        .client_apps_in(&client, &ids)?
        .into_iter()
        .filter_map(|a| {
            let p = projects.iter().find(|p| p.id == a.project)?;
            Some(seen(a, p))
        })
        .collect();
    Ok(Json(json!({ "account": account, "apps": apps })))
}

fn app_id(app: &str) -> Result<(), Nope> {
    let ok = !app.is_empty()
        && app.len() <= 64
        && app.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok { Ok(()) } else { Err(nope(StatusCode::BAD_REQUEST, "app", "An app's id is 1 to 64 letters, digits, '-', '_' or '.'.")) }
}

/// The account's role in `project`, read fresh, or why it has none there.
async fn role_in(app: &Platform, account: &str, project: &str) -> Result<(String, TeamRole), Nope> {
    membership(app, account, project, true).await.map_err(|e| match e.0 {
        StatusCode::NOT_FOUND => nope(StatusCode::FORBIDDEN, "not_member", format!("{account} isn't a member of that project.")),
        _ => e.into(),
    })
}

/// The project's name to an account in it.
async fn project_name(app: &Platform, account: &str, project: &str) -> Result<(String, bool), Nope> {
    let mine = projects_of(app, account).await?;
    Ok(mine.iter().find(|p| p.id == project).map(|p| (p.name.clone(), p.personal)).unwrap_or_default())
}

/// `GET /api/client/apps/{app}`: the app and the account's role in its
/// project, read fresh. 404 `no_app` when the client keeps no such app
/// here; 403 `not_member` when the account isn't in its project.
pub async fn show(State(app): State<Arc<Platform>>, headers: HeaderMap, Path(id): Path<String>) -> Answer<SeenApp> {
    app_id(&id)?;
    let (client, account) = acting(&app, &headers).await?;
    let found = app
        .db
        .client_app(&client, &id)?
        .ok_or_else(|| nope(StatusCode::NOT_FOUND, "no_app", format!("{client} keeps no app {id} here.")))?;
    let (_, role) = role_in(&app, &account, &found.project).await?;
    let (name, personal) = project_name(&app, &account, &found.project).await?;
    let p = ClientProject { id: found.project.clone(), name, personal, role: role.as_str().into(), members: vec![] };
    Ok(Json(seen(found, &p)))
}

#[derive(Debug, Deserialize)]
pub struct PutApp {
    pub project: String,
    #[serde(default)]
    pub title: String,
}

/// An editor or an owner there, or why not.
async fn editor_in(app: &Platform, account: &str, project: &str) -> Result<String, Nope> {
    let (project, role) = role_in(app, account, project).await?;
    if role == TeamRole::Viewer {
        return Err(nope(StatusCode::FORBIDDEN, "role", format!("{account} only views that project, so can't keep apps in it.")));
    }
    Ok(project)
}

/// `PUT /api/client/apps/{app}` `{project, title}`: keep the app in a
/// project the account edits or owns; a move needs that of the project it
/// leaves too.
pub async fn put(
    State(app): State<Arc<Platform>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<PutApp>,
) -> Answer<SeenApp> {
    app_id(&id)?;
    let (client, account) = acting(&app, &headers).await?;
    let target = editor_in(&app, &account, b.project.trim()).await?;
    let before = app.db.client_app(&client, &id)?;
    if let Some(b) = &before
        && b.project != target
    {
        editor_in(&app, &account, &b.project).await?;
    }
    let title: String = b.title.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_TITLE).collect();
    let title = if title.is_empty() { before.as_ref().map(|b| b.title.clone()).unwrap_or_else(|| id.clone()) } else { title };
    let kept = app.db.put_client_app(&client, &id, &target, &title, &account, now_ms())?;
    let moved = before.as_ref().map(|b| b.project.as_str()) != Some(target.as_str());
    if moved {
        info!(%client, app = %id, project = %target, %account, from = ?before.map(|b| b.project), "a client's app is in a project now");
    }
    let (_, role) = role_in(&app, &account, &target).await?;
    let (name, personal) = project_name(&app, &account, &target).await?;
    let p = ClientProject { id: target, name, personal, role: role.as_str().into(), members: vec![] };
    Ok(Json(seen(kept, &p)))
}

/// `DELETE /api/client/apps/{app}`: the client forgets its own app.
pub async fn remove(State(app): State<Arc<Platform>>, headers: HeaderMap, Path(id): Path<String>) -> Answer<Value> {
    app_id(&id)?;
    let client = client_of(&app, &headers).await?;
    let removed = app.db.remove_client_app(&client, &id)?;
    if removed {
        info!(%client, app = %id, "a client forgot its app");
    }
    Ok(Json(json!({ "removed": removed })))
}

/// `GET /api/client/apps/{app}/secrets/{name}`: the value of the app's
/// project's own secret, when its owners let this client read it.
pub async fn secret(
    State(app): State<Arc<Platform>>,
    headers: HeaderMap,
    Path((id, name)): Path<(String, String)>,
) -> Result<Response, Nope> {
    app_id(&id)?;
    let client = client_of(&app, &headers).await?;
    app.limits.check_client(SECRET_READS, &client)?;
    let found = app
        .db
        .client_app(&client, &id)?
        .ok_or_else(|| nope(StatusCode::NOT_FOUND, "no_app", format!("{client} keeps no app {id} here.")))?;
    if !crate::secrets::valid_name(&name) || !app.db.client_secret_granted(&found.project, &client, &name)? {
        return Err(nope(
            StatusCode::FORBIDDEN,
            "no_grant",
            format!("The owners of {id}'s project haven't let {client} read {name}. They do it in the project's settings."),
        ));
    }
    let master = app
        .secret_key
        .as_deref()
        .ok_or_else(|| nope(StatusCode::NOT_FOUND, "no_secret", "This platform keeps no secrets."))?;
    let value = crate::keys::open(&app.db, master, &found.project, &project_secret(&name))?.ok_or_else(|| {
        nope(StatusCode::NOT_FOUND, "no_secret", format!("{id}'s project has no secret {name}. Its owners set it in the project's settings."))
    })?;
    let now = now_ms();
    app.db.note_client_secret_read(&found.project, &client, &name, &id, now)?;
    info!(%client, app = %id, project = %found.project, name, "a client read a project's secret");
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(json!({ "app": id, "name": name, "value": value }))).into_response())
}

/// `PUT` (on) or `DELETE` (off) `/api/linked-products/{client}/projects`:
/// whether a registered client may see the signed-in account's projects
/// and keep its apps in the ones it edits. Only the page does this.
pub async fn set_consent(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(client): Path<String>,
    method: Method,
) -> Result<Json<Value>, ApiError> {
    if !app.hosted.broker.clients.policies.contains_key(&client) {
        return Err(err(StatusCode::NOT_FOUND, "no such client"));
    }
    let on = method == Method::PUT;
    app.db.set_client_consent(&account, &client, Scope::Projects, on, now_ms())?;
    info!(%account, %client, on, "an account changed whether a client sees its projects");
    app.analytics.capture(&account, "client projects consent", json!({ "client": client, "on": on }));
    Ok(Json(json!({ "client": client, "projects": on })))
}

/// `GET /api/projects/{project}/clients`: the clients' apps in a project,
/// for its members; and for its owners, which of its own secrets each
/// client may read, how often it has, and the latest events.
#[derive(Debug, Serialize)]
pub struct ProjectClients {
    pub project: String,
    /// The clients registered here.
    pub clients: Vec<String>,
    pub apps: Vec<ClientApp>,
    /// Owners only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grants: Option<Vec<SecretGrant>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<SecretEvent>>,
}

pub async fn project_clients(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<ProjectClients>, ApiError> {
    let (project, role) = membership(&app, &account, &project, false).await?;
    let owner = role == TeamRole::Owner;
    Ok(Json(ProjectClients {
        clients: app.hosted.broker.clients.policies.keys().cloned().collect(),
        apps: app.db.project_client_apps(&project)?,
        grants: owner.then(|| app.db.client_secret_grants(&project)).transpose()?,
        events: owner.then(|| app.db.client_secret_events(&project, EVENTS)).transpose()?,
        project,
    }))
}

/// An owner of `project`, read fresh, or why not.
async fn owner_of(app: &Platform, account: &str, project: &str, what: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, &format!("only the project's owners {what}")));
    }
    Ok(project)
}

/// `PUT` (let it) or `DELETE` (stop it)
/// `/api/projects/{project}/clients/{client}/secrets/{name}`: whether a
/// registered client may read one of the project's own secrets for its
/// apps there. Owners, from the page only; the secret must be set to be
/// granted.
pub async fn set_grant(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, client, name)): Path<(String, String, String)>,
    method: Method,
) -> Result<Json<ProjectClients>, ApiError> {
    let project = owner_of(&app, &account, &project, "let products read its secrets").await?;
    if !app.hosted.broker.clients.policies.contains_key(&client) {
        return Err(err(StatusCode::NOT_FOUND, "no such client"));
    }
    let on = method == Method::PUT;
    if on {
        if !crate::secrets::valid_name(&name) || !app.db.has_project_secret(&project, &project_secret(&name))? {
            return Err(err(StatusCode::NOT_FOUND, "the project has no secret by that name: set it first"));
        }
        app.db.grant_client_secret(&project, &client, &name, &account, now_ms())?;
    } else {
        app.db.revoke_client_secret(&project, &client, &name, &account, now_ms())?;
    }
    info!(%account, %project, %client, name, on, "a project's owner changed what a client may read");
    project_clients(State(app), Person { account }, Path(project)).await
}

#[derive(Debug, Deserialize)]
pub struct SetSecret {
    pub value: String,
}

/// `PUT /api/projects/{project}/secrets/{name}` `{value}`: an owner sets
/// one of the project's own secrets (`project/<NAME>`, as M230's
/// `set_secret` does), sealed with the project's key and never shown;
/// `DELETE` clears it, and ends every client's grant of it. From the page
/// only.
pub async fn set_project_secret(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, name)): Path<(String, String)>,
    method: Method,
    body: Option<Json<SetSecret>>,
) -> Result<Json<Value>, ApiError> {
    let project = owner_of(&app, &account, &project, "set its secrets").await?;
    if !crate::secrets::valid_name(&name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "a secret's name: capital letters, digits and '_', not starting with ARUGULA_, ANTHROPIC_, OPENAI_ or \
             another reserved prefix",
        ));
    }
    let stored = project_secret(&name);
    if method == Method::DELETE {
        let removed = crate::keys::remove(&app.db, &project, &stored)?;
        let revoked = app.db.revoke_client_secret_everywhere(&project, &name, &account, now_ms())?;
        info!(%account, %project, name, revoked, "a project's owner cleared its secret");
        return Ok(Json(json!({ "project": project, "name": name, "removed": removed, "grants_ended": revoked })));
    }
    let Some(Json(b)) = body else { return Err(err(StatusCode::BAD_REQUEST, "send {\"value\": ...}")) };
    if b.value.is_empty() || b.value.len() > MAX_SECRET || b.value.contains('\0') {
        return Err(err(StatusCode::BAD_REQUEST, "a secret's value is 1 to 16384 bytes, with no NUL"));
    }
    let master =
        app.secret_key.as_deref().ok_or_else(|| err(StatusCode::NOT_FOUND, "this platform keeps no secrets"))?;
    if !app.db.has_project_secret(&project, &stored)? && app.db.project_secrets(&project)?.len() >= MAX_PROJECT_SECRETS {
        return Err(err(StatusCode::CONFLICT, &format!("the project keeps {MAX_PROJECT_SECRETS} secrets of its own already")));
    }
    crate::keys::seal(&app.db, master, &project, &stored, &b.value)?;
    app.db.note_secret_writer(&project, &stored, &account, None)?;
    info!(%account, %project, name, "a project's owner set its secret");
    Ok(Json(json!({ "project": project, "name": name, "set": true })))
}

/// How often [`start`] tidies.
const TIDY: Duration = Duration::from_secs(3600);

/// Clients' secret events past their days go, hourly.
pub fn start(app: &Arc<Platform>) {
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            if !app.is_paused()
                && let Err(e) = app.db.tidy_client_secret_events(now_ms())
            {
                tracing::warn!("clients' secret events weren't tidied: {e:#}");
            }
            tokio::time::sleep(TIDY).await;
        }
    });
}
