//! `telemetry` (M101): whether this machine sends usage counts, the next
//! report as it would go, and the owner's answer. HTTP only, as `flags` is:
//! an owner's setting is not an agent's business.

use arugula_proto::{
    api::Empty,
    op::ops::{TelemetryGet, TelemetryNote, TelemetrySet},
    telemetry::{self as consent, TelemetryInfo, TelemetryNoteRequest, TelemetrySetRequest},
};

use super::{Cx, Handle, OpError};

/// The running daemon's telemetry, or one read from the state dir.
fn info(cx: &Cx<'_>) -> TelemetryInfo {
    match crate::telemetry::get() {
        Some(t) => t.info(),
        None => {
            let dir = cx.app.control.state_dir();
            let t = crate::telemetry::Telemetry::from_env(dir, &crate::home());
            t.set_signer(cx.app.control.clone());
            t.info()
        }
    }
}

/// The owner in person: not an agent (an MCP token, or the CLI an agent
/// runs), and not a guest.
fn in_person(cx: &Cx<'_>) -> Result<(), OpError> {
    if cx.agent_or_guest() {
        return Err(OpError::Forbidden("telemetry is the owner's to see and answer, not an agent's".into()));
    }
    Ok(())
}

impl Handle for TelemetryGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<TelemetryInfo, OpError> {
        in_person(cx)?;
        Ok(info(cx))
    }
}

impl Handle for TelemetrySet {
    async fn handle(cx: &Cx<'_>, _: (), req: TelemetrySetRequest) -> Result<TelemetryInfo, OpError> {
        in_person(cx)?;
        let via = req.via.as_deref().filter(|v| !v.is_empty()).unwrap_or("web");
        // A closed set: anything else is kept as `other`. `tui` is an older
        // CLI's (the TUI was removed).
        let via = if ["web", "desktop", "cli", "tui", "phone", "mcp"].contains(&via) { via } else { "other" };
        consent::set(cx.app.control.state_dir(), req.on, via, arugula_e2e::now_ms())
            .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", consent::FILE)))?;
        tracing::info!(on = req.on, via, "telemetry set");
        if let Some(t) = crate::telemetry::get() {
            t.answered();
        }
        Ok(info(cx))
    }
}

impl Handle for TelemetryNote {
    async fn handle(cx: &Cx<'_>, _: (), req: TelemetryNoteRequest) -> Result<Empty, OpError> {
        in_person(cx)?;
        // Off (not asked, no, or kept off), it counts nothing, and says
        // nothing of it: a page learns no more than `get` tells it.
        crate::telemetry::note(&req);
        Ok(Empty {})
    }
}
