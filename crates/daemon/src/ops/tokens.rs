//! `mcp/tokens`: the client tokens for MCP clients that reach `/mcp` over
//! HTTP (`arugula mcp token`). All are the owner's (`serve` checks that
//! before the handler runs).

use arugula_proto::{
    api::{Empty, McpTokenRequest, TokenInfo},
    op::ops::{McpTokenMint, McpTokenRevoke, McpTokensList},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::api::ApiError;

impl Handle for McpTokensList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<TokenInfo>, OpError> {
        Ok(cx.app.mcp.list())
    }
}

impl Handle for McpTokenMint {
    async fn handle(cx: &Cx<'_>, _: (), req: McpTokenRequest) -> Result<TokenInfo, OpError> {
        cx.app.mcp.mint(&req.name, req.scope).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e).into())
    }
}

impl Handle for McpTokenRevoke {
    async fn handle(cx: &Cx<'_>, name: String, _: Empty) -> Result<Empty, OpError> {
        if cx.app.mcp.revoke(&name) {
            Ok(Empty {})
        } else {
            Err(OpError::Status(StatusCode::NOT_FOUND, format!("no mcp token named {name}")))
        }
    }
}
