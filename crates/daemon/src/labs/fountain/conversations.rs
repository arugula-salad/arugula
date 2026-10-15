//! The account view's conversations (#942, F2): rows as a client draws
//! them, live status from the account stream, the label filter and a
//! conversation's tree. Pure: the block ([`super::FountainBlock`]) reads
//! Fountain and calls these.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

use super::{api::Agent, client::Conversation, client::TreeNode, stream::Event};

/// How many conversations a list reads at first, and how many more each
/// *More* adds (Fountain's `limit`, at most [`MAX_LIMIT`]).
pub const PAGE: u32 = 100;
pub const MAX_LIMIT: u32 = 500;

/// One conversation as a client draws it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Row {
    pub id: String,
    pub title: Option<String>,
    /// Fountain's: pending, running, idle, failed or terminated.
    pub status: String,
    /// What the stream said since the list was read (`running`, `waking`),
    /// until the next read.
    pub live: Option<String>,
    pub agent_id: Option<String>,
    /// The agent's name, when the agents list has it.
    pub agent: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub unread: bool,
    pub parent_id: Option<String>,
    /// How many of the listed conversations it started.
    pub children: usize,
    pub runtime: String,
    pub turn_count: u64,
    pub last_active_at: Option<String>,
    /// Its open permission requests.
    pub asking: usize,
}

/// The rows of a list, the agents' names filled in.
pub fn rows(convs: &[Conversation], agents: &[Agent]) -> Vec<Row> {
    let names: HashMap<&str, &str> = agents.iter().map(|a| (a.id.as_str(), a.name.as_str())).collect();
    let mut children: HashMap<&str, usize> = HashMap::new();
    for c in convs {
        if let Some(p) = &c.parent_conversation_id {
            *children.entry(p.as_str()).or_default() += 1;
        }
    }
    convs
        .iter()
        .map(|c| Row {
            id: c.id.clone(),
            title: c.title.clone().filter(|t| !t.trim().is_empty()),
            status: c.status.clone(),
            live: None,
            agent_id: c.agent_id.clone(),
            agent: c.agent_id.as_deref().and_then(|a| names.get(a)).map(|n| (*n).to_owned()),
            labels: c.labels.clone(),
            unread: c.unread,
            parent_id: c.parent_conversation_id.clone(),
            children: children.get(c.id.as_str()).copied().unwrap_or(0),
            runtime: c.runtime.clone(),
            turn_count: c.turn_count,
            last_active_at: c.last_active_at.clone(),
            asking: 0,
        })
        .collect()
}

/// What a row is called: its title, else its agent's name, else its id.
pub fn name(r: &Row) -> String {
    r.title.clone().or_else(|| r.agent.as_ref().map(|a| format!("{a} conversation"))).unwrap_or_else(|| short(&r.id))
}

/// An id's first eight characters.
pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// What an event off the stream changes about the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing.
    None,
    /// A row's live status changed.
    Changed,
    /// The list changed (or names a conversation it doesn't have): read it
    /// again.
    Relist,
}

/// Apply one event to the rows.
pub fn apply(rows: &mut [Row], ev: &Event) -> Effect {
    if ev.kind == "conversations" {
        return Effect::Relist;
    }
    let Some(conv) = ev.conversation_id.as_deref() else { return Effect::None };
    if ev.kind != "stage" {
        return Effect::None;
    }
    let Some(row) = rows.iter_mut().find(|r| r.id == conv) else {
        // A conversation the list doesn't have yet (new, or below the page):
        // a stage change says it's live, worth reading for.
        return Effect::Relist;
    };
    let (stage, state) = (ev.data["stage"].as_str().unwrap_or(""), ev.data["state"].as_str().unwrap_or(""));
    let live = match (stage, state) {
        ("turn", "started") => Some(Some("running")),
        ("turn", "done" | "failed" | "interrupted" | "cancelled") => Some(None),
        ("wake", "started") => Some(Some("waking")),
        ("wake", "done" | "failed") => Some(None),
        _ => None,
    };
    let Some(live) = live else { return Effect::None };
    let live = live.map(str::to_owned);
    if row.live == live {
        return Effect::None;
    }
    if live.is_none() && stage == "turn" {
        row.status = if state == "failed" { "failed".into() } else { "idle".into() };
    }
    row.live = live;
    Effect::Changed
}

