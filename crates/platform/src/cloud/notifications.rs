//! Durable, at-least-once Web Push for cloud approvals. Repeated deliveries
//! use the same notification tag. Cloud payloads are distinct from opaque
//! terminal notifications and contain no prompt or secret values.
#[cfg(test)]
use crate::hub::Pushed;
use crate::{Platform, hub::PushOptions};
use anyhow::Result;
use std::sync::Arc;

pub fn start(app: Arc<Platform>) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&app).await {
                tracing::warn!(%error,"cloud notifications will retry");
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

/// How long a push service keeps an approval for a device that's away:
/// five minutes, as Cloud's own pushes did before the hub sent them.
const APPROVAL: PushOptions = PushOptions { ttl: Some(std::time::Duration::from_secs(300)), any_subscription: false };

/// Each approval push still to send, through the hub (`HubPort::push_with`,
/// which holds the VAPID key and the subscriptions): delivered once no
/// push service failed it, else tried again in 30 s.
async fn tick(app: &Platform) -> Result<()> {
    tick_at(app, arugula_e2e::now_ms()).await
}

async fn tick_at(app: &Platform, now: u64) -> Result<()> {
    for n in app.db.cloud_notifications(now)? {
        let target = match &n.cloud_workspace {
            Some(cloud_workspace) => Target::Tab { cloud_workspace, tab: &n.conversation },
            None => Target::Run(&n.conversation),
        };
        let delivered = match app.hub.push_with(&n.account, &payload(target), APPROVAL).await {
            Ok(pushed) => pushed.failed == 0,
            Err(error) => {
                tracing::warn!(%error, "the hub didn't push a cloud approval");
                false
            }
        };
        app.db.cloud_notification_result(&n.conversation, &n.request, delivered, now + 30_000)?;
    }
    Ok(())
}

/// The conversation a push opens: a run, or a tab in a Cloud workspace.
#[derive(Clone, Copy)]
enum Target<'a> {
    Run(&'a str),
    Tab { cloud_workspace: &'a str, tab: &'a str },
}

/// What the push says: no prompt, no request, only where to go.
fn payload(target: Target<'_>) -> serde_json::Value {
    match target {
        Target::Run(run) => serde_json::json!({
            "title":"Arugula cloud agent", "body":"An agent needs your input.",
            "tag":format!("cloud-{run}"), "cloud_run":run,
        }),
        Target::Tab { cloud_workspace, tab } => serde_json::json!({
            "title":"Arugula cloud agent", "body":"An agent needs your input.",
            "tag":format!("cloud-{tab}"), "cloud_workspace":cloud_workspace, "cloud_tab":tab,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::{TestHub, unstarted_app_with};

    /// An approval waiting in a run's conversation: one push through the
    /// hub, saying where to go and nothing of the prompt, and none again
    /// once it went. A push a push service failed is tried again.
    #[tokio::test]
    async fn approvals_go_through_the_hubs_push_until_they_get_there() {
        let hub = Arc::new(TestHub::default());
        let app = unstarted_app_with(hub.clone()).start();
        let spec: crate::db::cloud::RunSpec = serde_json::from_value(serde_json::json!({
            "agent_id":"agent_test","agent_version":1,"model":"anthropic/test","runtime":"claude",
            "agent":{"name":"Test","instructions":"Fix tests"},
            "repository":"https://github.com/example/repo","git_ref":"main","prompt":"secret prompt",
        }))
        .unwrap();
        let run = app.db.insert_cloud_run("a", &spec, &Default::default(), "running", 0).unwrap();
        let job = app.db.claim_cloud_job(0).unwrap().unwrap();
        let ask =
            serde_json::json!({"id":1,"blocks":[{"kind":"permission_request","request_id":"r1","body":"allow?"}]});
        app.db.append_cloud_events(&job, &[ask]).unwrap();
        let now = arugula_e2e::now_ms();
        let pushes = || hub.pushes.lock().unwrap().len();
        *hub.pushed.lock().unwrap() = Pushed { sent: 0, gone: 0, failed: 1 };
        tick_at(&app, now).await.unwrap();
        assert_eq!(pushes(), 1);
        tick_at(&app, now + 1).await.unwrap();
        assert_eq!(pushes(), 1, "not again until it's due");
        *hub.pushed.lock().unwrap() = Pushed { sent: 1, gone: 0, failed: 0 };
        tick_at(&app, now + 31_000).await.unwrap();
        assert_eq!(pushes(), 2, "tried again");
        tick_at(&app, now + 62_000).await.unwrap();
        assert_eq!(pushes(), 2, "delivered");
        let (to, said) = hub.pushes.lock().unwrap()[0].clone();
        assert_eq!(to, "a");
        assert_eq!(*hub.options.lock().unwrap(), [APPROVAL; 2], "five minutes, to approved devices only");
        assert_eq!(said, payload(Target::Run(&run.id)));
        assert!(!said.to_string().contains("secret prompt") && !said.to_string().contains("allow?"));
        assert_eq!(
            payload(Target::Tab { cloud_workspace: "cw_1", tab: "cwc_1" }),
            serde_json::json!({"title":"Arugula cloud agent","body":"An agent needs your input.",
                "tag":"cloud-cwc_1","cloud_workspace":"cw_1","cloud_tab":"cwc_1"})
        );
    }
}
