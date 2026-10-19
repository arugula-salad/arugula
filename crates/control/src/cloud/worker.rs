//! Durable, bounded reconciliation steps. Browser connections never own work.
use crate::{
    App,
    db::cloud::{CloudTabJob, CloudWorkspaceJob, Job},
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
    let cloud_workspace = tick_cloud_workspace(app, now).await;
    tick_cloud_tab(app, now).await.and(cloud_workspace).and(run)
}

/// Read a conversation's next page of events and take them in under its
/// job's claim (`id` is the run's or the tab's). Returns the page.
async fn pump(
    app: &App,
    id: &str,
    account: &str,
    lease: &str,
    conversation: &str,
    cursor: &mut u64,
) -> Result<Vec<Value>> {
    let p = &app.cloud.as_ref().context("cloud is disabled")?.provider;
    let events = p.events(conversation, *cursor).await?;
    app.db.append_conversation_events(id, account, lease, &events)?;
    if let Some(last) = events.iter().filter_map(|e| e["id"].as_u64()).max() {
        *cursor = last;
    }
    Ok(events)
}

/// What a run stopped by the retirement says, when it has nothing to say.
const RUN_RETIRED: &str = "Cloud runs were retired before this run finished, so it was stopped. Its transcript is kept; Cloud workspaces replace runs.";

/// Cloud runs are retired (S40, decision 10): nothing starts one. A run a
/// deploy finds unfinished is stopped instead, without losing what it made:
/// a create whose response was lost is found first, the conversation is
/// read to its end and terminated, then its agent and environment go.
async fn tick_run(app: &App, now: u64) -> Result<()> {
    let Some(mut job) = app.db.claim_cloud_job(now)? else {
        return Ok(());
    };
    if let Err(error) = stop_run(app, &mut job, now).await {
        // Keep the durable intent on failure. In particular, never forget a
        // creation whose response may have been lost.
        app.db.checkpoint_cloud_job(&job, "recovering", None, Some(now + 10_000))?;
        tracing::warn!(run = %job.run.id, %error, "retired cloud run will reconcile");
    }
    Ok(())
}

fn save(app: &App, job: &Job, state: &str, result: Option<&Value>, now: u64) -> Result<()> {
    app.db.checkpoint_cloud_job(job, state, result, Some(now + 2_000))
}

