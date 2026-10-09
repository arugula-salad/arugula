//! `ls`: every pane and block. Over HTTP (`arugula ls`, the web) and as
//! MCP's `list` kind panes, where an agent block's token sees only its own
//! tab, and the answer is shaped for agents.

use arugula_proto::{
    Attention, PaneId,
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

    async fn args(_: &Call<'_>, _: &ListArgs) -> Result<((), Empty), String> {
        Ok(((), Empty {}))
    }

    fn answer(call: &Call<'_>, _: &ListArgs, _: &(), panes: Vec<PaneSummary>) -> Out {
        shape(&panes, call.me(), call.own_pane())
    }
}

/// The answer an agent gets: all the panes, or for an agent block (`me`),
/// those in its tab, with the caller's own pane (`own`) marked.
fn shape(panes: &[PaneSummary], me: Option<PaneId>, own: Option<PaneId>) -> Out {
    let tab = me.and_then(|me| panes.iter().find(|p| p.info.id == me).map(|p| p.tab));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: PaneId, tab: u32, attention: &str) -> PaneSummary {
        serde_json::from_value(serde_json::json!({
            "session": 1, "session_name": "s", "tab": tab, "tab_name": null,
            "id": id, "epoch": 0, "cwd": null, "command": null, "running": true,
            "policy": {"kind": "shell"}, "attention": attention,
        }))
        .unwrap()
    }

    fn listed(v: &serde_json::Value) -> Vec<(u64, bool)> {
        v["panes"].as_array().unwrap().iter().map(|p| (p["pane"].as_u64().unwrap(), p["you"] == true)).collect()
    }

    /// `list` kind panes is the grouped tool's default kind: a token with no
    /// agent block sees every pane, and an agent block sees only its own
    /// tab, with itself marked `you`.
    #[test]
    fn an_agent_blocks_list_is_its_tab_and_marks_itself() {
        let panes = [pane(1, 1, "idle"), pane(2, 1, "needs_input"), pane(3, 2, "needs_input")];

        let all = shape(&panes, None, Some(1)).unwrap();
        assert_eq!(listed(&all), [(1, true), (2, false), (3, false)]);
        assert_eq!(all["summary"], "3 panes and blocks; %2, %3 need input");

        let tab = shape(&panes, Some(2), Some(2)).unwrap();
        assert_eq!(listed(&tab), [(1, false), (2, true)]);
        assert_eq!(tab["summary"], "2 panes and blocks in this agent's tab; %2 need input");
    }
}
