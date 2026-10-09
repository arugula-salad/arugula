//! Getting started's buttons (#578): how far each part of setup is, and
//! Tailscale serve, joining Arugula control, Claude Code's MCP server and an
//! agent's adapter. HTTP only, on `setup.rs`'s router, all the owner's. The
//! status and the control answers are built as JSON values (their keys
//! depend on the call); a button's is an [`Outcome`].

use arugula_proto::{
    api::Empty,
    op::ops::{SetupAgent, SetupClaude, SetupControl, SetupControlConfirm, SetupGet, SetupTailscale},
    setup::{ControlConfirmRequest, ControlJoinRequest, Outcome, SetupQuery},
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
        Ok(crate::setup::control_join(cx.app, req).await)
    }
}

impl Handle for SetupControlConfirm {
    async fn handle(cx: &Cx<'_>, _: (), req: ControlConfirmRequest) -> Result<Value, OpError> {
        Ok(crate::setup::control_confirm(cx.app, req).await)
    }
}

impl Handle for SetupClaude {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::claude_mcp(cx.app).await)
    }
}

impl Handle for SetupAgent {
    async fn handle(cx: &Cx<'_>, kind: String, _: Empty) -> Result<Outcome, OpError> {
        Ok(crate::setup::use_agent(cx.app, kind).await)
    }
}
