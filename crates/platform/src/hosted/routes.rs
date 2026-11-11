//! Hosted workspaces' routes (M179), for a project's members: a person,
//! a CLI device, or a machine with a live `cloud.drive` grant acting for
//! its owner ([`CloudDriver`]), as the project's linked repos take them.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/workspaces` (every project's the caller is in) | members |
//! | `GET /api/projects/{project}/workspaces` | members |
//! | `POST /api/projects/{project}/workspaces` | owners and editors, under the launch switches ([`super::switches`]) |
//! | `GET /api/projects/{project}/workspaces/{id}` (open: wakes a paused box, unless its maker is suspended or past its hours) | members |
//! | `POST /api/projects/{project}/workspaces/{id}/rebuild` | owners and editors, under the launch switches |
//! | `DELETE /api/projects/{project}/workspaces/{id}` | owners and editors |
//! | `PUT /api/projects/{project}/workspaces/{id}/spec` ("rebuild needed" until it's rebuilt) | owners and editors |
//! | `GET /api/projects/{project}/workspaces/{id}/secrets` (names only) | members |
//! | `PUT`, `DELETE /api/projects/{project}/workspaces/{id}/secrets/{name}` | owners and editors |
//!
//! `{project}` is an id, or `personal`; `{id}` is a workspace's id or its
//! name in the project. A project someone isn't a member of, and a
//! workspace that isn't the project's, are both 404.
//!
//! A workspace names who made it (`created_by`) only while they're in the
//! project now ([`Members`]): one a former member made says nobody. The
//! row keeps the name, for the worker's plan of who pays for its
//! inference. Chat's link card for a workspace says the same.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use super::{Ask, HostedWorkspace, Spec};
use crate::{Platform, auth::Person, cloud::CloudDriver, db::hosted::Added, forge::links::membership, hub::Team};

/// Live workspaces a project may have at once.
pub const MAX_PER_PROJECT: u64 = 10;

/// `GET /api/projects/{project}/workspaces`.
#[derive(Debug, Serialize)]
pub struct Workspaces {
    pub project: String,
    pub workspaces: Vec<HostedWorkspace>,
}

/// `POST /api/projects/{project}/workspaces`.
#[derive(Debug, Deserialize)]
pub struct NewWorkspace {
    pub name: String,
    #[serde(default)]
    pub spec: Spec,
}

/// `GET /api/workspaces`: the hosted workspaces of every project the
/// caller is in (M184, for MCP's `list kind cloud_workspaces`).
#[derive(Debug, Serialize)]
pub struct AllWorkspaces {
    pub workspaces: Vec<Listed>,
}

/// A hosted workspace, with its project's name.
#[derive(Debug, Serialize)]
pub struct Listed {
    #[serde(flatten)]
    pub workspace: HostedWorkspace,
    pub project_name: String,
}

/// That hosted workspaces are set up here.
fn enabled(app: &Platform) -> Result<(), ApiError> {
    if app.compute.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "hosted workspaces aren't set up on this control"));
    }
    Ok(())
}

/// A member who may make, rebuild and delete workspaces: not a team's
/// viewer.
async fn changer(app: &Platform, account: &str, project: &str) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't change a project's cloud workspaces"));
    }
    Ok(project)
}

fn found(w: Option<HostedWorkspace>) -> Result<HostedWorkspace, ApiError> {
    w.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such cloud workspace"))
}

/// Who's in a project now, as its workspaces are shown (module docs): a
/// personal project's account, or a team's roster (through the cache, as
/// a read of the project is). Read once per project, never per workspace.
/// Someone on a locked team's roster is still in it: only who left is
/// not.
pub enum Members {
    Account(String),
    Team(Team),
    Nobody,
}

impl Members {
    pub async fn of(app: &Platform, project: &str) -> Result<Self, ApiError> {
        Ok(match app.db.project_holder(project)? {
            Some((Some(a), _)) => Members::Account(a),
            Some((None, Some(t))) => app.cache.team(&*app.hub, &t).await?.map_or(Members::Nobody, Members::Team),
            _ => Members::Nobody,
        })
    }

    /// [`Members::of`] where the answer only names people: a roster that
    /// can't be read now names nobody, rather than failing what's done.
    pub async fn or_nobody(app: &Platform, project: &str) -> Self {
        Self::of(app, project).await.unwrap_or_else(|e| {
            tracing::warn!(project, error = %e.1, "reading a project's members to name a workspace's maker");
            Members::Nobody
        })
    }

