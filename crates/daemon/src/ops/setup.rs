//! Getting started's buttons (#578): how far each part of setup is, and
//! Tailscale serve, joining Arugula control, Claude Code's MCP server and an
//! agent's adapter. HTTP only, on `setup.rs`'s router, all the owner's. The
//! status and the control answers are built as JSON values (their keys
//! depend on the call); a button's is an [`Outcome`]. The card *What your
//! agent can do here* is MCP's server's (`arugulad-mcp`).

use arugula_proto::{
    api::Empty,
    op::ops::{SetupAgent, SetupClaude, SetupControl, SetupGet, SetupHooks, SetupTailscale},
    setup::{ControlJoinRequest, Outcome, SetupQuery},
};
use serde_json::Value;

use super::{Cx, Handle, OpError};

impl Handle for SetupGet {
    async fn handle(cx: &Cx<'_>, _: (), q: SetupQuery) -> Result<Value, OpError> {
        Ok(crate::setup::status(cx.app, q).await)
    }
}

impl Handle for SetupTailscale {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::tailscale_serve(cx.app).await)
    }
}

impl Handle for SetupControl {
    async fn handle(cx: &Cx<'_>, _: (), req: ControlJoinRequest) -> Result<Value, OpError> {
        use arugula_proto::sharing;
        // Getting started's answer to "may people type into it through
        // Arugula cloud?" (Arugula #1194): taken only at the machine, as
        // `arugulad sharing remote-input` is.
        if req.remote_input.is_some() && !cx.at_the_machine() {
            return Err(OpError::Forbidden(
                "whether people may type here through Arugula cloud is set on the machine itself".into(),
            ));
        }
        // Unanswered (Getting started over the tailnet, which doesn't ask),
        // a machine joining for the first time is off; one already joined,
        // or set before, keeps what it has.
        let dir = cx.app.control.state_dir();
        let joined = cx.app.control.enrolled().is_some();
        if let Some(on) = sharing::join_sets(req.remote_input, joined, sharing::remote_input_said(dir)) {
            sharing::set_remote_input(dir, on)
                .map_err(|e| OpError::Failed(format!("couldn't write {}: {e}", sharing::FILE)))?;
            tracing::info!(on, answered = req.remote_input.is_some(), "remote input set as the machine joins");
        }
        Ok(crate::setup::control_join(cx.app, req).await)
    }
}

impl Handle for SetupClaude {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::claude_mcp(cx.app).await)
    }
}

impl Handle for SetupHooks {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::claude_hooks(cx.app).await)
    }
}

impl Handle for SetupAgent {
    async fn handle(cx: &Cx<'_>, kind: String, _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::use_agent(cx.app, kind).await)
    }
}
