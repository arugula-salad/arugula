//! Managed cloud execution, separate from the opaque terminal relay.
//! The cloud service processes prompts and results; terminal relay encryption
//! and device authorization remain unchanged.
pub mod migration;
pub mod notifications;
pub mod provider;
pub mod secrets;
pub mod worker;
pub mod workspaces;

use crate::{
    ApiError, App,
    auth::Session,
    db::cloud::{AgentDefinition, CloudAgent, RunLimits},
    err,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// A clone URL Managoat may fetch: HTTPS, with no credentials in it.
pub fn https_repository(repository: &str) -> bool {
    url::Url::parse(repository).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

pub struct Cloud {
    pub provider: provider::Provider,
    pub secret_cipher: Option<secrets::SecretCipher>,
    pub accounts: Vec<String>,
    pub model: String,
    pub runtime: String,
    pub limits: RunLimits,
}

fn enabled<'a>(app: &'a App, account: &str) -> Result<&'a Cloud, ApiError> {
    let cloud = app.cloud.as_ref().ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud agents are not configured"))?;
    if !cloud.accounts.iter().any(|a| a == account || a == "*") {
        return Err(err(StatusCode::FORBIDDEN, "cloud agents are in a limited beta"));
    }
    Ok(cloud)
}

fn curated_agents() -> Vec<CloudAgent> {
    [
        ("coding", "Coding agent", "Implement the requested change in the checked-out repository. Read its instructions first, run relevant checks, and explain the changes and verification."),
        ("review", "Code reviewer", "Review the checked-out repository for the requested concerns. Explain concrete findings with file references, severity, and suggested fixes. Do not modify files unless asked."),
    ].into_iter().map(|(id, name, instructions)| CloudAgent {
        id: format!("curated_{id}"), version: 1, curated: true,
        definition: AgentDefinition { name: name.into(), instructions: instructions.into(), mcp_servers: Default::default(), skills: vec![], secret_names: vec![] },
    }).collect()
}

pub async fn catalog(State(app): State<Arc<App>>, session: Session) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(&session.account)?);
    Ok(Json(json!({ "agents": agents, "model": cloud.model,
        "limits": {"daily_runs": cloud.limits.daily_runs, "concurrent_runs": cloud.limits.concurrent_runs, "duration_ms": cloud.limits.duration_ms} })))
}

pub async fn create_agent(
    State(app): State<Arc<App>>,
    session: Session,
    Json(definition): Json<AgentDefinition>,
) -> Result<Json<CloudAgent>, ApiError> {
    enabled(&app, &session.account)?;
    definition.validate().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    let secrets = definition.secret_names.len();
    let agent = app.db.create_cloud_agent(&session.account, definition)?;
    app.analytics.capture(&session.account, "cloud agent created", json!({ "secrets": secrets }));
    Ok(Json(agent))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUpdate {
    version: u64,
    definition: AgentDefinition,
}

pub async fn update_agent(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
    Json(update): Json<AgentUpdate>,
) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    update.definition.validate().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    if !app.db.cloud_agents(&session.account)?.iter().any(|a| a.id == id) {
        return Err(err(StatusCode::NOT_FOUND, "agent not found"));
    }
    if !app.db.update_cloud_agent(&session.account, &id, update.version, &update.definition)? {
        return Err(err(StatusCode::CONFLICT, "agent changed; reload before saving"));
    }
    app.analytics.capture(&session.account, "cloud agent updated", json!({ "version": update.version + 1 }));
    Ok(Json(json!({"version": update.version + 1})))
}

