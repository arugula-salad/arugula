//! Hosted workspaces' routes (M179), for a project's members: a person,
//! a CLI device, or a machine with a live `cloud.drive` grant acting for
//! its owner ([`CloudDriver`]), as the project's linked repos take them.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/workspaces` | members |
//! | `POST /api/projects/{project}/workspaces` | owners and editors, on `ARUGULA_HOSTED_ACCOUNTS` |
//! | `GET /api/projects/{project}/workspaces/{id}` (open: wakes a paused box) | members |
//! | `POST /api/projects/{project}/workspaces/{id}/rebuild` | owners and editors, on `ARUGULA_HOSTED_ACCOUNTS` |
//! | `DELETE /api/projects/{project}/workspaces/{id}` | owners and editors |
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
    info!(%account, %project, workspace = %w.id, "asked to delete a hosted workspace");
    app.analytics.capture(&account, "hosted workspace deleted", json!({}));
    Ok(Json(found(app.db.hosted_workspace(&project, &w.id)?)?))
}
