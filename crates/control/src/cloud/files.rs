//! `/api/cloud/workspaces/{id}/{files,file,diff,git-status}`: a Cloud
//! workspace's disk, read-only, proxied to Managoat's reads of its current
//! machine (S40, M118). Managoat redacts what they return and never resumes
//! a parked machine for one, so a read here never wakes it either: control
//! only ever sends these GETs. Every lookup is the caller's own: another
//! account's id is 404.
use super::{enabled, provider::ReadRefusal};
use crate::{ApiError, App, auth::Session, err, refusal};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::Arc;

/// Managoat's cap on `max_bytes`.
const MAX_BYTES: u64 = 4_194_304;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesQuery {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileQuery {
    path: String,
    #[serde(default)]
    max_bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffQuery {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    staged: Option<bool>,
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    max_bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitStatusQuery {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    untracked: Option<String>,
}

/// A directory listing: `{path, entries: [{name, type, size}], truncated}`.
pub async fn files(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Query(q): Query<FilesQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut query = vec![];
    push_path(&mut query, q.path)?;
    read(&app, &session, &id, "files", query, &["path", "entries", "truncated"]).await
}

/// One file, redacted: `{path, size, truncated, encoding, content}`.
pub async fn file(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut query = vec![];
    push_path(&mut query, Some(q.path))?;
    push_max_bytes(&mut query, q.max_bytes)?;
    read(&app, &session, &id, "file", query, &["path", "size", "truncated", "encoding", "content"]).await
}

/// `git diff` of the repository holding `path`, redacted:
/// `{path, repo_root, staged, ref, diff, truncated}`.
pub async fn diff(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Query(q): Query<DiffQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut query = vec![];
    push_path(&mut query, q.path)?;
    if q.staged == Some(true) {
        query.push(("staged", "true".to_owned()));
    }
    if let Some(git_ref) = q.git_ref {
        if git_ref.is_empty() || git_ref.len() > 256 || git_ref.chars().any(char::is_control) {
            return Err(refusal(StatusCode::UNPROCESSABLE_ENTITY, "invalid_ref", "that isn't a commit, branch or tag"));
        }
        query.push(("ref", git_ref));
    }
    push_max_bytes(&mut query, q.max_bytes)?;
    read(&app, &session, &id, "diff", query, &["path", "repo_root", "staged", "ref", "diff", "truncated"]).await
}

/// `git status` of the repository holding `path`, untracked files too:
/// `{path, repo_root, branch, untracked, entries: [{path, index, worktree,
/// renamed_from}], truncated}`.
pub async fn git_status(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Query(q): Query<GitStatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut query = vec![];
    push_path(&mut query, q.path)?;
    if let Some(untracked) = q.untracked {
        if !matches!(untracked.as_str(), "normal" | "all" | "no") {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "untracked is normal, all or no"));
        }
        query.push(("untracked", untracked));
    }
    read(
        &app,
        &session,
        &id,
        "git-status",
        query,
        &["path", "repo_root", "branch", "untracked", "entries", "truncated"],
    )
    .await
}

/// Managoat confines the path to the machine's roots; control only refuses
/// what can't be one.
fn push_path(query: &mut Vec<(&'static str, String)>, path: Option<String>) -> Result<(), ApiError> {
    let Some(path) = path else { return Ok(()) };
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(refusal(StatusCode::UNPROCESSABLE_ENTITY, "invalid_path", "that isn't a path on the machine"));
    }
    query.push(("path", path));
    Ok(())
}

fn push_max_bytes(query: &mut Vec<(&'static str, String)>, max_bytes: Option<u64>) -> Result<(), ApiError> {
    let Some(n) = max_bytes else { return Ok(()) };
    if n == 0 || n > MAX_BYTES {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "max_bytes is 1–4194304"));
    }
    query.push(("max_bytes", n.to_string()));
    Ok(())
}

async fn read(
    app: &App,
    session: &Session,
    id: &str,
    read: &str,
    query: Vec<(&'static str, String)>,
    fields: &[&str],
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(app, &session.account)?;
    let w = app
        .db
        .cloud_workspace(&session.account, id)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))?;
    if w.status == "deleting" {
        return Err(refusal(StatusCode::CONFLICT, "deleting", "this Cloud workspace is being deleted"));
    }
    let Some(machine) = app.db.cloud_workspace_machine(&session.account, id)? else {
        return Err(refusal(StatusCode::CONFLICT, "not_ready", "this Cloud workspace's machine isn't made yet"));
    };
    let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let value = cloud.provider.read_cloud_workspace(&machine, read, &query).await.map_err(refused)?;
    // Only the documented fields: nothing else Managoat adds reaches a client.
    let picked: Map<String, Value> =
        fields.iter().filter_map(|f| value.get(*f).map(|v| ((*f).to_owned(), v.clone()))).collect();
    Ok(Json(Value::Object(picked)))
}

/// Managoat's refusal as control's own words, with its code as the reason.
fn refused(e: anyhow::Error) -> ApiError {
    let Some(r) = e.downcast_ref::<ReadRefusal>() else {
        return err(StatusCode::BAD_GATEWAY, "could not reach the Cloud workspace's machine; try again");
    };
    match (r.status, r.code) {
        (409, _) if r.parked => refusal(
            StatusCode::CONFLICT,
            "parked",
            "this Cloud workspace is parked; reading it doesn't wake it, a prompt to one of its tabs does",
        ),
        (409, _) => refusal(
            StatusCode::CONFLICT,
            "not_ready",
            "this Cloud workspace's machine isn't ready: it's setting up, failed or lost",
        ),
        (503, _) => refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "sandbox_unavailable",
            "this Cloud workspace's machine is busy being parked or changed; try again shortly",
        ),
        (404, Some("ref_not_found")) => {
            refusal(StatusCode::NOT_FOUND, "ref_not_found", "no such commit, branch or tag")
        }
        (404, _) => {
            refusal(StatusCode::NOT_FOUND, "path_not_found", "nothing is there on the Cloud workspace's machine")
        }
        (422, Some(code)) => refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            match code {
                "path_outside_sandbox" => {
                    "that path is outside /home/sprite, the repositories and the tabs' directories"
                }
                "not_a_directory" => "that path isn't a directory",
                "is_a_directory" => "that path is a directory",
                "path_unreadable" => "the machine can't read that path",
                "not_a_repository" => "no git repository holds that path",
                "invalid_ref" => "that isn't a commit, branch or tag",
                _ => "that isn't a path on the machine",
            },
        ),
        (422, None) => err(StatusCode::UNPROCESSABLE_ENTITY, "Managoat couldn't read that"),
        _ => err(StatusCode::BAD_GATEWAY, "could not read the Cloud workspace's machine; try again"),
    }
}