    pub fn has(&self, account: &str) -> bool {
        match self {
            Members::Account(a) => a == account,
            Members::Team(t) => t.members.iter().any(|m| m.account == account),
            Members::Nobody => false,
        }
    }

    /// `account` if they're in, as a maker is named.
    pub fn named(&self, account: Option<String>) -> Option<String> {
        account.filter(|a| self.has(a))
    }

    /// The workspace as it's shown: its maker named only while they're in.
    pub fn shown(&self, mut w: HostedWorkspace) -> HostedWorkspace {
        w.created_by = self.named(w.created_by.take());
        w
    }
}

/// One workspace as it's shown to a member of `project`, before anything
/// is done to it.
async fn shown(app: &Platform, project: &str, w: HostedWorkspace) -> Result<HostedWorkspace, ApiError> {
    Ok(Members::of(app, project).await?.shown(w))
}

/// One workspace as it's shown after something was done to it (a wake, a
/// rebuild, a delete, a new spec): the maker's name is never what fails
/// an answer about a thing that's done.
async fn shown_after(app: &Platform, project: &str, w: HostedWorkspace) -> HostedWorkspace {
    Members::or_nobody(app, project).await.shown(w)
}

pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<Workspaces>, ApiError> {
    enabled(&app)?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let members = Members::of(&app, &project).await?;
    let workspaces = app.db.hosted_workspaces(&project)?.into_iter().map(|w| members.shown(w)).collect();
    Ok(Json(Workspaces { project, workspaces }))
}

pub async fn create(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
    Json(b): Json<NewWorkspace>,
) -> Result<(StatusCode, Json<HostedWorkspace>), ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let name = b.name.trim();
    if !super::good_name(name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name: 1-64 letters, digits, '-', '_' and '.', starting with a letter or digit",
        ));
    }
    b.spec.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?;
    // The launch switches (#531): new boxes open to this account, its
    // hours left, and how many it may have made.
    let mine = super::switches::may_make(&app, &account, true).await?;
    let id = super::new_id();
    let w = match app.db.add_hosted_workspace_capped(
        &id,
        &project,
        name,
        &b.spec,
        &account,
        MAX_PER_PROJECT,
        mine,
        now_ms(),
    )? {
        Added::Made(w) => *w,
        Added::NameTaken => {
            return Err(err(StatusCode::CONFLICT, &format!("this project has a cloud workspace called {name}")));
        }
        Added::Full => {
            return Err(err(
                StatusCode::CONFLICT,
                &format!("a project has at most {MAX_PER_PROJECT} cloud workspaces: delete one first"),
            ));
        }
        Added::MakerFull(made) => return Err(super::switches::box_cap(made)),
    };
    info!(%account, %project, workspace = %w.id, "made a hosted workspace");
    app.analytics.capture(&account, "hosted workspace created", json!({ "repos": b.spec.repos.len() }));
    Ok((StatusCode::CREATED, Json(w)))
}

/// Open: the workspace, its box asked to wake if it's paused (the page and
/// the CLI attach to its machine once it's `ready`).
pub async fn open(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    if w.state == "paused" {
        super::switches::may_wake(&app, &w).await?;
    }
    if w.state == "paused" && super::ask(&app, &w.id, Ask::Wake)? {
        let w = found(app.db.hosted_workspace(&project, &w.id)?)?;
        return Ok(Json(shown_after(&app, &project, w).await));
    }
    Ok(Json(shown(&app, &project, w).await?))
}

pub async fn rebuild(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    super::switches::may_make(&app, &account, false).await?;
    if !super::ask(&app, &w.id, Ask::Rebuild)? {
        return Err(err(StatusCode::CONFLICT, "this cloud workspace is being deleted"));
    }
    info!(%account, %project, workspace = %w.id, "asked to rebuild a hosted workspace");
    let w = found(app.db.hosted_workspace(&project, &w.id)?)?;
    Ok(Json(shown_after(&app, &project, w).await))
}

pub async fn delete(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    super::ask(&app, &w.id, Ask::Delete)?;
    // Its thread sessions end now (M256), not when their jobs next look.
    let stopped = app.db.stop_workspace_sessions(&w.id, now_ms())?;
    crate::features::chat().told(&app, &stopped)?;
    info!(%account, %project, workspace = %w.id, "asked to delete a hosted workspace");
    app.analytics.capture(&account, "hosted workspace deleted", json!({}));
    let w = found(app.db.hosted_workspace(&project, &w.id)?)?;
    Ok(Json(shown_after(&app, &project, w).await))
}

