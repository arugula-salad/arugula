//! Rebuild and Retry (S40, M120, #1077). A rebuild replaces a Cloud
//! workspace's machine with one set up from its settings as saved: the disk
//! is wiped, and its tabs keep their conversations and transcripts
//! (decision 9). Managoat refuses it while a tab is mid-turn, unless the
//! person chose to interrupt. A retry sets a failed one up again, with no
//! disk to wipe (decision 6, on Managoat's `retry`, #3013).
//!
//! Both ask Managoat at once, so a refusal reaches the person. A 503 (no
//! machine free, or the machine busy) changed nothing: it waits in
//! `rebuild` and the worker asks again for a while. Nothing else is asked
//! twice: a lost answer may have wiped the disk. The person can cancel one
//! that waits until the worker takes it to send.
use super::{enabled, provider::Refusal, worker::observe_cloud_workspace, workspaces::Sending};
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::cloud::{CancelledRebuild, CloudWorkspace, CloudWorkspaceJob, CloudWorkspaceRebuild},
};
use anyhow::{Context, Result};
use arugula_control_common::{ApiError, err, refusal};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// How long one Managoat had no machine for keeps being asked again.
const REBUILD_WAIT_MS: u64 = 10 * 60_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rebuild {
    /// End the tabs' running turns, rather than be refused.
    #[serde(default)]
    pub interrupt: bool,
}

/// `POST /api/cloud/workspaces/{id}/rebuild`. The answer says
/// `provisioning`, or `rebuild_queued` when Managoat had no machine free.
pub async fn rebuild(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
    Path(id): Path<String>,
    Json(input): Json<Rebuild>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let asked = CloudWorkspaceRebuild {
        retry: false,
        interrupt: input.interrupt,
        asked: arugula_e2e::now_ms(),
        sending: false,
    };
    ask(&app, &session, &id, asked).await
}

/// `POST /api/cloud/workspaces/{id}/retry`: a failed setup, again.
pub async fn retry(
    State(app): State<Arc<Platform>>,
    CloudDriver(session): CloudDriver,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let asked = CloudWorkspaceRebuild { retry: true, interrupt: false, asked: arugula_e2e::now_ms(), sending: false };
    ask(&app, &session, &id, asked).await
}

async fn ask(
    app: &Platform,
    session: &Person,
    id: &str,
    asked: CloudWorkspaceRebuild,
) -> Result<Json<CloudWorkspace>, ApiError> {
    let cloud = enabled(app, &session.account)?;
    let account = &session.account;
    let Some((mut w, workspace)) = app.db.cloud_workspace_machine(account, id)? else {
        return Err(err(StatusCode::NOT_FOUND, "cloud workspace not found"));
    };
    if w.status == "deleting" {
        return Err(err(StatusCode::CONFLICT, "it's being deleted"));
    }
    let Some(workspace) = workspace else {
        return Err(if w.status == "failed" {
            err(StatusCode::CONFLICT, "Managoat never set it up. Delete it and create it again.")
        } else {
            refusal(StatusCode::CONFLICT, "setting_up", "It's already setting up.")
        });
    };
    if !w.settings_applied {
        return Err(err(
            StatusCode::CONFLICT,
            "its settings are still on their way to Managoat; try again in a moment",
        ));
    }
    let Some(_sending) = Sending::take(id) else {
        return Err(err(StatusCode::CONFLICT, "its settings are being saved; try again in a moment"));
    };
    let event = if asked.retry { "cloud workspace retried" } else { "cloud workspace rebuilt" };
    match send(app, &workspace, &asked).await {
        Ok(answer) => {
            observe_cloud_workspace(&mut w, &answer);
            app.db.rebuilt_cloud_workspace(&w)?;
            app.analytics.capture(account, event, json!({"interrupt": asked.interrupt, "queued": false}));
        }
        Err(e) => match e.downcast_ref::<Refusal>() {
            Some(r) if r.status == 503 => {
                app.db.queue_cloud_workspace_rebuild(id, &asked)?;
                app.analytics.capture(account, event, json!({"interrupt": asked.interrupt, "queued": true}));
            }
            Some(r) => {
                // Managoat's own words differ for one already setting up.
                if r.code == "workspace_not_failed"
                    && cloud.provider.workspace(&workspace).await.is_ok_and(|v| v["status"] == "provisioning")
                {
                    return Err(refusal(StatusCode::CONFLICT, "setting_up", "It's already setting up."));
                }
                let names = app.db.cloud_tab_names(account, id, &r.conversation_ids)?;
                let (status, reason, text) = refused(r, &names);
                return Err(refusal(status, reason, &text));
            }
            None => {
                // It may have happened. Managoat says so if it did: the
                // workspace is setting up.
                tracing::warn!(cloud_workspace = %id, error = %e, "cloud workspace rebuild answer lost");
                match cloud.provider.workspace(&workspace).await {
                    Ok(answer) if answer["status"] == "provisioning" => {
                        observe_cloud_workspace(&mut w, &answer);
                        app.db.rebuilt_cloud_workspace(&w)?;
                        app.analytics.capture(account, event, json!({"interrupt": asked.interrupt, "queued": false}));
                    }
                    _ => {
                        return Err(err(
                            StatusCode::BAD_GATEWAY,
                            "Managoat didn't answer. Refresh before trying again: it may have started.",
                        ));
                    }
                }
            }
        },
    }
    app.db
        .cloud_workspace(account, id)?
        .map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))
}

