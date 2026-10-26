//! Hosted workspaces' routes (M179), for a project's members: a person,
//! a CLI device, or a machine with a live `cloud.drive` grant acting for
//! its owner ([`CloudDriver`]), as the project's linked repos take them.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/workspaces` (every project's the caller is in) | members |
//! | `GET /api/projects/{project}/workspaces` | members |
//! | `POST /api/projects/{project}/workspaces` | owners and editors, on `ARUGULA_HOSTED_ACCOUNTS` |
//! | `GET /api/projects/{project}/workspaces/{id}` (open: wakes a paused box) | members |
//! | `POST /api/projects/{project}/workspaces/{id}/rebuild` | owners and editors, on `ARUGULA_HOSTED_ACCOUNTS` |
//! | `DELETE /api/projects/{project}/workspaces/{id}` | owners and editors |
//! | `PUT /api/projects/{project}/workspaces/{id}/spec` ("rebuild needed" until it's rebuilt) | owners and editors |
//! | `GET /api/projects/{project}/workspaces/{id}/secrets` (names only) | members |
//! | `PUT`, `DELETE /api/projects/{project}/workspaces/{id}/secrets/{name}` | owners and editors |
//!
//! `{project}` is an id, or `personal`; `{id}` is a workspace's id or its
//! name in the project. A project someone isn't a member of, and a
//! workspace that isn't the project's, are both 404.

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
use crate::{Platform, auth::Person, cloud::CloudDriver, db::hosted::Added, forge::links::membership};

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
/// viewer. `making`: making a box, which also needs the account on the
/// allowlist.
async fn changer(app: &Platform, account: &str, project: &str, making: bool) -> Result<String, ApiError> {
    let (project, role) = membership(app, account, project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a viewer can't change a project's hosted workspaces"));
    }
    if making && !app.hosted.allows(account) {
        return Err(err(StatusCode::FORBIDDEN, "hosted workspaces are in a limited beta"));
    }
    Ok(project)
}

fn found(w: Option<HostedWorkspace>) -> Result<HostedWorkspace, ApiError> {
    w.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such hosted workspace"))
}

pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
) -> Result<Json<Workspaces>, ApiError> {
    enabled(&app)?;
    let (project, _) = membership(&app, &account, &project, false).await?;
    let workspaces = app.db.hosted_workspaces(&project)?;
    Ok(Json(Workspaces { project, workspaces }))
}

pub async fn create(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path(project): Path<String>,
    Json(b): Json<NewWorkspace>,
) -> Result<(StatusCode, Json<HostedWorkspace>), ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project, true).await?;
    let name = b.name.trim();
    if !super::good_name(name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name: 1-64 letters, digits, '-', '_' and '.', starting with a letter or digit",
        ));
    }
    b.spec.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?;
    let id = super::new_id();
    let w = match app.db.add_hosted_workspace(&id, &project, name, &b.spec, &account, MAX_PER_PROJECT, now_ms())? {
        Added::Made(w) => *w,
        Added::NameTaken => {
            return Err(err(StatusCode::CONFLICT, &format!("this project has a hosted workspace called {name}")));
        }
        Added::Full => {
            return Err(err(
                StatusCode::CONFLICT,
                &format!("a project has at most {MAX_PER_PROJECT} hosted workspaces: delete one first"),
            ));
        }
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
    if w.state == "paused" && super::ask(&app, &w.id, Ask::Wake)? {
        return Ok(Json(found(app.db.hosted_workspace(&project, &w.id)?)?));
    }
    Ok(Json(w))
}

pub async fn rebuild(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project, true).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    if !super::ask(&app, &w.id, Ask::Rebuild)? {
        return Err(err(StatusCode::CONFLICT, "this hosted workspace is being deleted"));
    }
    info!(%account, %project, workspace = %w.id, "asked to rebuild a hosted workspace");
    Ok(Json(found(app.db.hosted_workspace(&project, &w.id)?)?))
}

pub async fn delete(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id)): Path<(String, String)>,
) -> Result<Json<HostedWorkspace>, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project, false).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    super::ask(&app, &w.id, Ask::Delete)?;
    // Its thread sessions end now (M256), not when their jobs next look.
    app.db.stop_workspace_sessions(&w.id, now_ms())?;
    info!(%account, %project, workspace = %w.id, "asked to delete a hosted workspace");
    app.analytics.capture(&account, "hosted workspace deleted", json!({}));
    Ok(Json(found(app.db.hosted_workspace(&project, &w.id)?)?))
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
    let project = changer(&app, &account, &project, false).await?;
    let w = found(app.db.hosted_workspace(&project, &id)?)?;
    spec.check().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e))?;
    let w = found(app.db.set_hosted_spec(&project, &w.id, &spec, now_ms())?)?;
    info!(%account, %project, workspace = %w.id, rebuild_needed = w.rebuild_needed, "changed a hosted workspace's spec");
    Ok(Json(w))
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
/// name, or `GIT_TOKEN` and `GIT_USERNAME` to clone with.
pub async fn put_secret(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, name)): Path<(String, String, String)>,
    Json(b): Json<SecretValue>,
) -> Result<StatusCode, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project, false).await?;
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
    app.db.set_hosted_secret(&w.id, &name, Some(&sealed), now_ms())?;
    info!(%account, %project, workspace = %w.id, name, "set a hosted workspace's secret");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_secret(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, id, name)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    enabled(&app)?;
    let project = changer(&app, &account, &project, false).await?;
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
        projects.push((id, "Personal".to_owned()));
    }
    let ids = app.cache.teams_of(&*app.hub, &account).await?;
    let mut teams: Vec<_> = app.cache.teams(&*app.hub, &ids).await?.into_values().collect();
    teams.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    for t in teams.into_iter().filter(|t| t.role(&account).is_some()) {
        projects.push((app.db.team_project(&t.id, &t.name, now_ms())?, t.name.clone()));
    }
    for (project, name) in projects {
        for w in app.db.hosted_workspaces(&project)? {
            workspaces.push(Listed { workspace: w, project_name: name.clone() });
        }
    }
    Ok(Json(AllWorkspaces { workspaces }))
}
