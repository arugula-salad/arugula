//! Durable, bounded reconciliation steps. Browser connections never own work.
use crate::{
    App,
    db::cloud::{CloudWorkspaceJob, Job},
};
use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn start(app: Arc<App>) {
    for _ in 0..4 {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = tick(&app).await {
                    tracing::warn!(%error, "cloud reconciliation failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }
}

pub async fn tick(app: &App) -> Result<()> {
    tick_at(app, arugula_e2e::now_ms()).await
}

async fn tick_at(app: &App, now: u64) -> Result<()> {
    let Some(_) = &app.cloud else {
        return Ok(());
    };
    let run = tick_run(app, now).await;
    tick_cloud_workspace(app, now).await.and(run)
}

async fn tick_run(app: &App, now: u64) -> Result<()> {
    let Some(mut job) = app.db.claim_cloud_job(now)? else {
        return Ok(());
    };
    if let Err(error) = step(app, &mut job, now).await {
        let rejected = error
            .downcast_ref::<super::provider::HttpFailure>()
            .is_some_and(|e| e.1 == Method::POST && (400..500).contains(&e.0) && e.0 != 408);
        if rejected
            && matches!(
                job.execution.stage.as_str(),
                "environment_creating" | "agent_creating" | "conversation_creating"
            )
        {
            job.execution.outcome = Some("failed".into());
            job.execution.stage = "cleanup_agent".into();
            return save(
                app,
                &job,
                "cleaning_up",
                Some(&json!({"message":"Cloud setup was rejected by the execution service."})),
                now,
            );
        }
        // Keep the durable intent on failure. In particular, never repeat a
        // creation when its response may have been lost.
        app.db.checkpoint_cloud_job(&job, "recovering", None, Some(now + 10_000))?;
        tracing::warn!(run = %job.run.id, %error, "cloud run will reconcile");
    }
    Ok(())
}

fn save(app: &App, job: &Job, state: &str, result: Option<&Value>, now: u64) -> Result<()> {
    app.db.checkpoint_cloud_job(job, state, result, Some(now + 2_000))
}

async fn step(app: &App, job: &mut Job, now: u64) -> Result<()> {
    let cloud = app.cloud.as_ref().context("cloud is disabled")?;
    let p = &cloud.provider;
    let stopped = job.cancelled || now >= job.run.deadline;
    if stopped && job.execution.outcome.is_none() {
        job.execution.outcome = Some(if job.cancelled { "cancelled" } else { "expired" }.into());
    }
    // Never discard an in-flight creation: reconcile it before cleaning up.
    if stopped && matches!(job.execution.stage.as_str(), "" | "environment_ready" | "agent_ready") {
        job.execution.stage = "cleanup_agent".into();
        return save(app, job, "cleaning_up", Some(&json!({"message":"Run stopped before execution"})), now);
    }
    let spec = &job.run.spec;
    match job.execution.stage.as_str() {
        "" => {
            job.execution.stage = "environment_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let root = if spec.runtime == "gemini" { "/tmp/gemini-workspace/repo" } else { "/home/sprite/repo" };
            let mut repository = json!({"url":spec.repository,"mount_path":root});
            if !spec.git_ref.is_empty() {
                repository["ref"] = json!(spec.git_ref);
            }
            if let Some(name) = &spec.repository_secret {
                repository["secret_key"] = json!(name);
            }
            let environment = p
                .create(
                    "environments",
                    json!({
                        "name":job.run.id,"metadata":{"arugula_run":job.run.id},
                        "repositories":[repository],
                        "setup_script":format!("git -C {root} update-ref refs/arugula/base HEAD"),
                    }),
                )
                .await?;
            job.execution.environment = Some(environment);
            job.execution.stage = "environment_ready".into();
            save(app, job, "provisioning", None, now)
        }
        "environment_creating" => {
            job.execution.environment = p.find("environments", &job.run.id).await?;
            if job.execution.environment.is_some() {
                job.execution.stage = "environment_ready".into();
            }
            save(app, job, "recovering", None, now)
        }
        "environment_ready" => {
            let secrets = app.db.cloud_run_secrets(&job.account, &job.run.id)?;
            if let Some((name, sealed)) =
                secrets.iter().find(|(name, _)| !job.execution.secrets_uploaded.contains(name))
            {
                let cipher = cloud.secret_cipher.as_ref().context("cloud secrets key is unavailable")?;
                let value = cipher.open(&job.account, name, sealed)?;
                let environment = job.execution.environment.as_deref().context("missing environment")?;
                p.query(
                    Method::POST,
                    &["api", "environments", environment, "secrets"],
                    &[],
                    Some(json!({"key":name,"value":value})),
                )
                .await?;
                job.execution.secrets_uploaded.push(name.clone());
                return save(app, job, "provisioning", None, now);
            }
            job.execution.stage = "agent_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let id = p.create("agents", json!({
                "name":job.run.id,"metadata":{"arugula_run":job.run.id},
                "environment_id":job.execution.environment,
                "allowed_environment_ids":[],"allowed_vault_ids":[],
                "runtime":spec.runtime,"model":spec.model,
                "system":format!("{}\nWork in the repository at repo/ under your working directory.",spec.agent.instructions),
                "mcp_servers":spec.agent.mcp_servers,"skills":spec.agent.skills,
                "permission_policy":{"default":"ask"},
            })).await?;
            job.execution.agent = Some(id);
            job.execution.stage = "agent_ready".into();
            save(app, job, "provisioning", None, now)
        }
        "agent_creating" => {
            job.execution.agent = p.find("agents", &job.run.id).await?;
            if job.execution.agent.is_some() {
                job.execution.stage = "agent_ready".into();
            }
            save(app, job, "recovering", None, now)
        }
        "agent_ready" => {
            job.execution.stage = "conversation_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let value = p
                .launch(
                    job.execution.agent.as_deref().context("missing agent")?,
                    job.execution.environment.as_deref().context("missing environment")?,
                    &job.run.id,
                    &spec.prompt,
                )
                .await?;
            job.execution.conversation = Some(value["id"].as_str().context("missing conversation")?.into());
            job.execution.stage = "running".into();
            save(app, job, "running", None, now)
        }
        "conversation_creating" => {
            job.execution.conversation = p.find("conversations", &job.run.id).await?;
            if job.execution.conversation.is_some() {
                job.execution.stage = "running".into();
            }
            save(app, job, "recovering", None, now)
        }
        "running" => {
            let id = job.execution.conversation.as_deref().context("missing conversation")?;
            let c = p.conversation(id).await?;
            let state = c["status"].as_str().context("missing conversation status")?;
            // Interrupting an already idle turn may return a conflict. Read
            // status first; a racing completion still proceeds to preservation.
            if stopped
                && matches!(state, "running" | "pending")
                && let Err(error) = p.interrupt(id).await
                && !error.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 409)
            {
                return Err(error);
            }
            let events = p.events(id, job.execution.cursor).await?;
            app.db.append_cloud_events(job, &events)?;
            // A failed turn can leave its reusable provider conversation idle.
            // Persist failure before draining subsequent event pages.
            if job.execution.outcome.is_none()
                && events.iter().any(|event| event["stage"] == "turn" && event["state"] == "failed")
            {
                job.execution.outcome = Some("failed".into());
            }
            if let Some(cursor) = events.iter().filter_map(|e| e["id"].as_u64()).max() {
                job.execution.cursor = cursor;
            }
            job.execution.sandbox = c["sandbox_id"].as_str().map(str::to_owned);
            if matches!(state, "idle" | "failed" | "terminated") && events.len() < 1000 {
                if job.execution.outcome.is_none() {
                    job.execution.outcome = Some(if state == "idle" { "completed" } else { "failed" }.into());
                }
                job.execution.stage = "collecting".into();
                save(app, job, "saving_results", Some(&json!({"usage":c["usage_total"]})), now)
            } else {
                let waiting = !app.db.cloud_requests(&job.account, &job.run.id)?.is_empty();
                save(app, job, if waiting { "needs_input" } else { "running" }, None, now)
            }
        }
        "collecting" => {
            let Some(sandbox) = job.execution.sandbox.as_deref() else {
                job.execution.stage = "terminating".into();
                return save(
                    app,
                    job,
                    "cleaning_up",
                    Some(
                        &json!({"message":"No sandbox was provisioned","usage":job.run.result.as_ref().map(|r| &r["usage"])}),
                    ),
                    now,
                );
            };
            let sandbox_state = p.query(Method::GET, &["api", "sandboxes", sandbox], &[], None).await?;
            match sandbox_state["status"].as_str() {
                Some("failed" | "terminated") => {
                    if job.execution.outcome.as_deref() == Some("completed") {
                        job.execution.outcome = Some("failed".into());
                    }
                    job.execution.stage = "terminating".into();
                    let mut result = job.run.result.clone().unwrap_or_else(|| json!({}));
                    result["message"] = json!(
                        "The execution environment became unavailable before files could be saved. The captured transcript is retained."
                    );
                    result["files_preserved"] = json!(false);
                    return save(app, job, "cleaning_up", Some(&result), now);
                }
                Some("suspended") => {
                    let conversation = job.execution.conversation.as_deref().context("missing conversation")?;
                    p.query(Method::POST, &["api", "conversations", conversation, "wake"], &[], None).await?;
                    return save(app, job, "saving_results", None, now);
                }
                Some("ready") if sandbox_state["transition"].is_null() => {}
                _ => return save(app, job, "saving_results", None, now),
            }
            let diff = p
                .query(
                    Method::GET,
                    &["api", "sandboxes", sandbox, "diff"],
                    &[("path", "repo"), ("ref", "refs/arugula/base"), ("max_bytes", "4194304")],
                    None,
                )
                .await?;
            let status = p
                .query(
                    Method::GET,
                    &["api", "sandboxes", sandbox, "git-status"],
                    &[("path", "repo"), ("untracked", "all")],
                    None,
                )
                .await?;
            // Retain the environment if preservation would lose data.
            if diff["truncated"] == true || status["truncated"] == true {
                bail!("cloud result exceeds preservation limit");
            }
            // The provider's textual diff omits binary bodies. Keep the
            // sandbox until an operator can recover those files.
            if diff["diff"].as_str().is_some_and(|text| {
                text.lines().any(|line| line.starts_with("Binary files ") && line.ends_with(" differ"))
            }) {
                bail!("binary changes require manual preservation");
            }
            let result = json!({"usage":job.run.result.as_ref().map(|r| &r["usage"]),"diff":diff,"git_status":status,"files":{}});
            job.execution.stage = "collecting_files".into();
            save(app, job, "saving_results", Some(&result), now)
        }
        "collecting_files" => {
            let sandbox = job.execution.sandbox.as_deref().context("missing sandbox")?;
            let mut result = job.run.result.clone().context("missing saved diff")?;
            let entries = result["git_status"]["entries"].as_array().context("missing git status")?;
            let next = entries
                .iter()
                .filter(|e| e["index"] == "untracked" || e["worktree"] == "untracked")
                .filter_map(|e| e["path"].as_str())
                .find(|path| result["files"].get(*path).is_none())
                .map(str::to_owned);
            if let Some(path) = next {
                let full = format!("repo/{path}");
                let file = p
                    .query(
                        Method::GET,
                        &["api", "sandboxes", sandbox, "file"],
                        &[("path", &full), ("max_bytes", "4194304")],
                        None,
                    )
                    .await?;
                if file["truncated"] == true {
                    bail!("cloud file exceeds preservation limit");
                }
                result["files"][&path] = file;
                save(app, job, "saving_results", Some(&result), now)
            } else {
                job.execution.stage = "terminating".into();
                save(app, job, "cleaning_up", Some(&result), now)
            }
        }
        "terminating" => {
            if let Some(id) = &job.execution.conversation {
                let c = match p.conversation(id).await {
                    Ok(c) => c,
                    Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                        json!({"status":"terminated"})
                    }
                    Err(e) => return Err(e),
                };
                if c["status"] != "terminated" {
                    p.terminate(id).await?;
                    return save(app, job, "cleaning_up", None, now);
                }
            }
            job.execution.stage = "cleanup_agent".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_agent" => {
            if let Some(id) = &job.execution.agent {
                p.remove("agents", id).await?;
            }
            job.execution.agent = None;
            job.execution.stage = "cleanup_environment".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_environment" => {
            if let Some(id) = &job.execution.environment {
                p.remove("environments", id).await?;
            }
            job.execution.environment = None;
            job.execution.stage = "done".into();
            let outcome = job.execution.outcome.as_deref().unwrap_or("failed");
            save(app, job, outcome, None, now)?;
            app.analytics.capture(
                &job.account,
                "cloud run finished",
                json!({
                    "outcome": outcome, "agent_curated": spec.agent_id.starts_with("curated_"),
                    "runtime": spec.runtime, "model": spec.model,
                    "minutes": now.saturating_sub(job.run.created) / 60_000,
                }),
            );
            Ok(())
        }
        _ => bail!("unknown cloud execution stage"),
    }
}

