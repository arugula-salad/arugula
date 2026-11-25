//! `close`: a pane or block, ending what runs in it; its output stays in
//! history. Over HTTP (`arugula close`, the web) and as MCP's `close`.

use arugula_proto::{
    api::{Closed, Empty},
    op::ops::ClosePane,
};

use super::{Cx, Handle, OpError};
use crate::{
    machine_sessions::Pane,
    mcp::{
        ops::{McpOp, Surface},
        tools::{Call, CloseArgs, Human, Out, done},
    },
    mux::Api,
    server::App,
};

impl Handle for ClosePane {
    async fn handle(cx: &Cx<'_>, id: arugula_proto::PaneId, _: Empty) -> Result<Closed, OpError> {
        use arugula_proto::op::Op;
        cx.may(Self::ACCESS, id).await?;
        // An agent's invites (#234) are the owner's to close, not an agent's.
        if cx.agent_or_guest() && crate::api::is_invite(cx.app, id).await {
            return Err(OpError::Forbidden(crate::invite::CLOSE_OWNER_ONLY.into()));
        }
        close(cx.app, id).await.map(|(closed, _)| closed).ok_or(OpError::NoPane(id))
    }
}

/// Close block `id`, as `close` does once it's let: its worktree removed
/// only if nothing would be lost. A block a ladder start opened (aru-143)
/// is told to control at once, so its board's grant has the slot back for
/// the next start; the bool says control was told. None: no such pane.
pub(crate) async fn close(app: &App, id: arugula_proto::PaneId) -> Option<(Closed, bool)> {
    // A block that owns a worktree (AR-2), or that a ladder start opened:
    // read it before the block goes.
    let config = app.mux.api(|r| Api::Block(id, r)).await.flatten().map(|b| b.config());
    let owned: Option<crate::worktree::Owned> =
        config.as_ref().and_then(|c| c.get("worktree").cloned()).and_then(|w| serde_json::from_value(w).ok());
    let ladder = config.as_ref().is_some_and(|c| c["work"].get("ladder").is_some_and(|l| !l.is_null()));
    let place = match ladder {
        true => app
            .mux
            .api(Api::Panes)
            .await
            .unwrap_or_default()
            .iter()
            .find(|p| p.info.id == id)
            .map(|p| Pane::of(p).place),
        false => None,
    };
    if app.mux.api(|r| Api::Close(id, r)).await != Some(true) {
        return None;
    }
    let told = match &place {
        Some(place) => crate::board_state::tell_closed(app, id, place).await,
        None => false,
    };
    // The block is closed whatever comes of its worktree, which is
    // removed only if nothing would be lost.
    let worktree = match owned {
        Some(w) => Some(match crate::worktree::local(app).await {
            Ok(runner) => crate::worktree::settle(&runner, &w).await,
            Err(e) => crate::worktree::kept(&w, format!("couldn't check it: {e}")),
        }),
        None => None,
    };
    Some((Closed { worktree }, told))
}

impl McpOp for ClosePane {
    const SURFACE: Surface = Surface::Tool {
        name: "close",
        title: "Close a pane",
        description: "Close a pane or block, ending what runs in it. A block that owns a git worktree (start_agent with worktree, or an agent on an issue) has it removed, with its local branch, only if nothing is uncommitted or unpushed and the branch is merged into its base (or its squash-merged PR's head is exactly HEAD, which gh says); otherwise worktree says kept and why. Never forced. With to: an agent a board's start opened on another of your machines (start_agent with board and to), freeing its slot under the board's grant.",
        destructive: true,
        idempotent: false,
        open_world: false,
        human: Human::In("run"),
    };
    type Args = CloseArgs;
    type Answer = crate::mcp::results::ClosedPane;

    // A `to` that isn't this machine went to `ladder::stop` before here.
    async fn args(_: &Call<'_>, a: &CloseArgs) -> Result<(arugula_proto::PaneId, Empty), String> {
        Ok((a.pane.id()?, Empty {}))
    }

    fn answer(_: &Call<'_>, _: &CloseArgs, &pane: &arugula_proto::PaneId, closed: Closed) -> Out {
        let summary = match &closed.worktree {
            Some(w) => format!("Closed %{pane}; {}", crate::worktree::sentence(w)),
            None => format!("Closed %{pane}"),
        };
        done(summary, crate::mcp::results::ClosedPane { pane, worktree: closed.worktree })
    }
}
