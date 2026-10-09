//! `wait`: until a pane's command ends, it exits, its output matches, or an
//! agent block is idle or needs input (`arugula wait`). HTTP only: MCP's
//! `wait` has no limit of its own and words its errors as sentences, so it
//! keeps calling `api::wait_until`.

use std::time::Duration;

use arugula_proto::{
    PaneId,
    api::{WaitRequest, WaitResult},
    op::ops::PaneWait,
};

use super::{Cx, Handle, OpError};

impl Handle for PaneWait {
    async fn handle(cx: &Cx<'_>, id: PaneId, q: WaitRequest) -> Result<WaitResult, OpError> {
        let app = cx.app;
        let limit = q.timeout.map(Duration::from_secs_f64).unwrap_or(Duration::from_secs(365 * 24 * 3600));
        if q.until == "idle" || q.until == "needs-input" {
            let result =
                tokio::time::timeout(limit, crate::api::wait_attention(app, id, q.until == "needs-input")).await;
            return Ok(match result {
                Ok(r) => r?,
                Err(_) => WaitResult::Timeout,
            });
        }
        let p = crate::api::pane(app, id).await?;
        let result = tokio::time::timeout(limit, crate::api::wait_for(app, id, p, &q)).await;
        Ok(match result {
            Ok(r) => r?,
            Err(_) => WaitResult::Timeout,
        })
    }
}
