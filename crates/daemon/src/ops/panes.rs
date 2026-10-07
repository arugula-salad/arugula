//! `ls`: every pane and block. Over HTTP (`arugula ls`, the web) and as
//! MCP's `list` kind panes, where an agent block's token sees only its own
//! tab, and the answer is shaped for agents.

use arugula_proto::{
    Attention,
    api::{Empty, PaneSummary},
    op::ops::ListPanes,
};

use super::{Cx, Handle, OpError};
use crate::{
    mcp::{
        ops::{McpOp, Surface},
        results,
        tools::{Call, ListArgs, Out, done, entry},
    },
    mux::Api,
};

impl Handle for ListPanes {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<PaneSummary>, OpError> {
        Ok(cx.app.mux.api(Api::Panes).await.unwrap_or_default())
    }
}

impl McpOp for ListPanes {
    const SURFACE: Surface = Surface::Kind {
        tool: "list",
        kind: "panes",
        description: "Every pane and block (terminals, browsers, agents): where it is, what it runs, whether it needs attention, who started it.",
    };
    type Args = ListArgs;

    fn args(_: &Call<'_>, _: ListArgs) -> Result<((), Empty), String> {
        Ok(((), Empty {}))
    }

    fn answer(call: &Call<'_>, _: &(), panes: Vec<PaneSummary>) -> Out {
        let tab = call.me().and_then(|me| panes.iter().find(|p| p.info.id == me).map(|p| p.tab));
        let own = call.own_pane();
        let entries: Vec<results::PaneEntry> = panes
            .iter()
            .filter(|p| tab.is_none_or(|t| p.tab == t))
            .take(300)
            .map(|p| {
                let mut e = entry(p);
                if own == Some(p.info.id) {
                    e.you = Some(true);
                }
                e
            })
            .collect();
        let needs: Vec<String> = panes
            .iter()
            .filter(|p| tab.is_none_or(|t| p.tab == t) && p.info.attention == Attention::NeedsInput)
            .map(|p| format!("%{}", p.info.id))
            .collect();
        let mut summary = match tab {
            Some(_) => format!("{} panes and blocks in this agent's tab", entries.len()),
            None => format!("{} panes and blocks", entries.len()),
        };
        if !needs.is_empty() {
            summary.push_str(&format!("; {} need input", needs.join(", ")));
        }
        done(summary, results::Panes { panes: entries })
    }
}