async fn stop_run(app: &App, job: &mut Job, now: u64) -> Result<()> {
    let p = &app.cloud.as_ref().context("cloud is disabled")?.provider;
    let ex = &mut job.execution;
    match ex.stage.as_str() {
        // A create in flight: find what it made before cleaning up.
        "environment_creating" => {
            ex.environment = p.find("environments", &job.run.id).await?;
            ex.stage = "terminating".into();
            save(app, job, "cleaning_up", None, now)
        }
        "agent_creating" => {
            ex.agent = p.find("agents", &job.run.id).await?;
            ex.stage = "terminating".into();
            save(app, job, "cleaning_up", None, now)
        }
        "conversation_creating" => {
            ex.conversation = p.find("conversations", &job.run.id).await?;
            ex.stage = "terminating".into();
            save(app, job, "cleaning_up", None, now)
        }
        // Its changes are being saved, and the run went to recovery because
        // they couldn't be (a truncated diff, binary files): the sandbox may
        // hold the only copy, so it's left for an operator, as before.
        "collecting" | "collecting_files" => {
            tracing::warn!(run = %job.run.id, "retired cloud run holds changes that weren't saved; recover them by hand");
            app.db.checkpoint_cloud_job(job, "recovering", None, Some(now + 86_400_000))
        }
        "terminating" => {
            if let Some(id) = ex.conversation.clone() {
                let c = match p.conversation(&id).await {
                    Ok(c) => c,
                    Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                        json!({"status":"terminated"})
                    }
                    Err(e) => return Err(e),
                };
                // The transcript to its end, then the conversation goes.
                if pump(app, &job.run.id, &job.account, &job.lease, &id, &mut ex.cursor).await?.len() >= 1000 {
                    return save(app, job, "cleaning_up", None, now);
                }
                if c["status"] != "terminated" {
                    p.terminate(&id).await?;
                    return save(app, job, "cleaning_up", None, now);
                }
            }
            ex.stage = "cleanup_agent".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_agent" => {
            if let Some(id) = &ex.agent {
                p.remove("agents", id).await?;
            }
            ex.agent = None;
            ex.stage = "cleanup_environment".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_environment" => {
            if let Some(id) = &ex.environment {
                p.remove("environments", id).await?;
            }
            ex.environment = None;
            ex.stage = "done".into();
            let outcome = ex.outcome.get_or_insert_with(|| "cancelled".into()).clone();
            let mut result = job.run.result.clone().unwrap_or_else(|| json!({}));
            if result["message"].is_null() {
                result["message"] = json!(RUN_RETIRED);
            }
            save(app, job, &outcome, Some(&result), now)?;
            app.analytics.capture(&job.account, "cloud run stopped", json!({ "outcome": outcome }));
            Ok(())
        }
        // Anything else ("", environment_ready, agent_ready, running) has
        // nothing in flight: stop it.
        _ => {
            ex.stage = "terminating".into();
            save(app, job, "cleaning_up", None, now)
        }
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
        // Its tabs' agents name it too, so they go first.
        "delete_vault" => {
            if app.db.open_cloud_tabs(&id)? > 0 {
                return save_cloud_workspace(app, job, now);
            }
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

/// How soon a tab's job runs again: soon while a turn runs or waits on an
/// answer, less often while idle, and seldom once it has ended.
const CLOUD_TAB_BUSY_MS: u64 = 2_000;
const CLOUD_TAB_IDLE_MS: u64 = 5_000;
const CLOUD_TAB_ENDED_MS: u64 = 60_000;

async fn tick_cloud_tab(app: &App, now: u64) -> Result<()> {
    let Some(mut job) = app.db.claim_cloud_tab_job(now)? else {
        return Ok(());
    };
    if let Err(error) = step_cloud_tab(app, &mut job, now).await {
        // As for a Cloud workspace: a refused create made nothing.
        let rejected = error
            .downcast_ref::<super::provider::HttpFailure>()
            .is_some_and(|e| e.1 == Method::POST && (400..500).contains(&e.0) && !matches!(e.0, 408 | 429));
        if rejected && job.execution.stage.ends_with("_creating") {
            job.execution.stage = "rejected".into();
            job.tab.status = "failed".into();
            job.tab.error = Some("Managoat refused to open this tab.".into());
            return save_cloud_tab(app, &job, now);
        }
        app.db.checkpoint_cloud_tab_job(&job, Some(now + 10_000))?;
        tracing::warn!(cloud_tab = %job.tab.id, %error, "cloud tab will reconcile");
    }
    Ok(())
}

fn save_cloud_tab(app: &App, job: &CloudTabJob, now: u64) -> Result<()> {
    let wait = match job.tab.status.as_str() {
        _ if job.closing || job.execution.stage != "live" && job.execution.stage != "rejected" => CLOUD_TAB_BUSY_MS,
        "running" | "pending" | "needs_input" => CLOUD_TAB_BUSY_MS,
        "idle" => CLOUD_TAB_IDLE_MS,
        _ => CLOUD_TAB_ENDED_MS,
    };
    app.db.checkpoint_cloud_tab_job(job, Some(now + wait))
}

/// Open (agent, then conversation), then follow the conversation, then
/// close (conversation, then agent). Each create is journalled before
/// Managoat is asked, and found again after a lost response.
async fn step_cloud_tab(app: &App, job: &mut CloudTabJob, now: u64) -> Result<()> {
    let cloud = app.cloud.as_ref().context("cloud is disabled")?;
    let p = &cloud.provider;
    let id = job.tab.id.clone();
    let stage = job.execution.stage.clone();
    // Never forget an in-flight creation: find what it made first.
    if job.closing && !stage.ends_with("_creating") && !matches!(stage.as_str(), "terminate" | "delete_agent") {
        job.execution.stage = "terminate".into();
        return save_cloud_tab(app, job, now);
    }
    let not_made = |job: &mut CloudTabJob, retry: &str| {
        job.execution.stage = if job.closing { "terminate" } else { retry }.into();
    };
    match stage.as_str() {
        "" => {
            // Its Cloud workspace isn't made yet: wait for it.
            let (Some(environment), Some(vault), Some(_)) = (&job.environment, &job.vault, &job.workspace) else {
                return save_cloud_tab(app, job, now);
            };
            let spec = &job.spec;
            let body = json!({
                "name": id, "metadata": {"arugula_tab": id},
                "environment_id": environment, "allowed_environment_ids": [], "allowed_vault_ids": [vault],
                "runtime": spec.runtime, "model": spec.model,
                "system": format!(
                    "{}\nYou work in {}, on a machine other agents may share.",
                    spec.agent.instructions, spec.cwd
                ),
                "mcp_servers": spec.agent.mcp_servers, "skills": spec.agent.skills,
                "permission_policy": {"default": "ask"},
            });
            job.execution.stage = "agent_creating".into();
            app.db.checkpoint_cloud_tab_job(job, None)?;
            job.execution.agent = Some(p.create("agents", body).await?);
            job.execution.stage = "agent_ready".into();
            save_cloud_tab(app, job, now)
        }
        "agent_creating" => {
            job.execution.agent = p.find_cloud_tab_resource("agents", &id).await?;
            if job.execution.agent.is_some() {
                job.execution.stage = "agent_ready".into();
            } else {
                not_made(job, "");
            }
            save_cloud_tab(app, job, now)
        }
        "agent_ready" => {
            let agent = job.execution.agent.clone().context("missing agent")?;
            let workspace = job.workspace.clone().context("missing workspace")?;
            job.execution.stage = "conversation_creating".into();
            app.db.checkpoint_cloud_tab_job(job, None)?;
            let conversation =
                p.launch_in_cloud_workspace(&agent, &workspace, &job.spec.cwd, &id, job.spec.prompt.as_deref()).await?;
            job.execution.conversation = Some(conversation["id"].as_str().context("missing conversation")?.into());
            job.tab.status = conversation["status"].as_str().unwrap_or("pending").into();
            job.execution.stage = "live".into();
            save_cloud_tab(app, job, now)
        }
        "conversation_creating" => {
            job.execution.conversation = p.find_cloud_tab_resource("conversations", &id).await?;
            if job.execution.conversation.is_some() {
                job.tab.status = "pending".into();
                job.execution.stage = "live".into();
            } else {
                not_made(job, "agent_ready");
            }
            save_cloud_tab(app, job, now)
        }
        "live" => {
            let conversation = job.execution.conversation.clone().context("missing conversation")?;
            let c = match p.conversation(&conversation).await {
                Ok(c) => c,
                Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                    job.tab.status = "failed".into();
                    job.tab.error = Some("Managoat no longer has this conversation. Close the tab.".into());
                    return save_cloud_tab(app, job, now);
                }
                Err(e) => return Err(e),
            };
            let state = c["status"].as_str().context("missing conversation status")?.to_owned();
            let events = pump(app, &id, &job.account, &job.lease, &conversation, &mut job.execution.cursor).await?;
            if events.iter().any(|e| e["stage"] == "turn") {
                job.started = true;
            }
            let waiting = !app.db.cloud_requests(&job.account, &id)?.is_empty();
            job.tab.status = if waiting && !matches!(state.as_str(), "failed" | "terminated") {
                "needs_input".into()
            } else {
                state
            };
            // A full page means more to read: come back soon.
            if events.len() >= 1000 {
                return app.db.checkpoint_cloud_tab_job(job, Some(now + CLOUD_TAB_BUSY_MS));
            }
            save_cloud_tab(app, job, now)
        }
        // Nothing to follow: only a close moves it on.
        "rejected" => save_cloud_tab(app, job, now),
        "terminate" => {
            if let Some(conversation) = &job.execution.conversation {
                let c = match p.conversation(conversation).await {
                    Ok(c) => c,
                    Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                        json!({"status":"terminated"})
                    }
                    Err(e) => return Err(e),
                };
                if !matches!(c["status"].as_str(), Some("terminated" | "failed")) {
                    p.terminate(conversation).await?;
                    return save_cloud_tab(app, job, now);
                }
            }
            job.execution.stage = "delete_agent".into();
            save_cloud_tab(app, job, now)
        }
        "delete_agent" => {
            if let Some(agent) = &job.execution.agent {
                p.remove("agents", agent).await?;
            }
            job.execution.agent = None;
            app.db.finish_cloud_tab_close(job, now)
        }
        _ => bail!("unknown cloud tab stage"),
    }
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
        // A run left over from before runs were retired: its conversation
        // ("conv1") was terminated.
        terminated: bool,
        // Cloud workspaces.
        vault: Option<Value>,
        workspace: Option<Value>,
        /// Collections whose next create succeeds but loses its response.
        lose: Vec<&'static str>,
        workspace_reads: u32,
        failed_setup: bool,
        vault_in_use_once: bool,
        /// The workspace read that finds setup done (2 when left at 0).
        setup_reads: u32,
        // Tabs: their agents and conversations, by id, and each
        // conversation's events.
        agents: Vec<Value>,
        tab_conversations: std::collections::BTreeMap<String, Value>,
        tab_events: std::collections::BTreeMap<String, Vec<Value>>,
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
        if let Some(rest) = path.strip_prefix("/api/conversations/tabconv") {
            let (n, action) = rest.split_once('/').unwrap_or((rest, ""));
            let id = format!("tabconv{n}");
            let Some(conversation) = fake.tab_conversations.get(&id).cloned() else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let status = |fake: &mut Fake, status: &str| {
                fake.tab_conversations.get_mut(&id).unwrap()["status"] = json!(status);
            };
            let data = match (method.as_str(), action) {
                ("GET", "") => conversation,
                ("GET", "events") => {
                    let after: u64 = uri
                        .query()
                        .unwrap_or_default()
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("after="))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let events = fake.tab_events.get(&id).cloned().unwrap_or_default();
                    json!(events.into_iter().filter(|e| e["id"].as_u64().unwrap() > after).collect::<Vec<_>>())
                }
                ("POST", "prompts") => {
                    status(&mut fake, "running");
                    json!({"accepted":true})
                }
                ("POST", "interrupt") if conversation["status"] == "running" => {
                    status(&mut fake, "idle");
                    json!({})
                }
                ("POST", "interrupt") => return StatusCode::CONFLICT.into_response(),
                ("POST", "terminate") => {
                    status(&mut fake, "terminated");
                    json!({})
                }
                ("POST", action) if action.starts_with("requests/") => json!({}),
                _ => panic!("unexpected mock request: {method} {uri}"),
            };
            return axum::Json(json!({"data":data})).into_response();
        }
        let data = match (method.as_str(), path) {
            ("POST", "/api/environments") => {
                let mut environment = value.clone();
                environment["id"] = json!("env1");
                environment["revision"] = json!("rev1");
                fake.environment = Some(environment.clone());
                if fake.lost("environments") {
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
                let setup_reads = if fake.setup_reads == 0 { 2 } else { fake.setup_reads };
                let Some(workspace) = fake.workspace.as_mut() else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                // Set up on the second read.
                if reads >= setup_reads && failed {
                    workspace["status"] = json!("failed");
                    workspace["sandbox_status"] = Value::Null;
                    workspace["error"] = json!("setup_script exited 1: npm ERR! missing script: build");
                } else if reads >= setup_reads {
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
            ("GET", "/api/environments") => json!(fake.environment.iter().collect::<Vec<_>>()),
            ("POST", "/api/agents") if value["metadata"]["arugula_tab"].is_string() => {
                let mut agent = value.clone();
                agent["id"] = json!(format!("agent{}", fake.agents.len() + 2));
                fake.agents.push(agent.clone());
                if fake.lost("agents") {
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                agent
            }
            ("GET", "/api/agents") => json!(fake.agents),
            ("POST", "/api/conversations") if value["workspace_id"].is_string() => {
                let id = format!("tabconv{}", fake.tab_conversations.len() + 1);
                let status = if value["prompt"].is_string() { "running" } else { "idle" };
                let conversation = json!({
                    "id":id,"status":status,"workspace_id":value["workspace_id"],"cwd":value["cwd"],"labels":value["labels"],
                });
                fake.tab_conversations.insert(id, conversation.clone());
                if fake.lost("conversations") {
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                conversation
            }
            ("GET", "/api/conversations") => json!(fake.tab_conversations.values().collect::<Vec<_>>()),
            ("GET", "/api/conversations/conv1/events") => {
                if uri.query().unwrap_or_default().contains("after=0") {
                    json!([{"id":1,"stage":"turn","state":"done","blocks":[{"kind":"text","body":"Finished the change"}]}])
                } else {
                    json!([])
                }
            }
            ("GET", "/api/conversations/conv1") => {
                json!({"id":"conv1","status":if fake.terminated {"terminated"} else {"running"}})
            }
            ("POST", "/api/conversations/conv1/terminate") => {
                fake.terminated = true;
                json!({})
            }
            ("DELETE", _) => return StatusCode::NO_CONTENT.into_response(),
            _ => panic!("unexpected mock request: {method} {uri}"),
        };
        axum::Json(json!({"data":data})).into_response()
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

    /// Control's routes over `app`, and a client signed in as `account`.
    async fn routes(app: &Arc<App>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/cloud", listener.local_addr().unwrap());
        let app = app.clone();
        (base, tokio::spawn(async move { axum::serve(listener, crate::router(app)).await.unwrap() }))
    }

    async fn call(method: reqwest::Method, url: &str, account: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request =
            reqwest::Client::new().request(method, url).header("cookie", format!("ilg_session=session-{account}"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    fn calls(fake: &Mutex<Fake>, method: &str, path: &str) -> Vec<Value> {
        fake.lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(m, p, _)| m == method && p == path)
            .map(|(_, _, b)| b.clone())
            .collect()
    }

    /// A permission request in `conversation`, as its next event.
    fn ask(fake: &Mutex<Fake>, conversation: &str, request: &str) {
        let mut fake = fake.lock().unwrap();
        let events = fake.tab_events.entry(conversation.into()).or_default();
        let id = events.len() as u64 + 1;
        events.push(json!({"id":id,"stage":"turn","state":"started"}));
        events.push(json!({"id":id + 1,"blocks":[{"kind":"permission_request","request_id":request,"options":[{"optionId":"allow"}]}]}));
    }

    #[tokio::test]
    async fn tabs_run_side_by_side_and_close_on_their_own() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 30).await;
        let (base, routes) = routes(&app).await;
        let tabs = format!("{base}/workspaces/{}/tabs", created.id);
        let (status, models) = call(M::GET, &format!("{base}/models"), "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(models["models"][1], json!({"runtime":"codex","model":"openai/test-model"}));
        let open = |key: &str, runtime: &str, model: &str, cwd: Option<&str>| {
            let mut body = json!({"request_key":key,"agent_id":"curated_coding","runtime":runtime,"model":model,"prompt":"Run the tests"});
            if let Some(cwd) = cwd {
                body["cwd"] = json!(cwd);
            }
            body
        };
        // A pair that isn't listed, and a cwd outside the mounts and home.
        for bad in [
            open("x", "codex", "anthropic/test-model", None),
            open("x", "gemini", "google/test-model", None),
            open("x", "claude", "anthropic/test-model", Some("/etc")),
            open("x", "claude", "anthropic/test-model", Some("/home/sprite/.managoat/x")),
            open("x", "claude", "anthropic/test-model", Some("/work/api/../../etc")),
        ] {
            assert_eq!(call(M::POST, &tabs, "a", Some(bad.clone())).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        }
        let (status, claude) =
            call(M::POST, &tabs, "a", Some(open("one", "claude", "anthropic/test-model", None))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(claude["id"].as_str().unwrap().starts_with("cwc_"));
        assert_eq!(claude["status"], "opening");
        assert_eq!(claude["cwd"], "/home/sprite/app");
        let again = call(M::POST, &tabs, "a", Some(open("one", "claude", "anthropic/test-model", None))).await.1;
        assert_eq!(again["id"], claude["id"]);
        let (status, codex) =
            call(M::POST, &tabs, "a", Some(open("two", "codex", "openai/test-model", Some("/work/api/src")))).await;
        assert_eq!(status, StatusCode::OK);
        // Another account's Cloud workspace has no tabs to show it.
        assert_eq!(call(M::GET, &tabs, "b", None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            call(M::POST, &tabs, "b", Some(open("one", "claude", "anthropic/test-model", None))).await.0,
            StatusCode::NOT_FOUND
        );
        run(&app, 30_000, 20).await;
        let listed = call(M::GET, &tabs, "a", None).await.1;
        let listed = listed["cloud_tabs"].as_array().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["id"], claude["id"]);
        assert!(listed.iter().all(|t| t["status"] == "running" && t["waiting_for_setup"] == false), "{listed:?}");
        let agents = calls(&fake, "POST", "/api/agents");
        assert_eq!(agents.len(), 2);
        assert_eq!(
            (agents[0]["runtime"].as_str(), agents[0]["model"].as_str()),
            (Some("claude"), Some("anthropic/test-model"))
        );
        assert_eq!(
            (agents[1]["runtime"].as_str(), agents[1]["model"].as_str()),
            (Some("codex"), Some("openai/test-model"))
        );
        assert_eq!(agents[0]["permission_policy"]["default"], "ask");
        assert_eq!(agents[0]["allowed_vault_ids"], json!(["vault1"]));
        let launches = calls(&fake, "POST", "/api/conversations");
        assert_eq!(launches.len(), 2);
        assert_eq!(launches[0]["workspace_id"], "ws1");
        assert_eq!(launches[0]["cwd"], "/home/sprite/app");
        assert_eq!(launches[1]["cwd"], "/work/api/src");
        assert_eq!(launches[1]["agent_id"], "agent3");
        assert!(launches.iter().all(|l| l.get("sandbox_api_access").is_none() && l["prompt"] == "Run the tests"));
        // Each tab asks for approval: it shows on that tab alone, and pushes.
        ask(&fake, "tabconv1", "approve-1");
        ask(&fake, "tabconv2", "approve-2");
        run(&app, 50_000, 10).await;
        let (one, two) = (claude["id"].as_str().unwrap(), codex["id"].as_str().unwrap());
        for (tab, request) in [(one, "approve-1"), (two, "approve-2")] {
            let shown = call(M::GET, &format!("{tabs}/{tab}"), "a", None).await.1;
            assert_eq!(shown["status"], "needs_input");
            let requests = call(M::GET, &format!("{tabs}/{tab}/requests"), "a", None).await.1;
            assert_eq!(requests["requests"].as_array().unwrap().len(), 1);
            assert_eq!(requests["requests"][0]["request_id"], request);
            let events = call(M::GET, &format!("{tabs}/{tab}/events"), "a", None).await.1;
            assert_eq!(events["events"].as_array().unwrap().len(), 2);
            assert_eq!(call(M::GET, &format!("{tabs}/{tab}/requests"), "b", None).await.0, StatusCode::NOT_FOUND);
        }
        let pushes = app.db.cloud_notifications(i64::MAX as u64).unwrap();
        assert_eq!(pushes.len(), 2);
        assert!(pushes.iter().all(|n| n.account == "a" && n.cloud_workspace.as_deref() == Some(created.id.as_str())));
        // Approve, prompt and interrupt each tab.
        for (tab, conversation, request) in [(one, "tabconv1", "approve-1"), (two, "tabconv2", "approve-2")] {
            let answer = json!({"option_id":"allow"});
            let url = format!("{tabs}/{tab}/requests/{request}");
            assert_eq!(call(M::POST, &url, "b", Some(answer.clone())).await.0, StatusCode::NOT_FOUND);
            assert_eq!(call(M::POST, &url, "a", Some(answer)).await.0, StatusCode::OK);
            assert_eq!(calls(&fake, "POST", &format!("/api/conversations/{conversation}/requests/{request}")).len(), 1);
            let requests = call(M::GET, &format!("{tabs}/{tab}/requests"), "a", None).await.1;
            assert_eq!(requests["requests"], json!([]));
            let interrupt = format!("{tabs}/{tab}/interrupt");
            assert_eq!(call(M::POST, &interrupt, "a", None).await.0, StatusCode::OK);
            assert_eq!(call(M::POST, &interrupt, "a", None).await.0, StatusCode::CONFLICT, "nothing running");
            let prompt = format!("{tabs}/{tab}/prompt");
            assert_eq!(
                call(M::POST, &prompt, "a", Some(json!({"prompt":" "}))).await.0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
            assert_eq!(call(M::POST, &prompt, "b", Some(json!({"prompt":"Next"}))).await.0, StatusCode::NOT_FOUND);
            assert_eq!(call(M::POST, &prompt, "a", Some(json!({"prompt":"Next"}))).await.0, StatusCode::OK);
            let sent = calls(&fake, "POST", &format!("/api/conversations/{conversation}/prompts"));
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0]["prompt"], "Next");
            assert!(sent[0]["client_request_id"].as_str().unwrap().starts_with(tab));
        }
        assert!(app.db.cloud_notifications(i64::MAX as u64).unwrap().is_empty());
        // Closing one tab ends its conversation and its agent, nothing else.
        assert_eq!(call(M::DELETE, &format!("{tabs}/{one}"), "b", None).await.0, StatusCode::NOT_FOUND);
        let (status, closing) = call(M::DELETE, &format!("{tabs}/{one}"), "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(closing["status"], "closing");
        assert_eq!(
            call(M::POST, &format!("{tabs}/{one}/prompt"), "a", Some(json!({"prompt":"x"}))).await.0,
            StatusCode::CONFLICT
        );
        run(&app, 70_000, 20).await;
        assert_eq!(call(M::GET, &format!("{tabs}/{one}"), "a", None).await.0, StatusCode::NOT_FOUND);
        let listed = call(M::GET, &tabs, "a", None).await.1;
        assert_eq!(listed["cloud_tabs"].as_array().unwrap().len(), 1);
        assert_eq!(listed["cloud_tabs"][0]["id"], two);
        assert_eq!(calls(&fake, "POST", "/api/conversations/tabconv1/terminate").len(), 1);
        assert!(calls(&fake, "POST", "/api/conversations/tabconv2/terminate").is_empty());
        assert_eq!(calls(&fake, "DELETE", "/api/agents/agent2").len(), 1);
        assert!(calls(&fake, "DELETE", "/api/agents/agent3").is_empty());
        assert!(calls(&fake, "DELETE", "/api/workspaces/ws1").is_empty());
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready");
        // Deleting the Cloud workspace closes the tab left, and its vault
        // and environment go only after that tab's agent.
        assert_eq!(call(M::DELETE, &format!("{base}/workspaces/{}", created.id), "a", None).await.0, StatusCode::OK);
        assert_eq!(
            call(M::POST, &tabs, "a", Some(open("three", "claude", "anthropic/test-model", None))).await.0,
            StatusCode::CONFLICT
        );
        run(&app, 100_000, 60).await;
        assert!(app.db.cloud_workspace("a", &created.id).unwrap().is_none());
        assert_eq!(app.db.open_cloud_tabs(&created.id).unwrap(), 0);
        let fake = fake.lock().unwrap();
        let at = |m: &str, p: &str| fake.calls.iter().position(|(cm, cp, _)| cm == m && cp == p).unwrap();
        assert!(at("DELETE", "/api/agents/agent3") < at("DELETE", "/api/vaults/vault1"));
        drop(fake);
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_tab_opened_while_setting_up_waits_with_its_first_prompt() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { setup_reads: 1_000, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let tabs = format!("{base}/workspaces/{}/tabs", created.id);
        let body = |key: &str, prompt: Option<&str>| json!({"request_key":key,"agent_id":"curated_review","runtime":"claude","model":"anthropic/test-model","prompt":prompt});
        // Opened before control has even made the Managoat workspace.
        let (status, tab) = call(M::POST, &tabs, "a", Some(body("one", Some("Review it")))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(tab["waiting_for_setup"], true);
        let quiet = call(M::POST, &tabs, "a", Some(body("two", None))).await.1;
        assert_eq!(quiet["waiting_for_setup"], false, "no prompt to wait with");
        run(&app, 0, 30).await;
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "provisioning");
        let launches = calls(&fake, "POST", "/api/conversations");
        assert_eq!(launches.len(), 2);
        let id = tab["id"].as_str().unwrap();
        let launch = |tab: &str| launches.iter().find(|l| l["labels"]["arugula_tab"] == tab).unwrap().clone();
        assert_eq!(launch(id)["prompt"], "Review it");
        assert!(launch(quiet["id"].as_str().unwrap()).get("prompt").is_none());
        let conversation = if launch(id) == launches[0] { "tabconv1" } else { "tabconv2" };
        let shown = call(M::GET, &format!("{tabs}/{id}"), "a", None).await.1;
        assert_eq!(shown["waiting_for_setup"], true);
        // Setup ends and the prompt's turn starts.
        fake.lock().unwrap().setup_reads = 1;
        fake.lock()
            .unwrap()
            .tab_events
            .insert(conversation.into(), vec![json!({"id":1,"stage":"turn","state":"started"})]);
        run(&app, 30_000, 30).await;
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready");
        let shown = call(M::GET, &format!("{tabs}/{id}"), "a", None).await.1;
        assert_eq!(shown["waiting_for_setup"], false);
        assert_eq!(shown["status"], "running");
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn lost_tab_create_responses_are_found_not_repeated() {
        for lost in ["agents", "conversations"] {
            let (app, fake, server) = serve(Fake { lose: vec![lost], ..Default::default() }).await;
            let created = cloud_workspace(&app);
            run(&app, 0, 30).await;
            let spec = crate::db::cloud::CloudTabSpec {
                agent_id: "curated_coding".into(),
                agent_version: 1,
                agent: crate::cloud::curated_agents().remove(0).definition,
                runtime: "claude".into(),
                model: "anthropic/test-model".into(),
                cwd: "/home/sprite/app".into(),
                prompt: None,
            };
            let tab = app.db.create_cloud_tab("a", &created.id, "tab", &spec, 0).unwrap();
            run(&app, 30_000, 60).await;
            assert_eq!(app.db.cloud_tab("a", &created.id, &tab.id).unwrap().unwrap().status, "idle", "{lost}");
            assert_eq!(posts(&fake, "/api/agents"), 1, "{lost}");
            assert_eq!(posts(&fake, "/api/conversations"), 1, "{lost}");
            assert!(
                fake.lock()
                    .unwrap()
                    .calls
                    .iter()
                    .any(|(m, p, _)| m == "GET" && p.starts_with(&format!("/api/{lost}?")))
            );
            server.abort();
        }
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

    /// A run as one was made before runs were retired, its job at `stage`
    /// with everything it had made.
    fn retired_run(app: &App, stage: &str, made: bool) -> crate::db::cloud::CloudRun {
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
        let some = |id: &str| made.then(|| id.to_owned());
        let execution = crate::db::cloud::Execution {
            stage: stage.into(),
            environment: some("env1"),
            agent: some("agent1"),
            conversation: some("conv1"),
            ..Default::default()
        };
        app.db.insert_cloud_run("a", &spec, &execution, "running", 0).unwrap()
    }

    fn deletes(fake: &Mutex<Fake>) -> Vec<String> {
        fake.lock().unwrap().calls.iter().filter(|(m, _, _)| m == "DELETE").map(|(_, p, _)| p.clone()).collect()
    }

    #[tokio::test]
    async fn a_retired_run_is_stopped_keeping_its_transcript() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake::default()).await;
        let (base, api) = routes(&app).await;
        let old = retired_run(&app, "running", true);
        // No route starts, cancels or answers a run.
        let start = json!({"request_key":"k","agent_id":"curated_coding","repository":"https://github.com/example/repo","prompt":"Fix it"});
        assert_eq!(call(M::POST, &format!("{base}/runs"), "a", Some(start)).await.0, StatusCode::METHOD_NOT_ALLOWED);
        for path in ["cancel", "requests/approval"] {
            let (status, _) = call(M::POST, &format!("{base}/runs/{}/{path}", old.id), "a", Some(json!({}))).await;
            assert!(matches!(status, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED), "{path}: {status}");
        }
        run(&app, 0, 30).await;
        let saved = app.db.cloud_run("a", &old.id).unwrap().unwrap();
        assert_eq!(saved.state, "cancelled");
        assert_eq!(saved.result.as_ref().unwrap()["message"], RUN_RETIRED);
        assert!(fake.lock().unwrap().terminated);
        assert_eq!(deletes(&fake), ["/api/agents/agent1", "/api/environments/env1"]);
        for path in ["/api/environments", "/api/agents", "/api/conversations"] {
            assert_eq!(posts(&fake, path), 0, "nothing is made for it: {path}");
        }
        assert_eq!(app.db.active_cloud_runs("a").unwrap(), 0);
        // What it did stays readable, to its owner only.
        let (status, past) = call(M::GET, &format!("{base}/runs/{}", old.id), "a", None).await;
        assert_eq!((status, past["state"].as_str()), (StatusCode::OK, Some("cancelled")));
        let (_, events) = call(M::GET, &format!("{base}/runs/{}/events?after=0", old.id), "a", None).await;
        assert_eq!(events["events"].as_array().unwrap().len(), 1);
        let (_, list) = call(M::GET, &format!("{base}/runs"), "a", None).await;
        assert_eq!(list["runs"].as_array().unwrap().len(), 1);
        assert_eq!(call(M::GET, &format!("{base}/runs/{}", old.id), "b", None).await.0, StatusCode::NOT_FOUND);
        api.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_retired_runs_lost_create_is_found_then_removed() {
        let (app, fake, server) = serve(Fake::default()).await;
        let old = retired_run(&app, "environment_creating", false);
        fake.lock().unwrap().environment = Some(json!({"id":"env1","metadata":{"arugula_run":old.id}}));
        run(&app, 0, 30).await;
        assert_eq!(app.db.cloud_run("a", &old.id).unwrap().unwrap().state, "cancelled");
        assert_eq!(deletes(&fake), ["/api/environments/env1"]);
        assert_eq!(posts(&fake, "/api/environments"), 0);
        server.abort();
    }

    #[tokio::test]
    async fn a_retired_run_holding_unsaved_changes_is_left_for_recovery() {
        let (app, fake, server) = serve(Fake::default()).await;
        let old = retired_run(&app, "collecting", true);
        run(&app, 0, 30).await;
        assert_eq!(app.db.cloud_run("a", &old.id).unwrap().unwrap().state, "recovering");
        assert!(deletes(&fake).is_empty());
        assert!(!fake.lock().unwrap().terminated);
        server.abort();
    }
}