/// Every `key:value` label on the rows, and how many carry it, the most
/// used first (at most `n`).
pub fn label_counts(rows: &[Row], n: usize) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (k, v) in &r.labels {
            *counts.entry(format!("{k}:{v}")).or_default() += 1;
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// A label filter as typed: `key:value` (or `key=value`), each once.
pub fn parse_labels(v: &Value) -> Result<Vec<String>, String> {
    let items: Vec<String> = match v {
        Value::Null => vec![],
        Value::String(s) => s.split([',', ' ']).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        _ => return Err("labels: a list of \"key:value\"".into()),
    };
    let mut out: Vec<String> = vec![];
    for i in items {
        let i = i.trim();
        let (k, val) = i
            .split_once(':')
            .or_else(|| i.split_once('='))
            .ok_or_else(|| format!("label {i:?}: key:value, as Fountain's ?label= takes it"))?;
        if k.trim().is_empty() {
            return Err(format!("label {i:?} has no key"));
        }
        let l = format!("{}:{}", k.trim(), val.trim());
        if !out.contains(&l) {
            out.push(l);
        }
    }
    Ok(out)
}

/// The filter as Fountain's query takes it.
pub fn label_pairs(labels: &[String]) -> Vec<(String, String)> {
    labels.iter().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// One node of a tree as a client draws it: depth-first from the roots.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub status: Option<String>,
    /// `api` (a client started it) or `agent` (another conversation did).
    pub source: Option<String>,
    pub title: Option<String>,
}

/// A spawn tree, flattened in order: each node after its parent.
pub fn tree(nodes: &[TreeNode], rows: &[Row]) -> Vec<Node> {
    let titles: HashMap<&str, String> = rows.iter().map(|r| (r.id.as_str(), name(r))).collect();
    let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut kids: BTreeMap<Option<&str>, Vec<&TreeNode>> = BTreeMap::new();
    for n in nodes {
        // A parent outside the tree (it shouldn't be) makes it a root.
        let p = n.parent_id.as_deref().filter(|p| ids.contains(p));
        kids.entry(p).or_default().push(n);
    }
    let mut out = vec![];
    let mut stack: Vec<(&TreeNode, usize)> = kids.get(&None).into_iter().flatten().rev().map(|n| (*n, 0)).collect();
    while let Some((n, depth)) = stack.pop() {
        if out.iter().any(|o: &Node| o.id == n.id) {
            continue;
        }
        out.push(Node {
            id: n.id.clone(),
            parent_id: n.parent_id.clone(),
            depth,
            status: n.status.clone(),
            source: n.source.clone(),
            title: titles
                .get(n.id.as_str())
                .cloned()
                .or_else(|| n.extra.get("title").and_then(|t| t.as_str()).map(str::to_owned)),
        });
        for k in kids.get(&Some(n.id.as_str())).into_iter().flatten().rev() {
            stack.push((k, depth + 1));
        }
    }
    out
}

/// One row as `capture --text` shows it.
pub fn line(r: &Row) -> String {
    let dot = if r.unread { "● " } else { "  " };
    let status = r.live.as_deref().unwrap_or(&r.status);
    let mut s = format!("{dot}{} [{status}]", name(r));
    if let Some(a) = &r.agent {
        s.push_str(&format!(" {a}"));
    }
    if r.asking > 0 {
        s.push_str(&format!(" asks ({})", r.asking));
    }
    if r.children > 0 {
        s.push_str(&format!(" +{} started", r.children));
    }
    if !r.labels.is_empty() {
        let l: Vec<String> = r.labels.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        s.push_str(&format!(" {{{}}}", l.join(", ")));
    }
    s.push_str(&format!(" {}", short(&r.id)));
    s
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn conv(v: Value) -> Conversation {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn rows_name_their_agents_and_count_children() {
        let agents: Vec<Agent> = vec![serde_json::from_value(json!({ "id": "a1", "name": "games" })).unwrap()];
        let convs = [
            conv(
                json!({ "id": "c1", "status": "idle", "agent_id": "a1", "unread": true, "labels": { "env": "prod" } }),
            ),
            conv(
                json!({ "id": "c2", "status": "running", "agent_id": "zz", "parent_conversation_id": "c1", "title": "child" }),
            ),
        ];
        let r = rows(&convs, &agents);
        assert_eq!((r[0].agent.as_deref(), r[0].children, r[0].unread), (Some("games"), 1, true));
        assert_eq!(name(&r[0]), "games conversation");
        assert_eq!((r[1].agent.as_deref(), name(&r[1])), (None, "child".to_owned()));
        assert_eq!(line(&r[0]), "● games conversation [idle] games +1 started {env:prod} c1");
        assert_eq!(label_counts(&r, 5), [("env:prod".to_owned(), 1)]);
    }

    #[test]
    fn the_stream_changes_status_or_asks_for_a_read() {
        let mut r = rows(&[conv(json!({ "id": "c1", "status": "idle" }))], &[]);
        let ev = |kind: &str, conv: Option<&str>, data: Value| Event {
            id: 1,
            kind: kind.into(),
            conversation_id: conv.map(str::to_owned),
            data,
        };
        let started = ev("stage", Some("c1"), json!({ "stage": "turn", "state": "started" }));
        assert_eq!(apply(&mut r, &started), Effect::Changed);
        assert_eq!(r[0].live.as_deref(), Some("running"));
        assert_eq!(apply(&mut r, &started), Effect::None);
        assert_eq!(apply(&mut r, &ev("output", Some("c1"), json!({}))), Effect::None);
        let failed = ev("stage", Some("c1"), json!({ "stage": "turn", "state": "failed" }));
        assert_eq!(apply(&mut r, &failed), Effect::Changed);
        assert_eq!((r[0].live.as_deref(), r[0].status.as_str()), (None, "failed"));
        assert_eq!(apply(&mut r, &ev("conversations", None, json!({ "reason": "changed" }))), Effect::Relist);
        let other = ev("stage", Some("c9"), json!({ "stage": "turn", "state": "started" }));
        assert_eq!(apply(&mut r, &other), Effect::Relist);
    }

    #[test]
    fn labels_as_typed() {
        assert_eq!(parse_labels(&json!("env:prod, team=a")).unwrap(), ["env:prod", "team:a"]);
        assert_eq!(parse_labels(&json!(["path:a:b", "path:a:b"])).unwrap(), ["path:a:b"]);
        assert!(parse_labels(&json!(["nokey"])).is_err() && parse_labels(&json!([":v"])).is_err());
        assert_eq!(label_pairs(&["path:a:b".into()]), [("path".to_owned(), "a:b".to_owned())]);
    }

    #[test]
    fn a_tree_in_order() {
        let n = |id: &str, p: Option<&str>| TreeNode {
            id: id.into(),
            parent_id: p.map(str::to_owned),
            status: Some("idle".into()),
            source: None,
            extra: Default::default(),
        };
        let t = tree(&[n("c", Some("a")), n("a", None), n("b", Some("a")), n("d", Some("b"))], &[]);
        let got: Vec<(String, usize)> = t.iter().map(|n| (n.id.clone(), n.depth)).collect();
        assert_eq!(got, [("a".to_owned(), 0), ("c".to_owned(), 1), ("b".to_owned(), 1), ("d".to_owned(), 2)]);
    }
}
