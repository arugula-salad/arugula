//! `/api/cloud/workspaces`: Cloud workspaces, each a Managoat workspace on an
//! environment and a vault control keeps for it alone (S40, decision 1). The
//! routes only write the record and its job; `worker` talks to Managoat.
//! Every lookup is the caller's own: another account's id is 404.
use super::enabled;
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::cloud::{CloudWorkspace, CloudWorkspaceSpec, SpecRules},
};
use arugula_cloud_wire::CloudWorkspaces;
use arugula_control_common::{ApiError, err};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub async fn list(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
) -> Result<Json<CloudWorkspaces>, ApiError> {
    Ok(Json(CloudWorkspaces { cloud_workspaces: app.db.cloud_workspaces(&session.account)? }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub request_key: String,
    pub spec: CloudWorkspaceSpec,
}

pub async fn create(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
    Json(mut input): Json<Create>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    input.spec.normalize().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    if !input.spec.secret_names().is_empty() && cloud.secret_cipher.is_none() {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"));
    }
    let project = super::personal_project(&app, &session.account).await?;
    let created = app
        .db
        .create_cloud_workspace(&session.account, &project, &input.request_key, &input.spec, arugula_e2e::now_ms())
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
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    Ok(Json(found(&app, &session, &id)?))
}

/// Ends its conversations and destroys its machine, then its vault and
/// environment, in the worker. The answer says `deleting`.
pub async fn delete(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    found(&app, &session, &id)?;
    app.db.delete_cloud_workspace(&session.account, &id)?;
    app.analytics.capture(&session.account, "cloud workspace deleted", json!({}));
    Ok(Json(found(&app, &session, &id)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub spec: CloudWorkspaceSpec,
}

/// Save new settings (the whole spec) and send them: the environment's
/// build fields, which the next rebuild uses, and the vault's secrets,
/// which are live at once. The answer's `pending` says what a rebuild
/// would change. A save Managoat didn't get yet says `settings_applied:
/// false`, and the worker sends it again.
pub async fn update(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
    Json(mut input): Json<Update>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let cloud = enabled(&app, &session.account)?;
    found(&app, &session, &id)?;
    input.spec.normalize().map_err(|e| err(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()))?;
    if !input.spec.secret_names().is_empty() && cloud.secret_cipher.is_none() {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "cloud secrets are not configured"));
    }
    let Some(sending) = Sending::take(&id) else {
        return Err(err(StatusCode::CONFLICT, "its settings are being saved; try again in a moment"));
    };
    let saved = app
        .db
        .save_cloud_workspace_settings(&session.account, &id, &input.spec, arugula_e2e::now_ms())
        .map_err(|e| match e.to_string().as_str() {
            "cloud workspace not found" => err(StatusCode::NOT_FOUND, &e.to_string()),
            "cloud workspace is being deleted" => err(StatusCode::CONFLICT, &e.to_string()),
            "cloud workspace is not made yet" => {
                err(StatusCode::CONFLICT, "it's still being made; change its settings once it's setting up")
            }
            "required cloud secret is missing" => {
                err(StatusCode::UNPROCESSABLE_ENTITY, "connect every secret it names before saving")
            }
            _ => e.into(),
        })?;
    if saved {
        match send_settings(&app, &id).await {
            Ok(true) => {}
            Ok(false) => {
                return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "Managoat refused these settings; nothing changed"));
            }
            Err(error) => tracing::warn!(cloud_workspace = %id, %error, "cloud workspace settings will be sent again"),
        }
    }
    drop(sending);
    let w = found(&app, &session, &id)?;
    if saved {
        app.analytics.capture(&session.account, "cloud workspace settings saved", json!({"pending": w.pending.len()}));
    }
    Ok(Json(w))
}

/// Settings sends in flight, by Cloud workspace: one at a time, the
/// route's or the worker's, so the environment ends as the last save. A
/// rebuild takes it too, so it builds from settings Managoat has.
static SENDING: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

pub(super) struct Sending(String);

impl Sending {
    pub(super) fn take(id: &str) -> Option<Self> {
        SENDING.lock().unwrap().insert(id.to_owned()).then(|| Self(id.to_owned()))
    }
}

impl Drop for Sending {
    fn drop(&mut self) {
        SENDING.lock().unwrap().remove(&self.0);
    }
}

