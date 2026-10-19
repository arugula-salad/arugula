//! `/api/cloud/workspaces/{id}/tabs`: a Cloud workspace's tabs, each one
//! conversation on its machine with a Managoat agent of its own (S40,
//! decision 4). Opening and closing only write the record and its job;
//! `worker` makes and ends the agent and the conversation. Prompts,
//! interrupts and answers go straight to the tab's conversation. Every
//! lookup is the caller's own: another account's id is 404.
use super::{deliver_answer, enabled, find_agent, provider::HttpFailure};
use crate::{
    ApiError, App,
    auth::Session,
    db::cloud::{CloudTab, CloudTabSpec, CloudWorkspace, CloudWorkspaceSpec},
    err,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// A tab's `cwd` (decision 5): a repository's mount or a directory under
/// one, which may be outside `/home/sprite`, or a directory under
/// `/home/sprite` outside Managoat's config homes. `spec` is `on_disk`'s.
/// The settings the machine was built from: a repository saved since isn't
/// on its disk until a rebuild. The saved ones until the first build.
fn on_disk(w: &CloudWorkspace) -> &CloudWorkspaceSpec {
    w.built_spec.as_ref().unwrap_or(&w.saved_spec)
}

fn cwd_allowed(spec: &CloudWorkspaceSpec, cwd: &str) -> bool {
    let segments: Vec<&str> = cwd.split('/').skip(1).collect();
    if !cwd.starts_with('/')
        || cwd.len() > 256
        || cwd.chars().any(char::is_control)
        || segments.iter().any(|s| s.is_empty() || matches!(*s, "." | ".."))
    {
        return false;
    }
    let under = |root: &str| cwd == root || cwd.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'));
    !under("/home/sprite/.managoat") && (under("/home/sprite") || spec.repositories.iter().any(|r| under(&r.mount)))
}

fn cloud_workspace(app: &App, session: &Session, id: &str) -> Result<CloudWorkspace, ApiError> {
    app.db.cloud_workspace(&session.account, id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))
}

fn found(app: &App, session: &Session, id: &str, tab: &str) -> Result<CloudTab, ApiError> {
    app.db.cloud_tab(&session.account, id, tab)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "tab not found"))
}

/// The tab's Managoat conversation, once it's launched and while it's open.
fn conversation(app: &App, session: &Session, id: &str, tab: &str) -> Result<String, ApiError> {
    let found = found(app, session, id, tab)?;
    app.db.cloud_tab_conversation(&session.account, id, tab)?.ok_or_else(|| {
        let message = if found.status == "closing" { "this tab is closing" } else { "this tab is still opening" };
        err(StatusCode::CONFLICT, message)
    })
}

pub async fn list(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    cloud_workspace(&app, &session, &id)?;
    Ok(Json(json!({"cloud_tabs": app.db.cloud_tabs(&session.account, &id)?})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Open {
    pub request_key: String,
    pub agent_id: String,
    pub runtime: String,
    pub model: String,
    /// The first repository's mount when left out.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

pub async fn open(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Json(input): Json<Open>,
) -> Result<Json<CloudTab>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    let w = cloud_workspace(&app, &session, &id)?;
    if !cloud.models.iter().any(|m| m.runtime == input.runtime && m.model == input.model) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "that runtime and model aren't offered"));
    }
    let built = on_disk(&w);
    let cwd = input.cwd.unwrap_or_else(|| {
        built.repositories.first().map(|r| r.mount.clone()).unwrap_or_else(|| "/home/sprite".into())
    });
    if !cwd_allowed(built, &cwd) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "cwd must be the mount of a repository on the machine, or a directory under one or under /home/sprite (a repository saved since arrives with a rebuild)",
        ));
    }
    if input.prompt.as_ref().is_some_and(|p| p.trim().is_empty() || p.len() > 64_000) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "a first prompt must contain 1–64000 bytes"));
    }
    let agent = find_agent(&app, &session.account, &input.agent_id)?;
    // A tab's secrets are its Cloud workspace's vault: it brings none of its own.
    let held = w.saved_spec.secret_names();
    if agent.definition.secret_names.iter().any(|name| !held.contains(name)) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "add every secret this agent needs to the Cloud workspace first",
        ));
    }
    let props = json!({
        "agent_curated": agent.curated, "runtime": input.runtime, "model": input.model,
        "cwd_is_mount": built.repositories.iter().any(|r| r.mount == cwd),
        "prompt": input.prompt.is_some(), "setting_up": w.status == "provisioning",
    });
    let spec = CloudTabSpec {
        agent_id: agent.id,
        agent_version: agent.version,
        agent: agent.definition,
        runtime: input.runtime,
        model: input.model,
        cwd,
        prompt: input.prompt,
    };
    let tab = app
        .db
        .create_cloud_tab(&session.account, &id, &input.request_key, &spec, arugula_e2e::now_ms())
        .map_err(|e| match e.to_string().as_str() {
            "cloud workspace not found" => err(StatusCode::NOT_FOUND, "cloud workspace not found"),
            "cloud workspace is being deleted" => err(StatusCode::CONFLICT, "this Cloud workspace is being deleted"),
            "cloud workspace has no machine" => err(
                StatusCode::CONFLICT,
                "Managoat never made this Cloud workspace's machine; delete it and create it again",
            ),
            "request key already used with different input" => err(StatusCode::CONFLICT, &e.to_string()),
            "invalid request key" => err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()),
            _ => e.into(),
        })?;
    app.analytics.capture(&session.account, "cloud tab opened", props);
    Ok(Json(tab))
}