/// `PUT /api/projects/{project}/workspaces/{id}/spec`: what its box is
/// made from next. The box isn't touched: "rebuild needed" shows until it's
/// rebuilt (M183).
pub async fn set_spec(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
    Json(spec): Json<Spec>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    spec.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?;
    let w = found(app.db.set_hosted_spec(&project, &w.id, &spec, now_ms())?)?;
    info!(%account, %project, workspace = %w.id, rebuild_needed = w.rebuild_needed, "changed a hosted workspace's spec");
    Ok(Json(shown_after(&app, &project, w).await))
}

/// `GET /api/projects/{project}/workspaces/{id}/secrets`.
#[derive(Debug, Serialize)]
pub struct Secrets {
    /// Each secret's name and where its value is: `sealed` (here) or
    /// `broker` (the provider's). Never a value.
    pub secrets: Vec<SecretName>,
}

#[derive(Debug, Serialize)]
pub struct SecretName {
    pub name: String,
    pub store: String,
}

/// `PUT /api/projects/{project}/workspaces/{id}/secrets/{name}`.
#[derive(Deserialize)]
pub struct SecretValue {
    pub value: String,
}

/// The most a secret's value may be.
const MAX_SECRET: usize = 16 * 1024;

pub async fn secrets(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<Secrets>, ApiError> {
    enabled(&app)?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    let secrets =
        app.db.hosted_secret_names(&w.id)?.into_iter().map(|(name, store)| SecretName { name, store }).collect();
    Ok(Json(Secrets { secrets }))
}

/// Set a secret, sealed with the workspace's project's key (M209), bound to
/// the workspace and its name. Its box reads it on its next setup: a spec's env
/// name, or `GIT_TOKEN` and `GIT_USERNAME` to clone with. `GIT_TOKEN` is
/// bound to the hosts of the spec's repos as it's set (`setup::hosts_of`).
pub async fn put_secret(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, name)): Path<(String, String, String)>,
    Json(b): Json<SecretValue>,
) -> Result<StatusCode, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    if !crate::secrets::valid_name(&name) {
        return Err(err(StatusCode::BAD_REQUEST, "a secret's name: capital letters, digits and '_'"));
    }
    if b.value.is_empty() || b.value.len() > MAX_SECRET {
        return Err(err(StatusCode::BAD_REQUEST, "a secret's value: 1 byte to 16 KiB"));
    }
    let Some(cipher) = &app.hosted.cipher else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "this control can't keep secrets"));
    };
    let sealed =
        crate::keys::seal_value(&app.db.c(), cipher, &project, &crate::keys::hosted_name(&w.id, &name), &b.value)?;
    // The clone secret goes only to the hosts the spec's repos are on now.
    let hosts = (name == super::setup::GIT_TOKEN).then(|| super::setup::hosts_of(&w.spec));
    app.db.set_hosted_secret(&w.id, &name, Some(&sealed), hosts.as_deref(), now_ms())?;
    info!(%account, %project, workspace = %w.id, name, "set a hosted workspace's secret");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_secret(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, name)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    if !app.db.remove_hosted_secret(&w.id, &name)? {
        return Err(err(StatusCode::NOT_FOUND, "no such secret"));
    }
    info!(%account, %project, workspace = %w.id, name, "removed a hosted workspace's secret");
    Ok(StatusCode::NO_CONTENT)
}

/// Every hosted workspace in the caller's projects: its personal one's,
/// then each team's, by name. None where hosted workspaces aren't set up
/// (rather than a refusal: MCP's list says there are none).
pub async fn all(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
) -> Result<Json<AllWorkspaces>, ApiError> {
    let mut workspaces = Vec::new();
    if app.compute.is_none() {
        return Ok(Json(AllWorkspaces { workspaces }));
    }
    let mut projects = Vec::new();
    if let Some(id) = crate::projects::personal_project(&app, &account).await? {
        projects.push((id, "Personal".to_owned(), Members::Account(account.clone())));
    }
    let ids = app.cache.teams_of(&*app.hub, &account).await?;
    let mut teams: Vec<_> = app.cache.teams(&*app.hub, &ids).await?.into_values().collect();
    teams.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    for t in teams.into_iter().filter(|t| t.role(&account).is_some()) {
        projects.push((app.db.team_project(&t.id, &t.name, now_ms())?, t.name.clone(), Members::Team(t)));
    }
    for (project, name, members) in projects {
        for w in app.db.hosted_workspaces(&project)? {
            workspaces.push(Listed { workspace: members.shown(w), project_name: name.clone() });
        }
    }
    Ok(Json(AllWorkspaces { workspaces }))
}
