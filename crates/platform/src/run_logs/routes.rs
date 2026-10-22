//! A run's artifacts for a project's members (M213), from the run-log
//! bucket with every machine asleep. Its log is read through M216's
//! `GET /api/runs/{run}/log`, which [`crate::run_log::page`] fills from the
//! bucket.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/runs/{run}/artifacts` (each with its digest and a signed `GET`) | members |
//! | `GET /api/projects/{project}/runs/{run}/artifacts/{name}` (a redirect to a signed `GET`) | members |
//!
//! `{project}` is an id, or `personal`. A project someone isn't a member
//! of, a run that isn't the project's, and what's past the run-log days
//! are 404. Without a bucket, the list says it's off and the other is 404.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::run_logs::{ArtifactLink, RunArtifacts};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Redirect,
};

use super::GET_EXPIRES_S;
use crate::{Platform, auth::Person, db::runs::Run, forge::links::membership};

/// Run `run` in the project `project` names, if `account` is a member.
async fn members_run(app: &Platform, account: &str, project: &str, run: &str) -> Result<Run, ApiError> {
    let (project, _) = membership(app, account, project, false).await?;
    app.db.run(run)?.filter(|r| r.project == project).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such run"))
}

fn off() -> ApiError {
    err(StatusCode::NOT_FOUND, "this platform keeps no run logs")
}

/// `GET /api/projects/{project}/runs/{run}/artifacts`.
pub async fn list(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, run)): Path<(String, String)>,
) -> Result<Json<RunArtifacts>, ApiError> {
    let run = members_run(&app, &account, &project, &run).await?;
    let now = arugula_e2e::now_ms();
    let expires_at = now + GET_EXPIRES_S * 1000;
    let Some(b) = &app.run_logs else {
        return Ok(Json(RunArtifacts { run: run.id, on: false, artifacts: vec![], expires_at }));
    };
    let since = now.saturating_sub(b.ttl_ms);
    let artifacts = app
        .db
        .run_artifacts(&run.id)?
        .into_iter()
        .filter(|a| a.created_at >= since)
        .map(|a| ArtifactLink {
            url: b.download(&a.bucket_key, &a.name, GET_EXPIRES_S, now),
            name: a.name,
            size: a.size,
            digest: a.digest,
        })
        .collect();
    Ok(Json(RunArtifacts { run: run.id, on: true, artifacts, expires_at }))
}

/// `GET /api/projects/{project}/runs/{run}/artifacts/{name}`: to a signed
/// `GET` of it.
pub async fn artifact(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path((project, run, name)): Path<(String, String, String)>,
) -> Result<Redirect, ApiError> {
    let run = members_run(&app, &account, &project, &run).await?;
    let b = app.run_logs.as_ref().ok_or_else(off)?;
    let now = arugula_e2e::now_ms();
    let a = app
        .db
        .run_artifacts(&run.id)?
        .into_iter()
        .find(|a| a.name == name && a.created_at >= now.saturating_sub(b.ttl_ms))
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such artifact"))?;
    Ok(Redirect::temporary(&b.download(&a.bucket_key, &a.name, GET_EXPIRES_S, now)))
}