pub async fn show(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
) -> Result<Json<CloudTab>, ApiError> {
    Ok(Json(found(&app, &session, &id, &tab)?))
}

/// Ends its conversation and deletes its agent, in the worker. The other
/// tabs, and the machine, carry on. The answer says `closing`.
pub async fn close(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
) -> Result<Json<CloudTab>, ApiError> {
    found(&app, &session, &id, &tab)?;
    app.db.close_cloud_tab(&session.account, &id, &tab)?;
    app.analytics.capture(&session.account, "cloud tab closed", json!({}));
    Ok(Json(found(&app, &session, &id, &tab)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    prompt: String,
}

/// A follow-up. A parked machine wakes on it. A response lost on the way
/// back is a 502, never sent again here: the client reads the tab's events
/// before it retries.
pub async fn prompt(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
    Json(input): Json<Prompt>,
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    if input.prompt.trim().is_empty() || input.prompt.len() > 64_000 {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "a prompt must contain 1–64000 bytes"));
    }
    let conversation = conversation(&app, &session, &id, &tab)?;
    let request = format!("{tab}-{}", hex::encode(arugula_e2e::random::<8>()));
    cloud.provider.prompt(&conversation, &input.prompt, &request).await.map_err(|e| {
        match e.downcast_ref::<HttpFailure>().map(|f| f.0) {
            Some(409) => err(StatusCode::CONFLICT, "this tab can't take a prompt now; wait for its turn to end"),
            Some(422) => err(StatusCode::UNPROCESSABLE_ENTITY, "Managoat refused the prompt"),
            _ => err(StatusCode::BAD_GATEWAY, "could not deliver the prompt; refresh the tab before retrying"),
        }
    })?;
    app.analytics.capture(
        &session.account,
        "cloud tab prompted",
        json!({ "prompt_chars": input.prompt.chars().count() }),
    );
    Ok(Json(json!({})))
}

/// Stops the turn in flight. The tab stays open.
pub async fn interrupt(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    let conversation = conversation(&app, &session, &id, &tab)?;
    cloud.provider.interrupt(&conversation).await.map_err(|e| match e.downcast_ref::<HttpFailure>().map(|f| f.0) {
        Some(409) => err(StatusCode::CONFLICT, "no turn is running"),
        _ => err(StatusCode::BAD_GATEWAY, "could not interrupt the turn; refresh the tab before retrying"),
    })?;
    app.analytics.capture(&session.account, "cloud tab interrupted", json!({}));
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
pub struct EventQuery {
    #[serde(default)]
    after: u64,
}

pub async fn events(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
    Query(query): Query<EventQuery>,
) -> Result<Json<Value>, ApiError> {
    found(&app, &session, &id, &tab)?;
    Ok(Json(json!({"events": app.db.cloud_events(&session.account, &tab, query.after)?})))
}

pub async fn requests(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    found(&app, &session, &id, &tab)?;
    Ok(Json(json!({"requests": app.db.cloud_requests(&session.account, &tab)?})))
}

pub async fn answer(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, tab, request)): Path<(String, String, String)>,
    Json(answer): Json<super::Answer>,
) -> Result<Json<Value>, ApiError> {
    let conversation = conversation(&app, &session, &id, &tab)?;
    deliver_answer(&app, &session.account, &tab, &conversation, &request, answer).await
}