/// The worker, on a live Cloud workspace: send a save Managoat didn't get.
pub(super) async fn resend_settings(app: &Platform, id: &str) {
    let Some(_sending) = Sending::take(id) else {
        return;
    };
    if let Err(error) = send_settings(app, id).await {
        tracing::warn!(cloud_workspace = %id, %error, "cloud workspace settings will be sent again");
    }
}

/// Send the saved settings, if they're unsent: PATCH the environment, then
/// put and delete the vault's secrets, then read whether the machine is now
/// behind. False when Managoat refused the environment, and the saved
/// settings went back to the ones it has. Every step can be sent again.
async fn send_settings(app: &Platform, id: &str) -> anyhow::Result<bool> {
    use anyhow::Context;
    let cloud = app.cloud.as_ref().context("cloud is disabled")?;
    let Some(s) = app.db.unsent_cloud_workspace_settings(id)? else {
        return Ok(true);
    };
    let p = &cloud.provider;
    let environment = match p.update_environment(&s.environment, s.spec.environment_update()).await {
        Ok(environment) => environment,
        Err(e)
            if e.downcast_ref::<super::provider::HttpFailure>()
                .is_some_and(|f| (400..500).contains(&f.0) && !matches!(f.0, 408 | 429)) =>
        {
            tracing::warn!(cloud_workspace = %id, error = %e, "managoat refused cloud workspace settings");
            app.db.refused_cloud_workspace_settings(id)?;
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let wanted = s.spec.secret_names();
    let mut held = s.vault_secrets.clone();
    for name in wanted.iter().filter(|n| !s.vault_secrets.contains(n)) {
        let (project, sealed) = app.db.cloud_secret(&s.account, name)?.context("a secret it names was deleted")?;
        let cipher = cloud.secret_cipher.as_ref().context("cloud secrets key is unavailable")?;
        let value = crate::keys::open_cloud(&app.db.c(), cipher, &s.account, name, project.as_deref(), &sealed)?;
        p.put_vault_secret(&s.vault, name, &value).await?;
        held.push(name.clone());
        app.db.cloud_workspace_vault_secrets(id, &held)?;
    }
    for name in s.vault_secrets.iter().filter(|n| !wanted.contains(n)) {
        p.delete_vault_secret(&s.vault, name).await?;
        held.retain(|n| n != name);
        app.db.cloud_workspace_vault_secrets(id, &held)?;
    }
    app.db.sent_cloud_workspace_settings(id, &s.spec, environment["revision"].as_str(), arugula_e2e::now_ms())?;
    // Managoat says at once whether the machine is behind; the poll would
    // say so later.
    if let Ok(w) = p.workspace(&s.workspace).await {
        let changed: Vec<String> = w["changed_fields"]
            .as_array()
            .map(|f| f.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        app.db.observe_cloud_workspace_staleness(
            id,
            w["revision"].as_str(),
            w["built_from_revision"].as_str(),
            w["stale"].as_bool().unwrap_or(false),
            &changed,
        )?;
    }
    Ok(true)
}

fn found(app: &Platform, session: &Person, id: &str) -> Result<CloudWorkspace, ApiError> {
    app.db.cloud_workspace(&session.account, id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn another_accounts_cloud_workspace_is_not_found_on_every_route() {
        let app = crate::cloud::tests::test_app();
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db
            .put_cloud_secret(
                "a",
                "prj_a",
                "GITHUB_TOKEN",
                // Sealed the way it was before M209: it still opens.
                &crate::keys::Sealed {
                    key_version: crate::keys::MASTER,
                    bytes: cipher.seal("a", "GITHUB_TOKEN", "token").unwrap(),
                },
            )
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/cloud/workspaces", listener.local_addr().unwrap());
        let server =
            tokio::spawn(async move { axum::serve(listener, crate::cloud::tests::router(app)).await.unwrap() });
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

    /// #1094: a daemon drives its own account's Cloud workspaces (for the
    /// agents on its machine) and reaches no one else's; settings, secrets,
    /// deleting and the agent library stay a person's.
    #[tokio::test]
    async fn a_daemon_drives_its_accounts_cloud_workspaces_only() {
        let hub = Arc::new(crate::cloud::tests::TestHub::default());
        let app = crate::cloud::tests::unstarted_app_with(hub.clone()).start();
        let (da, db) = (format!("a {}", hub.daemon("a", "geek")), format!("b {}", hub.daemon("b", "geek")));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server =
            tokio::spawn(async move { axum::serve(listener, crate::cloud::tests::router(app)).await.unwrap() });
        let client = reqwest::Client::new();
        let call = |daemon: &str, method: &str, path: &str, body: Option<Value>| {
            let bytes = body.map(|b| serde_json::to_vec(&b).unwrap()).unwrap_or_default();
            let mut req =
                client.request(method.parse().unwrap(), format!("{base}{path}")).header("x-test-daemon", daemon);
            if !bytes.is_empty() {
                req = req.header("content-type", "application/json").body(bytes);
            }
            async move {
                let r = req.send().await.unwrap();
                (r.status(), r.json::<Value>().await.unwrap_or_default())
            }
        };
        let body = json!({"request_key":"one","spec":{"name":"Agent box"}});
        let (status, created) = call(&da, "POST", "/api/cloud/workspaces", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let id = created["id"].as_str().unwrap().to_owned();
        let (status, listed) = call(&da, "GET", "/api/cloud/workspaces", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["cloud_workspaces"][0]["id"], id.as_str());
        // A person of account a sees what a's daemon made.
        let mine: Value = client
            .get(format!("{base}/api/cloud/workspaces/{id}"))
            .header("cookie", "ilg_session=session-a")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(mine["name"], "Agent box");
        assert_eq!(call(&da, "GET", "/api/cloud/agents", None).await.0, StatusCode::OK);
        assert_eq!(call(&da, "GET", "/api/cloud/models", None).await.0, StatusCode::OK);
        let open =
            json!({"request_key":"t","agent_id":"curated_coding","runtime":"claude","model":"anthropic/test-model"});
        let (status, opened) = call(&da, "POST", &format!("/api/cloud/workspaces/{id}/tabs"), Some(open.clone())).await;
        assert_eq!(status, StatusCode::OK, "{opened}");
        let tab = format!("/api/cloud/workspaces/{id}/tabs/{}", opened["id"].as_str().unwrap());
        assert_eq!(call(&da, "GET", &tab, None).await.1["status"], "opening");

        // Account b's daemon: none of a's, on any route it may use.
        let (_, listed) = call(&db, "GET", "/api/cloud/workspaces", None).await;
        assert_eq!(listed["cloud_workspaces"], json!([]));
        for (method, path, body) in [
            ("GET", format!("/api/cloud/workspaces/{id}"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/tabs"), None),
            ("POST", format!("/api/cloud/workspaces/{id}/tabs"), Some(open)),
            ("GET", format!("/api/cloud/workspaces/{id}/files"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/file?path=%2Fhome%2Fsprite%2Fx"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/diff"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/git-status"), None),
            ("GET", format!("/api/cloud/workspaces/{id}/setup-log"), None),
            ("POST", format!("/api/cloud/workspaces/{id}/rebuild"), Some(json!({}))),
            ("POST", format!("/api/cloud/workspaces/{id}/retry"), None),
            ("GET", tab.clone(), None),
            ("DELETE", tab.clone(), None),
            ("POST", format!("{tab}/prompt"), Some(json!({"prompt":"hi"}))),
            ("POST", format!("{tab}/interrupt"), None),
            ("GET", format!("{tab}/events"), None),
            ("GET", format!("{tab}/requests"), None),
            ("POST", format!("{tab}/requests/r1"), Some(json!({"option_id":"allow"}))),
        ] {
            let (status, said) = call(&db, method, &path, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {said}");
        }

        // What stays a person's: a daemon is refused there, even its own.
        for (method, path, body) in [
            ("PATCH", format!("/api/cloud/workspaces/{id}"), Some(json!({"spec":{"name":"x"}}))),
            ("DELETE", format!("/api/cloud/workspaces/{id}"), None),
            ("DELETE", format!("/api/cloud/workspaces/{id}/rebuild"), None),
            ("GET", "/api/cloud/secrets".to_owned(), None),
            ("POST", "/api/cloud/agents".to_owned(), Some(json!({"name":"x","instructions":"y"}))),
            ("GET", "/api/cloud/past-tabs".to_owned(), None),
        ] {
            let (status, said) = call(&da, method, &path, body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {said}");
        }
        assert_eq!(call(&da, "GET", &format!("/api/cloud/workspaces/{id}"), None).await.1["status"], "provisioning");
        server.abort();
    }
}
