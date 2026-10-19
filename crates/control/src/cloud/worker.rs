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

/// A tab whose conversation failed. Managoat's `failed` is terminal, and a
/// first prompt that ran a Cloud workspace's setup fails with it.
const TAB_FAILED: &str = "This tab's conversation failed (a failed setup ends the conversation it ran in). If the Cloud workspace failed, Retry it first. Then send a prompt: it starts a new conversation in this tab, and what's above stays.";

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

fn observe(job: &mut CloudWorkspaceJob, workspace: &Value) {
    observe_cloud_workspace(&mut job.cloud_workspace, workspace);
}

/// Take in what Managoat says of its workspace. `error` is its failing step
/// and the end of what that printed, already redacted; the end is kept.
pub(super) fn observe_cloud_workspace(w: &mut crate::db::cloud::CloudWorkspace, workspace: &Value) {
    if let Some(status @ ("provisioning" | "ready" | "failed")) = workspace["status"].as_str() {
        w.status = status.into();
    }
    let text = |key: &str| workspace[key].as_str().map(str::to_owned);
    w.sandbox_status = text("sandbox_status");
    w.setup_step = if w.status == "provisioning" { text("setup_step") } else { None };
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
            super::workspaces::resend_settings(app, &id).await;
            match p.workspace(&workspace).await {
                Ok(workspace) => observe(job, &workspace),
                Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                    job.execution.stage = "lost".into();
                    fail(job, "Managoat no longer has this Cloud workspace. Delete it and create it again.");
                    return save_cloud_workspace(app, job, now);
                }
                Err(e) => return Err(e),
            }
            super::rebuild::send_queued(app, job, &workspace, now).await?;
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
        // A failed one asks Managoat nothing (`step_cloud_tab`): it only
        // waits for a prompt to relaunch it.
        "idle" | "failed" => CLOUD_TAB_IDLE_MS,
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
            let key = job.execution.launch_key(&id);
            let prompt =
                if job.execution.launches == 0 { job.spec.prompt.as_deref() } else { job.execution.prompt.as_deref() };
            let conversation = p.launch_in_cloud_workspace(&agent, &workspace, &job.spec.cwd, &key, prompt).await?;
            job.execution.conversation = Some(conversation["id"].as_str().context("missing conversation")?.into());
            job.tab.status = conversation["status"].as_str().unwrap_or("pending").into();
            job.execution.stage = "live".into();
            save_cloud_tab(app, job, now)
        }
        "conversation_creating" => {
            job.execution.conversation =
                p.find_cloud_tab_resource("conversations", &job.execution.launch_key(&id)).await?;
            if job.execution.conversation.is_some() {
                job.tab.status = "pending".into();
                job.execution.stage = "live".into();
            } else {
                not_made(job, "agent_ready");
            }
            save_cloud_tab(app, job, now)
        }
        "live" => {
            // Its conversation failed, and a prompt asks for a new one: open
            // it with that prompt. The old one's events stay the tab's, and
            // Managoat's event ids are global, so the new one's follow them.
            if let Some(prompt) = job.relaunch.clone() {
                job.relaunch_taken = true;
                job.execution.launches += 1;
                job.execution.prompt = Some(prompt);
                job.execution.conversation = None;
                job.execution.stage = "agent_ready".into();
                job.started = false;
                job.tab.status = "opening".into();
                job.tab.error = None;
                return save_cloud_tab(app, job, now);
            }
            // A failed conversation is terminal and was read to its end when
            // it failed: wait for a prompt, asking Managoat nothing.
            if job.tab.status == "failed" {
                return app.db.checkpoint_cloud_tab_job(job, Some(now + CLOUD_TAB_IDLE_MS));
            }
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
            if job.tab.status == "failed" {
                job.tab.error = Some(TAB_FAILED.into());
            }
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
        // Settings (M117): the environment a setup read when it started,
        // and how the next environment PATCH fails.
        built_environment: Option<Value>,
        refuse_patch: bool,
        lose_patch: bool,
        /// The workspace's machine is parked: its reads answer 409.
        parked: bool,
        // Rebuild and Retry (M120): how many asks find no machine free, and
        // whether the next one is taken but its answer lost.
        rebuild_unavailable: u32,
        lose_rebuild: bool,
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
                ("POST", "prompts") if matches!(conversation["status"].as_str(), Some("failed" | "terminated")) => {
                    return (StatusCode::GONE, axum::Json(json!({"error":"conversation_terminal"}))).into_response();
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
                let mut environment =
                    json!({"setup_timeout_seconds":120,"networking_type":"unrestricted","networking_config":{}});
                environment.as_object_mut().unwrap().extend(value.as_object().cloned().unwrap_or_default());
                environment["id"] = json!("env1");
                environment["revision"] = json!("rev1");
                fake.environment = Some(environment.clone());
                if fake.lost("environments") {
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                environment
            }
            // Each edit of a build field moves the revision, as Managoat's
            // trigger does.
            ("PATCH", "/api/environments/env1") => {
                if std::mem::take(&mut fake.refuse_patch) {
                    return StatusCode::UNPROCESSABLE_ENTITY.into_response();
                }
                let environment = fake.environment.get_or_insert_with(|| json!({"id":"env1","revision":"rev1"}));
                let before = environment.clone();
                for (k, v) in value.as_object().unwrap() {
                    environment[k] = v.clone();
                }
                if *environment != before {
                    let n = environment["revision"].as_str().unwrap()[3..].parse::<u32>().unwrap() + 1;
                    environment["revision"] = json!(format!("rev{n}"));
                }
                let environment = environment.clone();
                if std::mem::take(&mut fake.lose_patch) {
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                environment
            }
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
                    "built_from_revision":null,"stale":false,"changed_fields":[],"setup_step":"packages",
                });
                fake.workspace = Some(workspace.clone());
                fake.built_environment = fake.environment.clone();
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
                let (environment, built) = (fake.environment.clone(), fake.built_environment.clone());
                let built_revision = built.as_ref().map_or(json!("rev1"), |b| b["revision"].clone());
                let Some(workspace) = fake.workspace.as_mut() else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                // Behind its environment once built, field by field.
                if let (Some(environment), Some(built)) = (environment, built) {
                    workspace["revision"] = environment["revision"].clone();
                    if workspace["status"] == "ready" {
                        let fields = ["repositories", "packages", "setup_script", "setup_timeout_seconds", "env_vars"]
                            .into_iter()
                            .chain(["networking_type", "networking_config"])
                            .filter(|f| environment[f] != built[f])
                            .collect::<Vec<_>>();
                        workspace["stale"] = json!(!fields.is_empty());
                        workspace["changed_fields"] = json!(fields);
                    }
                }
                // Set up on the second read.
                if reads >= setup_reads && failed {
                    workspace["status"] = json!("failed");
                    workspace["sandbox_status"] = Value::Null;
                    workspace["error"] = json!("setup_script exited 1: npm ERR! missing script: build");
                    workspace["setup_step"] = Value::Null;
                } else if reads >= setup_reads {
                    workspace["status"] = json!("ready");
                    workspace["sandbox_status"] = json!("ready");
                    workspace["built_from_revision"] = built_revision;
                    workspace["setup_step"] = Value::Null;
                }
                workspace.clone()
            }
            // What the last setup printed, kept by Managoat: readable while
            // the machine is parked, failed or gone.
            ("GET", "/api/workspaces/ws1/setup-log") => {
                if fake.workspace.is_none() {
                    return StatusCode::NOT_FOUND.into_response();
                }
                json!({
                    "workspace_id":"ws1","sandbox_id":"sandbox9","status":"failed","step":"setup_script",
                    "started_at":"2026-10-19T12:00:00Z","finished_at":"2026-10-19T12:00:09Z","internal":"x",
                    "steps":[
                        {"step":"packages","status":"succeeded","started_at":"2026-10-19T12:00:01Z","finished_at":"2026-10-19T12:00:05Z",
                         "output":"Setting up jq\n","omitted_lines":0,"omitted_bytes":0,"sandbox_id":"sandbox9"},
                        {"step":"setup_script","status":"failed","started_at":"2026-10-19T12:00:05Z","finished_at":"2026-10-19T12:00:09Z",
                         "output":"npm ERR! missing script: build\n","omitted_lines":3,"omitted_bytes":120},
                    ],
                })
            }
            // The workspace's machine, read-only: Managoat answers a parked
            // one 409 and never wakes it.
            ("GET", read) if read.starts_with("/api/workspaces/ws1/") => {
                let query = uri.query().unwrap_or_default();
                if fake.parked {
                    return (
                        StatusCode::CONFLICT,
                        axum::Json(json!({"error":"sandbox_not_ready","message":"the sandbox is suspended","status":"suspended"})),
                    )
                        .into_response();
                }
                if query.contains("path=%2Fhome%2Fsprite&") || query.ends_with("path=%2Fhome%2Fsprite") {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        axum::Json(json!({"error":"not_a_repository","message":"echoed repo-secret"})),
                    )
                        .into_response();
                }
                match &read["/api/workspaces/ws1/".len()..] {
                    "diff" => json!({
                        "path":"/home/sprite/app","repo_root":"/home/sprite/app","staged":false,"ref":"HEAD",
                        "diff":"diff --git a/README.md b/README.md\n+docs","truncated":false,"sandbox_id":"sandbox9",
                    }),
                    "git-status" => json!({
                        "path":"/home/sprite/app","repo_root":"/home/sprite/app","branch":"main","untracked":"all",
                        "entries":[{"path":"README.md","index":"unchanged","worktree":"modified","renamed_from":null},
                                   {"path":"notes.txt","index":"untracked","worktree":"untracked","renamed_from":null}],
                        "truncated":false,
                    }),
                    "file" => {
                        json!({"path":"/home/sprite/app/notes.txt","size":5,"truncated":false,"encoding":"utf-8","content":"notes"})
                    }
                    "files" => {
                        json!({"path":"/home/sprite","entries":[{"name":"app","type":"directory","size":null}],"truncated":false})
                    }
                    _ => return StatusCode::NOT_FOUND.into_response(),
                }
            }
            // A new machine from the environment as it is now, set up over
            // the next reads. A rebuild is refused while a tab's turn runs,
            // unless it interrupts; a retry, unless the setup failed.
            ("POST", "/api/workspaces/ws1/rebuild" | "/api/workspaces/ws1/retry") => {
                let refuse = |code: &str, extra: Value| {
                    let mut body = json!({"error":code,"message":"refused"});
                    body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
                    body
                };
                if fake.rebuild_unavailable > 0 {
                    fake.rebuild_unavailable -= 1;
                    return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(refuse("fleet_full", json!({}))))
                        .into_response();
                }
                let status = fake.workspace.as_ref().unwrap()["status"].clone();
                if path.ends_with("/retry") && status != "failed" {
                    return (StatusCode::CONFLICT, axum::Json(refuse("workspace_not_failed", json!({}))))
                        .into_response();
                }
                if status == "provisioning" {
                    return (StatusCode::CONFLICT, axum::Json(refuse("rebuild_in_progress", json!({}))))
                        .into_response();
                }
                let running: Vec<String> = fake
                    .tab_conversations
                    .iter()
                    .filter(|(_, c)| c["status"] == "running")
                    .map(|(id, _)| id.clone())
                    .collect();
                if path.ends_with("/rebuild") && !running.is_empty() {
                    if value["interrupt"] != true {
                        let ids = json!({"conversation_ids":running});
                        return (StatusCode::CONFLICT, axum::Json(refuse("turns_running", ids))).into_response();
                    }
                    for id in running {
                        fake.tab_conversations.get_mut(&id).unwrap()["status"] = json!("idle");
                    }
                }
                fake.built_environment = fake.environment.clone();
                fake.workspace_reads = 0;
                let workspace = fake.workspace.as_mut().unwrap();
                workspace["status"] = json!("provisioning");
                workspace["sandbox_status"] = json!("pending");
                workspace["error"] = Value::Null;
                workspace["built_from_revision"] = Value::Null;
                let workspace = workspace.clone();
                if std::mem::take(&mut fake.lose_rebuild) {
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                return (StatusCode::ACCEPTED, axum::Json(json!({"data":workspace}))).into_response();
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
        // Both tabs stay readable as past tabs: the one closed on its own
        // and the one its Cloud workspace's delete ended. Only to their
        // account.
        let past = call(M::GET, &format!("{base}/past-tabs"), "a", None).await.1;
        let past = past["past_tabs"].as_array().unwrap().clone();
        assert_eq!(past.len(), 2, "{past:?}");
        assert_eq!((past[0]["id"].as_str(), past[1]["id"].as_str()), (Some(two), Some(one)), "newest closed first");
        assert!(past.iter().all(|t| t["cloud_workspace_deleted"] == true && t["cloud_workspace_name"] == "App box"));
        assert_eq!(past[1]["agent_name"], claude["agent_name"]);
        assert_eq!(call(M::GET, &format!("{base}/past-tabs"), "b", None).await.1, json!({"past_tabs": []}));
        for tab in [one, two] {
            let (status, events) = call(M::GET, &format!("{base}/past-tabs/{tab}/events"), "a", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(events["events"].as_array().unwrap().len(), 2, "its transcript is kept");
            assert_eq!(call(M::GET, &format!("{base}/past-tabs/{tab}"), "a", None).await.1["id"], tab);
            assert_eq!(
                call(M::GET, &format!("{base}/past-tabs/{tab}/requests"), "a", None).await.1,
                json!({"requests": []})
            );
            for path in ["", "/events", "/requests"] {
                assert_eq!(
                    call(M::GET, &format!("{base}/past-tabs/{tab}{path}"), "b", None).await.0,
                    StatusCode::NOT_FOUND
                );
            }
        }
        let fake = fake.lock().unwrap();
        let at = |m: &str, p: &str| fake.calls.iter().position(|(cm, cp, _)| cm == m && cp == p).unwrap();
        assert!(at("DELETE", "/api/agents/agent3") < at("DELETE", "/api/vaults/vault1"));
        drop(fake);
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn cloud_workspace_reads_are_account_scoped_and_never_wake_it() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake::default()).await;
        let w = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let item = format!("{base}/workspaces/{}", w.id);
        // Before Managoat has made its machine there's nothing to read.
        let (status, body) = call(M::GET, &format!("{item}/diff"), "a", None).await;
        assert_eq!((status, body["reason"].as_str()), (StatusCode::CONFLICT, Some("not_ready")));
        run(&app, 0, 60).await;
        assert_eq!(app.db.cloud_workspace("a", &w.id).unwrap().unwrap().status, "ready");

        let (status, diff) = call(M::GET, &format!("{item}/diff?path=/home/sprite/app&ref=HEAD"), "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(diff["repo_root"], "/home/sprite/app");
        assert!(diff["diff"].as_str().unwrap().contains("+docs"));
        // Only the documented fields reach the client.
        assert!(diff.get("sandbox_id").is_none());
        let (status, git) =
            call(M::GET, &format!("{item}/git-status?path=/home/sprite/app&untracked=all"), "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(git["entries"][1]["path"], "notes.txt");
        assert_eq!(git["branch"], "main");
        let (status, file) = call(M::GET, &format!("{item}/file?path=/home/sprite/app/notes.txt"), "a", None).await;
        assert_eq!((status, file["content"].as_str()), (StatusCode::OK, Some("notes")));
        let (status, files) = call(M::GET, &format!("{item}/files"), "a", None).await;
        assert_eq!((status, files["entries"][0]["name"].as_str()), (StatusCode::OK, Some("app")));
        let sent: Vec<String> = fake
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(_, p, _)| p.starts_with("/api/workspaces/ws1/"))
            .map(|(_, p, _)| p.clone())
            .collect();
        assert_eq!(
            sent,
            [
                "/api/workspaces/ws1/diff?path=%2Fhome%2Fsprite%2Fapp&ref=HEAD",
                "/api/workspaces/ws1/git-status?path=%2Fhome%2Fsprite%2Fapp&untracked=all",
                "/api/workspaces/ws1/file?path=%2Fhome%2Fsprite%2Fapp%2Fnotes.txt",
                "/api/workspaces/ws1/files",
            ]
        );

        // Another account's id is 404 on every read, one outside the beta
        // 403, and neither reaches Managoat.
        let before = fake.lock().unwrap().calls.len();
        for read in ["files", "file?path=/home/sprite/app/notes.txt", "diff", "git-status"] {
            assert_eq!(call(M::GET, &format!("{item}/{read}"), "b", None).await.0, StatusCode::NOT_FOUND, "{read}");
            assert_eq!(call(M::GET, &format!("{item}/{read}"), "c", None).await.0, StatusCode::FORBIDDEN, "{read}");
        }
        assert_eq!(fake.lock().unwrap().calls.len(), before);

        // Managoat's refusal comes back in control's words, never its body.
        let (status, body) = call(M::GET, &format!("{item}/diff?path=/home/sprite"), "a", None).await;
        assert_eq!((status, body["reason"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("not_a_repository")));
        assert!(!body.to_string().contains("repo-secret"));
        for bad in ["diff?path=%01", "git-status?untracked=everything", "file?path=a&max_bytes=0"] {
            assert_eq!(
                call(M::GET, &format!("{item}/{bad}"), "a", None).await.0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{bad}"
            );
        }
        assert_eq!(call(M::GET, &format!("{item}/diff?sandbox=1"), "a", None).await.0, StatusCode::BAD_REQUEST);

        // Parked: each read is refused, and nothing but those reads is sent,
        // so nothing wakes the machine.
        fake.lock().unwrap().parked = true;
        let before = fake.lock().unwrap().calls.len();
        for read in ["files", "file?path=/home/sprite/app/notes.txt", "diff", "git-status"] {
            let (status, body) = call(M::GET, &format!("{item}/{read}"), "a", None).await;
            assert_eq!((status, body["reason"].as_str()), (StatusCode::CONFLICT, Some("parked")), "{read}");
        }
        let calls = fake.lock().unwrap().calls[before..].to_vec();
        assert_eq!(calls.len(), 4);
        assert!(calls.iter().all(|(m, p, _)| m == "GET" && p.starts_with("/api/workspaces/ws1/")), "{calls:?}");

        // Deleting: no reads.
        app.db.delete_cloud_workspace("a", &w.id).unwrap();
        let (status, body) = call(M::GET, &format!("{item}/diff"), "a", None).await;
        assert_eq!((status, body["reason"].as_str()), (StatusCode::CONFLICT, Some("deleting")));
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

    // ---- settings (M117)

    async fn save(base: &str, id: &str, account: &str, spec: &Value) -> (StatusCode, Value) {
        call(reqwest::Method::PATCH, &format!("{base}/workspaces/{id}"), account, Some(json!({"spec":spec}))).await
    }

    #[tokio::test]
    async fn saved_settings_reach_the_environment_and_say_what_a_rebuild_changes() {
        let (app, fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db.put_cloud_secret("a", "NPM_TOKEN", &cipher.seal("a", "NPM_TOKEN", "npm-secret").unwrap()).unwrap();
        run(&app, 0, 60).await;
        let (base, routes) = routes(&app).await;
        let mut spec = serde_json::to_value(&created.saved_spec).unwrap();

        // A secret only: live in the vault, nothing to rebuild.
        spec["secrets"] = json!(["NPM_TOKEN"]);
        let (status, saved) = save(&base, &created.id, "a", &spec).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(
            (&saved["pending"], &saved["stale"], &saved["settings_applied"]),
            (&json!([]), &json!(false), &json!(true))
        );
        assert_eq!(
            calls(&fake, "POST", "/api/vaults/vault1/secrets").last(),
            Some(&json!({"key":"NPM_TOKEN","value":"npm-secret"}))
        );

        // A package and an environment variable: the environment has them,
        // and the machine is behind in those two fields.
        spec["secrets"] = json!([]);
        spec["packages"]["apt"] = json!(["ffmpeg", "jq"]);
        spec["env_vars"] = json!({"API_URL":"https://api.example"});
        let (status, saved) = save(&base, &created.id, "a", &spec).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(saved["saved_spec"]["packages"]["apt"], json!(["ffmpeg", "jq"]));
        assert_eq!(saved["built_spec"]["packages"]["apt"], json!(["ffmpeg"]));
        assert_eq!(saved["stale"], true);
        assert_eq!(
            saved["pending"],
            json!([
                {"field":"packages","before":{"apt":["ffmpeg"],"npm":[]},"after":{"apt":["ffmpeg","jq"],"npm":[]}},
                {"field":"env_vars","before":[],"after":["API_URL"]},
            ])
        );
        assert!(!saved["pending"].to_string().contains("api.example"));
        let patch = calls(&fake, "PATCH", "/api/environments/env1").last().cloned().unwrap();
        assert_eq!(patch["packages"]["apt"], json!(["ffmpeg", "jq"]));
        assert_eq!(patch["env_vars"], json!({"API_URL":"https://api.example"}));
        assert_eq!((&patch["setup_timeout_seconds"], &patch["networking_type"]), (&json!(120), &json!("unrestricted")));
        assert!(patch.get("name").is_none() && patch.get("metadata").is_none());
        assert_eq!(calls(&fake, "DELETE", "/api/vaults/vault1/secrets/NPM_TOKEN").len(), 1);

        // The poll keeps saying so, and the built spec stays the built one.
        run(&app, 60_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(shown.pending.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(), ["packages", "env_vars"]);
        assert_eq!(shown.built_spec.unwrap().packages.apt, ["ffmpeg"]);
        assert_eq!(shown.status, "ready");

        // Another account's is not found; bad settings are refused.
        assert_eq!(save(&base, &created.id, "b", &spec).await.0, StatusCode::NOT_FOUND);
        let mut bad = spec.clone();
        bad["setup_timeout_seconds"] = json!(901);
        assert_eq!(save(&base, &created.id, "a", &bad).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        bad = spec.clone();
        bad["secrets"] = json!(["MISSING"]);
        assert_eq!(save(&base, &created.id, "a", &bad).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        app.db.delete_cloud_workspace("a", &created.id).unwrap();
        assert_eq!(save(&base, &created.id, "a", &spec).await.0, StatusCode::CONFLICT);
        server.abort();
        routes.abort();
    }

    #[tokio::test]
    async fn a_refused_save_changes_nothing_and_a_lost_one_is_sent_again() {
        let (app, fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 60).await;
        let (base, routes) = routes(&app).await;
        let mut spec = serde_json::to_value(&created.saved_spec).unwrap();
        spec["packages"]["apt"] = json!(["ffmpeg", "jq"]);
        spec["name"] = json!("Renamed");

        fake.lock().unwrap().refuse_patch = true;
        assert_eq!(save(&base, &created.id, "a", &spec).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(
            (&shown.saved_spec, shown.name.as_str(), shown.settings_applied),
            (&created.saved_spec, "App box", true)
        );

        fake.lock().unwrap().lose_patch = true;
        let (status, saved) = save(&base, &created.id, "a", &spec).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&saved["settings_applied"], &saved["name"]), (&json!(false), &json!("Renamed")));
        run(&app, 60_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert!(shown.settings_applied);
        assert_eq!(shown.pending.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(), ["packages"]);
        // Refused, lost, then sent again by the worker, and no more.
        assert_eq!(calls(&fake, "PATCH", "/api/environments/env1").len(), 3);
        server.abort();
        routes.abort();
    }

    #[tokio::test]
    async fn a_save_while_setting_up_keeps_the_spec_the_setup_built() {
        let (app, fake, server) = serve(Fake { setup_reads: 6, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let mut spec = serde_json::to_value(&created.saved_spec).unwrap();
        spec["packages"]["apt"] = json!(["ffmpeg", "jq"]);
        // Not before Managoat has the workspace.
        assert_eq!(save(&base, &created.id, "a", &spec).await.0, StatusCode::CONFLICT);
        let mut at = 0;
        while fake.lock().unwrap().workspace.is_none() {
            tick_at(&app, at).await.unwrap();
            at += 1_000;
        }
        tick_at(&app, at + 3_000).await.unwrap();
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "provisioning");
        assert_eq!(save(&base, &created.id, "a", &spec).await.0, StatusCode::OK);
        run(&app, at + 6_000, 60).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(shown.status, "ready");
        assert_eq!(shown.built_spec.as_ref(), Some(&created.saved_spec));
        assert_eq!(shown.saved_spec.packages.apt, ["ffmpeg", "jq"]);
        assert_eq!(shown.pending.len(), 1);
        server.abort();
        routes.abort();
    }

    // ---- Rebuild and Retry (M120)

    #[tokio::test]
    async fn rebuild_is_refused_mid_turn_unless_interrupted_and_keeps_the_tabs() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 30).await;
        let (base, routes) = routes(&app).await;
        let item = format!("{base}/workspaces/{}", created.id);
        let mut spec = serde_json::to_value(&created.saved_spec).unwrap();
        spec["packages"]["apt"] = json!(["ffmpeg", "jq"]);
        let (status, saved) = save(&base, &created.id, "a", &spec).await;
        assert_eq!((status, &saved["stale"]), (StatusCode::OK, &json!(true)));
        // A tab mid-turn.
        let open = json!({"request_key":"one","agent_id":"curated_coding","runtime":"claude","model":"anthropic/test-model","prompt":"Run the tests"});
        let tab = call(M::POST, &format!("{item}/tabs"), "a", Some(open)).await.1;
        run(&app, 30_000, 10).await;
        let tab_url = format!("{item}/tabs/{}", tab["id"].as_str().unwrap());
        assert_eq!(call(M::GET, &tab_url, "a", None).await.1["status"], "running");

        let rebuild = format!("{item}/rebuild");
        assert_eq!(call(M::POST, &rebuild, "b", Some(json!({}))).await.0, StatusCode::NOT_FOUND);
        let (status, refused) = call(M::POST, &rebuild, "a", Some(json!({}))).await;
        assert_eq!((status, &refused["reason"]), (StatusCode::CONFLICT, &json!("turns_running")));
        assert_eq!(
            refused["error"],
            "Coding agent · claude is mid-turn. Wait for it to finish, or interrupt and rebuild."
        );
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready");

        let (status, rebuilt) = call(M::POST, &rebuild, "a", Some(json!({"interrupt":true}))).await;
        assert_eq!((status, &rebuilt["status"]), (StatusCode::OK, &json!("provisioning")));
        assert_eq!(calls(&fake, "POST", "/api/workspaces/ws1/rebuild"), [json!({}), json!({"interrupt":true})]);
        let (status, again) = call(M::POST, &rebuild, "a", Some(json!({}))).await;
        assert_eq!((status, &again["reason"]), (StatusCode::CONFLICT, &json!("setting_up")));

        // Ready again, built from the settings as saved.
        run(&app, 50_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.stale), ("ready", false));
        assert!(shown.pending.is_empty());
        assert_eq!(shown.built_from_revision.as_deref(), Some("rev2"));
        assert_eq!(shown.built_spec.as_ref(), Some(&shown.saved_spec));
        assert_eq!(shown.built_spec.unwrap().packages.apt, ["ffmpeg", "jq"]);
        // The tab is still open on the same conversation, which wasn't ended.
        let tabs = call(M::GET, &format!("{item}/tabs"), "a", None).await.1;
        assert_eq!(tabs["cloud_tabs"].as_array().unwrap().len(), 1);
        assert_eq!(tabs["cloud_tabs"][0]["id"], tab["id"]);
        assert_eq!(calls(&fake, "POST", "/api/conversations").len(), 1);
        assert!(calls(&fake, "POST", "/api/conversations/tabconv1/terminate").is_empty());
        assert!(!fake.lock().unwrap().calls.iter().any(|(m, p, _)| m == "DELETE" && p.starts_with("/api/agents/")));
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_rebuild_with_no_machine_free_is_asked_again_and_a_lost_answer_is_not() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { rebuild_unavailable: 2, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 30).await;
        let (base, routes) = routes(&app).await;
        let rebuild = format!("{base}/workspaces/{}/rebuild", created.id);
        let (status, queued) = call(M::POST, &rebuild, "a", Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&queued["rebuild_queued"], &queued["status"]), (&json!(true), &json!("ready")));
        // Asked again by the worker until Managoat has a machine.
        run(&app, 30_000, 60).await;
        assert_eq!(posts(&fake, "/api/workspaces/ws1/rebuild"), 3);
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.rebuild_queued, shown.rebuild_error), ("ready", false, None));

        // Taken, but its answer lost: Managoat says it's setting up, and it
        // isn't asked again.
        fake.lock().unwrap().lose_rebuild = true;
        let (status, rebuilt) = call(M::POST, &rebuild, "a", Some(json!({}))).await;
        assert_eq!((status, &rebuilt["status"]), (StatusCode::OK, &json!("provisioning")));
        run(&app, 90_000, 30).await;
        assert_eq!(posts(&fake, "/api/workspaces/ws1/rebuild"), 4);
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "ready");

        // No machine for ten minutes: let go, and said so.
        fake.lock().unwrap().rebuild_unavailable = u32::MAX;
        assert_eq!(call(M::POST, &rebuild, "a", Some(json!({}))).await.1["rebuild_queued"], true);
        run(&app, arugula_e2e::now_ms() + 11 * 60_000, 2).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert!(!shown.rebuild_queued);
        assert!(shown.rebuild_error.unwrap().starts_with("Managoat had no machine free for ten minutes"));
        assert_eq!(posts(&fake, "/api/workspaces/ws1/rebuild"), 5);
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_rebuild_waiting_for_a_machine_can_be_cancelled_until_it_is_sent() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { rebuild_unavailable: u32::MAX, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 30).await;
        let (base, routes) = routes(&app).await;
        let rebuild = format!("{base}/workspaces/{}/rebuild", created.id);
        assert_eq!(call(M::POST, &rebuild, "a", Some(json!({}))).await.1["rebuild_queued"], true);

        // Another account's isn't found; the person's is cancelled, and the
        // worker doesn't ask again.
        assert_eq!(call(M::DELETE, &rebuild, "b", None).await.0, StatusCode::NOT_FOUND);
        assert!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().rebuild_queued);
        let (status, cancelled) = call(M::DELETE, &rebuild, "a", None).await;
        assert_eq!(
            (status, &cancelled["rebuild_queued"], &cancelled["status"]),
            (StatusCode::OK, &json!(false), &json!("ready"))
        );
        let now = arugula_e2e::now_ms();
        run(&app, now, 30).await;
        assert_eq!(posts(&fake, "/api/workspaces/ws1/rebuild"), 1);
        let (status, none) = call(M::DELETE, &rebuild, "a", None).await;
        assert_eq!((status, &none["reason"]), (StatusCode::CONFLICT, &json!("not_waiting")));

        // Taken by the worker to send: too late. Back in the queue after
        // another 503, it can be cancelled again.
        assert_eq!(call(M::POST, &rebuild, "a", Some(json!({}))).await.1["rebuild_queued"], true);
        let (asked, taken) = app.db.claim_cloud_workspace_rebuild(&created.id).unwrap().unwrap();
        assert!(!taken);
        let (status, started) = call(M::DELETE, &rebuild, "a", None).await;
        assert_eq!((status, &started["reason"]), (StatusCode::CONFLICT, &json!("rebuild_started")));
        app.db.unclaim_cloud_workspace_rebuild(&created.id, &asked).unwrap();
        assert_eq!(call(M::DELETE, &rebuild, "a", None).await.0, StatusCode::OK);

        // One taken when control stopped may have gone: let go, not sent again.
        assert_eq!(call(M::POST, &rebuild, "a", Some(json!({}))).await.1["rebuild_queued"], true);
        app.db.claim_cloud_workspace_rebuild(&created.id).unwrap().unwrap();
        let sent = posts(&fake, "/api/workspaces/ws1/rebuild");
        run(&app, now + 30_000, 2).await;
        assert_eq!(posts(&fake, "/api/workspaces/ws1/rebuild"), sent);
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert!(!shown.rebuild_queued);
        assert!(shown.rebuild_error.unwrap().starts_with("Managoat didn't answer"));

        // Sent by the worker once there's a machine: it has started.
        assert_eq!(call(M::POST, &rebuild, "a", Some(json!({}))).await.1["rebuild_queued"], true);
        fake.lock().unwrap().rebuild_unavailable = 0;
        run(&app, now + 60_000, 1).await;
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "provisioning");
        let (status, started) = call(M::DELETE, &rebuild, "a", None).await;
        assert_eq!((status, &started["reason"]), (StatusCode::CONFLICT, &json!("rebuild_started")));
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_tab_starts_in_a_repository_once_it_is_built() {
        use reqwest::Method as M;
        let (app, _fake, server) = serve(Fake::default()).await;
        let created = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let item = format!("{base}/workspaces/{}", created.id);
        let open = |key: &str, cwd: &str| json!({"request_key":key,"agent_id":"curated_coding","runtime":"claude","model":"anthropic/test-model","cwd":cwd});
        // Before the first build, the saved settings are what it's built from.
        assert_eq!(call(M::POST, &format!("{item}/tabs"), "a", Some(open("one", "/work/api"))).await.0, StatusCode::OK);
        run(&app, 0, 30).await;
        assert!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().built_spec.is_some());

        // A repository saved since isn't on the disk until a rebuild.
        let mut spec = serde_json::to_value(&created.saved_spec).unwrap();
        spec["repositories"]
            .as_array_mut()
            .unwrap()
            .push(json!({"url":"https://github.com/example/web","mount":"/work/web"}));
        assert_eq!(save(&base, &created.id, "a", &spec).await.0, StatusCode::OK);
        let (status, refused) = call(M::POST, &format!("{item}/tabs"), "a", Some(open("two", "/work/web"))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        for (key, cwd) in [("three", "/work/api/src"), ("four", "/home/sprite/notes")] {
            assert_eq!(
                call(M::POST, &format!("{item}/tabs"), "a", Some(open(key, cwd))).await.0,
                StatusCode::OK,
                "{cwd}"
            );
        }
        let (status, rebuilt) = call(M::POST, &format!("{item}/rebuild"), "a", Some(json!({"interrupt":true}))).await;
        assert_eq!((status, &rebuilt["status"]), (StatusCode::OK, &json!("provisioning")), "{rebuilt}");
        run(&app, 30_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!(shown.built_spec.unwrap().repositories.len(), 3);
        assert_eq!(
            call(M::POST, &format!("{item}/tabs"), "a", Some(open("five", "/work/web"))).await.0,
            StatusCode::OK
        );
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn retry_sets_a_failed_cloud_workspace_up_again() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { failed_setup: true, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        run(&app, 0, 60).await;
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "failed");
        let (base, routes) = routes(&app).await;
        let retry = format!("{base}/workspaces/{}/retry", created.id);
        assert_eq!(call(M::POST, &retry, "b", None).await.0, StatusCode::NOT_FOUND);
        fake.lock().unwrap().failed_setup = false;
        let (status, retried) = call(M::POST, &retry, "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((&retried["status"], &retried["error"]), (&json!("provisioning"), &Value::Null));
        assert_eq!((posts(&fake, "/api/workspaces/ws1/retry"), posts(&fake, "/api/workspaces/ws1/rebuild")), (1, 0));
        run(&app, 60_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.error), ("ready", None));
        assert_eq!(shown.built_spec.as_ref(), Some(&shown.saved_spec));
        // Not failed: nothing to retry.
        let (status, refused) = call(M::POST, &retry, "a", None).await;
        assert_eq!((status, &refused["reason"]), (StatusCode::CONFLICT, &json!("not_failed")));
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn a_tab_whose_conversation_failed_gets_a_new_one_on_its_next_prompt() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { failed_setup: true, setup_reads: 1_000, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let tabs = format!("{base}/workspaces/{}/tabs", created.id);
        let body = |key: &str| json!({"request_key":key,"agent_id":"curated_review","runtime":"claude","model":"anthropic/test-model","prompt":"Review it"});
        let tab = call(M::POST, &tabs, "a", Some(body("one"))).await.1;
        let id = tab["id"].as_str().unwrap().to_owned();
        let other = call(M::POST, &tabs, "a", Some(body("two"))).await.1;
        let other = other["id"].as_str().unwrap().to_owned();
        run(&app, 0, 30).await;
        // The setup its first prompt waited for fails, and Managoat fails the
        // conversation with it: `failed` is terminal.
        fake.lock().unwrap().setup_reads = 1;
        let conversation = |fake: &Mutex<Fake>, key: &str| {
            fake.lock()
                .unwrap()
                .tab_conversations
                .iter()
                .find(|(_, c)| c["labels"]["arugula_tab"] == key)
                .map(|(id, _)| id.clone())
                .unwrap()
        };
        let first = conversation(&fake, &id);
        {
            let mut f = fake.lock().unwrap();
            f.tab_conversations.get_mut(&first).unwrap()["status"] = json!("failed");
            f.tab_events.insert(
                first.clone(),
                vec![
                    json!({"id":10,"stage":"turn","state":"started","blocks":[{"kind":"prompt","body":"Review it"}]}),
                    json!({"id":11,"kind":"output","stage":"setup","data":"npm ERR! missing script: build\n"}),
                    json!({"id":12,"stage":"setup","state":"failed","data":"{\"exit_code\":127}"}),
                ],
            );
        }
        run(&app, 30_000, 30).await;
        assert_eq!(app.db.cloud_workspace("a", &created.id).unwrap().unwrap().status, "failed");
        let shown = call(M::GET, &format!("{tabs}/{id}"), "a", None).await.1;
        assert_eq!(shown["status"], "failed");
        assert!(shown["error"].as_str().unwrap().contains("Retry"), "{shown}");
        // Retry, then prompt it: a new conversation for the tab, labelled
        // apart from the failed one, started with that prompt.
        fake.lock().unwrap().failed_setup = false;
        assert_eq!(
            call(M::POST, &format!("{base}/workspaces/{}/retry", created.id), "a", None).await.0,
            StatusCode::OK
        );
        run(&app, 60_000, 10).await;
        let prompt = format!("{tabs}/{id}/prompt");
        assert_eq!(call(M::POST, &prompt, "b", Some(json!({"prompt":"Again"}))).await.0, StatusCode::NOT_FOUND);
        let (status, answer) = call(M::POST, &prompt, "a", Some(json!({"prompt":"Again"}))).await;
        assert_eq!((status, answer), (StatusCode::OK, json!({"relaunched": true})));
        let shown = call(M::GET, &format!("{tabs}/{id}"), "a", None).await.1;
        assert_eq!((&shown["status"], &shown["error"]), (&json!("opening"), &Value::Null));
        assert_eq!(call(M::POST, &prompt, "a", Some(json!({"prompt":"Twice"}))).await.0, StatusCode::CONFLICT);
        assert!(calls(&fake, "POST", &format!("/api/conversations/{first}/prompts")).is_empty());
        run(&app, 70_000, 20).await;
        let second = conversation(&fake, &format!("{id}.1"));
        assert_ne!(second, first);
        let launch = calls(&fake, "POST", "/api/conversations")
            .into_iter()
            .find(|l| l["labels"]["arugula_tab"] == format!("{id}.1"))
            .unwrap();
        assert_eq!((&launch["prompt"], &launch["client_request_id"]), (&json!("Again"), &json!(format!("{id}.1"))));
        fake.lock().unwrap().tab_events.insert(
            second.clone(),
            vec![json!({"id":20,"stage":"turn","state":"started","blocks":[{"kind":"prompt","body":"Again"}]})],
        );
        run(&app, 100_000, 10).await;
        let shown = call(M::GET, &format!("{tabs}/{id}"), "a", None).await.1;
        assert_eq!((&shown["status"], &shown["error"]), (&json!("running"), &Value::Null));
        // The old transcript stays above the new one.
        let events = call(M::GET, &format!("{tabs}/{id}/events"), "a", None).await.1;
        let ids: Vec<u64> = events["events"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()).collect();
        assert_eq!(ids, [10, 11, 12, 20]);
        // A conversation Managoat ended before control saw it: its prompt is
        // refused as gone (410), and the tab relaunches the same way.
        let gone = conversation(&fake, &other);
        fake.lock().unwrap().tab_conversations.get_mut(&gone).unwrap()["status"] = json!("terminated");
        let (status, answer) =
            call(M::POST, &format!("{tabs}/{other}/prompt"), "a", Some(json!({"prompt":"Hello"}))).await;
        assert_eq!((status, answer), (StatusCode::OK, json!({"relaunched": true})));
        run(&app, 120_000, 20).await;
        let launch = calls(&fake, "POST", "/api/conversations")
            .into_iter()
            .find(|l| l["labels"]["arugula_tab"] == format!("{other}.1"))
            .unwrap();
        assert_eq!(launch["prompt"], "Hello");
        routes.abort();
        server.abort();
    }

    #[tokio::test]
    async fn setup_step_and_setup_log_are_read_through_control() {
        use reqwest::Method as M;
        let (app, fake, server) = serve(Fake { failed_setup: true, setup_reads: 1_000, ..Default::default() }).await;
        let created = cloud_workspace(&app);
        let (base, routes) = routes(&app).await;
        let log = format!("{base}/workspaces/{}/setup-log", created.id);
        // Before Managoat has the workspace there's nothing to read.
        assert_eq!(call(M::GET, &log, "a", None).await, (StatusCode::OK, json!({"log": null})));
        run(&app, 0, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.setup_step.as_deref()), ("provisioning", Some("packages")));
        let listed = call(M::GET, &format!("{base}/workspaces/{}", created.id), "a", None).await.1;
        assert_eq!(listed["setup_step"], "packages", "{listed}");
        fake.lock().unwrap().setup_reads = 1;
        run(&app, 30_000, 30).await;
        let shown = app.db.cloud_workspace("a", &created.id).unwrap().unwrap();
        assert_eq!((shown.status.as_str(), shown.setup_step), ("failed", None));
        // Only the documented fields: no ids, nothing else Managoat adds.
        let (status, read) = call(M::GET, &log, "a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read["log"]["status"], "failed");
        assert_eq!(read["log"]["step"], "setup_script");
        assert_eq!(read["log"]["steps"][1]["output"], "npm ERR! missing script: build\n");
        assert_eq!(read["log"]["steps"][1]["omitted_lines"], 3);
        let text = read.to_string();
        assert!(!text.contains("sandbox9") && !text.contains("ws1") && !text.contains("internal"), "{text}");
        assert_eq!(call(M::GET, &log, "b", None).await.0, StatusCode::NOT_FOUND);
        // Reading it never touches the machine.
        let fake_calls = fake.lock().unwrap().calls.clone();
        assert!(
            fake_calls
                .iter()
                .filter(|(_, p, _)| p.starts_with("/api/workspaces/ws1/"))
                .all(|(m, p, _)| m == "GET" && p == "/api/workspaces/ws1/setup-log")
        );
        routes.abort();
        server.abort();
    }
}