/// How soon a Cloud workspace's job runs again: soon while it's being set up
/// or deleted, then often enough to see it park, wake or fall behind.
const CLOUD_WORKSPACE_BUSY_MS: u64 = 3_000;
const CLOUD_WORKSPACE_SETTLED_MS: u64 = 15_000;

async fn tick_cloud_workspace(app: &App, now: u64) -> Result<()> {
    let Some(mut job) = app.db.claim_cloud_workspace_job(now)? else {
        return Ok(());
    };
    if let Err(error) = step_cloud_workspace(app, &mut job, now).await {
        // A refused create made nothing. 408, 429 and 5xx (fleet_full among
        // them) may have, or may yet: those reconcile.
        let rejected = error
            .downcast_ref::<super::provider::HttpFailure>()
            .is_some_and(|e| e.1 == Method::POST && (400..500).contains(&e.0) && !matches!(e.0, 408 | 429));
        if rejected && job.execution.stage.ends_with("_creating") {
            job.execution.stage = "rejected".into();
            fail(&mut job, "Managoat refused to set up this Cloud workspace.");
            return save_cloud_workspace(app, &job, now);
        }
        app.db.checkpoint_cloud_workspace_job(&job, Some(now + 10_000))?;
        tracing::warn!(cloud_workspace = %job.cloud_workspace.id, %error, "cloud workspace will reconcile");
    }
    Ok(())
}

