//! Managed cloud execution, separate from the opaque terminal relay.
//! The cloud service processes prompts and results; terminal relay encryption
//! and device authorization remain unchanged.
pub mod files;
pub mod migration;
pub mod notifications;
pub mod provider;
pub mod rebuild;
pub mod secrets;
pub mod tabs;
pub mod worker;
pub mod workspaces;

use crate::{
    ApiError, App,
    auth::{CloudDriver, Session},
    db::cloud::{AgentDefinition, CloudAgent},
    err,
};
use anyhow::{Result, bail};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
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

/// A runtime and model a tab may run on (S40, decision 4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CloudModel {
    pub runtime: String,
    pub model: String,
}

/// Whether `runtime` drives `model`, a canonical `provider/model_id`.
pub fn model_fits_runtime(runtime: &str, model: &str) -> bool {
    let prefix = match runtime {
        "claude" => "anthropic/",
        "codex" => "openai/",
        "gemini" => "google/",
        _ => return false,
    };
    model.starts_with(prefix) && model.len() > prefix.len() && !model.chars().any(|c| c.is_whitespace())
}

/// `ARUGULA_CLOUD_MODELS`: `runtime:model` pairs, the first the default.
/// Each pair's platform inference must be set up on the Managoat account.
pub fn parse_models(pairs: &[String]) -> Result<Vec<CloudModel>> {
    let mut models: Vec<CloudModel> = vec![];
    for pair in pairs.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let Some((runtime, model)) = pair.split_once(':') else {
            bail!("ARUGULA_CLOUD_MODELS takes runtime:model pairs, not {pair:?}");
        };
        let model = CloudModel { runtime: runtime.trim().into(), model: model.trim().into() };
        if !model_fits_runtime(&model.runtime, &model.model) {
            bail!("cloud model {:?} must match its runtime {:?}", model.model, model.runtime);
        }
        if models.contains(&model) {
            bail!("ARUGULA_CLOUD_MODELS lists {pair:?} twice");
        }
        models.push(model);
    }
    Ok(models)
}

/// What tabs may run on: `ARUGULA_CLOUD_MODELS`, or else the one pair
/// `ARUGULA_CLOUD_RUNTIME`:`ARUGULA_CLOUD_MODEL`, so a deploy from before
/// the list keeps working. With the list set, the other two are unused.
pub fn configured_models(model: Option<String>, runtime: String, pairs: &[String]) -> Result<Vec<CloudModel>> {
    let models = parse_models(pairs)?;
    if !models.is_empty() {
        return Ok(models);
    }
    let Some(model) = model.filter(|m| !m.trim().is_empty()) else {
        bail!("ARUGULA_CLOUD_MODEL or ARUGULA_CLOUD_MODELS is required for cloud agents");
    };
    if !model_fits_runtime(&runtime, &model) {
        bail!("cloud model must match the configured runtime");
    }
    Ok(vec![CloudModel { runtime, model }])
}

pub struct Cloud {
    pub provider: provider::Provider,
    pub secret_cipher: Option<secrets::SecretCipher>,
    pub accounts: Vec<String>,
    /// What a tab may run on; never empty.
    pub models: Vec<CloudModel>,
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

pub async fn catalog(State(app): State<Arc<App>>, CloudDriver(session): CloudDriver) -> Result<Json<Value>, ApiError> {
    enabled(&app, &session.account)?;
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(&session.account)?);
    Ok(Json(json!({ "agents": agents })))
}

/// The runtimes and models **+ Agent** offers, the first the default.
pub async fn models(State(app): State<Arc<App>>, CloudDriver(session): CloudDriver) -> Result<Json<Value>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    Ok(Json(json!({ "models": cloud.models })))
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

/// A curated agent or one of the account's.
fn find_agent(app: &App, account: &str, id: &str) -> Result<CloudAgent, ApiError> {
    let mut agents = curated_agents();
    agents.extend(app.db.cloud_agents(account)?);
    agents.into_iter().find(|a| a.id == id).ok_or_else(|| err(StatusCode::NOT_FOUND, "agent not found"))
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

/// A past run's pending requests: none once it's stopped, but the
/// transcript view still reads them.
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
/// Answer a request in a tab's conversation (`id`), whose Managoat
/// conversation is `conversation`.
async fn deliver_answer(
    app: &App,
    account: &str,
    id: &str,
    conversation: &str,
    request: &str,
    answer: Answer,
) -> Result<Json<Value>, ApiError> {
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
    cloud.provider.answer(conversation, request, body).await.map_err(|e| {
        if let Some(provider::HttpFailure(code, _)) = e.downcast_ref::<provider::HttpFailure>() {
            if *code == 409 {
                let _ = app.db.resolve_cloud_request(account, id, request);
                return err(StatusCode::CONFLICT, "request already answered or expired");
            }
            if *code == 422 {
                return err(StatusCode::UNPROCESSABLE_ENTITY, "answer does not match the request");
            }
        }
        err(StatusCode::BAD_GATEWAY, "could not deliver the answer; refresh before retrying")
    })?;
    app.db.resolve_cloud_request(account, id, request)?;
    app.analytics.capture(account, "cloud request answered", json!({ "answer": kind }));
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn test_app() -> Arc<App> {
        unstarted_app().start()
    }

    /// [`test_app`] before it's started, to change first.
    pub(crate) fn unstarted_app() -> App {
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
            models: vec![
                CloudModel { runtime: "claude".into(), model: "anthropic/test-model".into() },
                CloudModel { runtime: "codex".into(), model: "openai/test-model".into() },
            ],
        });
        app
    }

    #[test]
    fn models_fall_back_to_the_one_configured_pair() {
        let pair = |runtime: &str, model: &str| CloudModel { runtime: runtime.into(), model: model.into() };
        let models = configured_models(Some("anthropic/one".into()), "claude".into(), &[]).unwrap();
        assert_eq!(models, [pair("claude", "anthropic/one")]);
        let listed = ["claude:anthropic/a".to_owned(), " codex:openai/b ".into(), "gemini:google/c".into()];
        let models = configured_models(None, "claude".into(), &listed).unwrap();
        assert_eq!(models, [pair("claude", "anthropic/a"), pair("codex", "openai/b"), pair("gemini", "google/c")]);
        // The list wins when both are set.
        let models = configured_models(Some("openai/old".into()), "codex".into(), &listed).unwrap();
        assert_eq!(models[0], pair("claude", "anthropic/a"));
        for (model, runtime, pairs) in [
            (None, "claude", vec![]),
            (Some("openai/x"), "claude", vec![]),
            (None, "claude", vec!["codex:anthropic/x".to_owned()]),
            (None, "claude", vec!["anthropic/x".to_owned()]),
            (None, "claude", vec!["acp:anthropic/x".to_owned()]),
            (None, "claude", vec!["claude:anthropic/x".to_owned(), "claude:anthropic/x".into()]),
        ] {
            assert!(
                configured_models(model.map(str::to_owned), runtime.into(), &pairs).is_err(),
                "{model:?} {pairs:?}"
            );
        }
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
