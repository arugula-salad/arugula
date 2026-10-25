//! `close`: a pane or block, ending what runs in it; its output stays in
//! history. Over HTTP (`arugula close`, the web) and as MCP's `close`.

use arugula_proto::{
    api::{Closed, Empty},
    op::ops::ClosePane,
};

use super::{Cx, Handle, OpError};
use crate::{
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, Out, PaneOnly, done},
    },
    mux::Api,
};

impl Handle for ClosePane {
    async fn handle(cx: &Cx<'_>, id: arugula_proto::PaneId, _: Empty) -> Result<Closed, OpError> {
        use arugula_proto::op::Op;
        cx.may(Self::ACCESS, id).await?;
        // An agent's invites (#234) are the owner's to close, not an agent's.
        if cx.agent_or_guest() && crate::api::is_invite(cx.app, id).await {
            return Err(OpError::Forbidden(crate::invite::CLOSE_OWNER_ONLY.into()));
        }
        // A block that owns a worktree (AR-2): read it before the block goes.
        let owned = match cx.app.mux.api(|r| Api::Block(id, r)).await.flatten() {
            Some(b) => b.config().get("worktree").cloned().and_then(|w| serde_json::from_value(w).ok()),
            None => None,
        };
        match cx.app.mux.api(|r| Api::Close(id, r)).await {
            Some(true) => {}
            _ => return Err(OpError::NoPane(id)),
        }
        // The block is closed whatever comes of its worktree, which is
        // removed only if nothing would be lost.
        let worktree = match owned {
            Some(w) => Some(match crate::worktree::local(cx.app).await {
                Ok(runner) => crate::worktree::settle(&runner, &w).await,
                Err(e) => crate::worktree::kept(&w, format!("couldn't check it: {e}")),
            }),
            None => None,
        };
        Ok(Closed { worktree })
    }
}

impl McpOp for ClosePane {
    const SURFACE: Surface = Surface::Tool {
        name: "close",
        title: "Close a pane",
        description: "Close a pane or block, ending what runs in it. A block that owns a git worktree (start_agent with worktree, or an agent on an issue) has it removed, with its local branch, only if nothing is uncommitted or unpushed and the branch is merged into its base; otherwise worktree says kept and why. Never forced.",
        destructive: true,
        idempotent: false,
        open_world: false,
    };
    type Args = PaneOnly;
    type Answer = crate::mcp::results::ClosedPane;

    async fn args(_: &Call<'_>, a: &PaneOnly) -> Result<(arugula_proto::PaneId, Empty), String> {
        Ok((a.pane.id()?, Empty {}))
    }

    fn answer(_: &Call<'_>, _: &PaneOnly, &pane: &arugula_proto::PaneId, closed: Closed) -> Out {
        let summary = match &closed.worktree {
            Some(w) => format!("Closed %{pane}; {}", crate::worktree::sentence(w)),
            None => format!("Closed %{pane}"),
        };
        done(summary, crate::mcp::results::ClosedPane { pane, worktree: closed.worktree })
    }
}
