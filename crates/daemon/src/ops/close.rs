//! `close`: a pane or block, ending what runs in it; its output stays in
//! history. Over HTTP (`arugula close`, the web) and as MCP's `close`.

use arugula_proto::{api::Empty, op::ops::ClosePane};

use super::{Cx, Handle, OpError};
use crate::{
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, Out, PaneOnly, done},
    },
    mux::Api,
};

impl Handle for ClosePane {
    async fn handle(cx: &Cx<'_>, id: arugula_proto::PaneId, _: Empty) -> Result<Empty, OpError> {
        use arugula_proto::op::Op;
        cx.may(Self::ACCESS, id).await?;
        // An agent's invites (#234) are the owner's to close, not an agent's.
        if cx.agent_or_guest() && crate::api::is_invite(cx.app, id).await {
            return Err(OpError::Forbidden(crate::invite::CLOSE_OWNER_ONLY.into()));
        }
        match cx.app.mux.api(|r| Api::Close(id, r)).await {
            Some(true) => Ok(Empty {}),
            _ => Err(OpError::NoPane(id)),
        }
    }
}

impl McpOp for ClosePane {
    const SURFACE: Surface = Surface::Tool {
        name: "close",
        title: "Close a pane",
        description: "Close a pane or block, ending what runs in it.",
        destructive: true,
        idempotent: false,
        open_world: false,
    };
    type Args = PaneOnly;

    fn args(_: &Call<'_>, a: PaneOnly) -> Result<(arugula_proto::PaneId, Empty), String> {
        Ok((a.pane.id()?, Empty {}))
    }

    fn answer(_: &Call<'_>, &pane: &arugula_proto::PaneId, _: Empty) -> Out {
        done(format!("Closed %{pane}"), crate::mcp::results::PaneOnly { pane })
    }
}