pub async fn delete_agent(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    if !app.db.archive_cloud_agent(&session.account, &id)? {
        return Err(err(StatusCode::NOT_FOUND, "agent not found"));
    }
    app.analytics.capture(&session.account, "cloud agent deleted", json!({}));
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRun {
    pub request_key: String,
    pub agent_id: String,
    pub repository: String,
    #[serde(default)]
    pub git_ref: String,
    #[serde(default)]
    pub repository_secret: Option<String>,
    pub prompt: String,
}

pub async fn start_run(
    State(app): State<Arc<App>>,
    session: Session,
    Json(input): Json<StartRun>,
) -> Result<Json<crate::db::cloud::CloudRun>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    if !https_repository(&input.repository) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "repository must be an HTTPS URL without embedded credentials",
        ));
    }
    if input.git_ref.starts_with('-')
        || input.git_ref.chars().any(char::is_control)
        || input.git_ref.len() > 256
        || input.prompt.trim().is_empty()
        || input.prompt.len() > 64_000
        || input.request_key.is_empty()
        || input.request_key.len() > 128
    {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid ref, prompt, or request key"));
    }
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(&session.account)?);
    let agent = agents
        .into_iter()
        .find(|a| a.id == input.agent_id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "agent not found"))?;
    if input.repository_secret.as_ref().is_some_and(|name| !secrets::valid_name(name)) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid repository secret name"));
    }
    if (!agent.definition.secret_names.is_empty() || input.repository_secret.is_some()) && cloud.secret_cipher.is_none()
    {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"));
    }
    let props = json!({
        "agent_curated": agent.curated, "runtime": cloud.runtime, "model": cloud.model,
        "repository_secret": input.repository_secret.is_some(), "agent_secrets": agent.definition.secret_names.len(),
        "git_ref": !input.git_ref.is_empty(), "prompt_chars": input.prompt.chars().count(),
    });
    let spec = crate::db::cloud::RunSpec {
        agent_id: agent.id,
        agent_version: agent.version,
        agent: agent.definition,
        model: cloud.model.clone(),
        runtime: cloud.runtime.clone(),
        repository: input.repository,
        git_ref: input.git_ref,
        repository_secret: input.repository_secret,
        prompt: input.prompt,
    };
    let run = app
        .db
        .reserve_cloud_run(&session.account, &input.request_key, &spec, &cloud.limits, arugula_e2e::now_ms())
        .map_err(|e| match e.to_string().as_str() {
            "daily cloud run limit reached" | "concurrent cloud run limit reached" => {
                err(StatusCode::TOO_MANY_REQUESTS, &e.to_string())
            }
            "required cloud secret is missing" => {
                err(StatusCode::UNPROCESSABLE_ENTITY, "connect every required secret before starting a run")
            }
            "request key already used with different input" => err(StatusCode::CONFLICT, &e.to_string()),
            _ => e.into(),
        })?;
    app.analytics.capture(&session.account, "cloud run started", props);
    Ok(Json(run))
}

pub async fn runs(State(app): State<Arc<App>>, session: Session) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({"runs":app.db.cloud_runs(&session.account)?})))
}

pub async fn run(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<crate::db::cloud::CloudRun>, ApiError> {
    Ok(Json(app.db.cloud_run(&session.account, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "run not found"))?))
}

pub async fn cancel_run(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if app.db.cloud_run(&session.account, &id)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "run not found"));
    }
    app.db.cancel_cloud_run(&session.account, &id)?;
    app.analytics.capture(&session.account, "cloud run cancelled", json!({}));
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
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<EventQuery>,
) -> Result<Json<Value>, ApiError> {
    if app.db.cloud_run(&session.account, &id)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "run not found"));
    }
    Ok(Json(json!({"events":app.db.cloud_events(&session.account,&id,query.after)?})))
}

pub async fn requests(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if app.db.cloud_run(&session.account, &id)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "run not found"));
    }
    Ok(Json(json!({"requests":app.db.cloud_requests(&session.account,&id)?})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    option_id: Option<String>,
    action: Option<String>,
    content: Option<Value>,
}
pub async fn answer(
    State(app): State<Arc<App>>,
    session: Session,
    Path((id, request)): Path<(String, String)>,
    Json(answer): Json<Answer>,
) -> Result<Json<Value>, ApiError> {
    let conversation = app
        .db
        .cloud_conversation(&session.account, &id, arugula_e2e::now_ms())?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "active run not found"))?;
    let cloud =
        app.cloud.as_ref().ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "cloud execution is unavailable"))?;
    let kind = if answer.option_id.is_some() { "option" } else { answer.action.as_deref().unwrap_or("") };
    let kind = kind.to_owned();
    let body = match (answer.option_id, answer.action) {
        (Some(option), None) if !option.is_empty() && option.len() <= 256 && answer.content.is_none() => {
            json!({"option_id":option})
        }
        (None, Some(action)) if matches!(action.as_str(), "accept" | "decline" | "cancel") => {
            json!({"action":action,"content":answer.content})
        }
        _ => return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid answer")),
    };
    cloud.provider.answer(&conversation, &request, body).await.map_err(|e| {
        if let Some(provider::HttpFailure(code, _)) = e.downcast_ref::<provider::HttpFailure>() {
            if *code == 409 {
                let _ = app.db.resolve_cloud_request(&session.account, &id, &request);
                return err(StatusCode::CONFLICT, "request already answered or expired");
            }
            if *code == 422 {
                return err(StatusCode::UNPROCESSABLE_ENTITY, "answer does not match the request");
            }
        }
        err(StatusCode::BAD_GATEWAY, "could not deliver the answer; refresh the run before retrying")
    })?;
    app.db.resolve_cloud_request(&session.account, &id, &request)?;
    app.analytics.capture(&session.account, "cloud request answered", json!({ "answer": kind }));
    Ok(Json(json!({})))
}