/// `DELETE /api/cloud/workspaces/{id}/rebuild`: cancel a rebuild or retry
/// waiting for a machine. 409 `rebuild_started` once the worker has sent it
/// (or is sending it), and `not_waiting` with none waiting.
pub async fn cancel(
    State(app): State<Arc<Platform>>,
    session: Person,
    Path(id): Path<String>,
) -> Result<Json<CloudWorkspace>, ApiError> {
    enabled(&app, &session.account)?;
    let account = &session.account;
    match app.db.cancel_cloud_workspace_rebuild(account, &id)? {
        CancelledRebuild::Cancelled => {
            app.analytics.capture(account, "cloud workspace rebuild cancelled", json!({}));
        }
        CancelledRebuild::Started => {
            return Err(refusal(
                StatusCode::CONFLICT,
                "rebuild_started",
                "Too late to cancel: Managoat has a machine and it has started.",
            ));
        }
        CancelledRebuild::NotWaiting => {
            return Err(refusal(StatusCode::CONFLICT, "not_waiting", "Nothing is waiting for a machine."));
        }
        CancelledRebuild::NotFound => return Err(err(StatusCode::NOT_FOUND, "cloud workspace not found")),
    }
    app.db
        .cloud_workspace(account, &id)?
        .map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "cloud workspace not found"))
}

async fn send(app: &Platform, workspace: &str, asked: &CloudWorkspaceRebuild) -> Result<Value> {
    let p = &app.cloud.as_ref().context("cloud is disabled")?.provider;
    if asked.retry { p.retry_workspace(workspace).await } else { p.rebuild_workspace(workspace, asked.interrupt).await }
}

/// What a refusal tells the person: its status, reason and words. `names`
/// are the tabs mid-turn, for `turns_running`.
fn refused(r: &Refusal, names: &[String]) -> (StatusCode, &'static str, String) {
    match r.code.as_str() {
        "turns_running" => {
            let who = match names {
                [] => "A tab is".to_owned(),
                [one] => format!("{one} is"),
                [rest @ .., last] => format!("{} and {last} are", rest.join(", ")),
            };
            let them = if names.len() > 1 { "them" } else { "it" };
            (
                StatusCode::CONFLICT,
                "turns_running",
                format!("{who} mid-turn. Wait for {them} to finish, or interrupt and rebuild."),
            )
        }
        "rebuild_in_progress" => (StatusCode::CONFLICT, "setting_up", "It's already setting up.".into()),
        "workspace_not_failed" => {
            (StatusCode::CONFLICT, "not_failed", "Its setup didn't fail, so there's nothing to retry.".into())
        }
        "sandbox_provider_disabled" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "sandbox_provider_disabled",
            "Managoat's machine provider for this Cloud workspace is turned off. Nothing changed.".into(),
        ),
        _ if r.status == 404 => (
            StatusCode::CONFLICT,
            "lost",
            "Managoat no longer has this Cloud workspace. Delete it and create it again.".into(),
        ),
        _ => (StatusCode::BAD_GATEWAY, "refused", format!("Managoat refused it (HTTP {}). Nothing changed.", r.status)),
    }
}

/// The worker, on a live Cloud workspace it has just read: ask again for a
/// rebuild or retry Managoat had no machine for. One that no longer applies
/// (a setup is running, or a retry's workspace isn't failed) is let go.
pub(super) async fn send_queued(app: &Platform, job: &mut CloudWorkspaceJob, workspace: &str, now: u64) -> Result<()> {
    let id = job.cloud_workspace.id.clone();
    let Some(asked) = app.db.queued_cloud_workspace_rebuild(&id)? else {
        return Ok(());
    };
    let status = job.cloud_workspace.status.as_str();
    if status == "provisioning" || (asked.retry && status != "failed") {
        return app.db.drop_cloud_workspace_rebuild(&id, None);
    }
    if now > asked.asked + REBUILD_WAIT_MS {
        let what = if asked.retry { "set up again" } else { "rebuilt" };
        return app.db.drop_cloud_workspace_rebuild(
            &id,
            Some(&format!("Managoat had no machine free for ten minutes, so it wasn't {what}. Try again.")),
        );
    }
    // Settings still on their way: the rebuild waits for them.
    if !job.cloud_workspace.settings_applied {
        return Ok(());
    }
    let Some(_sending) = Sending::take(&id) else {
        return Ok(());
    };
    // Taken in a transaction a cancel also takes: from here a cancel is
    // answered `rebuild_started`, and one before it leaves nothing to take.
    let Some((asked, taken)) = app.db.claim_cloud_workspace_rebuild(&id)? else {
        return Ok(());
    };
    if taken {
        // Taken before control stopped: it may have gone.
        return app
            .db
            .drop_cloud_workspace_rebuild(&id, Some("Managoat didn't answer. If it isn't setting up now, try again."));
    }
    match send(app, workspace, &asked).await {
        Ok(answer) => {
            observe_cloud_workspace(&mut job.cloud_workspace, &answer);
            app.db.rebuilt_cloud_workspace(&job.cloud_workspace)
        }
        Err(e) => match e.downcast_ref::<Refusal>() {
            Some(r) if r.status == 503 => app.db.unclaim_cloud_workspace_rebuild(&id, &asked),
            Some(r) if matches!(r.code.as_str(), "rebuild_in_progress" | "workspace_not_failed") => {
                app.db.drop_cloud_workspace_rebuild(&id, None)
            }
            Some(r) => {
                let names = app.db.cloud_tab_names(&job.account, &id, &r.conversation_ids)?;
                app.db.drop_cloud_workspace_rebuild(&id, Some(&refused(r, &names).2))
            }
            None => {
                tracing::warn!(cloud_workspace = %id, error = %e, "queued cloud workspace rebuild answer lost");
                app.db.drop_cloud_workspace_rebuild(
                    &id,
                    Some("Managoat didn't answer. If it isn't setting up now, try again."),
                )
            }
        },
    }
}
