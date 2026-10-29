//! Agents on hosted machines (M184, #1245): the hub's part.
//!
//! - **Archiving.** A hosted machine sends a closed agent block's transcript
//!   ([`transcript`]); the platform keeps it under the workspace its
//!   certificate names (`PlatformPort::hosted_transcript`). Only that
//!   machine's signature opens the route.
//! - **Driving.** A project person's own machine reaches a hosted machine
//!   for MCP (`run`, `start_agent`, `read_output`, `show` there) only while
//!   it holds a live `cloud.drive` grant for its account ("Grants, not
//!   impersonation"): the relay asks the platform ([`machine_may_drive`])
//!   before it routes a machine to one. People are routed as before.

use std::sync::Arc;

use arugula_control_wire::transcripts::{TranscriptArchived, TranscriptPost};
use arugula_platform_api::{Archived, PlatformPort as _};
use axum::{Json, extract::State, http::StatusCode};
use serde_json::Value;

use crate::{ApiError, App, auth::HostedAuth, err};

/// `POST /api/hosted/transcripts`: a hosted machine's closed agent block,
/// kept by the platform under its workspace's project.
pub async fn transcript(
    State(app): State<Arc<App>>,
    h: HostedAuth,
    Json(b): Json<TranscriptPost>,
) -> Result<Json<Value>, ApiError> {
    match app.platform.hosted_transcript(&h.0.cert.workspace, &h.0.cert.device, &b).await? {
        Archived::Kept { id } => Ok(Json(serde_json::to_value(TranscriptArchived { id })?)),
        Archived::Refused { status, error } => {
            Err(err(StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN), &error))
        }
    }
}

/// May machine `daemon`, of `account`, be routed to `id`? Always, unless
/// `id` is a hosted machine: then only with a live `cloud.drive` grant for
/// the account (whether the account is in the project is the relay's
/// usual check, `teams::daemon_may_reach`).
pub async fn machine_may_drive(app: &App, daemon: &str, account: &str, id: &str) -> anyhow::Result<bool> {
    if app.db.hosted(id)?.is_none() {
        return Ok(true);
    }
    app.platform.machine_drives(daemon, account).await
}