pub async fn list_secrets(State(app): State<Arc<App>>, session: Session) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    Ok(Json(json!({"names":app.db.cloud_secret_names(&session.account)?,"available":cloud.secret_cipher.is_some()})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretInput {
    value: String,
}
pub async fn put_secret(
    State(app): State<Arc<App>>,
    session: Session,
    Path(name): Path<String>,
    Json(input): Json<SecretInput>,
) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    let cipher = cloud
        .secret_cipher
        .as_ref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"))?;
    if !secrets::valid_name(&name) || input.value.is_empty() || input.value.len() > 64_000 {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid secret name or value"));
    }
    if app.db.cloud_secret_names(&session.account)?.len() >= 100
        && !app.db.cloud_secret_names(&session.account)?.contains(&name)
    {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "secret limit reached"));
    }
    app.db.put_cloud_secret(&session.account, &name, &cipher.seal(&session.account, &name, &input.value)?)?;
    app.analytics.capture(&session.account, "cloud secret saved", json!({}));
    Ok(Json(json!({"name":name})))
}
pub async fn delete_secret(
    State(app): State<Arc<App>>,
    session: Session,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    app.db.delete_cloud_secret(&session.account, &name)?;
    app.analytics.capture(&session.account, "cloud secret deleted", json!({}));
    Ok(Json(json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn test_app() -> Arc<App> {
        let db = crate::db::Db::memory();
        let now = arugula_e2e::now_ms();
        for id in ["a", "b", "c"] {
            db.account_for("test", id, id, id, now).unwrap();
        }
        db.add_session(&crate::auth::hash("session-a"), "a", now, now + 60_000).unwrap();
        db.add_session(&crate::auth::hash("session-b"), "b", now, now + 60_000).unwrap();
        db.add_session(&crate::auth::hash("session-c"), "c", now, now + 60_000).unwrap();
        let mut app = App::for_tests("https://control.example");
        app.db = db;
        app.cloud = Some(Cloud {
            secret_cipher: Some(secrets::SecretCipher::new(&"01".repeat(32)).unwrap()),
            provider: provider::Provider::new("https://provider.example", "platform-key".into()).unwrap(),
            accounts: vec!["a".into(), "b".into()],
            model: "anthropic/test-model".into(),
            runtime: "claude".into(),
            limits: RunLimits { daily_runs: 10, concurrent_runs: 2, duration_ms: 60_000 },
        });
        Arc::new(app)
    }

    #[tokio::test]
    async fn catalog_http_auth_isolation_and_edit_conflicts() {
        let app = test_app();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/cloud/agents", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
        let client = reqwest::Client::new();
        assert_eq!(client.get(&base).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            client.get(&base).header("cookie", "ilg_session=session-c").send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let definition =
            json!({"name":"My agent", "instructions":"Review code", "mcp_servers":{}, "skills":[], "secret_names":[]});
        assert_eq!(
            client
                .post(&base)
                .header("cookie", "ilg_session=session-a")
                .header("origin", "https://other.example")
                .json(&definition)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response =
            client.post(&base).header("cookie", "ilg_session=session-a").json(&definition).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = response.json().await.unwrap();
        let item = format!("{base}/{}", created["id"].as_str().unwrap());
        let b: Value =
            client.get(&base).header("cookie", "ilg_session=session-b").send().await.unwrap().json().await.unwrap();
        assert_eq!(b["agents"].as_array().unwrap().len(), 2);
        assert!(b["agents"].as_array().unwrap().iter().all(|a| a["curated"] == true));
        assert!(!b.to_string().contains("platform-key"));
        assert_eq!(
            client.delete(&item).header("cookie", "ilg_session=session-b").send().await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let update = json!({"version":1, "definition":definition});
        assert_eq!(
            client.put(&item).header("cookie", "ilg_session=session-a").json(&update).send().await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            client.put(&item).header("cookie", "ilg_session=session-a").json(&update).send().await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            client.delete(&item).header("cookie", "ilg_session=session-a").send().await.unwrap().status(),
            StatusCode::OK
        );
        let a: Value =
            client.get(&base).header("cookie", "ilg_session=session-a").send().await.unwrap().json().await.unwrap();
        assert_eq!(a["agents"].as_array().unwrap().len(), 2);
        server.abort();
    }
}
