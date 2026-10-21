//! Deleting an account (M137): what Cloud asks of it first
//! ([`delete_blockers`]), and what it erases once the hub has deleted it
//! ([`account_deleted`]). Both were the hub's own (`account::plan` and
//! `Db::delete_account`) while Cloud lived there; the hub now asks through
//! [`PlatformPort`](crate::PlatformPort).

use anyhow::Result;

use crate::Platform;

/// What stands in the way of deleting the account, for Cloud: runs still
/// stopping, and with `start`, its Cloud workspaces until Managoat has
/// deleted them (which `start` asks for: the worker does it).
pub(crate) fn delete_blockers(app: &Platform, account: &str, start: bool) -> Result<Vec<String>> {
    if app.db.active_cloud_runs(account)? > 0 {
        return Ok(vec!["Wait for your cloud runs to finish stopping before deleting your account. A run holding changes that couldn't be saved needs them recovered first.".into()]);
    }
    if !start {
        return Ok(vec![]);
    }
    // Cloud workspaces' Managoat resources go in the worker: ask for that,
    // and refuse until it's done.
    app.db.delete_cloud_workspaces(account)?;
    if !app.db.cloud_workspaces_gone(account)? {
        return Ok(vec!["Your Cloud workspaces are being deleted. Try again in a minute.".into()]);
    }
    Ok(vec![])
}

/// The hub deleted the account: erase what Cloud keeps of it. Its Managoat
/// resources went first ([`delete_blockers`]); one made since, in the
/// moment between, is erased all the same and named in the log, since the
/// account it belonged to is gone.
pub(crate) fn account_deleted(app: &Platform, account: &str) -> Result<()> {
    let left = app.db.erase_cloud_account(account)?;
    if !left.is_empty() {
        tracing::warn!(%account, managoat = ?left, "a deleted account's Cloud workspaces were still at Managoat");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        PlatformPort,
        cloud::tests::{TestHub, unstarted_app_with},
        db::cloud::SpecRules,
    };

    /// Through the port the hub calls: a run still stopping blocks, Cloud
    /// workspaces block a deletion that's started until Managoat has
    /// deleted them, and once the hub has deleted the account, nothing of
    /// its stays and nothing more of its is kept.
    #[tokio::test]
    async fn deleting_an_account_waits_for_managoat_then_erases_it() {
        let hub = Arc::new(TestHub::default());
        let app = unstarted_app_with(hub.clone()).start();
        let spec: crate::db::cloud::RunSpec = serde_json::from_value(serde_json::json!({
            "agent_id":"agent_test","agent_version":1,"model":"anthropic/test","runtime":"claude",
            "agent":{"name":"Test","instructions":"Fix tests"},
            "repository":"https://github.com/example/repo","git_ref":"main","prompt":"fix tests",
        }))
        .unwrap();
        app.db.insert_cloud_run("a", &spec, &Default::default(), "running", 0).unwrap();
        let blockers = app.delete_blockers("a", false).await.unwrap();
        assert!(blockers[0].starts_with("Wait for your cloud runs"), "{blockers:?}");
        assert!(app.delete_blockers("b", false).await.unwrap().is_empty(), "another account's run");
        let job = app.db.claim_cloud_job(0).unwrap().unwrap();
        app.db.checkpoint_cloud_job(&job, "cancelled", Some(&serde_json::json!({})), None).unwrap();

        let mut box_spec: crate::db::cloud::CloudWorkspaceSpec =
            serde_json::from_value(serde_json::json!({"name":"Box"})).unwrap();
        box_spec.normalize().unwrap();
        let w = app.db.create_cloud_workspace("a", "prj_a", "k", &box_spec, 0).unwrap();
        let kept = app.db.create_cloud_workspace("b", "prj_b", "k", &box_spec, 0).unwrap();
        assert!(app.delete_blockers("a", false).await.unwrap().is_empty(), "the preview starts nothing");
        assert_eq!(app.db.cloud_workspace("a", &w.id).unwrap().unwrap().status, "provisioning");
        let blockers = app.delete_blockers("a", true).await.unwrap();
        assert_eq!(blockers, ["Your Cloud workspaces are being deleted. Try again in a minute."]);
        assert_eq!(app.db.cloud_workspace("a", &w.id).unwrap().unwrap().status, "deleting");
        let job = loop {
            let job = app.db.claim_cloud_workspace_job(0).unwrap().unwrap();
            if job.cloud_workspace.id == w.id {
                break job;
            }
        };
        app.db.finish_cloud_workspace_delete(&job, 1).unwrap();
        assert!(app.delete_blockers("a", true).await.unwrap().is_empty());

        hub.accounts.lock().unwrap().remove("a");
        let gone = crate::Deleted { account: "a".into(), machines: vec![], disbanded: vec![] };
        app.account_deleted(&gone).await.unwrap();
        app.account_deleted(&gone).await.unwrap();
        for table in ["cloud_runs", "cloud_workspaces", "cloud_tabs", "cloud_secrets", "cloud_agents"] {
            let n: u64 = app
                .db
                .c()
                .query_row(&format!("SELECT count(*) FROM {table} WHERE account = 'a'"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
        assert!(app.db.cloud_workspace("b", &kept.id).unwrap().is_some(), "another account's stays");
        // Nothing more is kept for it: the hub says it's gone.
        assert!(crate::cloud::personal_project(&app, "a").await.is_err());
        assert!(crate::cloud::require_account(&app, "a").await.is_err());
        assert!(crate::cloud::personal_project(&app, "b").await.unwrap().starts_with("prj_"));
    }
}