fn save_cloud_workspace(app: &App, job: &CloudWorkspaceJob, now: u64) -> Result<()> {
    let settled = job.execution.stage == "live" && job.cloud_workspace.status != "provisioning" && !job.deleting;
    let wait = if settled || matches!(job.execution.stage.as_str(), "rejected" | "lost") {
        CLOUD_WORKSPACE_SETTLED_MS
    } else {
        CLOUD_WORKSPACE_BUSY_MS
    };
    app.db.checkpoint_cloud_workspace_job(job, Some(now + wait))
}

fn fail(job: &mut CloudWorkspaceJob, error: &str) {
    job.cloud_workspace.status = "failed".into();
    job.cloud_workspace.error = Some(error.into());
}

/// Take in what Managoat says of its workspace. `error` is its failing step
/// and the end of what that printed, already redacted; the end is kept.
fn observe(job: &mut CloudWorkspaceJob, workspace: &Value) {
    let w = &mut job.cloud_workspace;
    if let Some(status @ ("provisioning" | "ready" | "failed")) = workspace["status"].as_str() {
        w.status = status.into();
    }
    let text = |key: &str| workspace[key].as_str().map(str::to_owned);
    w.sandbox_status = text("sandbox_status");
    w.error = text("error").map(|mut error| {
        if error.len() > 16_000 {
            let mut start = error.len() - 16_000;
            while !error.is_char_boundary(start) {
                start += 1;
            }
            error = error.split_off(start);
        }
        error
    });
    w.revision = text("revision");
    w.built_from_revision = text("built_from_revision");
    w.stale = workspace["stale"].as_bool().unwrap_or(false);
    w.changed_fields = workspace["changed_fields"]
        .as_array()
        .map(|fields| fields.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
}

/// Create (environment, vault and its secrets, workspace), then poll, then
/// delete (workspace, vault, environment). Each create is journalled before
/// Managoat is asked, and found again after a lost response.
async fn step_cloud_workspace(app: &App, job: &mut CloudWorkspaceJob, now: u64) -> Result<()> {
    let cloud = app.cloud.as_ref().context("cloud is disabled")?;
    let p = &cloud.provider;
    let id = job.cloud_workspace.id.clone();
    let stage = job.execution.stage.clone();
    // Never forget an in-flight creation: a `_creating` stage finds what it
    // made first, and goes on to delete from there.
    if job.deleting && !stage.ends_with("_creating") && !stage.starts_with("delete_") {
        job.execution.stage = "delete_workspace".into();
        return save_cloud_workspace(app, job, now);
    }
    let not_made = |job: &mut CloudWorkspaceJob, retry: &str| {
        job.execution.stage = if job.deleting { "delete_workspace" } else { retry }.into();
    };
    match stage.as_str() {
        "" => {
            job.execution.stage = "environment_creating".into();
            app.db.checkpoint_cloud_workspace_job(job, None)?;
            let mut body = job.cloud_workspace.saved_spec.environment();
            body["name"] = json!(id);
            body["metadata"] = json!({"arugula_workspace": id});
            let environment = p.create_value("environments", body).await?;
            environment_made(job, &environment);
            save_cloud_workspace(app, job, now)
        }
        "environment_creating" => {
            match p.find_cloud_workspace_resource("environments", &id).await? {
                Some(environment) => environment_made(job, &environment),
                None => not_made(job, ""),
            }
            save_cloud_workspace(app, job, now)
        }
        "environment_ready" => {
            job.execution.stage = "vault_creating".into();
            app.db.checkpoint_cloud_workspace_job(job, None)?;
            let vault = p.create("vaults", json!({"name": id, "metadata": {"arugula_workspace": id}})).await?;
            job.execution.vault = Some(vault);
            job.execution.stage = "vault_ready".into();
            save_cloud_workspace(app, job, now)
        }
        "vault_creating" => {
            match p.find_cloud_workspace_resource("vaults", &id).await? {
                Some(vault) => {
                    job.execution.vault = vault["id"].as_str().map(str::to_owned);
                    job.execution.stage = "vault_ready".into();
                }
                None => not_made(job, "environment_ready"),
            }
            save_cloud_workspace(app, job, now)
        }
        "vault_ready" => {
            let vault = job.execution.vault.clone().context("missing vault")?;
            let next = job
                .cloud_workspace
                .saved_spec
                .secret_names()
                .into_iter()
                .find(|name| !job.execution.secrets_uploaded.contains(name));
            if let Some(name) = next {
                let Some(sealed) = app.db.cloud_secret(&job.account, &name)? else {
                    job.execution.stage = "rejected".into();
                    fail(
                        job,
                        &format!(
                            "The secret {name} was deleted before this Cloud workspace was set up. Delete it and create it again."
                        ),
                    );
                    return save_cloud_workspace(app, job, now);
                };
                let cipher = cloud.secret_cipher.as_ref().context("cloud secrets key is unavailable")?;
                let value = cipher.open(&job.account, &name, &sealed)?;
                p.put_vault_secret(&vault, &name, &value).await?;
                job.execution.secrets_uploaded.push(name);
                return save_cloud_workspace(app, job, now);
            }
            let environment = job.execution.environment.clone().context("missing environment")?;
            job.execution.stage = "workspace_creating".into();
            app.db.checkpoint_cloud_workspace_job(job, None)?;
            let workspace = p.create_workspace(&id, &environment, &vault).await?;
            workspace_made(job, &workspace);
            save_cloud_workspace(app, job, now)
        }
        "workspace_creating" => {
            let environment = job.execution.environment.clone().context("missing environment")?;
            match p.find_workspace(&id, &environment).await? {
                Some(workspace) => workspace_made(job, &workspace),
                None => not_made(job, "vault_ready"),
            }
            save_cloud_workspace(app, job, now)
        }
        "live" => {
            let workspace = job.execution.workspace.clone().context("missing workspace")?;
            match p.workspace(&workspace).await {
                Ok(workspace) => observe(job, &workspace),
                Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                    job.execution.stage = "lost".into();
                    fail(job, "Managoat no longer has this Cloud workspace. Delete it and create it again.");
                }
                Err(e) => return Err(e),
            }
            save_cloud_workspace(app, job, now)
        }
        // Nothing to read: only a delete moves these on.
        "rejected" | "lost" => save_cloud_workspace(app, job, now),
        "delete_workspace" => {
            if let Some(workspace) = &job.execution.workspace {
                p.remove("workspaces", workspace).await?;
            }
            job.execution.workspace = None;
            job.execution.stage = "delete_vault".into();
            save_cloud_workspace(app, job, now)
        }
        // Managoat refuses (409) while its workspace still names it: retried.
        "delete_vault" => {
            if let Some(vault) = &job.execution.vault {
                p.remove("vaults", vault).await?;
            }
            job.execution.vault = None;
            job.execution.stage = "delete_environment".into();
            save_cloud_workspace(app, job, now)
        }
        "delete_environment" => {
            if let Some(environment) = &job.execution.environment {
                p.remove("environments", environment).await?;
            }
            job.execution.environment = None;
            app.db.finish_cloud_workspace_delete(job, now)
        }
        _ => bail!("unknown cloud workspace stage"),
    }
}

