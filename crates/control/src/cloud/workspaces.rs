//! `/api/cloud/workspaces`: Cloud workspaces, each a Managoat workspace on an
//! environment and a vault control keeps for it alone (S40, decision 1). The
//! routes only write the record and its job; `worker` talks to Managoat.
//! Every lookup is the caller's own: another account's id is 404.
use super::enabled;
use crate::{
    ApiError, App,
    auth::Session,
    db::cloud::{CloudWorkspace, CloudWorkspaceSpec},
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

pub async fn list(State(app): State<Arc<App>>, session: Session) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({"cloud_workspaces": app.db.cloud_workspaces(&session.account)?})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub request_key: String,
    pub spec: CloudWorkspaceSpec,
}

pub async fn create(
    State(app): State<Arc<App>>,
    session: Session,
    Json(mut input): Json<Create>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    input.spec.normalize().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    if !input.spec.secret_names().is_empty() && cloud.secret_cipher.is_none() {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"));
    }
    let created = app
        .db
        .create_cloud_workspace(&session.account, &input.request_key, &input.spec, arugula_e2e::now_ms())
        .map_err(|e| match e.to_string().as_str() {
            "required cloud secret is missing" => {
                err(StatusCode::UNPROCESSABLE_ENTITY, "connect every secret it names before creating it")
            }
            "request key already used with different input" => err(StatusCode::CONFLICT, &e.to_string()),
            "invalid request key" => err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()),
            _ => e.into(),
        })?;
    let spec = &input.spec;
    app.analytics.capture(
        &session.account,
        "cloud workspace created",
        json!({
            "repositories": spec.repositories.len(), "secrets": spec.secret_names().len(),
            "packages": spec.packages.apt.len() + spec.packages.npm.len(),
            "setup_script": !spec.setup_script.is_empty(), "env_vars": spec.env_vars.len(),
        }),
    );
    Ok(Json(created))
}

pub async fn show(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    Ok(Json(found(&app, &session, &id)?))
}

/// Ends its conversations and destroys its machine, then its vault and
/// environment, in the worker. The answer says `deleting`.
pub async fn delete(
    State(app): State<Arc<App>>,
    session: Session,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    found(&app, &session, &id)?;
    app.db.delete_cloud_workspace(&session.account, &id)?;
    app.analytics.capture(&session.account, "cloud workspace deleted", json!({}));
    Ok(Json(found(&app, &session, &id)?))
}

fn found(app: &App, session: &Session, id: &str) -> Result<CloudWorkspace, ApiError> {
    app.db.cloud_workspace(&session.account, id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn another_accounts_cloud_workspace_is_not_found_on_every_route() {
        let app = crate::cloud::tests::test_app();
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db.put_cloud_secret("a", "GITHUB_TOKEN", &cipher.seal("a", "GITHUB_TOKEN", "token").unwrap()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/cloud/workspaces", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() });
        let client = reqwest::Client::new();
        let (a, b, c) = ("ilg_session=session-a", "ilg_session=session-b", "ilg_session=session-c");
        let body = json!({"request_key":"one","spec":{
            "name":" App box ",
            "repositories":[{"url":"https://github.com/example/app.git","secret":"GITHUB_TOKEN"},
                            {"url":"https://github.com/example/api","ref":"main","mount":"/work/api"}],
            "packages":{"apt":["ffmpeg"]},
        }});
        assert_eq!(client.post(&base).json(&body).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            client.post(&base).header("cookie", c).json(&body).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        // Account b has no GITHUB_TOKEN of its own.
        assert_eq!(
            client.post(&base).header("cookie", b).json(&body).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let response = client.post(&base).header("cookie", a).json(&body).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = response.json().await.unwrap();
        let id = created["id"].as_str().unwrap();
        assert!(id.starts_with("cw_"));
        assert_eq!(created["name"], "App box");
        assert_eq!(created["status"], "provisioning");
        assert_eq!(created["saved_spec"]["repositories"][0]["mount"], "/home/sprite/app");
        assert_eq!(created["saved_spec"]["repositories"][1]["mount"], "/work/api");
        assert!(created.get("environment").is_none() && created.get("provider_id").is_none());
        // The same request again is the same Cloud workspace; other settings
        // under that key are a conflict.
        let again: Value =
            client.post(&base).header("cookie", a).json(&body).send().await.unwrap().json().await.unwrap();
        assert_eq!(again["id"], id);
        let mut changed = body.clone();
        changed["spec"]["name"] = json!("Another");
        assert_eq!(
            client.post(&base).header("cookie", a).json(&changed).send().await.unwrap().status(),
            StatusCode::CONFLICT
        );
        let item = format!("{base}/{id}");
        let listed: Value = client.get(&base).header("cookie", b).send().await.unwrap().json().await.unwrap();
        assert_eq!(listed["cloud_workspaces"], json!([]));
        assert_eq!(client.get(&item).header("cookie", b).send().await.unwrap().status(), StatusCode::NOT_FOUND);
        assert_eq!(client.delete(&item).header("cookie", b).send().await.unwrap().status(), StatusCode::NOT_FOUND);
        let listed: Value = client.get(&base).header("cookie", a).send().await.unwrap().json().await.unwrap();
        assert_eq!(listed["cloud_workspaces"][0]["id"], id);
        assert_eq!(listed["cloud_workspaces"][0]["status"], "provisioning");
        let deleting: Value = client.delete(&item).header("cookie", a).send().await.unwrap().json().await.unwrap();
        assert_eq!(deleting["status"], "deleting");
        let shown: Value = client.get(&item).header("cookie", a).send().await.unwrap().json().await.unwrap();
        assert_eq!(shown["status"], "deleting");
        for bad in [
            json!({"name":"x","repositories":[{"url":"http://github.com/example/app"}]}),
            json!({"name":"x","repositories":[{"url":"https://user:pw@github.com/example/app"}]}),
            json!({"name":"x","repositories":[{"url":"https://github.com/a/app"},{"url":"https://github.com/b/app"}]}),
            json!({"name":"x","repositories":[{"url":"https://github.com/a/app","mount":"/home/sprite/../etc"}]}),
            json!({"name":"x","packages":{"pip":["requests"]}}),
            json!({"name":"x","env_vars":{"MANAGOAT_API_KEY":"x"}}),
            json!({"name":"x","setup_timeout_seconds":901}),
            json!({"name":"x","networking":{"type":"open"}}),
            json!({"name":""}),
        ] {
            let status = client
                .post(&base)
                .header("cookie", a)
                .json(&json!({"request_key":"bad","spec":bad}))
                .send()
                .await
                .unwrap()
                .status();
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        }
        server.abort();
    }
}
