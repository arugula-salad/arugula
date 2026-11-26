//! `ls`: every pane and block. Over HTTP (`arugula ls`, the web) and as
//! MCP's `list` kind panes, shaped for agents.

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
        tools::{Call, Out, done, entry},
    },
    mux::Api,
};

impl Handle for ListPanes {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<PaneSummary>, OpError> {
        Ok(cx.app.mux.api(Api::Panes).await.unwrap_or_default())
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema, Default)]
pub struct PanesArgs {
    /// Only this tab, by name or number.
    #[serde(default)]
    pub tab: Option<String>,
}

impl McpOp for ListPanes {
    const SURFACE: Surface = Surface::Kind {
        tool: "list",
        kind: "panes",
        description: "Every pane and block in any tab (terminals, browsers, agents): what it runs, if it needs attention, who opened it, agents' cost.",
    };
    type Args = PanesArgs;
    type Answer = serde_json::Value;

    async fn args(_: &Call<'_>, _: &PanesArgs) -> Result<((), Empty), String> {
        Ok(((), Empty {}))
    }

    fn answer(call: &Call<'_>, a: &PanesArgs, _: &(), panes: Vec<PaneSummary>) -> Out {
        shape(&panes, call.me(), call.own_pane(), a.tab.as_deref())
    }
}

/// The answer an agent gets: every pane on the machine, or one tab's
/// (`tab`, by name or number), with the caller's own pane (`own`) marked.
/// An agent block (`me`) reads every tab too: reading isn't acting.
fn shape(panes: &[PaneSummary], me: Option<PaneId>, own: Option<PaneId>, tab: Option<&str>) -> Out {
    let mine = me.or(own).and_then(|me| panes.iter().find(|p| p.info.id == me));
    let shown: Vec<&PaneSummary> = match tab.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            let n = t.parse::<u32>().ok();
            let shown: Vec<_> = panes.iter().filter(|p| Some(p.tab) == n || p.tab_name.as_deref() == Some(t)).collect();
            if shown.is_empty() {
                return Err(format!("no tab {t:?}: list with no tab shows every one"));
            }
            shown
        }
        None => panes.iter().collect(),
    };
    let entries: Vec<results::PaneEntry> = shown
        .iter()
        .take(300)
        .map(|p| {
            let mut e = entry(p);
            if own == Some(p.info.id) {
                e.you = Some(true);
            }
            e
        })
        .collect();
    let needs: Vec<String> =
        shown.iter().filter(|p| p.info.attention == Attention::NeedsInput).map(|p| format!("%{}", p.info.id)).collect();
    let mut tabs: Vec<(u32, &str)> = shown.iter().map(|p| (p.tab, p.session_name.as_str())).collect();
    tabs.sort_unstable();
    tabs.dedup();
    let mut summary =
        format!("{} panes and blocks in {} tab{}", entries.len(), tabs.len(), if tabs.len() == 1 { "" } else { "s" });
    if let Some(p) = mine {
        let name = p.tab_name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default();
        summary.push_str(&format!("; yours is tab {}{name}", p.tab));
    }
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

    fn tabbed(id: PaneId, tab: u32, name: Option<&str>, attention: &str) -> PaneSummary {
        let mut p = pane(id, tab, attention);
        p.tab_name = name.map(str::to_owned);
        p
    }

    /// An agent block reads every tab, as any other caller does, with itself
    /// marked `you`; its summary names its own tab.
    #[test]
    fn an_agent_blocks_list_is_every_tab_and_marks_itself() {
        let panes = [pane(1, 1, "idle"), tabbed(2, 1, Some("work"), "needs_input"), pane(3, 2, "needs_input")];

        let all = shape(&panes, None, Some(1), None).unwrap();
        assert_eq!(listed(&all), [(1, true), (2, false), (3, false)]);
        assert_eq!(all["summary"], "3 panes and blocks in 2 tabs; yours is tab 1; %2, %3 need input");

        let agent = shape(&panes, Some(2), Some(2), None).unwrap();
        assert_eq!(listed(&agent), [(1, false), (2, true), (3, false)]);
        assert_eq!(agent["summary"], "3 panes and blocks in 2 tabs; yours is tab 1 (work); %2, %3 need input");
    }

    /// `tab` narrows the answer to one tab, by name or by number.
    #[test]
    fn tab_narrows_the_list_by_name_or_number() {
        let panes = [
            tabbed(1, 1, Some("work"), "idle"),
            tabbed(2, 2, Some("agent-work"), "idle"),
            tabbed(3, 2, Some("agent-work"), "idle"),
        ];

        for tab in ["agent-work", "2", " 2 "] {
            let v = shape(&panes, Some(1), Some(1), Some(tab)).unwrap();
            assert_eq!(listed(&v), [(2, false), (3, false)], "{tab}");
            assert_eq!(v["summary"], "2 panes and blocks in 1 tab; yours is tab 1 (work)", "{tab}");
        }
        let v = shape(&panes, Some(1), Some(1), Some("work")).unwrap();
        assert_eq!(listed(&v), [(1, true)]);
        assert!(shape(&panes, Some(1), Some(1), Some("nope")).unwrap_err().contains("no tab"));
    }
}