fn environment_made(job: &mut CloudWorkspaceJob, environment: &Value) {
    job.execution.environment = environment["id"].as_str().map(str::to_owned);
    job.saved_revision = environment["revision"].as_str().map(str::to_owned);
    job.execution.stage = "environment_ready".into();
}

fn workspace_made(job: &mut CloudWorkspaceJob, workspace: &Value) {
    job.execution.workspace = workspace["id"].as_str().map(str::to_owned);
    observe(job, workspace);
    job.execution.stage = "live".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode, Uri},
        response::IntoResponse,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Vec<(String, String, Value)>,
        environment: Option<Value>,
        terminated: bool,
        lose_create_response: bool,
        deny_lookup_once: bool,
        failed_sandbox: bool,
        truncate_diff: bool,
        binary_diff: bool,
        failed_turn: bool,
        // Cloud workspaces.
        vault: Option<Value>,
        workspace: Option<Value>,
        /// Collections whose next create succeeds but loses its response.
        lose: Vec<&'static str>,
        workspace_reads: u32,
        failed_setup: bool,
        vault_in_use_once: bool,
    }

    impl Fake {
        fn lost(&mut self, collection: &str) -> bool {
            let lost = self.lose.contains(&collection);
            self.lose.retain(|c| *c != collection);
            lost
        }
    }

    async fn handle(
        State(fake): State<Arc<Mutex<Fake>>>,
        method: axum::http::Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> axum::response::Response {
        assert_eq!(headers["authorization"], "Bearer mock-key");
        let mut fake = fake.lock().unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        fake.calls.push((method.to_string(), uri.to_string(), value.clone()));
        let path = uri.path();
        let data = match (method.as_str(), path) {
            ("POST", "/api/environments") => {
                let mut environment = value.clone();
                environment["id"] = json!("env1");
                environment["revision"] = json!("rev1");
                fake.environment = Some(environment.clone());
                if fake.lose_create_response || fake.lost("environments") {
                    fake.lose_create_response = false;
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                environment
            }
            ("PATCH", "/api/environments/env1") => json!({"id":"env1","revision":"rev2"}),
            ("POST", "/api/vaults") => {
                let mut vault = value.clone();
                vault["id"] = json!("vault1");
                fake.vault = Some(vault.clone());
                if fake.lost("vaults") {
                    return StatusCode::GATEWAY_TIMEOUT.into_response();
                }
                vault
            }
            ("GET", "/api/vaults") => json!(fake.vault.iter().collect::<Vec<_>>()),
            ("POST", "/api/vaults/vault1/secrets") => json!({"key":value["key"]}),
            ("POST", "/api/workspaces") => {
                let workspace = json!({
                    "id":"ws1","name":value["name"],"environment_id":value["environment_id"],"vault_id":value["vault_id"],
                    "status":"provisioning","error":null,"sandbox_status":"pending","revision":"rev1",
                    "built_from_revision":null,"stale":false,"changed_fields":[],
                });
                fake.workspace = Some(workspace.clone());
                if fake.lost("workspaces") {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                workspace
            }
            ("GET", "/api/workspaces") => json!(fake.workspace.iter().collect::<Vec<_>>()),
            ("GET", "/api/workspaces/ws1") => {
                let failed = fake.failed_setup;
                fake.workspace_reads += 1;
                let reads = fake.workspace_reads;
                let Some(workspace) = fake.workspace.as_mut() else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                // Set up on the second read.
                if reads >= 2 && failed {
                    workspace["status"] = json!("failed");
                    workspace["sandbox_status"] = Value::Null;
                    workspace["error"] = json!("setup_script exited 1: npm ERR! missing script: build");
                } else if reads >= 2 {
                    workspace["status"] = json!("ready");
                    workspace["sandbox_status"] = json!("ready");
                    workspace["built_from_revision"] = json!("rev1");
                }
                workspace.clone()
            }
            ("DELETE", "/api/workspaces/ws1") => {
                fake.workspace = None;
                return StatusCode::NO_CONTENT.into_response();
            }
            ("DELETE", "/api/vaults/vault1") if fake.vault_in_use_once => {
                fake.vault_in_use_once = false;
                return StatusCode::CONFLICT.into_response();
            }
            ("GET", "/api/environments") => {
                if fake.deny_lookup_once {
                    fake.deny_lookup_once = false;
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                json!(fake.environment.iter().collect::<Vec<_>>())
            }
            ("POST", "/api/environments/env1/secrets") => json!({"key":value["key"]}),
            ("POST", "/api/agents") => json!({"id":"agent1"}),
            ("POST", "/api/conversations") => json!({"id":"conv1"}),
            ("GET", "/api/conversations/conv1/events") => {
                if uri.query().unwrap_or_default().contains("after=0") {
                    json!([{"id":1,"stage":"turn","state":if fake.failed_turn {"failed"} else {"done"},"blocks":[{"kind":"text","body":"Finished the change"}]}])
                } else {
                    json!([])
                }
            }
            ("GET", "/api/conversations/conv1") => {
                json!({"id":"conv1","status":if fake.terminated {"terminated"} else {"idle"},"sandbox_id":"sandbox1","usage_total":{"cost_usd":0.01}})
            }
            ("GET", "/api/sandboxes/sandbox1") => {
                json!({"status":if fake.failed_sandbox {"failed"} else {"ready"},"transition":null})
            }
            ("GET", "/api/sandboxes/sandbox1/diff") => {
                json!({"diff":if fake.binary_diff { "Binary files a/image.png and b/image.png differ" } else { "diff --git a/a b/a\n+fixed" },"truncated":fake.truncate_diff})
            }
            ("GET", "/api/sandboxes/sandbox1/git-status") => {
                json!({"entries":[{"path":"new.txt","index":"untracked","worktree":"untracked"}],"truncated":false})
            }
            ("GET", "/api/sandboxes/sandbox1/file") => {
                json!({"content":"new content","encoding":"utf-8","truncated":false})
            }
            ("POST", "/api/conversations/conv1/terminate") => {
                fake.terminated = true;
                json!({})
            }
            ("POST", "/api/conversations/conv1/interrupt") => return StatusCode::CONFLICT.into_response(),
            ("DELETE", _) => return StatusCode::NO_CONTENT.into_response(),
            _ => panic!("unexpected mock request: {method} {uri}"),
        };
        axum::Json(json!({"data":data})).into_response()
    }

    async fn exercise(scenario: Fake, cancel_at: Option<u64>, duration_ms: u64, expected: &str) {
        let failed_sandbox = scenario.failed_sandbox;
        let truncated = scenario.truncate_diff || scenario.binary_diff;
        let fake = Arc::new(Mutex::new(scenario));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(handle).with_state(fake.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut app = crate::cloud::tests::test_app();
        Arc::get_mut(&mut app).unwrap().cloud.as_mut().unwrap().provider =
            crate::cloud::provider::Provider::new(&base, "mock-key".into()).unwrap();
        let spec = crate::db::cloud::RunSpec {
            agent_id: "curated_coding".into(),
            agent_version: 1,
            agent: crate::cloud::curated_agents().remove(0).definition,
            model: "anthropic/test".into(),
            runtime: "claude".into(),
            repository: "https://github.com/example/repo".into(),
            git_ref: "main".into(),
            repository_secret: None,
            prompt: "Fix it".into(),
        };
        let limits = crate::db::cloud::RunLimits { daily_runs: 10, concurrent_runs: 2, duration_ms };
        let mut spec = spec;
        spec.repository_secret = Some("GITHUB_TOKEN".into());
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db
            .put_cloud_secret("a", "GITHUB_TOKEN", &cipher.seal("a", "GITHUB_TOKEN", "repo-secret").unwrap())
            .unwrap();
        let run = app.db.reserve_cloud_run("a", "request", &spec, &limits, 0).unwrap();
        for i in 0..30 {
            if cancel_at == Some(i) {
                app.db.cancel_cloud_run("a", &run.id).unwrap();
            }
            tick_at(&app, i * 11_000).await.unwrap();
        }
        let saved = app.db.cloud_run("a", &run.id).unwrap().unwrap();
        assert_eq!(saved.state, expected);
        if failed_sandbox {
            assert_eq!(saved.result.as_ref().unwrap()["files_preserved"], false);
            assert!(!fake.lock().unwrap().calls.iter().any(|(_, p, _)| p.contains("/diff?")));
        }
        if truncated {
            assert!(!fake.lock().unwrap().calls.iter().any(|(_, p, _)| p.ends_with("/terminate")));
        }
        if expected == "completed" {
            let result = saved.result.unwrap();
            assert_eq!(result["files"]["new.txt"]["content"], "new content");
            assert_eq!(app.db.cloud_events("a", &run.id, 0).unwrap().len(), 1);
            assert!(app.db.cloud_events("b", &run.id, 0).unwrap().is_empty());
            let fake = fake.lock().unwrap();
            let secret_at = fake.calls.iter().position(|(_, p, _)| p.ends_with("/secrets")).unwrap();
            let launch_at = fake.calls.iter().position(|(m, p, _)| m == "POST" && p == "/api/conversations").unwrap();
            assert!(secret_at < launch_at);
            assert_eq!(fake.calls[secret_at].2["value"], "repo-secret");
            let environment = fake.calls.iter().find(|(m, p, _)| m == "POST" && p == "/api/environments").unwrap();
            assert_eq!(environment.2["repositories"][0]["secret_key"], "GITHUB_TOKEN");
            let file_at = fake.calls.iter().position(|(_, p, _)| p.contains("/file?")).unwrap();
            let terminate_at = fake.calls.iter().position(|(_, p, _)| p.ends_with("/terminate")).unwrap();
            assert!(file_at < terminate_at);
            let launch = fake.calls.iter().find(|(m, p, _)| m == "POST" && p == "/api/conversations").unwrap();
            assert_eq!(launch.2["sandbox_api_access"], "none");
        }
        assert_eq!(
            fake.lock().unwrap().calls.iter().filter(|(m, p, _)| m == "POST" && p == "/api/environments").count(),
            1
        );
        server.abort();
    }

    async fn serve(scenario: Fake) -> (Arc<App>, Arc<Mutex<Fake>>, tokio::task::JoinHandle<()>) {
        let fake = Arc::new(Mutex::new(scenario));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(handle).with_state(fake.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut app = crate::cloud::tests::test_app();
        Arc::get_mut(&mut app).unwrap().cloud.as_mut().unwrap().provider =
            crate::cloud::provider::Provider::new(&base, "mock-key".into()).unwrap();
        (app, fake, server)
    }

    fn cloud_workspace(app: &App) -> crate::db::cloud::CloudWorkspace {
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db
            .put_cloud_secret("a", "GITHUB_TOKEN", &cipher.seal("a", "GITHUB_TOKEN", "repo-secret").unwrap())
            .unwrap();
        let mut spec: crate::db::cloud::CloudWorkspaceSpec = serde_json::from_value(json!({
            "name":"App box",
            "repositories":[{"url":"https://github.com/example/app","secret":"GITHUB_TOKEN"},
                            {"url":"https://github.com/example/api","ref":"main","mount":"/work/api"}],
            "packages":{"apt":["ffmpeg"]},
            "setup_script":"cd /home/sprite/app && npm ci",
        }))
        .unwrap();
        spec.normalize().unwrap();
        app.db.create_cloud_workspace("a", "request", &spec, 0).unwrap()
    }

    fn posts(fake: &Mutex<Fake>, path: &str) -> usize {
        fake.lock().unwrap().calls.iter().filter(|(m, p, _)| m == "POST" && p == path).count()
    }

    /// Run the worker for `ticks` seconds of its clock, from `from`.
    async fn run(app: &App, from: u64, ticks: u64) {
        for i in 0..ticks {
            tick_at(app, from + i * 1_000).await.unwrap();
        }
    }

    #[tokio::test]
    async fn cloud_workspace_is_set_up_and_reaches_ready() {
        let (app, fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 60).await;
        let saved = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(saved.status, "ready");
        assert_eq!(saved.sandbox_status.as_deref(), Some("ready"));
        assert_eq!(saved.built_from_revision.as_deref(), Some("rev1"));
        assert_eq!(saved.built_spec.as_ref(), Some(&saved.saved_spec));
        assert!(app.db.cloud_workspace("b", &created.id).unwrap().is_none());
        let fake = fake.lock().unwrap();
        let at = |m: &str, p: &str| fake.calls.iter().position(|(cm, cp, _)| cm == m && cp == p).unwrap();
        let (environment, vault) = (at("POST", "/api/environments"), at("POST", "/api/vaults"));
        let (secret, workspace) = (at("POST", "/api/vaults/vault1/secrets"), at("POST", "/api/workspaces"));
        assert!(environment < vault && vault < secret && secret < workspace);
        let body = &fake.calls[environment].2;
        assert_eq!(body["name"], created.id);
        assert_eq!(body["metadata"]["arugula_workspace"], created.id);
        assert_eq!(body["repositories"][0]["mount_path"], "/home/sprite/app");
        assert_eq!(body["repositories"][0]["secret_key"], "GITHUB_TOKEN");
        assert_eq!(body["repositories"][1]["ref"], "main");
        assert_eq!(body["packages"]["apt"], json!(["ffmpeg"]));
        assert_eq!(fake.calls[vault].2["metadata"]["arugula_workspace"], created.id);
        assert_eq!(fake.calls[secret].2, json!({"key":"GITHUB_TOKEN","value":"repo-secret"}));
        assert_eq!(fake.calls[workspace].2, json!({"name":created.id,"environment_id":"env1","vault_id":"vault1"}));
        for path in ["/api/environments", "/api/vaults", "/api/workspaces", "/api/vaults/vault1/secrets"] {
            assert_eq!(fake.calls.iter().filter(|(m, p, _)| m == "POST" && p == path).count(), 1, "{path}");
        }
        drop(fake);
        server.abort();
    }

    #[tokio::test]
    async fn lost_cloud_workspace_create_responses_are_found_not_repeated() {
        for lost in ["environments", "vaults", "workspaces"] {
            let (app, fake, server) = serve(Fake { lose: vec![lost], ..Default::default() }).await;
            let created = cloud_workspace(&app);
            run(&app, 0, 120).await;
            assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready", "{lost}");
            for path in ["/api/environments", "/api/vaults", "/api/workspaces"] {
                assert_eq!(posts(&fake, path), 1, "{lost}: {path}");
            }
            assert!(fake.lock().unwrap().calls.iter().any(|(m, p, _)| m == "GET" && p == &format!("/api/{lost}")));
            server.abort();
        }
    }

    #[tokio::test]
    async fn failed_setup_keeps_managoats_error() {
        let (app, _fake, server) = serve(Fake { failed_setup: true, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 60).await;
        let saved = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(saved.status, "failed");
        assert_eq!(saved.error.as_deref(), Some("setup_script exited 1: npm ERR! missing script: build"));
        assert_eq!(saved.sandbox_status, None);
        assert!(saved.built_spec.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn cloud_workspace_delete_survives_a_restart_part_way() {
        let (app, fake, server) = serve(Fake { vault_in_use_once: true, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 60).await;
        assert!(app.db.delete_cloud_workspace("a", &created.id).unwrap());
        assert!(!app.db.delete_cloud_workspace("b", &created.id).unwrap());
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "deleting");
        // Up to the workspace's delete, then a worker dies holding the job.
        let mut now = 60_000;
        while !fake.lock().unwrap().calls.iter().any(|(m, p, _)| m == "DELETE" && p == "/api/workspaces/ws1") {
            tick_at(&app, now).await.unwrap();
            now += 1_000;
            assert!(now < 200_000, "the workspace was never deleted");
        }
        now += 3_000;
        let held = app.db.claim_cloud_workspace_job(now).unwrap().unwrap();
        assert_eq!(held.execution.stage, "delete_vault");
        drop(held);
        let calls = fake.lock().unwrap().calls.len();
        run(&app, now, 60).await;
        assert_eq!(fake.lock().unwrap().calls.len(), calls, "the lease still holds");
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "deleting");
        // The restarted worker takes it up once the lease runs out, and a
        // vault Managoat still counts as in use is tried again.
        run(&app, now + 121_000, 60).await;
        assert!(app.db.cloud_workspace("a", &created.id).unwrap().is_none());
        assert!(app.db.cloud_workspaces("a").unwrap().is_empty());
        let fake = fake.lock().unwrap();
        for path in ["/api/workspaces/ws1", "/api/vaults/vault1", "/api/environments/env1"] {
            assert!(fake.calls.iter().any(|(m, p, _)| m == "DELETE" && p == path), "{path}");
        }
        let deletes =
            fake.calls.iter().filter(|(m, _, _)| m == "DELETE").map(|(_, p, _)| p.as_str()).collect::<Vec<_>>();
        assert_eq!(deletes.last(), Some(&"/api/environments/env1"));
        drop(fake);
        assert!(app.db.claim_cloud_workspace_job(u64::MAX / 2).unwrap().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn delete_during_a_lost_create_cleans_up_what_was_made() {
        let (app, fake, server) = serve(Fake { lose: vec!["environments"], ..Default::default() }).await;
        let created = cloud_workspace(&app);
        tick_at(&app, 0).await.unwrap();
        app.db.delete_cloud_workspace("a", &created.id).unwrap();
        run(&app, 20_000, 60).await;
        assert!(app.db.cloud_workspace("a", &created.id).unwrap().is_none());
        assert_eq!(posts(&fake, "/api/environments"), 1);
        assert_eq!(posts(&fake, "/api/vaults"), 0);
        assert_eq!(posts(&fake, "/api/workspaces"), 0);
        assert!(fake.lock().unwrap().calls.iter().any(|(m, p, _)| m == "DELETE" && p == "/api/environments/env1"));
        server.abort();
    }

    #[tokio::test]
    async fn environment_update_is_a_partial_patch() {
        let (app, fake, server) = serve(Fake::default()).await;
        let provider = &app.cloud.as_ref().unwrap().provider;
        let updated = provider.update_environment("env1", json!({"packages":{"apt":["jq"]}})).await.unwrap();
        assert_eq!(updated["revision"], "rev2");
        assert_eq!(fake.lock().unwrap().calls[0].0, "PATCH");
        server.abort();
    }

    #[tokio::test]
    async fn failed_turn_in_idle_conversation_is_not_success() {
        exercise(Fake { failed_turn: true, ..Default::default() }, None, 500_000, "failed").await;
    }

    /// Opt-in smoke test. Uses a real, dedicated provider account and durable
    /// SQLite state so an interrupted test can resume cleanup on the next run.
    #[tokio::test]
    #[ignore = "requires a dedicated funded provider account; creates a real sandbox"]
    async fn live_subscription_run() {
        let key = std::env::var("ARUGULA_MANAGOAT_API_KEY").expect("provider key required");
        let model = std::env::var("ARUGULA_CLOUD_MODEL").expect("model required");
        let runtime = std::env::var("ARUGULA_CLOUD_RUNTIME").expect("runtime required");
        let path = std::env::var("ARUGULA_LIVE_TEST_DB").expect("durable test database required");
        let mut app = crate::cloud::tests::test_app();
        let inner = Arc::get_mut(&mut app).unwrap();
        inner.db = crate::db::Db::open(std::path::Path::new(&path)).unwrap();
        inner.db.account_for("test", "live-smoke", "live-smoke", "live-smoke", 0).unwrap();
        inner.cloud.as_mut().unwrap().provider = crate::cloud::provider::Provider::new(
            &std::env::var("ARUGULA_MANAGOAT_URL").unwrap_or_else(|_| "https://managoat.com".into()),
            key,
        )
        .unwrap();
        let spec = crate::db::cloud::RunSpec {
            agent_id: "curated_coding".into(), agent_version: 1,
            agent: crate::cloud::curated_agents().remove(0).definition,
            model, runtime, repository: "https://github.com/octocat/Hello-World".into(),
            git_ref: "master".into(), repository_secret: None,
            prompt: "This is an isolated integration smoke test. In repo/, create arugula-smoke.txt containing exactly ARUGULA_LIVE_OK followed by a newline. Do not change any other file or contact any external service. Then reply ARUGULA_LIVE_OK.".into(),
        };
        let run = app
            .db
            .reserve_cloud_run(
                "live-smoke",
                "subscription-smoke-v1",
                &spec,
                &crate::db::cloud::RunLimits { daily_runs: 1, concurrent_runs: 1, duration_ms: 600_000 },
                arugula_e2e::now_ms(),
            )
            .unwrap();
        eprintln!("Live run {} uses durable database {}", run.id, path);
        let mut previous = String::new();
        for _ in 0..450 {
            tick(&app).await.unwrap();
            let saved = app.db.cloud_run("live-smoke", &run.id).unwrap().unwrap();
            if saved.state != previous {
                eprintln!("Live run state: {}", saved.state);
                // Never blindly approve arbitrary provider commands, even in
                // a disposable checkout. Inspect the request before answering.
                if saved.state == "needs_input" {
                    eprintln!("Approval pending; inspect the durable database before answering.");
                }
                previous = saved.state.clone();
            }
            if matches!(saved.state.as_str(), "completed" | "failed" | "cancelled" | "expired") {
                assert_eq!(saved.state, "completed", "live execution did not complete");
                let result = saved.result.unwrap();
                assert_eq!(result["files"]["arugula-smoke.txt"]["content"], "ARUGULA_LIVE_OK\n");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        panic!("live test observation timed out; resume with the same database to reconcile resources");
    }

    #[tokio::test]
    async fn binary_changes_keep_the_sandbox_for_recovery() {
        exercise(Fake { binary_diff: true, ..Default::default() }, None, 500_000, "recovering").await;
    }

    #[tokio::test]
    async fn saves_results_before_cleanup_without_a_connected_browser() {
        exercise(Fake::default(), None, 500_000, "completed").await;
    }
    #[tokio::test]
    async fn reconciles_lost_create_response_without_duplicate_environment() {
        exercise(Fake { lose_create_response: true, ..Default::default() }, None, 500_000, "completed").await;
    }
    #[tokio::test]
    async fn cancellation_is_durable_and_cleans_up() {
        exercise(Fake::default(), Some(3), 500_000, "cancelled").await;
    }
    #[tokio::test]
    async fn recovery_lookup_failure_does_not_discard_an_uncertain_creation() {
        exercise(
            Fake { lose_create_response: true, deny_lookup_once: true, ..Default::default() },
            None,
            500_000,
            "completed",
        )
        .await;
    }
    #[tokio::test]
    async fn cancelling_an_idle_turn_does_not_loop_on_interrupt_conflict() {
        exercise(Fake::default(), Some(4), 500_000, "cancelled").await;
    }
    #[tokio::test]
    async fn unavailable_sandbox_preserves_transcript_and_releases_capacity() {
        exercise(Fake { failed_sandbox: true, ..Default::default() }, None, 500_000, "failed").await;
    }
    #[tokio::test]
    async fn incomplete_diff_never_triggers_destructive_cleanup() {
        exercise(Fake { truncate_diff: true, ..Default::default() }, None, 500_000, "recovering").await;
    }
    #[tokio::test]
    async fn deadline_stops_a_run_without_marking_it_successful() {
        exercise(Fake::default(), None, 40_000, "expired").await;
    }
}
